# aiclient-api Design Spec

Unified AI gateway service in Rust. Authenticates against GitHub Copilot and Kiro, exposes their models through OpenAI-compatible and Anthropic-compatible API endpoints. Full CLI control, daemon-based runtime, no web UI.

## 1. Architecture Overview

Layered monolith — single binary, clean module boundaries.

```
┌─────────────────────────────────────────────────────┐
│                    aiclient-api binary               │
├──────────┬──────────────────────────────────────────┤
│  CLI     │  Control Client (Unix Socket JSON-RPC)   │
│  (clap)  │  auth / start / stop / status / config   │
├──────────┴──────────────────────────────────────────┤
│                   Daemon Process                     │
│  ┌────────────────┐  ┌───────────────────────────┐  │
│  │ HTTP Server     │  │ Control Server             │ │
│  │ (axum, TCP)     │  │ (axum, Unix Socket)        │ │
│  │ /v1/chat/compl. │  │ status / config / logs     │ │
│  │ /v1/messages    │  │ provider enable/disable    │ │
│  │ /v1/models      │  │                            │ │
│  └──────┬─────────┘  └───────────────────────────┘  │
│         │                                            │
│  ┌──────▼─────────────────────────────────────────┐  │
│  │              Format Converter                   │ │
│  │  OpenAI ↔ ProviderNative ↔ Anthropic            │ │
│  └──────┬─────────────────────────────────────────┘  │
│         │                                            │
│  ┌──────▼─────────────────────────────────────────┐  │
│  │              Provider Layer                      │ │
│  │  ┌──────────────┐  ┌─────────────────────┐      │ │
│  │  │ Copilot      │  │ Kiro                │      │ │
│  │  │ (GitHub API) │  │ (AWS → CodeWhisperer│      │ │
│  │  └──────────────┘  └─────────────────────┘      │ │
│  └──────┬─────────────────────────────────────────┘  │
│         │                                            │
│  ┌──────▼─────────────────────────────────────────┐  │
│  │         Auth & Token Manager                    │ │
│  │  Token refresh loops (tokio::spawn)              │ │
│  │  XDG file-based persistence (0600)               │ │
│  └─────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

Two listen channels:
- **TCP HTTP** — external API serving clients
- **Unix Domain Socket** — internal control from CLI

## 2. Project Structure

```
aiclient-api/
├── Cargo.toml
├── config.example.toml
├── src/
│   ├── main.rs                    # clap CLI dispatch
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── auth.rs                # auth copilot | kiro | list | revoke
│   │   ├── start.rs               # start [--foreground]
│   │   ├── stop.rs                # stop
│   │   ├── restart.rs             # restart
│   │   ├── status.rs              # status (via socket)
│   │   ├── config_cmd.rs          # config show | set | reload
│   │   ├── models.rs              # models (via socket)
│   │   ├── provider_cmd.rs        # provider enable | disable
│   │   ├── logs.rs                # logs --lines --level
│   │   ├── update.rs              # self-update
│   │   └── uninstall.rs           # stop + cleanup
│   ├── config/
│   │   ├── mod.rs                 # load / merge / hot-reload
│   │   └── types.rs               # Config structs (serde + toml)
│   ├── auth/
│   │   ├── mod.rs                 # AuthManager trait
│   │   ├── copilot.rs             # GitHub device flow OAuth
│   │   ├── kiro.rs                # AWS SSO token extraction
│   │   └── token_store.rs         # XDG file persistence (0600)
│   ├── daemon/
│   │   ├── mod.rs                 # daemon fork, PID file, signal handling
│   │   ├── control.rs             # Unix Socket control server (JSON-RPC)
│   │   └── log_stream.rs          # live log streaming to CLI
│   ├── server/
│   │   ├── mod.rs                 # axum Router assembly (API routes)
│   │   ├── middleware.rs          # auth, CORS, rate-limit, request logging
│   │   └── state.rs              # AppState: providers, config, tokens
│   ├── routes/
│   │   ├── mod.rs
│   │   ├── openai.rs              # POST /v1/chat/completions, GET /v1/models
│   │   ├── anthropic.rs           # POST /v1/messages
│   │   └── health.rs              # GET /healthz
│   ├── providers/
│   │   ├── mod.rs                 # Provider trait
│   │   ├── copilot/
│   │   │   ├── mod.rs
│   │   │   ├── client.rs          # reqwest calls to api.githubcopilot.com
│   │   │   ├── models.rs          # model listing & normalization
│   │   │   └── headers.rs         # VSCode header spoofing
│   │   └── kiro/
│   │       ├── mod.rs
│   │       ├── client.rs          # CodeWhisperer API calls
│   │       ├── models.rs
│   │       └── cw_types.rs        # CodeWhisperer request/response types
│   ├── convert/
│   │   ├── mod.rs
│   │   ├── openai_types.rs        # OpenAI request/response structs
│   │   ├── anthropic_types.rs     # Anthropic request/response structs
│   │   ├── to_openai.rs           # ProviderNative → OpenAI
│   │   ├── to_anthropic.rs        # ProviderNative → Anthropic
│   │   ├── from_openai.rs         # OpenAI → ProviderNative
│   │   ├── from_anthropic.rs      # Anthropic → ProviderNative
│   │   └── stream.rs              # SSE chunk conversion (both formats)
│   └── util/
│       ├── mod.rs
│       ├── error.rs               # AppError, HTTP error mapping
│       ├── stream.rs              # SSE stream helpers
│       ├── machine_id.rs          # SHA256 machine ID generation
│       └── xdg.rs                 # XDG path resolution
└── docs/
```

Single crate. No workspace — this scope doesn't warrant it.

## 3. CLI Command Specification

```
aiclient-api <COMMAND>

LIFECYCLE:
  start                    Start daemon (background by default)
    --port <PORT>            API port [default: 9090]
    --host <HOST>            Bind address [default: 127.0.0.1]
    --foreground             Run in foreground (for debugging)
    --api-key <KEY>          Protect API with bearer token
    --log-file <PATH>        Log output file [default: $XDG_STATE_HOME/aiclient-api/daemon.log]
  stop                     Graceful stop (SIGTERM, 10s timeout → SIGKILL)
  restart                  stop + start

AUTHENTICATION (runs interactively, does NOT require daemon):
  auth copilot             GitHub device flow OAuth
    --account-type <TYPE>    individual | business | enterprise [default: individual]
  auth kiro                Kiro / AWS SSO token setup
  auth list                Show authenticated providers & token expiry
  auth revoke <PROVIDER>   Delete stored tokens

RUNTIME CONTROL (sends JSON-RPC to daemon via Unix Socket):
  status                   Daemon uptime, token health, active connections, memory
  config show              Current effective config (merged)
  config set <KEY> <VALUE> Hot-update a config value
  config reload            Re-read config.toml from disk
  models                   List available models across all providers
  provider enable <NAME>   Enable a provider
  provider disable <NAME>  Disable a provider

LOGS:
  logs                     Tail daemon logs in real-time
    --lines <N>              Show last N lines [default: 50]
    --level <LEVEL>          Filter: trace | debug | info | warn | error

MAINTENANCE:
  update                   Self-update from GitHub Releases
  uninstall                Stop daemon + remove config + remove binary
```

### Control Protocol

Unix Socket path: `$XDG_RUNTIME_DIR/aiclient-api/ctl.sock`
Fallback: `~/.config/aiclient-api/ctl.sock`

JSON-RPC style over the socket:

```json
// Request
{ "method": "status", "params": {} }
{ "method": "config.set", "params": { "key": "default_format", "value": "anthropic" } }
{ "method": "provider.enable", "params": { "name": "kiro" } }
{ "method": "logs.stream", "params": { "lines": 50, "level": "info" } }

// Success response
{ "ok": true, "data": { "uptime_secs": 3600, "providers": [...] } }

// Error response
{ "ok": false, "error": "provider 'codex' not configured" }
```

### Hot-updatable vs restart-required config

Hot-update (no restart):
- `default_provider`, `default_format`
- `api_key`
- `rate_limit_seconds`
- `logging.level`

Restart required:
- `server.host`, `server.port`
- Adding new provider sections

### Process Management

- `start` → `daemonize` crate forks, writes PID to `$XDG_RUNTIME_DIR/aiclient-api/daemon.pid`
- `stop` → reads PID file → sends `SIGTERM` → polls for exit (10s timeout) → `SIGKILL`
- `restart` → `stop` then `start`
- Daemon installs signal handlers: `SIGTERM` → graceful shutdown (drain connections), `SIGHUP` → config reload

## 4. Authentication

### 4.1 GitHub Copilot — Device Flow OAuth

Interactive command: `aiclient-api auth copilot`

```
Step 1: Request device code
  POST https://github.com/login/device/code
  Body: client_id=Iv1.b507a08c87ecfe98&scope=read:user
  Response: { device_code, user_code, verification_uri, interval }

Step 2: Display to user
  "Please visit: https://github.com/login/device"
  "Enter code: XXXX-YYYY"
  Auto-open browser via `open` crate

Step 3: Poll for access token
  POST https://github.com/login/oauth/access_token
  Body: client_id, device_code, grant_type=urn:ietf:params:oauth:grant-type:device_code
  Poll every (interval + 1) seconds
  Handle: authorization_pending, slow_down, expired_token, access_denied

Step 4: Persist
  Save { access_token, token_type, scope } to
  ~/.config/aiclient-api/copilot/github_token.json (mode 0600)
```

On daemon start, Copilot provider init:

```
Step 5: Fetch Copilot session token
  GET https://api.github.com/copilot_internal/v2/token
  Headers: Authorization: token {github_token}
  Response: { token, expires_at, refresh_in }

Step 6: Background refresh loop
  tokio::spawn → sleep(refresh_in - 60 seconds) → re-fetch token
  On failure: retry every 15 seconds
  Store in Arc<RwLock<CopilotToken>> for concurrent access
```

Copilot base URLs by account type:
- individual: `https://api.githubcopilot.com`
- business: `https://api.business.githubcopilot.com`
- enterprise: `https://copilot-api.{enterprise_domain}`

### 4.2 Kiro — AWS Builder ID / Social Auth

Interactive command: `aiclient-api auth kiro`

Kiro supports three OAuth flows. The CLI offers a menu:

**Option A: AWS Builder ID Device Code (recommended — no browser callback needed)**

```
Step 1: Register OIDC client
  POST https://oidc.{region}.amazonaws.com/client/register
  Body: { clientName: "aiclient-api", clientType: "public", scopes: [...],
          grantTypes: ["urn:ietf:params:oauth:grant-type:device_code", "refresh_token"],
          issuerUrl: "https://identitycenter.amazonaws.com/ssoins-..." }
  Response: { clientId, clientSecret, clientSecretExpiresAt }

Step 2: Start device authorization
  POST https://oidc.{region}.amazonaws.com/device_authorization
  Body: { clientId, clientSecret, startUrl: "https://view.awsapps.com/start" }
  Response: { deviceCode, userCode, verificationUri, verificationUriComplete, interval }

Step 3: Display to user
  "Please visit: {verificationUriComplete}"
  "Or go to {verificationUri} and enter code: {userCode}"
  Auto-open browser

Step 4: Poll for token
  POST https://oidc.{region}.amazonaws.com/token
  Body: { clientId, clientSecret, deviceCode, grantType: "urn:ietf:params:oauth:grant-type:device_code" }
  Poll every (interval + 1) seconds
  Response: { accessToken, refreshToken, expiresIn, idToken }

Step 5: Persist
  Save { accessToken, refreshToken, clientId, clientSecret, authMethod: "builder_id",
         region, idcRegion } to ~/.config/aiclient-api/kiro/token.json (mode 0600)
```

**Option B: Google Social Auth (PKCE + localhost callback)**

```
Step 1: Generate PKCE code_verifier + code_challenge
Step 2: Open browser to Kiro social auth endpoint with Google provider
  https://prod.{region}.auth.desktop.kiro.dev/socialAuth
  Params: provider=Google, redirectUri=http://127.0.0.1:{port}/callback,
          codeChallenge, state
Step 3: Listen on localhost:{port} for OAuth callback with authorization code
Step 4: Exchange code for tokens
  POST https://prod.{region}.auth.desktop.kiro.dev/exchangeToken
  Body: { code, codeVerifier, redirectUri }
  Response: { accessToken, refreshToken, expiresIn, profileArn }
Step 5: Persist (same format, authMethod: "google")
```

**Option C: GitHub Social Auth (same PKCE flow as Google, provider=GitHub)**

On daemon start, Kiro provider init:

```
Step 6: Load stored tokens
Step 7: Background refresh loop
  For Builder ID:
    POST https://oidc.{region}.amazonaws.com/token
    Body: { clientId, clientSecret, refreshToken, grantType: "refresh_token" }
  For Social Auth:
    POST https://prod.{region}.auth.desktop.kiro.dev/refreshToken
    Body: { refreshToken }
  Refresh when token is within 5 minutes of expiry
  On failure: retry every 15 seconds
Step 8: Store tokens in Arc<RwLock<KiroToken>> for concurrent access
```

### 4.2.1 Kiro Upstream API — CodeWhisperer

Kiro does NOT use the standard Anthropic Messages API. It uses AWS CodeWhisperer's proprietary endpoint:

```
POST https://q.{region}.amazonaws.com/generateAssistantResponse

Request body:
{
  "conversationState": {
    "chatTriggerType": "MANUAL",
    "currentMessage": {
      "userInputMessage": {
        "content": "...",
        "userInputMessageContext": { ... }
      }
    },
    "history": [ ... ]
  }
}

Required headers:
  Authorization: Bearer {accessToken}
  x-amzn-codewhisperer-optout: false
  x-amzn-kiro-agent-mode: true
  amz-sdk-invocation-id: {UUID}
  amz-sdk-request: attempt=1
  x-amz-user-agent: kiro/{version}
  Content-Type: application/json
```

The Kiro provider implementation must convert `ProviderRequest` into the CodeWhisperer `conversationState` format, and parse the streaming response back into `ProviderChunk` events. This is the most complex conversion in the system.

### 4.3 Token Store Interface

```rust
#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn load(&self, provider: &str) -> Result<TokenData>;
    async fn save(&self, provider: &str, data: &TokenData) -> Result<()>;
    async fn delete(&self, provider: &str) -> Result<()>;
    fn is_expired(&self, data: &TokenData) -> bool;
}

/// Provider-specific token data
pub enum TokenData {
    Copilot {
        github_token: String,         // long-lived OAuth token
        copilot_token: Option<String>, // short-lived session token (fetched at runtime)
        expires_at: Option<i64>,
    },
    Kiro {
        access_token: String,
        refresh_token: String,
        client_id: Option<String>,    // for Builder ID flow
        client_secret: Option<String>,// for Builder ID flow
        auth_method: String,          // "builder_id" | "google" | "github"
        region: String,               // e.g., "us-east-1"
        idc_region: Option<String>,
        profile_arn: Option<String>,  // for social auth
        expires_at: i64,
    },
}
```

Implementation: `XdgTokenStore` — JSON files under `~/.config/aiclient-api/<provider>/`, permissions 0600.

### 4.4 Request Header Spoofing (Copilot)

Every request to `api.githubcopilot.com` includes:

```
Authorization: Bearer {copilot_token}
x-request-id: {UUID v4}
x-agent-task-id: {UUID v4}
editor-version: vscode/{vscode_version}
editor-plugin-version: copilot-chat/0.38.2
copilot-integration-id: vscode-chat
user-agent: GitHubCopilotChat/0.38.2
openai-intent: conversation-agent
x-github-api-version: 2025-10-01
x-interaction-type: conversation-agent
x-initiator: user
x-vscode-user-agent-library-version: electron-fetch
vscode-machineid: {SHA256(mac_address)}
vscode-sessionid: {UUID+timestamp, rotated hourly}
```

**`vscode-machineid`**: Derived from the system's primary MAC address (first non-internal network interface), SHA256 hashed. Fallback to a random UUID persisted in config dir if no MAC is found. This matches the reference implementation's behavior.

**`vscode_version`**: Defaults to `1.110.1`. Stored in config so it can be updated without recompilation.

**`vscode-sessionid`**: `{UUID v4}{timestamp_ms}`, regenerated every hour via background timer.

**`x-initiator`**: Set to `user` for direct requests, `agent` for follow-up/subagent requests. This affects billing (premium vs non-premium).

**`x-interaction-type`**: `conversation-agent` for normal requests, `conversation-subagent` for subagent traffic.

## 5. Provider Trait

```rust
/// Normalized internal model representation
pub struct Model {
    pub id: String,           // e.g., "gpt-5.4", "claude-opus-4.6"
    pub provider: String,     // "copilot" | "kiro"
    pub vendor: String,       // "openai" | "anthropic"
    pub display_name: String,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
}

/// Provider-native request (intermediate representation)
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<ToolChoice>,
}

pub enum ProviderResponse {
    Complete(serde_json::Value),
    Stream(Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>),
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn is_healthy(&self) -> bool;
    async fn list_models(&self) -> Result<Vec<Model>>;

    /// Send a chat request via provider-native intermediate format
    async fn chat(
        &self,
        request: ProviderRequest,
    ) -> Result<ProviderResponse>;

    /// Some providers support certain formats natively (e.g., Copilot supports
    /// both OpenAI and Anthropic endpoints). When available, passthrough avoids
    /// double-conversion and preserves features like thinking blocks.
    /// Returns None if passthrough is not supported for this format.
    fn supports_passthrough(&self, format: OutputFormat) -> bool { false }

    /// Pass the raw request body directly to the upstream endpoint.
    /// Only called when supports_passthrough() returns true.
    async fn passthrough(
        &self,
        model: &str,
        body: serde_json::Value,
        format: OutputFormat,
        stream: bool,
    ) -> Result<ProviderResponse> {
        Err(anyhow::anyhow!("passthrough not supported"))
    }
}
```

Each provider implements this trait. The daemon holds `HashMap<String, Arc<dyn Provider>>` in `AppState`.

## 6. Request Routing

### 6.1 Model-to-Provider Resolution

Three ways to specify which provider handles a request:

1. **Model prefix**: `copilot/gpt-5.4` or `kiro/claude-opus-4.6` — explicit routing
2. **Header**: `X-Provider: copilot` — override for the request
3. **Config default**: `default_provider = "copilot"` — fallback

Resolution order: model prefix > X-Provider header > config default.

### 6.2 Request Flow

```
Client request
    │
    ▼
[API Route Handler]  ─── parse incoming format (OpenAI or Anthropic)
    │
    ▼
[Provider Router]    ─── resolve provider from model/header/default
    │
    ├── provider.supports_passthrough(format)?
    │       │
    │       ├── YES → [Provider.passthrough()]  ─── forward raw body to upstream
    │       │              │
    │       │              ▼
    │       │         Client response (as-is from upstream)
    │       │
    │       └── NO ──▼
    │
    ▼
[Format Converter]   ─── incoming format → ProviderRequest
    │
    ▼
[Provider.chat()]    ─── send to upstream, get ProviderResponse
    │
    ▼
[Format Converter]   ─── ProviderResponse → outgoing format
    │
    ▼
Client response (SSE stream or JSON)
```

**Passthrough optimization**: Copilot's upstream API natively supports both `/chat/completions` (OpenAI) and `/v1/messages` (Anthropic). When the client format matches a native upstream endpoint, we skip conversion entirely and forward the request body as-is. This preserves features like thinking blocks that may be lost in conversion.

### 6.3 Upstream Path Mapping

Gateway endpoints map to upstream paths (which differ per provider):

**Copilot** (base: `https://api.githubcopilot.com`):
- Client `/v1/chat/completions` → Upstream `/chat/completions` (no `/v1/` prefix)
- Client `/v1/messages` → Upstream `/v1/messages` (kept as-is)
- Client `/v1/models` → Upstream `/models` (no `/v1/` prefix)

**Kiro** (base: `https://q.{region}.amazonaws.com`):
- All requests → Upstream `/generateAssistantResponse` (CodeWhisperer format, always converted)

## 7. Format Conversion

### 7.1 Endpoints

| Endpoint | Request Format | Response Format |
|---|---|---|
| `POST /v1/chat/completions` | OpenAI | OpenAI |
| `POST /v1/messages` | Anthropic | Anthropic |
| `GET /v1/models` | — | OpenAI model list |
| `GET /healthz` | — | `{ "status": "ok" }` |

Both endpoints are always available regardless of `default_format` setting. The `default_format` config controls which format is used when the content-type is ambiguous.

### 7.2 Conversion Functions

Stateless functions — no trait polymorphism needed:

```rust
pub mod convert {
    // Incoming → ProviderRequest
    pub fn from_openai(req: OpenAIChatRequest) -> Result<ProviderRequest>;
    pub fn from_anthropic(req: AnthropicMessagesRequest) -> Result<ProviderRequest>;

    // ProviderResponse → Outgoing (non-streaming)
    pub fn to_openai(resp: ProviderResponse, model: &str) -> OpenAIChatResponse;
    pub fn to_anthropic(resp: ProviderResponse, model: &str) -> AnthropicMessagesResponse;

    // Stream chunk conversion
    pub fn chunk_to_openai(chunk: ProviderChunk) -> Vec<u8>;     // SSE bytes
    pub fn chunk_to_anthropic(chunk: ProviderChunk) -> Vec<u8>;  // SSE bytes
}
```

### 7.3 Key Conversion Mappings

**Messages:**
- OpenAI `messages[].role` (system/user/assistant/tool) ↔ Anthropic `system` + `messages[].role` (user/assistant)
- Anthropic extracts `system` from the top-level field; OpenAI keeps it inline

**Content types:**
- OpenAI `content: string | [{ type, text }]` ↔ Anthropic `content: [{ type: "text", text }]`
- Images: OpenAI `image_url` ↔ Anthropic `image` with `source.type: "base64"|"url"`

**Tool calls:**
- OpenAI `tool_calls: [{ id, function: { name, arguments } }]` ↔ Anthropic `content: [{ type: "tool_use", id, name, input }]`
- Tool results: OpenAI `role: "tool"` message ↔ Anthropic `role: "user"` with `content: [{ type: "tool_result" }]`

**Streaming:**
- OpenAI: `data: {"id":"...","choices":[{"delta":{"content":"..."}}]}\n\n`
- Anthropic: `event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}\n\n`

**Thinking:**
- Anthropic `thinking` blocks → OpenAI: included as a prefixed text chunk or stripped (configurable)

## 8. Configuration

### 8.1 Config File

Path: `~/.config/aiclient-api/config.toml`

```toml
# Default output format when both endpoints are available
default_format = "openai"           # "openai" | "anthropic"

# Default provider when model has no prefix
default_provider = "copilot"

# API key to protect the gateway (empty string = no auth required)
api_key = ""

[server]
host = "127.0.0.1"
port = 9090
rate_limit_seconds = 0              # 0 = disabled

# VSCode version for Copilot header spoofing (keep updated)
vscode_version = "1.110.1"

[providers.copilot]
type = "copilot"
enabled = true
account_type = "individual"         # individual | business | enterprise
# enterprise_url = ""               # required if account_type = enterprise

[providers.kiro]
type = "kiro"
enabled = true
region = "us-east-1"
# idc_region = "us-east-1"

[logging]
level = "info"                      # trace | debug | info | warn | error
file = ""                           # empty = $XDG_STATE_HOME/aiclient-api/daemon.log
```

### 8.2 Config Merge Priority

CLI flags > environment variables > config.toml > hardcoded defaults

### 8.3 Config Structs

```rust
#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub default_format: Format,
    pub default_provider: String,
    pub api_key: String,
    pub vscode_version: String,
    pub server: ServerConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub logging: LogConfig,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub rate_limit_seconds: u64,
}

/// Each provider table has a `type` field for discrimination
#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum ProviderConfig {
    #[serde(rename = "copilot")]
    Copilot {
        enabled: bool,
        account_type: AccountType,
        enterprise_url: Option<String>,
    },
    #[serde(rename = "kiro")]
    Kiro {
        enabled: bool,
        region: String,
        idc_region: Option<String>,
    },
}
```

The `type` field inside each `[providers.*]` TOML table drives serde's internally-tagged enum deserialization. Example: `[providers.copilot]` contains `type = "copilot"`.

Config wrapped in `Arc<ArcSwap<Config>>` for lock-free hot-reload.

## 9. Server & Middleware

### 9.1 AppState

```rust
pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub providers: Arc<RwLock<HashMap<String, Arc<dyn Provider>>>>,
    pub start_time: Instant,
}
```

### 9.2 Middleware Stack (axum Tower layers)

Applied in order:
1. **Request ID** — inject `x-request-id` UUID
2. **Logging** — tracing span per request (method, path, status, duration)
3. **CORS** — allow all origins (local gateway use case)
4. **Rate Limit** — token bucket per client IP (if configured)
5. **Auth** — validate `Authorization: Bearer {api_key}` if `api_key` is set
6. **Error Mapping** — catch panics, map to JSON error responses

### 9.3 Error Format

Errors returned in the format matching the endpoint:

OpenAI endpoint errors:
```json
{ "error": { "message": "...", "type": "...", "code": "..." } }
```

Anthropic endpoint errors:
```json
{ "type": "error", "error": { "type": "...", "message": "..." } }
```

## 10. Streaming (SSE)

Both endpoints support `"stream": true`.

Implementation uses `axum::response::Sse` with `tokio_stream::Stream`:

```rust
async fn handle_stream(
    provider_stream: Pin<Box<dyn Stream<Item = Result<ProviderChunk>>>>,
    format: OutputFormat,
) -> Sse<impl Stream<Item = Result<Event>>> {
    let mapped = provider_stream.map(move |chunk| {
        let bytes = match format {
            OutputFormat::OpenAI => convert::chunk_to_openai(chunk?),
            OutputFormat::Anthropic => convert::chunk_to_anthropic(chunk?),
        };
        Ok(Event::default().data(String::from_utf8_lossy(&bytes)))
    });
    Sse::new(mapped).keep_alive(KeepAlive::default())
}
```

Upstream provider streams are consumed via `reqwest`'s `bytes_stream()`, parsed into `ProviderChunk` enum variants, then converted to the target format on-the-fly.

## 11. Error Handling & Resilience

- **Token expiry mid-request**: return 401 with message directing user to `aiclient-api auth <provider>`. Background refresh should prevent this in practice.
- **Provider unavailable**: return 503 with provider name.
- **Upstream 429**: forward rate-limit response as-is with `Retry-After` header.
- **Upstream 5xx**: return 502 Bad Gateway with upstream error body.
- **No retry logic in the gateway** — clients handle retries. Gateway is a thin proxy.
- **Graceful shutdown**: `SIGTERM` → stop accepting new connections → drain active (10s timeout) → exit.
- **Panic recovery**: `tower::catch_panic` layer returns 500 instead of crashing.

## 12. File System Layout

```
~/.config/aiclient-api/
├── config.toml                     # User configuration
├── copilot/
│   └── github_token.json           # GitHub OAuth token (0600)
└── kiro/
    └── token.json                  # Kiro token: accessToken, refreshToken,
                                    # clientId, clientSecret, authMethod,
                                    # region, idcRegion, profileArn (0600)

$XDG_RUNTIME_DIR/aiclient-api/     (or /tmp/aiclient-api-{uid}/)
├── ctl.sock                        # Unix domain socket for control
└── daemon.pid                      # PID file

$XDG_STATE_HOME/aiclient-api/      (or ~/.local/state/aiclient-api/)
└── daemon.log                      # Log file
```

## 13. Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `clap` (derive) | 4.x | CLI argument parsing |
| `axum` | 0.8.x | HTTP server (API + control socket) |
| `tokio` | 1.x | Async runtime |
| `reqwest` | 0.12.x | HTTP client (upstream API calls) |
| `serde` + `serde_json` | 1.x | JSON serialization |
| `toml` | 0.8.x | Config file parsing |
| `tracing` + `tracing-subscriber` | 0.1.x / 0.3.x | Structured logging |
| `tracing-appender` | 0.2.x | Log file output |
| `tokio-stream` | 0.1.x | Stream combinators for SSE |
| `tower` + `tower-http` | 0.5.x | Middleware (CORS, timeout, catch-panic) |
| `arc-swap` | 1.x | Lock-free config hot-reload |
| `sha2` | 0.10.x | Machine ID hashing |
| `mac_address` | 1.x | MAC address for vscode-machineid |
| `uuid` | 1.x | Request/session IDs |
| `open` | 5.x | Browser opening for OAuth |
| `dirs` | 6.x | XDG directory resolution |
| `daemonize` | 0.5.x | Process fork + PID file |
| `hyper-util` | 0.1.x | Unix socket listener |
| `self_update` | 0.41.x | GitHub Releases self-update |
| `anyhow` | 1.x | Application error handling |
| `thiserror` | 2.x | Library error types |
| `async-trait` | 0.1.x | Async trait support |

## 14. Future Extensibility

Adding a new provider (e.g., Codex, Gemini):

1. Create `src/providers/<name>/` with `mod.rs`, `client.rs`, `models.rs`
2. Implement `Provider` trait
3. Add variant to config `ProviderConfig` enum
4. Register in daemon startup provider initialization
5. No changes needed to routes, conversion, or CLI

The `Provider` trait is the extension point. Format conversion is shared.

## 15. Non-Goals

- No web UI — CLI only
- No multi-tenant / multi-user auth — single-user local gateway
- No request caching or response storage
- No load balancing across multiple accounts of the same provider (can be added later via provider pools)
- No metrics/prometheus endpoint (can be added as a separate control socket method later)

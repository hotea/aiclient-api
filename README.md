# aiclient-api

**English** | [中文](#中文)

A unified AI gateway daemon that authenticates against **GitHub Copilot** and **Kiro** (AWS CodeWhisperer), exposing standard OpenAI-compatible and Anthropic-compatible API endpoints locally.

Use your existing Copilot or Kiro subscription with any tool that speaks the OpenAI or Anthropic API — Claude Code, Cursor, Continue, or your own scripts.

## Features

- **Dual provider support** — GitHub Copilot (Individual / Business / Enterprise) and Kiro (Builder ID / Google / GitHub / IAM Identity Center)
- **OpenAI-compatible endpoint** — `POST /v1/chat/completions`, `GET /v1/models`
- **Anthropic-compatible endpoint** — `POST /v1/messages`
- **Usage tracking** — `GET /v1/usage` to monitor token consumption by provider and model
- **Automatic format conversion** — OpenAI <-> Anthropic, transparently
- **Passthrough mode** — Skip conversion when the provider natively supports the target format
- **SSE streaming** — Full streaming support with format-aware chunk conversion
- **Smart model routing** — By model prefix (`copilot/gpt-4`), `X-Provider` header, or config default
- **Bearer token auth** — Optional API key protection on all `/v1/*` routes
- **Per-IP rate limiting** — Configurable request interval
- **Daemon mode** — Runs in background with PID management, or `--foreground` for debugging
- **Unix socket control** — CLI commands talk to the running daemon over a Unix socket
- **Hot config reload** — `SIGHUP` or `config reload` to apply changes without restart
- **XDG-compliant paths** — Config, tokens, logs, PID, socket all follow XDG conventions

## Quick Start

### 1. Build

```bash
cargo build --release
```

### 2. Authenticate

```bash
# GitHub Copilot (device flow — opens browser)
aiclient-api auth copilot

# Kiro / AWS Builder ID (interactive menu)
aiclient-api auth kiro

# Kiro / IAM Identity Center (organization identity)
aiclient-api auth kiro --start-url https://my-org.awsapps.com/start --region us-east-1
```

### 3. Start the daemon

```bash
# Background mode (default)
aiclient-api start

# Foreground mode (for debugging)
aiclient-api start --foreground

# Custom port and API key
aiclient-api start --port 8080 --api-key my-secret
```

### 4. Use it

```bash
# OpenAI format
curl http://127.0.0.1:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "copilot/gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Anthropic format
curl http://127.0.0.1:9090/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "copilot/claude-sonnet-4",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 1024
  }'

# Kiro provider
curl http://127.0.0.1:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "kiro/claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# List available models
curl http://127.0.0.1:9090/v1/models

# Check usage statistics
curl http://127.0.0.1:9090/v1/usage
```

## Usage Tracking

Monitor your token consumption in real-time:

```bash
# Get usage statistics
curl http://127.0.0.1:9090/v1/usage | jq .

# Example response
{
  "providers": {
    "copilot": {
      "request_count": 10,
      "input_tokens": 150,
      "output_tokens": 200,
      "total_tokens": 350,
      "models": {
        "gpt-4o": { "request_count": 5, "input_tokens": 80, ... },
        "claude-sonnet-4.5": { "request_count": 5, "input_tokens": 70, ... }
      }
    },
    "kiro": { ... }
  },
  "total": {
    "request_count": 10,
    "input_tokens": 150,
    "output_tokens": 200,
    "total_tokens": 350
  }
}

# Reset statistics
curl -X DELETE http://127.0.0.1:9090/v1/usage
```

**Features:**
- ✅ Real-time tracking per provider and model
- ✅ Tracks input/output tokens separately
- ✅ Aggregated total statistics
- ⚠️ In-memory storage (resets on restart)

See [USAGE_TRACKING.md](./USAGE_TRACKING.md) for full documentation.

## Configuration

Config file: `~/.config/aiclient-api/config.toml`

```toml
default_format = "openai"       # "openai" or "anthropic"
default_provider = "copilot"    # default provider when no prefix
api_key = ""                    # empty = no auth required

[server]
host = "127.0.0.1"
port = 9090
rate_limit_seconds = 0          # 0 = disabled

[providers.copilot]
type = "copilot"
enabled = true
account_type = "individual"     # "individual", "business", "enterprise"

[providers.kiro]
type = "kiro"
enabled = true
region = "us-east-1"

[logging]
level = "info"
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `aiclient-api start` | Start the daemon |
| `aiclient-api stop` | Stop the daemon |
| `aiclient-api restart` | Restart the daemon |
| `aiclient-api status` | Show daemon status and provider health |
| `aiclient-api auth copilot` | Authenticate with GitHub Copilot |
| `aiclient-api auth kiro` | Authenticate with Kiro (interactive) |
| `aiclient-api auth kiro --start-url <url> --region <region>` | Kiro IAM Identity Center auth |
| `aiclient-api auth list` | List authenticated providers |
| `aiclient-api auth revoke <provider>` | Revoke a provider's tokens |
| `aiclient-api models` | List available models from all providers |
| `aiclient-api config init` | Interactive configuration wizard |
| `aiclient-api config show` | Show current config |
| `aiclient-api config reload` | Reload config from disk |
| `aiclient-api provider enable <name>` | Enable a provider |
| `aiclient-api provider disable <name>` | Disable a provider |
| `aiclient-api logs` | Tail daemon logs |
| `aiclient-api logs --level error` | Filter by log level |

## Model Routing

Models are routed by three mechanisms, in priority order:

1. **Model prefix** — `copilot/gpt-4o` routes to the `copilot` provider with model `gpt-4o`
2. **X-Provider header** — `X-Provider: kiro` routes to the `kiro` provider
3. **Config default** — Falls back to `default_provider` in config

## API Endpoints

| Endpoint | Method | Format | Description |
|----------|--------|--------|-------------|
| `/healthz` | GET | — | Health check (no auth) |
| `/v1/chat/completions` | POST | OpenAI | Chat completions |
| `/v1/models` | GET | OpenAI | List models |
| `/v1/messages` | POST | Anthropic | Messages API |
| `/v1/usage` | GET | JSON | Get usage statistics |
| `/v1/usage` | DELETE | JSON | Reset usage statistics |

## Architecture

```
src/
  auth/           # Token management
    copilot.rs    # GitHub device flow OAuth
    kiro.rs       # Builder ID + social auth (PKCE)
    token_store.rs
  cli/            # CLI commands
  config/         # TOML config loading
  convert/        # OpenAI <-> Anthropic format conversion
    stream.rs     # SSE chunk conversion
  daemon/         # Process management + Unix socket control
  providers/      # Provider implementations
    copilot/      # GitHub Copilot (VSCode header spoofing)
    kiro/         # Kiro / CodeWhisperer API
    router.rs     # Model-based provider routing
  routes/         # HTTP route handlers
    usage.rs      # Usage tracking endpoints
  server/         # Axum server, middleware (auth, rate-limit, CORS)
  usage/          # Token usage tracking
    tracker.rs    # UsageTracker implementation
  util/           # XDG paths, error types, streaming helpers
```

## File Locations

| File | Path |
|------|------|
| Config | `~/.config/aiclient-api/config.toml` |
| Copilot token | `~/.config/aiclient-api/copilot/token.json` |
| Kiro token | `~/.config/aiclient-api/kiro/token.json` |
| PID file | `~/.local/state/aiclient-api/daemon.pid` |
| Unix socket | `~/.local/state/aiclient-api/daemon.sock` |
| Log file | `~/.local/state/aiclient-api/daemon.log` |

## License

MIT

---

<a id="中文"></a>

# aiclient-api

[English](#aiclient-api) | **中文**

统一 AI 网关守护进程，通过认证接入 **GitHub Copilot** 和 **Kiro**（AWS CodeWhisperer），在本地暴露标准的 OpenAI 兼容和 Anthropic 兼容 API 端点。

用你已有的 Copilot 或 Kiro 订阅，配合任何支持 OpenAI 或 Anthropic API 的工具使用 — Claude Code、Cursor、Continue，或你自己的脚本。

## 功能特性

- **双 Provider 支持** — GitHub Copilot（Individual / Business / Enterprise）和 Kiro（Builder ID / Google / GitHub / IAM Identity Center）
- **OpenAI 兼容端点** — `POST /v1/chat/completions`、`GET /v1/models`
- **Anthropic 兼容端点** — `POST /v1/messages`
- **使用量跟踪** — `GET /v1/usage` 实时监控按 Provider 和模型的 Token 消耗
- **自动格式转换** — OpenAI <-> Anthropic 双向透明转换
- **直通模式** — Provider 原生支持目标格式时跳过转换，直接透传
- **SSE 流式** — 完整的流式支持，带格式感知的 chunk 转换
- **智能模型路由** — 通过模型前缀（`copilot/gpt-4`）、`X-Provider` 请求头或配置默认值
- **Bearer Token 认证** — 可选的 API Key 保护，作用于所有 `/v1/*` 路由
- **IP 限流** — 可配置的请求频率限制
- **守护进程模式** — 后台运行 + PID 管理，或 `--foreground` 前台调试
- **Unix Socket 控制** — CLI 命令通过 Unix Socket 与运行中的守护进程通信
- **热重载配置** — `SIGHUP` 信号或 `config reload` 命令即时生效
- **XDG 路径规范** — 配置、Token、日志、PID、Socket 均遵循 XDG 目录规范

## 快速开始

### 1. 构建

```bash
cargo build --release
```

### 2. 认证

```bash
# GitHub Copilot（设备码流程 — 自动打开浏览器）
aiclient-api auth copilot

# Kiro / AWS Builder ID（交互式菜单）
aiclient-api auth kiro

# Kiro / IAM Identity Center（组织身份认证）
aiclient-api auth kiro --start-url https://my-org.awsapps.com/start --region us-east-1
```

### 3. 启动守护进程

```bash
# 后台模式（默认）
aiclient-api start

# 前台模式（调试用）
aiclient-api start --foreground

# 自定义端口和 API Key
aiclient-api start --port 8080 --api-key my-secret
```

### 4. 使用

```bash
# OpenAI 格式
curl http://127.0.0.1:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "copilot/gpt-4o",
    "messages": [{"role": "user", "content": "你好！"}]
  }'

# Anthropic 格式
curl http://127.0.0.1:9090/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "copilot/claude-sonnet-4",
    "messages": [{"role": "user", "content": "你好！"}],
    "max_tokens": 1024
  }'

# 使用 Kiro Provider
curl http://127.0.0.1:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "kiro/claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "你好！"}]
  }'

# 列出可用模型
curl http://127.0.0.1:9090/v1/models

# 查询使用统计
curl http://127.0.0.1:9090/v1/usage
```

## 使用量跟踪

实时监控你的 Token 消耗：

```bash
# 获取使用统计
curl http://127.0.0.1:9090/v1/usage | jq .

# 响应示例
{
  "providers": {
    "copilot": {
      "request_count": 10,
      "input_tokens": 150,
      "output_tokens": 200,
      "total_tokens": 350,
      "models": {
        "gpt-4o": { "request_count": 5, "input_tokens": 80, ... },
        "claude-sonnet-4.5": { "request_count": 5, "input_tokens": 70, ... }
      }
    },
    "kiro": { ... }
  },
  "total": {
    "request_count": 10,
    "input_tokens": 150,
    "output_tokens": 200,
    "total_tokens": 350
  }
}

# 重置统计数据
curl -X DELETE http://127.0.0.1:9090/v1/usage
```

**功能特点：**
- ✅ 按 Provider 和模型实时跟踪
- ✅ 分别统计输入/输出 tokens
- ✅ 聚合总计统计
- ⚠️ 内存存储（重启后清零）

详细文档参见 [USAGE_TRACKING.md](./USAGE_TRACKING.md)。

## 配置

配置文件路径：`~/.config/aiclient-api/config.toml`

```toml
default_format = "openai"       # "openai" 或 "anthropic"
default_provider = "copilot"    # 无前缀时的默认 Provider
api_key = ""                    # 留空 = 不需要认证

[server]
host = "127.0.0.1"
port = 9090
rate_limit_seconds = 0          # 0 = 不限流

[providers.copilot]
type = "copilot"
enabled = true
account_type = "individual"     # "individual"、"business"、"enterprise"

[providers.kiro]
type = "kiro"
enabled = true
region = "us-east-1"

[logging]
level = "info"
```

## CLI 命令

| 命令 | 说明 |
|------|------|
| `aiclient-api start` | 启动守护进程 |
| `aiclient-api stop` | 停止守护进程 |
| `aiclient-api restart` | 重启守护进程 |
| `aiclient-api status` | 查看守护进程状态和 Provider 健康度 |
| `aiclient-api auth copilot` | GitHub Copilot 认证 |
| `aiclient-api auth kiro` | Kiro 认证（交互式） |
| `aiclient-api auth kiro --start-url <url> --region <region>` | Kiro IAM Identity Center 认证 |
| `aiclient-api auth list` | 列出已认证的 Provider |
| `aiclient-api auth revoke <provider>` | 撤销指定 Provider 的 Token |
| `aiclient-api models` | 列出所有 Provider 的可用模型 |
| `aiclient-api config init` | 交互式配置向导 |
| `aiclient-api config show` | 显示当前配置 |
| `aiclient-api config reload` | 重新加载配置文件 |
| `aiclient-api provider enable <name>` | 启用 Provider |
| `aiclient-api provider disable <name>` | 禁用 Provider |
| `aiclient-api logs` | 查看守护进程日志 |
| `aiclient-api logs --level error` | 按日志级别过滤 |

## 模型路由

模型按以下优先级路由：

1. **模型前缀** — `copilot/gpt-4o` 路由到 `copilot` Provider，使用模型 `gpt-4o`
2. **X-Provider 请求头** — `X-Provider: kiro` 路由到 `kiro` Provider
3. **配置默认值** — 回退到配置文件中的 `default_provider`

## API 端点

| 端点 | 方法 | 格式 | 说明 |
|------|------|------|------|
| `/healthz` | GET | — | 健康检查（无需认证） |
| `/v1/chat/completions` | POST | OpenAI | Chat 补全 |
| `/v1/models` | GET | OpenAI | 模型列表 |
| `/v1/messages` | POST | Anthropic | Messages API |
| `/v1/usage` | GET | JSON | 获取使用统计 |
| `/v1/usage` | DELETE | JSON | 重置使用统计 |

## 项目结构

```
src/
  auth/           # Token 管理
    copilot.rs    # GitHub 设备码 OAuth 流程
    kiro.rs       # Builder ID + 社交认证 (PKCE)
    token_store.rs
  cli/            # CLI 命令
  config/         # TOML 配置加载
  convert/        # OpenAI <-> Anthropic 格式转换
    stream.rs     # SSE chunk 转换
  daemon/         # 进程管理 + Unix Socket 控制服务
  providers/      # Provider 实现
    copilot/      # GitHub Copilot（VSCode 请求头伪装）
    kiro/         # Kiro / CodeWhisperer API
    router.rs     # 基于模型名的 Provider 路由
  routes/         # HTTP 路由处理器
    usage.rs      # 使用量跟踪端点
  server/         # Axum 服务器、中间件（认证、限流、CORS）
  usage/          # Token 使用量跟踪
    tracker.rs    # UsageTracker 实现
  util/           # XDG 路径、错误类型、流式辅助工具
```

## 文件位置

| 文件 | 路径 |
|------|------|
| 配置文件 | `~/.config/aiclient-api/config.toml` |
| Copilot Token | `~/.config/aiclient-api/copilot/token.json` |
| Kiro Token | `~/.config/aiclient-api/kiro/token.json` |
| PID 文件 | `~/.local/state/aiclient-api/daemon.pid` |
| Unix Socket | `~/.local/state/aiclient-api/daemon.sock` |
| 日志文件 | `~/.local/state/aiclient-api/daemon.log` |

## License

MIT

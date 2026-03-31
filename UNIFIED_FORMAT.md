# 统一输出格式配置

## 概述

aiclient-api 现在支持统一的输出格式配置。无论使用哪个 provider (Kiro 或 Copilot)，也无论使用哪个端点 (`/v1/messages` 或 `/v1/chat/completions`)，都可以通过配置强制返回指定的输出格式（Anthropic 或 OpenAI）。

## 配置方式

### 1. 全局配置（config.toml）

在配置文件中设置 `default_format`：

```toml
# config.toml
default_format = "anthropic"    # "anthropic" | "openai"
```

- `"anthropic"`: 所有响应都转换为 Anthropic Messages API 格式
- `"openai"`: 所有响应都转换为 OpenAI Chat Completions API 格式

### 2. 请求头覆盖

可以通过 `x-output-format` 请求头临时覆盖全局配置：

```bash
curl -X POST http://localhost:9090/v1/messages \
  -H "x-output-format: openai" \
  ...
```

优先级：**请求头 > 全局配置**

## 使用示例

### 场景 1: 配置 OpenAI 格式，使用 Anthropic 端点

**配置：**
```toml
default_format = "openai"
```

**请求（Anthropic 端点）：**
```bash
curl -X POST http://localhost:9090/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: test-key" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "kiro/claude-sonnet-4-6",
    "max_tokens": 50,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

**响应（OpenAI 格式）：**
```json
{
  "id": "chatcmpl-xxx",
  "object": "chat.completion",
  "model": "kiro/claude-sonnet-4-6",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Hello! How can I help you?"
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 20,
    "total_tokens": 30
  }
}
```

### 场景 2: 配置 Anthropic 格式，使用 OpenAI 端点

**配置：**
```toml
default_format = "anthropic"
```

**请求（OpenAI 端点）：**
```bash
curl -X POST http://localhost:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer test-key" \
  -H "x-provider: kiro" \
  -d '{
    "model": "claude-sonnet-4-6",
    "max_tokens": 50,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

**响应（Anthropic 格式）：**
```json
{
  "id": "msg_xxx",
  "type": "message",
  "role": "assistant",
  "model": "claude-sonnet-4-6",
  "content": [{
    "type": "text",
    "text": "Hello! How can I help you?"
  }],
  "stop_reason": "end_turn",
  "usage": {
    "input_tokens": 10,
    "output_tokens": 20
  }
}
```

### 场景 3: 使用请求头覆盖配置

**配置：**
```toml
default_format = "openai"
```

**请求（带 x-output-format 头）：**
```bash
curl -X POST http://localhost:9090/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: test-key" \
  -H "anthropic-version: 2023-06-01" \
  -H "x-output-format: anthropic" \
  -d '{
    "model": "kiro/claude-sonnet-4-6",
    "max_tokens": 50,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

**响应（Anthropic 格式，覆盖了全局 OpenAI 配置）：**
```json
{
  "id": "msg_xxx",
  "type": "message",
  "role": "assistant",
  ...
}
```

## 支持的 Provider

### ✅ Kiro (AWS CodeWhisperer)
- 支持格式转换
- 两种端点都可用
- 两种输出格式都支持

### ✅ GitHub Copilot
- 支持格式转换
- 推荐使用 OpenAI 端点
- 两种输出格式都支持

## 自动格式转换

服务会自动处理以下转换：

1. **Anthropic → OpenAI**: 将 Anthropic Messages API 格式转换为 OpenAI Chat Completions 格式
2. **OpenAI → Anthropic**: 将 OpenAI Chat Completions 格式转换为 Anthropic Messages API 格式

转换包括：
- 消息格式
- 角色映射
- 完成原因
- Token 使用统计
- 流式响应格式

## 最佳实践

1. **统一客户端代码**: 配置统一的输出格式后，客户端代码只需要处理一种响应格式
2. **灵活性**: 对于需要特定格式的特殊请求，使用 `x-output-format` 头
3. **调试**: 使用不同的格式可以帮助调试和对比不同 AI 服务的行为

## 注意事项

- 流式响应也支持格式转换
- 某些 provider 特有的字段可能在转换中丢失
- 建议测试你的具体使用场景以确保转换符合预期

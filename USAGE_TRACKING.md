# Usage Tracking (额度查询功能)

`aiclient-api` 现在支持实时跟踪 AI 服务使用量（token 消耗统计）。

## 功能特点

- ✅ **实时使用量跟踪** - 自动记录每个请求的 token 使用量
- ✅ **按 Provider 分组统计** - 分别统计 Copilot 和 Kiro 的使用量
- ✅ **按模型分组统计** - 每个 provider 下按模型细分使用量
- ✅ **聚合统计** - 提供总计使用量
- ✅ **RESTful API** - 通过 HTTP 端点查询和重置统计数据

## API 端点

### 查询使用统计

```bash
GET /v1/usage
```

**响应示例：**

```json
{
  "providers": {
    "copilot": {
      "request_count": 2,
      "input_tokens": 21,
      "output_tokens": 25,
      "total_tokens": 46,
      "models": {
        "gpt-4o": {
          "request_count": 1,
          "input_tokens": 13,
          "output_tokens": 20,
          "total_tokens": 33
        },
        "claude-sonnet-4.5": {
          "request_count": 1,
          "input_tokens": 8,
          "output_tokens": 5,
          "total_tokens": 13
        }
      }
    },
    "kiro": {
      "request_count": 1,
      "input_tokens": 9,
      "output_tokens": 10,
      "total_tokens": 19,
      "models": {
        "claude-sonnet-4.5": {
          "request_count": 1,
          "input_tokens": 9,
          "output_tokens": 10,
          "total_tokens": 19
        }
      }
    }
  },
  "total": {
    "request_count": 3,
    "input_tokens": 30,
    "output_tokens": 35,
    "total_tokens": 65
  }
}
```

### 重置使用统计

```bash
DELETE /v1/usage
```

**响应示例：**

```json
{
  "message": "Usage statistics reset successfully"
}
```

## 使用示例

### 1. 查询当前使用量

```bash
curl http://localhost:9090/v1/usage | jq .
```

### 2. 查询特定 provider 的使用量

```bash
curl http://localhost:9090/v1/usage | jq '.providers.copilot'
```

### 3. 查询总使用量

```bash
curl http://localhost:9090/v1/usage | jq '.total'
```

### 4. 提取特定指标

```bash
# 总请求数
curl -s http://localhost:9090/v1/usage | jq '.total.request_count'

# 总消耗 tokens
curl -s http://localhost:9090/v1/usage | jq '.total.total_tokens'

# Copilot 的输入 tokens
curl -s http://localhost:9090/v1/usage | jq '.providers.copilot.input_tokens'
```

### 5. 重置统计

```bash
curl -X DELETE http://localhost:9090/v1/usage
```

## 数据说明

### 统计字段

- **request_count** - 请求总数
- **input_tokens** - 输入/提示 tokens 总数（对应 prompt_tokens）
- **output_tokens** - 输出/完成 tokens 总数（对应 completion_tokens）
- **total_tokens** - 总 tokens 数（input + output）

### Provider 层级

统计数据按以下层级组织：

```
providers
├── copilot
│   ├── request_count
│   ├── input_tokens
│   ├── output_tokens
│   ├── total_tokens
│   └── models
│       ├── gpt-4o
│       ├── claude-sonnet-4.5
│       └── ...
├── kiro
│   └── ...
└── total (所有 providers 的聚合)
```

## 实现细节

### 自动记录

使用量在每次 API 请求完成后自动记录，支持：

- ✅ OpenAI Chat Completions 端点 (`/v1/chat/completions`)
- ✅ Anthropic Messages 端点 (`/v1/messages`)
- ✅ 非流式响应（完全支持）
- ⚠️ 流式响应（暂不支持 - 未来版本）

### Provider 支持

| Provider | 使用量跟踪 | 说明 |
|----------|-----------|------|
| **GitHub Copilot** | ✅ 完整支持 | 从响应的 `usage` 字段提取 |
| **Kiro (AWS CodeWhisperer)** | ⚠️ 部分支持 | 基于 MeteringEvent 估算（AWS 可能不提供详细 usage）|

### 数据持久化

**当前版本：** 使用量数据仅保存在内存中
- ✅ 实时查询快速
- ⚠️ 服务重启后数据清零
- ⚠️ 不支持历史数据

**未来改进：**
- 持久化到 SQLite 数据库
- 按时间段统计（每日/每周/每月）
- 导出历史数据
- 使用趋势图表

## 注意事项

1. **内存统计** - 当前版本的使用统计只保存在内存中，服务重启后会清零
2. **流式响应** - 流式响应暂不记录使用量（需要在流结束时统计）
3. **Kiro 精度** - Kiro/AWS CodeWhisperer 的使用量统计可能不如 Copilot 精确（取决于 AWS API 返回的 metering 数据）
4. **认证保护** - `/v1/usage` 端点受到与其他 API 相同的认证和速率限制保护

## 相关文件

- `/src/usage/mod.rs` - 使用量跟踪模块
- `/src/usage/tracker.rs` - UsageTracker 实现
- `/src/routes/usage.rs` - HTTP 端点实现
- `/src/routes/openai.rs` - OpenAI 端点集成
- `/src/routes/anthropic.rs` - Anthropic 端点集成
- `/src/server/state.rs` - AppState 集成

## 更新日志

**2026-03-31**
- ✅ 实现 `UsageTracker` 模块
- ✅ 添加 `GET /v1/usage` 端点
- ✅ 添加 `DELETE /v1/usage` 端点
- ✅ 集成到 OpenAI 和 Anthropic 路由
- ✅ 支持 passthrough 和 conversion 路径
- ✅ 按 provider 和 model 分组统计

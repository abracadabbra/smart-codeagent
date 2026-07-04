# Phase 1.1: 切换 LLM Provider 到 SenseNova（deepseek-v4-flash）

## 背景

Phase 1 已跑通 Anthropic Messages API SSE 流式链路，但项目方希望切换到商汤日日新
SenseNova 平台，使用托管的 `deepseek-v4-flash` 模型。

**好消息**：SenseNova `/v1/messages` 与 Anthropic Messages API 协议**完全兼容**：
- 鉴权：OpenAI 兼容接口与 Messages 接口**共用** API Key，使用 `Authorization: Bearer`
- Body 字段：`model` / `messages` / `max_tokens` / `system` / `stream` 全部兼容
- SSE 事件序列：`message_start` → `content_block_start` → `content_block_delta` →
  `content_block_stop` → `message_delta` → `message_stop`，与 Anthropic 一致

实测 curl 已通过：`curl https://token.sensenova.cn/v1/messages` 返回 200，body 正常。

## 目标

1. 把默认 Provider 切到 SenseNova + `deepseek-v4-flash`
2. **协议层零改动**（复用现有 `AnthropicClient`），只换鉴权头和 base_url
3. 修复 `loop_.rs` 缺 `use crate::providers::Provider` 的旧编译错误（顺手）

## AC（验收标准）

- **AC1** `.env` 用通用变量 `LLM_API_KEY` / `LLM_BASE_URL` / `LLM_MODEL`；
  默认 `https://token.sensenova.cn` / `deepseek-v4-flash`。
  `.env.example` 同步更新，并附上获取 Key 的链接。
- **AC2** `src-tauri/src/config.rs` 中 `AnthropicConfig::from_env()` 改为读 `LLM_*`
  变量；类型名暂保留 `AnthropicConfig`（兼容 Anthropic Messages API 协议），
  文档注释更新为 "Anthropic 兼容协议 / SenseNova 默认"。
- **AC3** `src-tauri/src/providers/anthropic.rs` 把
  `x-api-key` + `anthropic-version` 头替换为 `Authorization: Bearer ${api_key}`。
- **AC4** `src-tauri/src/agent/loop_.rs` 补 `use crate::providers::Provider;`
  解决当前编译错误（`stream_chat` 在 trait 中，未 import 找不到方法）。
- **AC5** `cargo check --manifest-path src-tauri/Cargo.toml` 通过，零 error。
- **AC6** `curl -N` 跑通流式：
  ```
  curl -N https://token.sensenova.cn/v1/messages \
    -H "Authorization: Bearer $LLM_API_KEY" \
    -H "Content-Type: application/json" \
    -d '{"model":"deepseek-v4-flash","max_tokens":128,"stream":true,"messages":[{"role":"user","content":"hi"}]}'
  ```
  能看到 `content_block_delta` 事件序列。
- **AC7**（可选，手测）`npm run tauri dev` 启动后输入 "hi"，UI 流式显示回复，
  401/网络错误走 `agent:error` 事件并在前端可见。

## 改动文件

| 文件 | 改动 |
|---|---|
| `.env.example` | 变量名换 LLM_*, 附 SenseNova Key 链接 |
| `.env` | 同上（Key 由用户在控制台填入，本任务只填占位，提示用户轮换） |
| `src-tauri/src/config.rs` | env 变量名 + 默认值 + 注释 |
| `src-tauri/src/providers/anthropic.rs` | 鉴权头换 Bearer |
| `src-tauri/src/providers/mod.rs` | 注释措辞调整 |
| `src-tauri/src/agent/loop_.rs` | 加 `use crate::providers::Provider` |

## Out of Scope（不在本任务）

- 工具系统（Phase 2）
- Session 隔离、多 Provider 选择 UI
- 重新命名 `AnthropicClient` / `AnthropicConfig`（协议仍是 Anthropic Messages，
  只是 base_url 指向 SenseNova，保留名字避免 PR diff 过大）

## 影响分析（前置 fix-impact）

- 直接调用方：`commands::send_message`、`AgentLoop::run_inner` 都通过 `Provider` trait
  抽象，无破坏
- 数据结构：`MessagesRequest` / `TokenStream` / `Message` / `AgentState` 全部不变
- 前端：`useAgentEvents` 监听 `agent:token/status/error/done` 事件名不变
- 错误路径：`ProviderError::Api { status, message }` 已覆盖 401/网络错
- 工具调用：`deepseek-v4-flash` 支持 `tools` + `json_mode` + `reasoning`，
  Phase 2 工具系统无需改
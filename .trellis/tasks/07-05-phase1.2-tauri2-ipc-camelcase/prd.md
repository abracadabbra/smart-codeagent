# Phase 1.2: 修复 Tauri 2 IPC 参数名不匹配（camelCase + 去除 args wrapper）

## 背景

Phase 1 + 1.1 完成后，用户在桌面应用里输入消息"无反应"。诊断发现两个互相叠加的 IPC bug：

### Bug 1: invoke 参数外层 wrapper 错误

前端（`src/hooks/useAgentEvents.ts:106`）：
```ts
await invoke("send_message", { args: { text, assistantId } });
```

Rust 端（`src-tauri/src/ipc/commands.rs`）：
```rust
pub async fn send_message(text: String, assistant_id: String) -> Result<(), String>
```

Tauri 2 把 `invoke` payload 的**顶层 key** 直接映射到 Rust 函数参数，**不再支持 `args` wrapper**（Tauri 1 才有）。
→ `text` / `assistant_id` 在 Rust 端都是 `undefined`，命令调用直接失败。

### Bug 2: 字段命名风格不匹配

Rust 端：
- 命令参数：`assistant_id`（snake_case）
- 事件 payload 字段：`msg_id`（snake_case，见 `src-tauri/src/ipc/events.rs`）

前端（`src/hooks/useAgentEvents.ts`）：
- 调用：`assistantId`（camelCase）
- 事件 payload 类型：`msgId`（camelCase）

Tauri 2 **默认按字段原名匹配**，不做 snake↔camel 转换。
→ 即使 Bug 1 修好，Rust 事件 payload 的 `msg_id` 在前端 JS 里也是 `msg_id`，
不是 `msgId`。前端 `appendToken(p.msgId, ...)` 拿到 `undefined`，
匹配不到 assistant message，token 永远 append 不上。

## 目标

按 Tauri 2 官方推荐的 **camelCase 协议**统一前后端字段命名，
并去掉 `args` wrapper。

## AC（验收标准）

- **AC1** `src-tauri/src/ipc/events.rs`：四个 payload struct
  (`AgentTokenPayload` / `AgentStatusPayload` / `AgentErrorPayload` / `AgentDonePayload`)
  全部加 `#[serde(rename_all = "camelCase")]`。
- **AC2** `src-tauri/src/agent/mod.rs`：`Message` 结构
  (前后端消息载体，含 `role` / `content`) 加 `#[serde(rename_all = "camelCase")]`
  以保险起见（注：现有字段都是单词，可能不必要，但 `AgentState` 枚举已有 `PascalCase`
  保持一致即可）。
- **AC3** `src/hooks/useAgentEvents.ts:106`：
  `invoke("send_message", { args: { text, assistantId } })`
  → `invoke("send_message", { text, assistantId })`。
- **AC4** `src/types/message.ts`：TS 端 `Message.role` 保持 `"user" | "assistant"`
  小写不变（语义层），与 Rust `Message.role: String` 兼容。
- **AC5** `cargo check` 零 error 零 warning。
- **AC6** **手动 E2E 验证**（重要 — 这次必须跑，不能口头说"修好"）：
  - `npm run tauri dev` 启动桌面应用
  - 输入 "hi"，回车
  - 看到用户气泡 + 流式 assistant 回复（不再是空白/卡死）
  - devtools console 无 `JSON deserialization error` / `command not found` 类报错
  - 多轮对话：第一轮结束后继续追问，能在前文基础上回答
  - 故意填错 `LLM_API_KEY` 触发 401，前端 status bar / chat 中能看到 error 提示

## 改动文件

| 文件 | 改动 |
|---|---|
| `src-tauri/src/ipc/events.rs` | 四个 payload 加 `rename_all = "camelCase"` |
| `src-tauri/src/agent/mod.rs` | `Message` 加 `rename_all = "camelCase"`（保险） |
| `src/hooks/useAgentEvents.ts` | `invoke` payload 去 `args` wrapper |

## Out of Scope

- 不动 `AgentState` 枚举（已经是 PascalCase，前端类型也是）
- 不动 `assistantId` / `msgId` 前端字段名（已经是 camelCase，刚好对得上）
- 不引入新依赖

## 影响分析（fix-impact）

- 直接调用方：仅前端 `sendMessage` 一个调用点 + 后端 `commands::send_message` 一个入口
- 数据结构：事件 payload 序列化字段名变化，但前端 TS 类型已是 camelCase，
  类型层零改动
- 错误路径：`ProviderError::Api { status, message }` 仍按原结构返回，emit 出去的
  payload 也会自动变 camelCase；前端 `markError(p.msgId, p.message)` 不变
- 多轮对话：`history.push(Message { role, content })` 不变；`Message` 即使加
  `rename_all = "camelCase"` 也不影响单字段结构
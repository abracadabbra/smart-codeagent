# Phase 1: 项目脚手架 + 核心循环

## Goal

跑通端到端的最小链路：在桌面应用里输入文本 → Anthropic Claude 通过 SSE 流式回复 → 三栏 Qoder 风格 UI 中逐 token 渲染。

成功标志：一句话不够，必须能进入多轮对话（第一轮回复结束后用户继续追问，LLM 在前文基础上继续回答）。

## Scope

### In

| 类别 | 内容 |
|---|---|
| 项目脚手架 | Tauri 2 + React 19 + Vite 6 + TypeScript + Tailwind 3 |
| 后端核心 | Agent Loop 4 状态骨架：Idle / Prepare / Stream / Stop |
| Provider | Anthropic Provider，单一 provider，Messages API + SSE 流式 |
| IPC | Tauri command `send_message`，4 个事件：`agent:token` / `agent:status` / `agent:error` / `agent:done` |
| 前端组件 | `ChatView` 三栏容器、`MessageBubble` 消息气泡（左用户 / 右助手）、`StreamingText` 流式渲染、`InputBar` 输入框 |
| 前端状态 | Zustand `chatStore`（消息列表 + 发送动作）、`agentStore`（agent 状态机） |
| 前端 hook | `useAgentEvents` 订阅 4 个 Tauri 事件 → 更新 store |
| API Key | 通过 `.env` 或环境变量 `ANTHROPIC_API_KEY` 注入，Rust 通过 `std::env::var` 读取 |
| UI 风格 | Qoder 三栏：左 Session 列表（占位）+ 中对话流 + 右文件预览区（占位）；深色主题；用户和助手消息都有气泡 |

### Out（明确不做，留给后续 phase）

- 工具调用（Read / Write / Bash）→ Phase 2
- 错误恢复（重试、退避、Context Trim）→ Phase 2
- 多 Provider（OpenAI / Gemini / DeepSeek）→ Phase 2
- 会话持久化 / 多 Session 切换 → Phase 3
- 设置面板（API Key 配置 UI）→ Phase 3，Phase 1 用 `.env`
- MCP 集成 → Phase 3
- shadcn/ui 引入 → Phase 4 打磨
- 文件 diff 展示、消息搜索、虚拟列表 → Phase 4

## UI 规范（Qoder 三栏）

```
┌───────────────────────────────────────────────────────────────┐
│ 自定义标题栏                                                   │
├──────────────┬─────────────────────────┬───────────────────────┤
│ Session 列表 │        对话区           │    文件预览 / 工具结果 │
│ (占位)       │  ┌──────────────────┐   │    (占位)             │
│              │  │ User bubble      │   │                       │
│ Phase 3 添加 │  │ User bubble      │   │ Phase 2 添加文件预览    │
│              │  │                  │   │                       │
│              │  │      Assistant   │   │                       │
│              │  │      bubble      │   │                       │
│              │  │      (streaming) │   │                       │
│              │  └──────────────────┘   │                       │
│              │                         │                       │
│              │  [ InputBar ........ ]  │                       │
│              │                         │                       │
├──────────────┴─────────────────────────┴───────────────────────┤
│ StatusBar（当前 AgentState）                                   │
└───────────────────────────────────────────────────────────────┘
```

样式决策：
- 主题：深色，Tailwind `bg-zinc-900` / `bg-zinc-800` / `text-zinc-100`
- 三栏宽度：左 240px，中 flex-1，右 320px（在 ChatView 容器用 grid 或 flex 实现）
- 用户气泡：右侧，`bg-blue-600/20`，`rounded-2xl`，`px-4 py-2`
- 助手气泡：左侧，`bg-zinc-800`，`rounded-2xl`，`px-4 py-2`
- 流式渲染：助手消息末尾闪烁光标（`▍` + CSS `@keyframes blink`）
- 代码块：`bg-zinc-950` `rounded-md` `p-3`，等宽字体

## 数据流（Phase 1 简化版）

```
[InputBar]
   │ invoke("send_message", { text })
   ▼
[Tauri Command: send_message]
   │ agent_loop.run(text)
   ▼
[AgentLoop.run]
   │
   ├── 1. state = Prepare
   │      推送 agent:status { state: "Prepare" }
   │      组装 messages: [{ role: "user", content: text }]
   │      组装 system prompt
   │
   ├── 2. state = Stream
   │      推送 agent:status { state: "Stream" }
   │      provider.stream_chat(messages) -> Stream<Item>
   │      for item in stream {
   │          match item {
   │              ContentBlockDelta(text) => emit agent:token { text }
   │              MessageStop           => break
   │              Error(e)              => emit agent:error { ... }; break
   │          }
   │      }
   │
   ├── 3. state = Stop
   │      推送 agent:status { state: "Stop" }
   │      推送 agent:done {}
   │
   └── 4. state = Idle
          （Loop 不主动退出，等待下一轮 send_message）

[React: useAgentEvents]
   │
   ├── agent:token   -> chatStore.appendToken(msgId, text)
   ├── agent:status  -> agentStore.setState(state)
   ├── agent:error   -> chatStore.markError(msgId, message)
   └── agent:done    -> chatStore.markComplete(msgId)
```

事件载荷（Phase 1 范围内）：

| 事件 | 载荷 | 触发时机 |
|---|---|---|
| `agent:token` | `{ msgId: string, text: string }` | LLM 每吐一个 text delta |
| `agent:status` | `{ state: "Prepare" \| "Stream" \| "Stop" \| "Idle" }` | 状态转移时 |
| `agent:error` | `{ msgId: string, message: string }` | LLM 错误或网络错误 |
| `agent:done` | `{ msgId: string }` | 一轮流式结束 |

Phase 1 内**不实现**的事件（虽然 PRD 里列了）：`agent:tool_call` / `agent:tool_result` / `user:cancel`（取消按钮也延后）。

## API Key 配置

- 项目根目录新建 `.env`（gitignored）
- 内容：`ANTHROPIC_API_KEY=sk-ant-xxx`
- Rust 通过 `std::env::var("ANTHROPIC_API_KEY")` 在 Anthropic Provider 构造时读取
- 启动时如果读取不到，直接 panic 并打印明确错误信息（不让 Loop 静默失败）

## Module 落地（Phase 1 子集）

按 PRD §4 的全量结构，Phase 1 只生成这些文件，其余文件先建空模块占位但不实现：

```
src-tauri/src/
├── main.rs                       # Tauri 入口
├── lib.rs                        # 模块导出
├── agent/
│   ├── mod.rs                    # 公共类型: AgentState, Message
│   ├── loop_.rs                  # 4 状态主循环
│   └── stream.rs                 # SSE 流式解析（仅 Anthropic）
├── providers/
│   ├── mod.rs                    # ProviderClient trait 骨架
│   └── anthropic.rs              # Anthropic SSE 实现
├── ipc/
│   ├── mod.rs
│   ├── commands.rs               # send_message command
│   └── events.rs                 # 4 个事件的 payload + emit helper
└── tools/mod.rs                  # 空模块占位（Phase 2 实现）

src/
├── main.tsx                      # React 入口
├── App.tsx                       # 根组件 → 渲染 ChatView
├── index.css                     # Tailwind + 全局样式
├── components/chat/
│   ├── ChatView.tsx              # 三栏布局
│   ├── MessageBubble.tsx
│   ├── StreamingText.tsx         # 流式 + 闪烁光标
│   └── InputBar.tsx
├── stores/
│   ├── chatStore.ts              # messages[], appendToken, markComplete
│   └── agentStore.ts             # state: AgentState
├── hooks/useAgentEvents.ts       # 订阅 4 个事件
└── types/
    ├── message.ts                # Message, Role
    └── agent.ts                  # AgentState type
```

## Acceptance Criteria

按顺序验证，每条都可独立测试：

- [ ] **AC1 脚手架**：`cargo tauri dev` 启动桌面窗口，看到占位 UI，无报错
- [ ] **AC2 三栏布局**：左 / 中 / 右三栏可见，左侧 Session 列表显示"占位 - Phase 3"，右侧显示"占位 - Phase 2"，中间是空白对话区
- [ ] **AC3 深色主题**：背景为深色，文字浅色，无明色块
- [ ] **AC4 输入**：在底部输入框打字并点击"发送"，用户消息以气泡形式出现在中间对话区右侧
- [ ] **AC5 调用 Claude**：输入实际提示后，后端日志显示 Anthropic API 请求被发出（200 响应）
- [ ] **AC6 流式输出**：助手回复以 token 粒度逐字出现在中间对话区左侧，气泡末尾有闪烁光标
- [ ] **AC7 完成信号**：助手回复结束后，气泡停止光标闪烁，inputbar 重新可输入
- [ ] **AC8 多轮对话**：在第一轮结束后，立即追问一个新问题，能看到助手在两个气泡中分别回复，上下文连续
- [ ] **AC9 错误展示**：手动设置错误的 `ANTHROPIC_API_KEY`，重启后发送消息，UI 显示错误提示且不崩溃
- [ ] **AC10 状态栏**：底部状态栏在流式过程中显示"Streaming"，空闲时显示"Idle"
- [ ] **AC11 代码质量**：`cargo check` 通过、`cargo clippy` 无 warning、`npm run type-check` 通过、`npm run lint` 通过

## Constraints

| 约束 | 取值 |
|---|---|
| Rust edition | 2024 |
| Tauri | 2.x |
| React | 19.x |
| 状态管理 | Zustand 5.x |
| 样式 | Tailwind 3.x（暂不引入 shadcn） |
| LLM | 仅 Anthropic Claude（claude-sonnet-4-5 或 claude-sonnet-4 都可） |
| Key 来源 | `.env` / 环境变量 |
| 工具调用 | 不实现 |
| 多 Session | 不实现，单会话硬编码 |

## Risks & Open Questions

| 风险 | 缓解 |
|---|---|
| Anthropic SSE 解析（含 ping 帧、error 帧、心跳）需要小心 | Phase 1 先实现端到端 happy path；ping 帧目前忽略，超时只在断开时处理 |
| Tauri command 是同步阻塞 vs async 阻塞 | send_message 用 `tokio::spawn`，command 立即返回，避免阻塞 IPC |
| `StreamingText` 高频 token 重渲染导致掉帧 | React 用 ref 直接更新 DOM 而不通过 setState；Zustand 只在 token 累计到一定长度时 flush，Phase 1 不优化，先用 setState 跑通 |
| 右侧栏占位在 Phase 1 中是否显得空荡 | 接受空荡，右侧栏 Phase 2 才有内容；占位文字说明"Phase 2: 文件预览 / 工具结果" |

## Out of Scope（再次强调，防止任务膨胀）

绝对不要在 Phase 1 实现以下内容：

- 任何工具调用（Read/Write/Bash/Edit/Glob/Grep）
- 错误恢复机制（重试、退避、Context Trim）
- 任何 Provider 切换
- API Key 设置面板
- 会话持久化
- 会话切换
- MCP
- shadcn 组件库
- 文件 diff
- 消息搜索
- 取消按钮（user:cancel）
- 多窗口支持

## Definition of Done

满足 AC1 - AC11 全部条目，且满足以下两点：
- 所有源代码已 commit 到 git
- Trellis 任务可 archive

## Notes

- PRD 是全局方案书，Phase 1 的 PRD 是落地切片
- Phase 2 启动前，需要更新 PRD.md 添加 Phase 1 的实际踩坑记录（如有）
- 后续 Phase 的 PRD 也按此模式从全局 PRD 切片
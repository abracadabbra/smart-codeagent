# Smart CodeAgent - 项目方案书

## 1. 项目概述

### 1.1 目标

构建一个桌面端 AI Coding Agent，具备：

- 自然语言理解编程需求并自动执行
- 文件读写、代码编辑、命令执行等核心工具
- 流式输出 LLM 推理过程
- 可视化展示工具调用和结果
- 支持多 LLM Provider（Anthropic / OpenAI / Google / DeepSeek）
- 通过 MCP 协议扩展能力

### 1.2 技术栈

| 层 | 技术 | 用途 |
|---|---|---|
| 桌面框架 | Tauri 2 | 跨平台桌面壳子，Rust 后端 + Web 前端 |
| 后端语言 | Rust edition 2024 | Agent Loop、工具执行、Provider 集成 |
| 异步运行时 | tokio | 异步并发 |
| HTTP/SSE | reqwest | LLM API 调用 |
| 序列化 | serde / serde_json | IPC 与 API 数据交换 |
| 错误处理 | anyhow + thiserror | 结构化错误 |
| 前端框架 | React 19 | UI |
| 构建工具 | Vite 6 | 前端打包 |
| UI 组件 | shadcn/ui | 组件库 |
| 状态管理 | Zustand 5 | 前端状态 |
| 流式渲染 | Tauri Events | Rust 到 React 推流 |
| MCP | custom | 工具协议扩展 |

---

## 2. 架构总览

```
┌─────────────────────────────────────────────────────────┐
│                   桌面窗口 (Tauri 2)                     │
│                                                         │
│  ┌───────────── Frontend (React 19) ──────────────┐     │
│  │   ChatView    ToolCallLog    FilePreview        │     │
│  │   SettingPanel ProviderSelect  OutputConsole    │     │
│  └─────────────────┬──────────────────────────────┘     │
│                    │ Tauri IPC (事件总线)                │
│  ┌─────────────────▼──────────────────────────────┐     │
│  │              Backend (Rust)                     │     │
│  │                                                 │     │
│  │  ┌──────────────────────────────────────┐       │     │
│  │  │          Agent Loop                   │       │     │
│  │  │  ┌─────┐ ┌─────┐ ┌──────┐ ┌──────┐ │       │     │
│  │  │  │Idle │→│Wait │→│Stream│→│Exec  │ │       │     │
│  │  │  └─────┘ └─────┘ └──────┘ └──────┘ │       │     │
│  │  │         ↕        ↕                   │       │     │
│  │  │     ┌────────┐ ┌─────────┐          │       │     │
│  │  │     │Prepare │ │Recovery │          │       │     │
│  │  │     └────────┘ └─────────┘          │       │     │
│  │  └──────────────────────────────────────┘       │     │
│  │                                                 │     │
│  │  ┌────────┐ ┌──────────┐ ┌────────┐            │     │
│  │  │Tools   │ │Providers │ │MCP     │            │     │
│  │  │Executor│ │Clients   │ │Manager │            │     │
│  │  └────────┘ └──────────┘ └────────┘            │     │
│  └─────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────┘
```

---

## 3. Agent Loop 状态机

参考 Kivio 的 loop_.rs 6 状态设计，共计 8 个状态。

### 3.1 状态定义

```
AgentState 枚举:
├── Idle        — 初始/空闲，等待用户输入
├── Prepare     — 准备阶段：系统提示组装、工具列表注入、Context trim
├── Stream      — LLM 流式响应：逐 token 推送到前端
├── ToolCall    — LLM 请求工具调用：解析 tool_use block
├── Execute     — 执行工具：并发/超时/权限检查
├── Recover     — 错误恢复：分类匹配恢复策略
└── Stop        — 停止：资源清理、日志写入
```

### 3.2 状态转移

```
用户输入
   │
   ▼
┌────────┐    ┌─────────┐    ┌──────────┐    ┌──────────┐
│  Idle  │───▶│ Prepare │───▶│  Stream  │───▶│ ToolCall │
└────────┘    └─────────┘    └──────────┘    └──────────┘
                                                  │
                                          有无工具调用？
                                         ┌────┴────┐
                                         ▼         ▼
                                      有工具    无工具(回复)
                                         │         │
                                    ┌─────────┐   │
                                    │ Execute │   │
                                    └────┬────┘   │
                                   失败/超时│       │
                                         │         │
                                    ┌─────────┐    │
                                    │ Recover │    │
                                    └────┬────┘    │
                                      成功│        │
                                         │         │
                                         ▼         ▼
                                    ┌─────────┐
                                    │  Idle   │
                                    └─────────┘
```

---

## 4. 模块拆分

### 4.1 Rust 后端模块

```
src-tauri/src/
├── main.rs                   # Tauri 入口
├── lib.rs                    # 模块导出
│
├── agent/                    # Agent Loop 核心
│   ├── mod.rs                # 模块导出 + 公共类型
│   ├── loop_.rs              # 状态机主循环
│   ├── prepare.rs            # Context 准备 + 系统提示组装
│   ├── stream.rs             # SSE 流式解析
│   ├── execute.rs            # 工具调用执行
│   └── recovery.rs           # 错误恢复策略
│
├── tools/                    # 工具系统
│   ├── mod.rs                # 工具 trait + 注册
│   ├── read_file.rs          # 文件读取
│   ├── write_file.rs         # 文件写入
│   ├── edit_file.rs          # 精准编辑 (search/replace)
│   ├── run_bash.rs           # 命令执行
│   ├── glob_.rs              # 文件搜索
│   ├── grep_.rs              # 内容搜索
│   └── list_dir.rs           # 目录列表
│
├── providers/                # LLM 提供者
│   ├── mod.rs                # Provider trait
│   ├── anthropic.rs          # Anthropic API (SSE)
│   ├── openai.rs             # OpenAI 兼容 API
│   ├── google.rs             # Gemini API
│   └── deepseek.rs           # DeepSeek API
│
├── mcp/                      # MCP 协议
│   ├── mod.rs                # MCP 管理器
│   ├── transport.rs          # stdio / SSE 传输
│   └── protocol.rs           # 协议消息编解码
│
├── ipc/                      # Tauri IPC 通信
│   ├── mod.rs                # 事件定义
│   ├── commands.rs           # Tauri commands
│   └── events.rs             # 事件推送器
│
├── session/                  # 会话管理
│   ├── mod.rs
│   └── persistence.rs        # JSONL 持久化
│
└── config/                   # 配置
    ├── mod.rs
    └── settings.rs           # Provider/Key 管理
```

### 4.2 React 前端模块

```
src/
├── main.tsx                   # React 入口
├── App.tsx                    # 根组件
├── index.css                  # 全局样式
│
├── components/                # UI 组件
│   ├── chat/                  # 聊天区域
│   │   ├── ChatView.tsx       # 消息列表容器
│   │   ├── MessageBubble.tsx  # 单条消息气泡
│   │   ├── InputBar.tsx       # 输入框 + 提交
│   │   └── StreamingText.tsx  # 流式文本渲染
│   │
│   ├── tools/                 # 工具展示
│   │   ├── ToolCallCard.tsx   # 工具调用卡片
│   │   ├── ToolResult.tsx     # 工具结果展示
│   │   └── FileDiff.tsx      # 文件 diff 展示
│   │
│   ├── settings/              # 设置
│   │   ├── SettingsPanel.tsx  # 设置面板
│   │   └── ProviderConfig.tsx # API Key 配置
│   │
│   └── common/                # 公共
│       ├── StatusBar.tsx      # 状态栏
│       └── ErrorBoundary.tsx  # 错误边界
│
├── stores/                    # Zustand 状态
│   ├── chatStore.ts           # 消息和 session
│   ├── agentStore.ts          # Agent 状态
│   └── settingsStore.ts       # 设置持久化
│
├── hooks/                     # 自定义 hooks
│   ├── useAgentEvents.ts      # 订阅 Tauri 事件
│   └── useStreamingText.ts    # 流式文本 hook
│
└── types/                     # TypeScript 类型
    ├── message.ts             # 消息类型
    └── agent.ts               # Agent 状态类型
```

---

## 5. 数据流设计

### 5.1 核心数据流

```
用户点击发送
   │
   ▼
React: InputBar
   │ invoke("send_message", { text })
   ▼
Tauri IPC Command
   │
   ▼
Rust: commands::send_message
   │ agent_loop.run(text)
   ▼
Rust: Agent Loop (异步)
   │
   ├── prepare() → 组装系统提示
   ├── stream()  → 调 LLM API
   │   └── 推送 "agent:token" 事件 → React 逐字渲染
   ├── tool_call() → 识别工具调用
   ├── execute()  → 执行工具
   │   └── 推送 "agent:tool_result" 事件 → React 展示
   └── (回到 stream 继续)
   │
   ▼
Rust: 推送 "agent:done" 事件
   │
   ▼
React: 渲染完成，状态回到 Idle
```

### 5.2 Tauri 事件协议

| 事件名 | 方向 | 载荷 | 触发时机 |
|---|---|---|---|
| agent:token | Rust → React | text: string | LLM 每吐一个 token |
| agent:tool_call | Rust → React | tool, args, id | LLM 请求工具调用 |
| agent:tool_result | Rust → React | id, success, output | 工具执行完毕 |
| agent:status | Rust → React | state: AgentState | 状态转移 |
| agent:error | Rust → React | code, message | 发生错误 |
| agent:done | Rust → React | {} | 一轮交互完成 |
| user:cancel | React → Rust | {} | 用户点击取消 |

---

## 6. LLM Provider 抽象

### 6.1 Provider 协议类型

不按厂商区分，按协议类型区分：

```rust
pub enum ProviderProtocol {
    Anthropic,         // Anthropic Messages API (SSE streaming)
    OpenAICompatible,  // OpenAI / DeepSeek / Groq / Together
    Gemini,            // Google Gemini API
}

#[async_trait]
pub trait ProviderClient: Send + Sync {
    fn protocol(&self) -> ProviderProtocol;
    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
    ) -> Result<Box<dyn ProviderStream>>;
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
    ) -> Result<ProviderMessage>;
}
```

### 6.2 支持的 Provider 矩阵

| Provider | 协议 | 实测难度 | 备注 |
|---|---|---|---|
| Anthropic Claude | Anthropic | 低 | 原生 SSE 流 |
| OpenAI GPT-4o | OpenAICompatible | 低 | 标准 chat completions |
| DeepSeek | OpenAICompatible | 低 | 兼容 OpenAI 格式 |
| Google Gemini | Gemini | 中 | 协议不同，需单独实现 |
| Groq | OpenAICompatible | 低 | 兼容 OpenAI 格式 |
| Together AI | OpenAICompatible | 低 | 兼容 OpenAI 格式 |

---

## 7. 工具系统

### 7.1 工具 Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> serde_json::Value;
    fn requires_permission(&self) -> bool;
    async fn execute(&self, args: serde_json::Value) -> ToolResult;
}

pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}
```

### 7.2 核心工具清单

| 工具 | 敏感 | 用途 | 权限策略 |
|---|---|---|---|
| Read | 否 | 读取文件内容 | 仅 workspace 内 |
| Write | 是 | 写入/覆写文件 | 弹窗确认 |
| Edit | 是 | search/replace 精准编辑 | 弹窗确认 |
| Bash | 是 | 执行 shell 命令 | 弹窗确认 + 沙箱 |
| Glob | 否 | 文件名模式匹配 | 无限制 |
| Grep | 否 | 文件内容搜索 | 无限制 |
| ListDir | 否 | 列出目录 | 仅 workspace 内 |
| FileTree | 否 | 项目目录树 | 仅 workspace 内 |

### 7.3 权限分级

- 无感执行：Read / Glob / Grep / ListDir / FileTree
- 通知即可：Write（项目内文件）
- 弹窗确认：Write（项目外）、Edit
- 弹窗确认 + 沙箱：Bash

---

## 8. 错误恢复策略

### 8.1 失败类型 × 恢复策略

| 失败类型 | 恢复策略 | 默认动作 |
|---|---|---|
| RateLimited | RetryBackoff | 指数退避 + jitter，最多 5 次 |
| ContextOverflow | TrimAndRetry | 丢弃最旧 1/3 消息历史 |
| ToolFailed | ReportAndContinue | 将错误加入 tool_result，让 LLM 自行修复 |
| ToolTimeout | RetryOnce | 重试一次，超时时间翻倍 |
| ToolNotFound | ReportError | 返回错误信息给 LLM |
| AuthFailed | NotifyUser | 推送到前端展示设置面板 |
| NetworkError | RetryBackoff | 同 RateLimited |
| ParseError | TrimAndRetry | 清除最后一条 assistant 消息重试 |

---

## 9. MCP 扩展

### 9.1 架构

```
Agent Loop
    │
    ▼
MCP Manager
    │
    ├── stdio transport: 子进程 MCP server
    │   spawn("uvx", ["mcp-server-fetch"])
    │   stdin/stdout JSON-RPC
    │
    └── SSE transport: 远程 MCP server
        GET /sse + POST /message
        事件流
```

### 9.2 集成方式

```
Agent Loop 中的 tool_call
    │
    ├── 内置工具名匹配 → 直接执行
    └── 不匹配内置工具
        │
        ▼
    MCP Manager: 查询已注册的 MCP tools
        │
        ├── 命中 MCP tool → delegate 到 MCP transport
        └── 未命中 → 返回 ToolNotFound
```

---

## 10. 前端 UI 设计

### 10.1 布局

```
┌────────────────────────────────────────────┐
│   标题栏 (自定义)                            │
├─────────────────┬──────────────────────────┤
│   左侧面板       │    主内容区               │
│                  │                          │
│  对话列表        │  聊天消息流               │
│  Session 1      │  ┌────────────┐          │
│  Session 2      │  │ Message    │          │
│  Session 3      │  │ ToolCall   │          │
│                  │  │ Result     │          │
│  设置入口        │  └────────────┘          │
│                  │                          │
│                  │  输入框                  │
└─────────────────┴──────────────────────────┘
```

### 10.2 核心组件

| 组件 | 功能 | 状态 |
|---|---|---|
| MessageBubble | 用户/助手消息气泡 | 普通/流式/错误 |
| StreamingText | 逐 token 渲染 | 末尾闪烁光标 |
| ToolCallCard | 工具调用卡片 | pending/running/done/error |
| ToolResult | 结果展示 | 成功预览/失败原因 |
| FileDiff | 文件修改 diff | 增加/删除/变更高亮 |
| StatusBar | 底部状态栏 | 当前状态 + token 计数 |
| InputBar | 输入框 | 正常/禁用 |

---

## 11. 开发阶段

### Phase 1：项目脚手架 + 核心循环（2 周）

- create-tauri-app 初始化：Tauri 2 + React 19 + Vite + TypeScript
- Rust 后端核心：Agent Loop 状态机骨架 (Idle → Prepare → Stream → Stop)
- Anthropic Provider (SSE 流式)
- Read / Write / Bash 三个核心工具
- React 前端核心：ChatView + MessageBubble + StreamingText + InputBar
- Zustand chatStore
- IPC 打通：invoke("send_message") → Agent Loop → event("agent:token")
- React 侧 useAgentEvents hook
- 里程碑：输入文本 → 调 Claude → 流式输出 → 展示

### Phase 2：工具完善 + 错误恢复（2 周）

- 工具系统：Edit / Glob / Grep / ListDir + 工具注册表 + 权限分级
- 错误恢复：5 类失败分类 + 指数退避 + Context Trim
- 前端展示错误状态
- Provider 扩展：OpenAI 兼容协议 (DeepSeek / Groq) + Provider 选择 UI
- 里程碑：多工具调用链 + 自动错误恢复

### Phase 3：MCP + 会话管理（2 周）

- MCP 集成：stdio transport + MCP tool 注册 + 路由 + 前端展示状态
- 会话管理：多会话切换 + JSONL 持久化 + 历史消息懒加载
- 设置面板：API Key 管理 + Provider 配置 + Workspace 选择
- 里程碑：安装 MCP server、多会话切换

### Phase 4：打磨 + 发布（2 周）

- UI 打磨：暗色/亮色主题 + 文件 diff 展示 + 消息搜索 + 快捷键
- 性能优化：Context 窗口管理 + 消息虚拟列表 + Tauri 打包优化
- 打包发布：macOS .dmg + Windows .msi + 自动更新
- 里程碑：发布 v0.1 可用版本

---

## 12. 关键依赖

### Rust (Cargo.toml)

```toml
[dependencies]
tauri = { version = "2", features = ["devtools"] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
futures = "0.3"
tracing = "0.1"
tracing-subscriber = "0.3"
uuid = { version = "1", features = ["v4"] }
glob = "0.3"
ignore = "0.4"
similar = "2"
```

### React (package.json)

```json
{
  "dependencies": {
    "react": "^19",
    "react-dom": "^19",
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-shell": "^2",
    "@tauri-apps/plugin-dialog": "^2",
    "@tauri-apps/plugin-fs": "^2",
    "zustand": "^5",
    "react-markdown": "^9",
    "react-diff-viewer-continued": "^4",
    "lucide-react": "^0.400"
  },
  "devDependencies": {
    "@vitejs/plugin-react": "^4",
    "typescript": "^5.6",
    "vite": "^6",
    "tailwindcss": "^3",
    "autoprefixer": "^10",
    "postcss": "^8",
    "@tauri-apps/cli": "^2"
  }
}
```

---

## 13. 参考项目

| 项目 | 学什么 | 不学什么 |
|---|---|---|
| Kivio | Agent Loop 状态机、错误恢复、MCP 锁设计 | macOS AX、离线 Python、SCK |
| codeg | Snapshot+Replay 协议、Event Bridge 设计 | 多代理聚合、Codex CLI 委托 |
| desktop-cc-gui | 写安全策略、ABCD 4 阶段 loop | 商业逻辑 |
| Aider | Repo map 设计、Edit 工具格式 | Python 代码、CLI-only 形态 |
| OpenCode | 极简 CLI 架构、TUI 状态管理 | Go 语言、终端-only |

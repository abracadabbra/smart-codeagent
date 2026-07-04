# Smart CodeAgent

桌面端 AI Coding Agent，基于 Tauri 2 + Rust + React 19。详细方案见 [PRD.md](./PRD.md)。

## 当前进度

**Phase 1：项目脚手架 + 核心循环** — 进行中

跑通端到端最小链路：输入文本 → Claude 流式输出 → Qoder 风格三栏 UI 渲染。

## 前置要求

- Rust 1.85+（`rustup`）
- Node 20+
- Xcode Command Line Tools（macOS：`xcode-select --install`）
- Anthropic API Key

## 安装

```bash
# 安装 Rust（如未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 复制环境变量模板并填入 Key
cp .env.example .env
$EDITOR .env

# 安装前端依赖
npm install

# 启动桌面应用（Rust 首次编译需要几分钟）
npm run tauri dev
```

## 目录结构

```
src/                       # React 前端
  components/chat/         # 三栏 UI + 4 个核心组件
  stores/                  # Zustand stores
  hooks/                   # Tauri 事件订阅 hook
  types/                   # 类型定义

src-tauri/                # Rust 后端
  src/
    agent/                # 4 状态 Agent Loop
    providers/            # Anthropic SSE Provider
    ipc/                  # Tauri command + event 推送
    tools/                # 工具系统（Phase 2）
```

## 文档

- [PRD.md](./PRD.md) — 全局方案书
- `.trellis/tasks/07-04-phase1-scaffold-core-loop/prd.md` — Phase 1 详细 PRD
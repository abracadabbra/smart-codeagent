# Phase 3.3 Settings Panel - Technical Design

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Frontend                                │
│  ┌─────────────┐    ┌──────────────────┐    ┌───────────────┐  │
│  │ SessionList │───▶│ SettingsPanel    │◀───│ mcpStore      │  │
│  │ (Settings   │    │ (MCP server      │    │ (CRUD actions)│  │
│  │  Button)    │    │  list/form)      │    └───────────────┘  │
│  └─────────────┘    └────────┬─────────┘                       │
│                              │                                 │
│                              ▼                                 │
│                    ┌─────────────────┐                         │
│                    │  Tauri IPC       │                         │
│                    │  Commands        │                         │
│                    └────────┬─────────┘                         │
└─────────────────────────────┼───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Backend (Rust)                           │
│  ┌──────────────┐    ┌──────────────┐    ┌───────────────────┐  │
│  │ Settings     │───▶│ McpManager   │◀───│ IPC Commands     │  │
│  │ (save/reload │    │ (reconnect   │    │ (save_settings,  │  │
│  │  sanitize)   │    │  server)     │    │  reload_settings)│  │
│  └──────────────┘    └──────────────┘    └───────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│                    ┌─────────────────┐                          │
│                    │  settings.json  │                          │
│                    │  (disk storage) │                          │
│                    └─────────────────┘                          │
└─────────────────────────────────────────────────────────────────┘
```

## 2. Backend Changes

### 2.1 Settings Module (`settings.rs`)

Add methods for saving and reloading:

```rust
impl Settings {
    /// 写入 settings.json（创建目录 + 写文件）。
    pub fn save_to_disk(&self, app: &AppHandle) -> Result<(), String>;
    
    /// 从磁盘重新加载（用于热重载）。
    pub fn reload_from_disk(&mut self, app: &AppHandle);
}
```

### 2.2 McpManager Module (`manager.rs`)

Add method to reconnect servers based on new settings:

```rust
impl McpManager {
    /// 根据新 settings 重新连接所有 server（断开旧连接，连接新配置）。
    pub async fn reconnect_all(&self, settings: &Settings);
    
    /// 断开并移除指定 server。
    pub async fn disconnect_server(&self, server_id: &str);
    
    /// 测试单个 server 是否能正常连接（用于前端"测试连接"按钮）。
    pub async fn test_connection(&self, server: &ChatMcpServer) -> Result<(), String>;
}
```

### 2.3 IPC Commands (`commands.rs`)

新增命令：

| Command | Input | Output | Purpose |
|---------|-------|--------|---------|
| `save_settings` | `Settings` | `Result<(), String>` | 保存配置到磁盘 |
| `reload_settings` | 无 | `Result<Vec<ChatMcpServer>, String>` | 热重载配置并返回新列表 |
| `test_mcp_server` | `ChatMcpServer` | `Result<(), String>` | 测试单个 server 连接 |

## 3. Frontend Changes

### 3.1 State Management (`mcpStore.ts`)

扩展现有 store：

```typescript
interface McpStore {
    servers: ChatMcpServer[];
    serverStates: Record<string, McpServerState>;
    // 新增
    loading: boolean;
    saving: boolean;
    addServer: (server: ChatMcpServer) => Promise<void>;
    updateServer: (id: string, updates: Partial<ChatMcpServer>) => Promise<void>;
    deleteServer: (id: string) => Promise<void>;
    toggleServerEnabled: (id: string) => Promise<void>;
    saveSettings: () => Promise<void>;
    reloadSettings: () => Promise<void>;
    testServer: (server: ChatMcpServer) => Promise<void>;
}
```

### 3.2 Settings Panel Component (`SettingsPanel.tsx`)

**结构：**
- 左侧：分类导航（MCP Servers / LLM Provider）
- 右侧：内容区域
  - MCP Servers 页面：列表 + 添加按钮 + 导入/导出
  - 列表项：名称、状态图标、启用开关、操作按钮（编辑/删除/测试）

**样式：**
- 遵循现有深色主题（`bg-ink-900`, `border-ink-800`）
- 表单字段使用一致的 input 样式
- 模态框或侧边栏形式

### 3.3 McpServerForm Component (`McpServerForm.tsx`)

**表单字段：**
- `id` (required) - 唯一标识符
- `name` (required) - 显示名称
- `command` (required) - 启动命令
- `args` (optional) - 命令参数（多行或逗号分隔）
- `cwd` (optional) - 工作目录
- `enabled` (boolean) - 是否启用

### 3.4 SessionList Integration

在 SessionList 底部设置按钮添加点击事件，打开设置面板。

## 4. Data Flow

### 4.1 添加/编辑 Server

```
User fills form → McpServerForm.onSubmit
  ├── mcpStore.addServer(server) / mcpStore.updateServer(id, updates)
  ├── invoke('save_settings', { mcp: { servers: newList } })
  ├── invoke('reload_settings')
  ├── McpManager.reconnect_all(newSettings)
  ├── emit('mcp-server-state', ...)
  └── mcpStore 更新 serverStates
```

### 4.2 删除 Server

```
User clicks delete → mcpStore.deleteServer(id)
  ├── invoke('save_settings', { mcp: { servers: filteredList } })
  ├── invoke('reload_settings')
  ├── McpManager.disconnect_server(id)
  ├── emit('mcp-server-state', { serverId: id, state: { kind: 'disconnected' } })
  └── mcpStore 更新 serverStates
```

### 4.3 热重载

```
Frontend calls reloadSettings()
  ├── invoke('reload_settings')
  ├── Settings.reload_from_disk(app)
  ├── McpManager.reconnect_all(newSettings)
  ├── 返回新的 servers 列表
  └── mcpStore 同步更新
```

## 5. Rollback Plan

- 后端：删除新增的 IPC commands + 移除 Settings/McpManager 新增方法 → 回退到 Phase 3.1 状态
- 前端：删除 SettingsPanel/McpServerForm 组件 + 还原 mcpStore → 回退到 Phase 3.1 状态
- 配置文件：`settings.json` 不受影响，只是无法通过 UI 编辑

## 6. Testing

### 6.1 后端测试

- `settings.rs`：测试 `save_to_disk` / `reload_from_disk`
- `manager.rs`：测试 `reconnect_all` / `disconnect_server` / `test_connection`
- `commands.rs`：测试新 IPC commands

### 6.2 前端测试

- 手动测试：打开设置面板、添加/编辑/删除 server、测试连接、导入导出
- 端到端：配置文件修改后立即生效，StatusBar 状态更新

## 7. Dependencies

- 后端：已有 `Settings` / `ChatMcpServer` / `McpManager`，无需新增依赖
- 前端：已有 `mcpStore` / `ChatMcpServer` 类型，无需新增依赖

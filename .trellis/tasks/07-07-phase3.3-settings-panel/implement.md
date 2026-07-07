# Phase 3.3 Settings Panel - Implementation Plan

## 1. Overview

实现步骤分 6 个 round，每个 round 独立可验证：

1. 后端：Settings 模块添加 save/reload 方法
2. 后端：McpManager 添加 reconnect_all/disconnect_server/test_connection
3. 后端：新增 IPC commands（save_settings, reload_settings, test_mcp_server）
4. 前端：扩展 mcpStore 添加 CRUD actions
5. 前端：实现 McpServerForm 组件
6. 前端：实现 SettingsPanel 组件 + 集成到 SessionList

## 2. Round 1: Backend Settings Save/Reload

**Files:**
- `src-tauri/src/settings.rs`

**Changes:**
- 添加 `save_to_disk` 方法：写入 `app_data_dir/settings.json`
- 添加 `reload_from_disk` 方法：从磁盘重新加载并 sanitize

**Validation:**
```bash
cd src-tauri && cargo test --lib settings
```

## 3. Round 2: Backend McpManager Reconnect

**Files:**
- `src-tauri/src/mcp/manager.rs`

**Changes:**
- 添加 `reconnect_all`：断开所有现有连接，根据新 settings 重新连接
- 添加 `disconnect_server`：断开指定 server
- 添加 `test_connection`：测试单个 server 是否能正常握手

**Validation:**
```bash
cd src-tauri && cargo test --lib mcp::manager
```

## 4. Round 3: Backend IPC Commands

**Files:**
- `src-tauri/src/ipc/commands.rs`

**Changes:**
- 添加 `save_settings` 命令：接收 Settings 对象，保存到磁盘
- 添加 `reload_settings` 命令：重新加载并返回新列表


- 添加 `test_mcp_server` 命令：测试单个 server 连接

**Validation:**
```bash
cd src-tauri && cargo check
```

## 5. Round 4: Frontend mcpStore CRUD

**Files:**
- `src/stores/mcpStore.ts`

**Changes:**
- 添加 `loading` / `saving` 状态
- 添加 `addServer` / `updateServer` / `deleteServer` / `toggleServerEnabled`
- 添加 `saveSettings` / `reloadSettings` / `testServer`

**Validation:**
```bash
npm run typecheck
```

## 6. Round 5: Frontend McpServerForm

**Files:**
- `src/components/McpServerForm.tsx`

**Changes:**
- 实现表单组件：id, name, command, args, cwd, enabled
- 支持添加和编辑模式
- 验证逻辑：id 和 name 必填

**Validation:**
```bash
npm run typecheck
```

## 7. Round 6: Frontend SettingsPanel

**Files:**
- `src/components/SettingsPanel.tsx`
- `src/components/SessionList.tsx`

**Changes:**
- 实现 SettingsPanel 组件：导航 + MCP Servers 列表
- 集成 McpServerForm 用于添加/编辑
- 添加导入/导出功能
- 在 SessionList 设置按钮添加点击事件

**Validation:**
```bash
npm run typecheck
npm run tauri dev
```

## 8. Final Verification

1. 打开设置面板：点击 SessionList 底部设置按钮
2. 添加 MCP server：填写表单，点击保存
3. 验证连接：查看 StatusBar 是否显示 connected
4. 编辑/删除：验证修改即时生效
5. 导入/导出：验证配置文件正确读写

## 9. Rollback Points

- Round 1: `git reset --hard HEAD~1` 回退 settings.rs 改动
- Round 2: `git reset --hard HEAD~1` 回退 manager.rs 改动
- Round 3: `git reset --hard HEAD~1` 回退 commands.rs 改动
- Round 4: `git reset --hard HEAD~1` 回退 mcpStore.ts 改动
- Round 5: `git reset --hard HEAD~1` 回退 McpServerForm.tsx
- Round 6: `git reset --hard HEAD~1` 回退 SettingsPanel.tsx + SessionList.tsx

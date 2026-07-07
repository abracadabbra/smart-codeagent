# Phase 3.3 Settings Panel

## Goal

提供一个图形化设置面板，让用户无需手编 JSON 即可配置 MCP server、LLM provider API key 等关键配置。

## Requirements

### 1. MCP Server 管理

- **列出已配置的 MCP server**：显示 id、名称、启用状态、连接状态
- **添加 MCP server**：表单填写 id、名称、命令、参数，支持 env、cwd、enabledTools 可选字段
- **编辑 MCP server**：修改已有配置
- **删除 MCP server**：确认后移除
- **启用/禁用**：切换单个 server 的启用状态
- **测试连接**：验证命令是否能正常启动并握手

### 2. 配置持久化与热重载

- **保存配置**：修改后立即写入 `settings.json`
- **热重载**：无需重启应用，配置变更即时生效（MCP server 重新连接）
- **导入/导出**：支持导出当前配置为 JSON，导入 JSON 文件

### 3. LLM Provider 配置（预留）

- **API Key 管理**：输入/修改 LLM provider 的 API key
- **Provider 切换**：在不同 LLM provider 间切换
- **Key 安全**：显示时掩码，仅保存到本地文件

## Acceptance Criteria

- [ ] 设置面板可从 SessionList 底部设置按钮打开
- [ ] MCP server 列表显示所有配置项及连接状态
- [ ] 添加/编辑/删除 server 功能正常
- [ ] 启用/禁用切换后即时生效（重新连接/断开）
- [ ] 配置修改后立即保存到磁盘
- [ ] 无需重启应用即可应用新配置
- [ ] 设置面板样式与现有 UI 风格一致（深色主题）

## Notes

- 后端已有 `Settings` 和 `ChatMcpServer` 结构，只需添加保存和热重载逻辑
- 前端已有 `mcpStore` 和 `ChatMcpServer` 类型，可复用
- 参考 Kivio 的设置面板设计（`kivio_code/settings_panel.rs`）
- 配置文件路径：`~/Library/Application Support/com.shentao.smartcodeagent/settings.json`

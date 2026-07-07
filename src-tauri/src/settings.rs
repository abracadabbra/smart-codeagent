//! settings.json 加载层：MCP server 配置 + 未来扩展点。
//!
//! Phase 3.1 仅承载 MCP server 配置；Provider / API Key 仍走 .env（`config.rs`）。
//! 文件位置：`<app_data_dir>/settings.json`，macOS = `~/Library/Application Support/com.shentao.smartcodeagent/`。
//!
//! 加载策略（Q2 决策）：启动时读一次入内存。Phase 3.3 设置面板支持热重载——
//! 用户修改配置后调用 `reload_from_disk` + McpManager 重新连接，无需重启 app。
//!
//! 设计参考 Kivio `settings_loader.rs` + `settings.rs::ChatMcpServer`，砍掉了 OAuth /
//! connector / HTTP transport / headers / connector_id / auth 字段（Phase 3.1 stdio-only）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tracing::warn;

/// 顶层 settings.json 结构。Phase 3.1 仅含 `mcp` 字段；Phase 3.3 会加 provider / workspace 等。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub mcp: McpSettings,
}

/// MCP server 列表容器。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSettings {
    #[serde(default)]
    pub servers: Vec<ChatMcpServer>,
}

/// 单个 MCP server 配置（stdio transport）。
///
/// 对照 Kivio `settings.rs::ChatMcpServer`（line 602-623），砍掉了：
/// - `url` / `headers`（HTTP transport 才用，Phase 3.1 stdio-only）
/// - `connector_id` / `auth`（OAuth 连接器，Phase 3.1 不做）
///   保留了 `enabled_tools` 白名单语义（空 = 全启用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatMcpServer {
    /// 全局唯一 id。用于 `mcp__{id}__{tool_name}` 命名空间。
    pub id: String,
    /// 显示名（StatusBar / 未来设置面板用）。
    pub name: String,
    /// 是否启用。禁用的 server 不进 list_all_tools。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 传输方式。Phase 3.1 固定 "stdio"；保留字段防 future。
    #[serde(default = "default_stdio")]
    pub transport: String,
    /// stdio 启动命令（如 `npx` / `node` / `python`）。
    pub command: String,
    /// 命令参数。
    #[serde(default)]
    pub args: Vec<String>,
    /// 子进程环境变量。
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// 子进程工作目录。None = 继承父进程。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Tool 白名单（按 MCP tool.name 过滤）。空 = 全启用。
    #[serde(default)]
    pub enabled_tools: Vec<String>,
}

impl Default for ChatMcpServer {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            transport: "stdio".to_string(),
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            enabled_tools: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_stdio() -> String {
    "stdio".to_string()
}

impl Settings {
    /// 从 `<app_data_dir>/settings.json` 加载。文件不存在 / JSON 损坏 → 默认空配置（不 panic）。
    pub fn load_from_disk(app: &AppHandle) -> Self {
        let path = match app.path().app_data_dir() {
            Ok(dir) => dir.join("settings.json"),
            Err(err) => {
                warn!("app_data_dir 不可用，使用空 settings: {err}");
                return Self::default();
            }
        };
        Self::load_from_path(&path)
    }

    /// 从指定路径加载（unit test 用）。文件不存在 / JSON 损坏 → 默认空配置。
    pub fn load_from_path(path: &Path) -> Self {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    warn!("settings.json 读取失败 ({}): {err}", path.display());
                }
                return Self::default();
            }
        };
        Self::load_from_str(&raw)
    }

    /// 从 JSON 字符串加载（unit test 用）。JSON 损坏 → 默认空配置。
    pub fn load_from_str(raw: &str) -> Self {
        match serde_json::from_str::<Settings>(raw) {
            Ok(mut s) => {
                s.sanitize();
                s
            }
            Err(err) => {
                warn!("settings.json 解析失败，使用空配置: {err}");
                Self::default()
            }
        }
    }

    /// 规范化：去重 server id（保留第一个，其余 warn 跳过）。
    pub fn sanitize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let before = self.mcp.servers.len();
        self.mcp.servers.retain(|s| {
            if s.id.is_empty() {
                warn!("settings.json: 跳过空 id 的 server (name={})", s.name);
                return false;
            }
            if !seen.insert(s.id.clone()) {
                warn!("settings.json: 跳过重复 id 的 server: {}", s.id);
                return false;
            }
            true
        });
        let dropped = before - self.mcp.servers.len();
        if dropped > 0 {
            warn!("settings.json: sanitize 丢弃 {dropped} 个无效 server 条目");
        }
    }

    /// 返回 settings.json 应当写入的路径（生产代码用）。
    pub fn settings_path(app: &AppHandle) -> Option<PathBuf> {
        app.path().app_data_dir().ok().map(|d| d.join("settings.json"))
    }

    /// 写入 settings.json。先创建目录（确保存在），然后序列化写入。
    /// 返回 Err 表示路径获取失败或写入失败。
    pub fn save_to_disk(&self, app: &AppHandle) -> Result<(), String> {
        let path = match Self::settings_path(app) {
            Some(p) => p,
            None => return Err("app_data_dir 不可用".to_string()),
        };

        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return Err(format!("创建目录失败: {err}"));
            }
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化失败: {e}"))?;

        std::fs::write(&path, json)
            .map_err(|e| format!("写入 settings.json 失败 ({:?}): {e}", path))?;

        Ok(())
    }

    /// 从磁盘重新加载（用于热重载）。当前实例被覆盖为磁盘上的最新配置。
    pub fn reload_from_disk(&mut self, app: &AppHandle) {
        *self = Self::load_from_disk(app);
    }

    /// 从指定路径重新加载（unit test 用）。
    pub fn reload_from_path(&mut self, path: &Path) {
        *self = Self::load_from_path(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_mcp_server_deserialize_minimal() {
        let raw = r#"{
            "id": "fs",
            "name": "Filesystem",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
        }"#;
        let s: ChatMcpServer = serde_json::from_str(raw).unwrap();
        assert_eq!(s.id, "fs");
        assert_eq!(s.name, "Filesystem");
        assert_eq!(s.command, "npx");
        assert_eq!(s.args.len(), 3);
        assert!(s.enabled, "enabled 默认 true");
        assert_eq!(s.transport, "stdio", "transport 默认 stdio");
        assert!(s.env.is_empty());
        assert!(s.cwd.is_none());
        assert!(s.enabled_tools.is_empty());
    }

    #[test]
    fn chat_mcp_server_deserialize_with_optional_fields() {
        let raw = r#"{
            "id": "github",
            "name": "GitHub",
            "enabled": false,
            "transport": "stdio",
            "command": "npx",
            "args": [],
            "env": { "GITHUB_TOKEN": "ghp_xxx" },
            "cwd": "/tmp/work",
            "enabledTools": ["create_issue", "list_issues"]
        }"#;
        let s: ChatMcpServer = serde_json::from_str(raw).unwrap();
        assert_eq!(s.id, "github");
        assert!(!s.enabled);
        assert_eq!(s.env.get("GITHUB_TOKEN").unwrap(), "ghp_xxx");
        assert_eq!(s.cwd.as_deref(), Some("/tmp/work"));
        assert_eq!(s.enabled_tools, vec!["create_issue", "list_issues"]);
    }

    #[test]
    fn chat_mcp_server_serialize_camel_case() {
        let s = ChatMcpServer {
            id: "x".into(),
            name: "X".into(),
            enabled: true,
            transport: "stdio".into(),
            command: "echo".into(),
            args: vec!["hi".into()],
            env: HashMap::new(),
            cwd: Some("/tmp".into()),
            enabled_tools: vec!["t1".into()],
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["id"], "x");
        assert_eq!(v["enabledTools"], serde_json::json!(["t1"]));
        assert_eq!(v["transport"], "stdio");
        // snake_case 字段不应出现在 wire 上
        assert!(v.get("enabled_tools").is_none());
    }

    #[test]
    fn settings_deserialize_with_servers() {
        let raw = r#"{
            "mcp": {
                "servers": [
                    { "id": "a", "name": "A", "command": "echo" },
                    { "id": "b", "name": "B", "command": "ls" }
                ]
            }
        }"#;
        let s: Settings = serde_json::from_str(raw).unwrap();
        assert_eq!(s.mcp.servers.len(), 2);
        assert_eq!(s.mcp.servers[0].id, "a");
        assert_eq!(s.mcp.servers[1].id, "b");
    }

    #[test]
    fn settings_default_empty() {
        let s = Settings::default();
        assert!(s.mcp.servers.is_empty());
    }

    #[test]
    fn load_from_str_minimal() {
        let raw = r#"{
            "mcp": {
                "servers": [
                    { "id": "fs", "name": "FS", "command": "npx" }
                ]
            }
        }"#;
        let s = Settings::load_from_str(raw);
        assert_eq!(s.mcp.servers.len(), 1);
        assert_eq!(s.mcp.servers[0].id, "fs");
    }

    #[test]
    fn load_from_str_malformed_returns_default() {
        let s = Settings::load_from_str("{ not valid json ");
        assert!(s.mcp.servers.is_empty(), "malformed JSON 应返回默认空配置");
    }

    #[test]
    fn load_from_str_missing_mcp_field() {
        let s = Settings::load_from_str("{}");
        assert!(s.mcp.servers.is_empty(), "缺 mcp 字段应返回默认空配置");
    }

    #[test]
    fn load_from_path_missing_file_returns_default() {
        let path = std::env::temp_dir().join("smart-codeagent-test-no-such-file.json");
        let s = Settings::load_from_path(&path);
        assert!(s.mcp.servers.is_empty());
    }

    #[test]
    fn load_from_path_valid_file() {
        let dir = std::env::temp_dir().join(format!("smart-codeagent-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{
                "mcp": {
                    "servers": [
                        { "id": "fs", "name": "FS", "command": "npx", "args": ["-y", "fs-mcp"] }
                    ]
                }
            }"#,
        )
        .expect("write settings");

        let s = Settings::load_from_path(&path);
        assert_eq!(s.mcp.servers.len(), 1);
        assert_eq!(s.mcp.servers[0].id, "fs");
        assert_eq!(s.mcp.servers[0].args, vec!["-y", "fs-mcp"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_from_path_corrupt_file_returns_default() {
        let dir = std::env::temp_dir().join(format!("smart-codeagent-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ broken json ").expect("write garbage");

        let s = Settings::load_from_path(&path);
        assert!(s.mcp.servers.is_empty(), "损坏 JSON 应返回默认空配置");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_drops_duplicate_ids() {
        let raw = r#"{
            "mcp": {
                "servers": [
                    { "id": "fs", "name": "First", "command": "a" },
                    { "id": "fs", "name": "Dup", "command": "b" },
                    { "id": "git", "name": "Git", "command": "c" }
                ]
            }
        }"#;
        let s = Settings::load_from_str(raw);
        assert_eq!(s.mcp.servers.len(), 2, "重复 id 应保留第一个");
        assert_eq!(s.mcp.servers[0].id, "fs");
        assert_eq!(s.mcp.servers[0].name, "First");
        assert_eq!(s.mcp.servers[1].id, "git");
    }

    #[test]
    fn sanitize_drops_empty_id() {
        let raw = r#"{
            "mcp": {
                "servers": [
                    { "id": "", "name": "NoId", "command": "a" },
                    { "id": "fs", "name": "FS", "command": "b" }
                ]
            }
        }"#;
        let s = Settings::load_from_str(raw);
        assert_eq!(s.mcp.servers.len(), 1, "空 id 应被丢弃");
        assert_eq!(s.mcp.servers[0].id, "fs");
    }

    #[test]
    fn round_trip_serialization() {
        let original = Settings {
            mcp: McpSettings {
                servers: vec![ChatMcpServer {
                    id: "fs".into(),
                    name: "Filesystem".into(),
                    enabled: true,
                    transport: "stdio".into(),
                    command: "npx".into(),
                    args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()],
                    env: HashMap::new(),
                    cwd: None,
                    enabled_tools: Vec::new(),
                }],
            },
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mcp.servers.len(), 1);
        assert_eq!(back.mcp.servers[0].id, "fs");
        assert_eq!(back.mcp.servers[0].command, "npx");
    }

    #[test]
    fn save_to_path_and_load_back() {
        let dir = std::env::temp_dir().join(format!("smart-codeagent-save-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("settings.json");

        let original = Settings {
            mcp: McpSettings {
                servers: vec![ChatMcpServer {
                    id: "fs".into(),
                    name: "Filesystem".into(),
                    enabled: true,
                    transport: "stdio".into(),
                    command: "npx".into(),
                    args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()],
                    env: HashMap::new(),
                    cwd: None,
                    enabled_tools: Vec::new(),
                }],
            },
        };

        std::fs::write(&path, serde_json::to_string_pretty(&original).unwrap())
            .expect("write settings");

        let loaded = Settings::load_from_path(&path);
        assert_eq!(loaded.mcp.servers.len(), 1);
        assert_eq!(loaded.mcp.servers[0].id, "fs");
        assert_eq!(loaded.mcp.servers[0].command, "npx");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reload_from_disk_updates_instance() {
        let dir = std::env::temp_dir().join(format!("smart-codeagent-reload-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("settings.json");

        let mut settings = Settings::default();
        assert!(settings.mcp.servers.is_empty());

        std::fs::write(
            &path,
            r#"{
                "mcp": {
                    "servers": [
                        { "id": "fs", "name": "FS", "command": "npx" }
                    ]
                }
            }"#,
        )
        .expect("write settings");

        settings.reload_from_path(&path);
        assert_eq!(settings.mcp.servers.len(), 1);
        assert_eq!(settings.mcp.servers[0].id, "fs");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

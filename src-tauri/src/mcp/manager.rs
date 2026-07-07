//! 多 MCP server 协调器。
//!
//! 持有所有 enabled server 的 `StdioMcpClient`，并发拉取工具列表，路由 tool call。
//! 状态事件经 `CachingSink` 双写：更新内部状态缓存 + 转发到前端（TauriEventSink）。
//!
//! 参照 Kivio `mcp/manager.rs` + `mcp/registry.rs::list_enabled_tool_defs`，砍掉：
//! - HTTP transport / OAuth（Phase 3.1 不做）
//! - idle reaper（Phase 3.1 不做，退出钩子兜底）
//! - 配置指纹热重建（Phase 3.3 实现热重载：reconnect_all + Settings 重新加载）

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::settings::{ChatMcpServer, Settings};

use super::client::{McpEventSink, StdioMcpClient};
use super::types::{McpServerState, McpServerStatePayload, McpTool, McpToolCallResult};
use super::types::tool_definition_from_mcp;
use crate::agent::tools::ChatToolDefinition;

/// 默认 tool 超时（30s）。生产路径可由调用方覆盖。
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// 生产用 sink：通过 `AppHandle.emit("mcp-server-state", payload)` 推前端。
pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl McpEventSink for TauriEventSink {
    fn emit_server_state(&self, server: &ChatMcpServer, state: &McpServerState) {
        let _ = self.app.emit(
            "mcp-server-state",
            McpServerStatePayload {
                server_id: server.id.clone(),
                state: state.clone(),
            },
        );
    }

    fn emit_disconnected(&self, server_id: &str) {
        let _ = self.app.emit(
            "mcp-server-state",
            McpServerStatePayload {
                server_id: server_id.to_string(),
                state: McpServerState::Disconnected,
            },
        );
    }
}

/// 包装一个 sink，在 emit 时同步更新内部状态缓存。
/// `states` 用 `std::sync::Mutex`（非 tokio）因为临界区内无 await，且 trait 方法是 sync。
struct CachingSink {
    inner: Arc<dyn McpEventSink>,
    states: Arc<StdMutex<HashMap<String, McpServerState>>>,
}

impl McpEventSink for CachingSink {
    fn emit_server_state(&self, server: &ChatMcpServer, state: &McpServerState) {
        if let Ok(mut states) = self.states.lock() {
            states.insert(server.id.clone(), state.clone());
        }
        self.inner.emit_server_state(server, state);
    }

    fn emit_disconnected(&self, server_id: &str) {
        if let Ok(mut states) = self.states.lock() {
            states.insert(server_id.to_string(), McpServerState::Disconnected);
        }
        self.inner.emit_disconnected(server_id);
    }
}

/// 多 MCP server 协调器。
pub struct McpManager {
    /// `server_id -> client`，懒初始化：首次 `list_all_tools` 时才创建 client。
    clients: Arc<Mutex<HashMap<String, Arc<StdioMcpClient>>>>,
    /// 状态缓存：由 `CachingSink` 在 emit 时同步更新。
    states: Arc<StdMutex<HashMap<String, McpServerState>>>,
    /// 包装了 `CachingSink` 的 sink，传给每个 `StdioMcpClient`。
    sink: Arc<dyn McpEventSink>,
    /// 传给 `StdioMcpClient::new` 的超时。
    timeout_ms: u64,
}

impl McpManager {
    /// 生产构造：用 `TauriEventSink` 把状态事件推前端。
    pub fn new(app: AppHandle) -> Self {
        Self::new_with_sink(Arc::new(TauriEventSink::new(app)), DEFAULT_TIMEOUT_MS)
    }

    /// 测试构造：注入自定义 sink（如 `Arc::new(())`），无需真实 AppHandle。
    pub fn new_with_sink(sink: Arc<dyn McpEventSink>, timeout_ms: u64) -> Self {
        let states = Arc::new(StdMutex::new(HashMap::new()));
        let caching_sink: Arc<dyn McpEventSink> = Arc::new(CachingSink {
            inner: sink,
            states: states.clone(),
        });
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            states,
            sink: caching_sink,
            timeout_ms,
        }
    }

    /// 获取或创建 server 对应的 client（懒初始化）。
    /// 已存在的 client 直接 clone Arc 返回，保证同一 server_id 复用同一 client。
    async fn get_or_init_client(&self, server: &ChatMcpServer) -> Arc<StdioMcpClient> {
        let mut clients = self.clients.lock().await;
        clients
            .entry(server.id.clone())
            .or_insert_with(|| {
                Arc::new(StdioMcpClient::new(
                    server.clone(),
                    self.sink.clone(),
                    self.timeout_ms,
                ))
            })
            .clone()
    }

    /// 并发拉取所有 enabled server 的工具列表，合并为 `Vec<ChatToolDefinition>`。
    ///
    /// 单 server 失败：`tracing::warn!` 跳过，不影响其他 server。空 servers → 空 Vec。
    /// `enabled_tools` 非空时只保留列出的工具（白名单过滤）。
    ///
    /// **保证**：所有成功握手的 client 会注册到 `self.clients` 池中，后续 `call_tool`
    /// 能按 server_id 找到同一 client（复用持久会话）。
    pub async fn list_all_tools(&self, settings: &Settings) -> Vec<ChatToolDefinition> {
        let enabled: Vec<ChatMcpServer> = settings
            .mcp
            .servers
            .iter()
            .filter(|s| s.enabled)
            .cloned()
            .collect();

        if enabled.is_empty() {
            return Vec::new();
        }

        // 先顺序初始化所有 client 并注册到池。
        // `get_or_init_client` 锁持有极短（仅 HashMap entry + clone Arc），顺序调用无性能问题。
        // 这样保证后续 `call_tool` 能按 server_id 找到同一 client，复用持久会话。
        let clients: Vec<(ChatMcpServer, Arc<StdioMcpClient>)> = Vec::with_capacity(enabled.len());
        let mut clients = clients;
        for server in enabled {
            let client = self.get_or_init_client(&server).await;
            clients.push((server, client));
        }

        // 并发拉取工具列表（client 已池化，此处不持 self.clients 锁）。
        let futures: Vec<_> = clients
            .into_iter()
            .map(|(server, client)| async move {
                match client.list_tools().await {
                    Ok(tools) => {
                        let filtered = filter_tools(&server, tools);
                        filtered
                            .into_iter()
                            .map(|t| tool_definition_from_mcp(&server, t))
                            .collect::<Vec<_>>()
                    }
                    Err(err) => {
                        tracing::warn!(
                            "MCP server {} list_tools failed, skipping: {}",
                            server.id,
                            err
                        );
                        Vec::new()
                    }
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;
        results.into_iter().flatten().collect()
    }

    /// 调用某个 server 的工具。server 未初始化 → Err。
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult, String> {
        let client = {
            let clients = self.clients.lock().await;
            clients.get(server_id).cloned()
        };
        let client = client.ok_or_else(|| {
            format!(
                "MCP server '{}' is not configured or not yet initialized",
                server_id
            )
        })?;
        client.call_tool(tool_name, arguments).await
    }

    /// 排干所有 client：每个 `StdioMcpClient::disconnect` 触发 Drop → kill 子进程。
    /// 退出钩子用。
    pub async fn disconnect_all(&self) {
        let drained: Vec<(String, Arc<StdioMcpClient>)> = {
            let mut clients = self.clients.lock().await;
            clients.drain().collect()
        };
        for (_, client) in drained {
            client.disconnect().await;
        }
    }

    /// 状态快照（给前端 `list_mcp_server_states` 命令用）。
    /// 未在缓存中的 server（未初始化）由前端视作 Disconnected。
    pub async fn list_server_states(&self) -> HashMap<String, McpServerState> {
        let states = self.states.lock().expect("states mutex poisoned");
        states.clone()
    }

    /// 根据新 settings 重新连接所有 server。
    /// 步骤：
    /// 1. 断开所有现有连接（清空 clients 池 + emit Disconnected）
    /// 2. 按新 settings 初始化并握手（注册到池 + emit Connecting/Connected）
    /// 3. 返回新的工具列表（供下次 LLM 请求使用）
    pub async fn reconnect_all(&self, settings: &Settings) -> Vec<ChatToolDefinition> {
        self.disconnect_all().await;
        self.list_all_tools(settings).await
    }

    /// 断开指定 server（从池移除 + emit Disconnected）。
    pub async fn disconnect_server(&self, server_id: &str) {
        let client = {
            let mut clients = self.clients.lock().await;
            clients.remove(server_id)
        };
        if let Some(client) = client {
            client.disconnect().await;
            self.sink.emit_disconnected(server_id);
        }
    }

    /// 测试单个 server 是否能正常连接（不注册到池，测试完毕立即断开）。
    /// 用于前端"测试连接"按钮。
    pub async fn test_connection(&self, server: &ChatMcpServer) -> Result<(), String> {
        let client = StdioMcpClient::new(server.clone(), self.sink.clone(), self.timeout_ms);
        client.list_tools().await?;
        client.disconnect().await;
        Ok(())
    }
}

/// `enabled_tools` 白名单过滤。空 vec = 全启用。
fn filter_tools(server: &ChatMcpServer, tools: Vec<McpTool>) -> Vec<McpTool> {
    if server.enabled_tools.is_empty() {
        return tools;
    }
    tools
        .into_iter()
        .filter(|t| server.enabled_tools.iter().any(|n| n == &t.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 复用 client.rs 的 fake server 脚本（简化版，无 die/delay/marker）。
    fn write_fake_server(tool_name: &str) -> std::path::PathBuf {
        let tool_name = tool_name.to_string();
        let script = format!(
            r#"#!/usr/bin/env python3
import sys, json
tool_name = {tool_name:?}
while True:
    line = sys.stdin.readline()
    if not line:
        break
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    mid = msg.get("id")
    method = msg.get("method")
    if mid is None:
        continue
    if method == "initialize":
        resp = {{"jsonrpc":"2.0","id":mid,"result":{{"protocolVersion":"2025-06-18","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fake","version":"1.0.0"}}}}}}
    elif method == "tools/list":
        resp = {{"jsonrpc":"2.0","id":mid,"result":{{"tools":[{{"name":tool_name,"description":"Echo","inputSchema":{{"type":"object","properties":{{"text":{{"type":"string"}}}}}}}}]}}}}
    elif method == "tools/call":
        text = ""
        try:
            text = msg["params"]["arguments"].get("text","")
        except Exception:
            text = ""
        resp = {{"jsonrpc":"2.0","id":mid,"result":{{"content":[{{"type":"text","text":"echo: "+str(text)}}]}}}}
    else:
        resp = {{"jsonrpc":"2.0","id":mid,"result":{{}}}}
    sys.stdout.write(json.dumps(resp)+"\n")
    sys.stdout.flush()
"#,
        );
        let mut path = std::env::temp_dir();
        path.push(format!("sca-mgr-fake-{}.py", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).expect("create fake server");
        file.write_all(script.as_bytes()).expect("write fake server");
        path
    }

    fn make_server(id: &str, tool_name: &str) -> (ChatMcpServer, std::path::PathBuf) {
        let script = write_fake_server(tool_name);
        let server = ChatMcpServer {
            id: id.to_string(),
            name: format!("Test {}", id),
            enabled: true,
            transport: "stdio".to_string(),
            command: "python3".to_string(),
            args: vec!["-u".to_string(), script.to_string_lossy().into_owned()],
            env: HashMap::new(),
            cwd: None,
            enabled_tools: Vec::new(),
        };
        (server, script)
    }

    /// 测试辅助：直接构造 `Settings`（`Settings` 仅含 `mcp` 字段）。
    fn settings_with(servers: Vec<ChatMcpServer>) -> Settings {
        Settings {
            mcp: crate::settings::McpSettings { servers },
        }
    }

    #[tokio::test]
    async fn manager_list_all_tools_concurrent() {
        // 2 个 server，各暴露 1 个工具；并发拉取后合并为 2 个 ChatToolDefinition。
        let (s1, script1) = make_server("srv1", "tool_a");
        let (s2, script2) = make_server("srv2", "tool_b");
        let settings = settings_with(vec![s1, s2]);
        let manager = McpManager::new_with_sink(Arc::new(()), 5_000);

        let defs = manager.list_all_tools(&settings).await;
        assert_eq!(defs.len(), 2, "should collect from both servers");
        let names: Vec<_> = defs.iter().map(|d| d.name.clone()).collect();
        assert!(names.contains(&"mcp__srv1__tool_a".to_string()));
        assert!(names.contains(&"mcp__srv2__tool_b".to_string()));
        // 命名空间：mcp__{server_id}__{tool_name}
        assert!(
            defs.iter().any(|d| d.name == "mcp__srv1__tool_a"),
            "should have namespaced name"
        );

        manager.disconnect_all().await;
        let _ = std::fs::remove_file(&script1);
        let _ = std::fs::remove_file(&script2);
    }

    #[tokio::test]
    async fn manager_list_all_tools_skips_failed_server() {
        // 1 个好 server + 1 个坏 server（command 不存在）；好 server 的工具仍返回。
        let (good, script) = make_server("good", "tool_ok");
        let bad = ChatMcpServer {
            id: "bad".to_string(),
            name: "Bad".to_string(),
            enabled: true,
            transport: "stdio".to_string(),
            command: "/no/such/binary".to_string(),
            args: vec![],
            env: HashMap::new(),
            cwd: None,
            enabled_tools: Vec::new(),
        };
        let settings = settings_with(vec![good, bad]);
        let manager = McpManager::new_with_sink(Arc::new(()), 5_000);

        let defs = manager.list_all_tools(&settings).await;
        assert_eq!(defs.len(), 1, "only good server should contribute");
        assert_eq!(defs[0].name, "mcp__good__tool_ok");

        manager.disconnect_all().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn manager_list_all_tools_empty_when_no_servers() {
        let settings = settings_with(vec![]);
        let manager = McpManager::new_with_sink(Arc::new(()), 5_000);

        let defs = manager.list_all_tools(&settings).await;
        assert!(defs.is_empty());
    }

    #[tokio::test]
    async fn manager_list_all_tools_filters_enabled_tools() {
        // server 暴露 tool_a + tool_b，但 enabled_tools 只列 tool_a → 只返回 tool_a。
        let script = write_fake_server_multi_tools();
        let server = ChatMcpServer {
            id: "filtered".to_string(),
            name: "Filtered".to_string(),
            enabled: true,
            transport: "stdio".to_string(),
            command: "python3".to_string(),
            args: vec!["-u".to_string(), script.to_string_lossy().into_owned()],
            env: HashMap::new(),
            cwd: None,
            enabled_tools: vec!["keep".to_string()],
        };
        let settings = settings_with(vec![server]);
        let manager = McpManager::new_with_sink(Arc::new(()), 5_000);

        let defs = manager.list_all_tools(&settings).await;
        assert_eq!(defs.len(), 1, "only enabled tool should appear");
        assert_eq!(defs[0].name, "mcp__filtered__keep");

        manager.disconnect_all().await;
        let _ = std::fs::remove_file(&script);
    }

    fn write_fake_server_multi_tools() -> std::path::PathBuf {
        let script = r#"#!/usr/bin/env python3
import sys, json
while True:
    line = sys.stdin.readline()
    if not line:
        break
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    mid = msg.get("id")
    method = msg.get("method")
    if mid is None:
        continue
    if method == "initialize":
        resp = {"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1.0.0"}}}
    elif method == "tools/list":
        resp = {"jsonrpc":"2.0","id":mid,"result":{"tools":[
            {"name":"keep","description":"Keep","inputSchema":{"type":"object"}},
            {"name":"drop","description":"Drop","inputSchema":{"type":"object"}}
        ]}}
    elif method == "tools/call":
        resp = {"jsonrpc":"2.0","id":mid,"result":{"content":[{"type":"text","text":"ok"}]}}
    else:
        resp = {"jsonrpc":"2.0","id":mid,"result":{}}
    sys.stdout.write(json.dumps(resp)+"\n")
    sys.stdout.flush()
"#;
        let mut path = std::env::temp_dir();
        path.push(format!("sca-mgr-multi-{}.py", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).expect("create");
        file.write_all(script.as_bytes()).expect("write");
        path
    }

    #[tokio::test]
    async fn manager_call_tool_routes_to_correct_client() {
        // 2 个 server，各暴露 echo 工具；call_tool 按 server_id 路由到正确的 client。
        // list_all_tools 会通过 get_or_init_client 把 client 注册到池，call_tool 能直接复用。
        let (s1, script1) = make_server("srv1", "echo");
        let (s2, script2) = make_server("srv2", "echo");
        let settings = settings_with(vec![s1, s2]);
        let manager = McpManager::new_with_sink(Arc::new(()), 5_000);

        // list_all_tools 初始化 client 并注册到池。
        let defs = manager.list_all_tools(&settings).await;
        assert_eq!(defs.len(), 2);

        let r1 = manager
            .call_tool("srv1", "echo", serde_json::json!({ "text": "from-srv1" }))
            .await
            .expect("srv1 call ok");
        assert_eq!(r1.content, "echo: from-srv1");

        let r2 = manager
            .call_tool("srv2", "echo", serde_json::json!({ "text": "from-srv2" }))
            .await
            .expect("srv2 call ok");
        assert_eq!(r2.content, "echo: from-srv2");

        manager.disconnect_all().await;
        let _ = std::fs::remove_file(&script1);
        let _ = std::fs::remove_file(&script2);
    }

    #[tokio::test]
    async fn manager_call_tool_unknown_server_returns_error() {
        let manager = McpManager::new_with_sink(Arc::new(()), 5_000);

        let err = manager
            .call_tool("nonexistent", "echo", serde_json::json!({}))
            .await
            .expect_err("unknown server should error");
        assert!(
            err.contains("not configured") || err.contains("not yet initialized"),
            "error should mention not configured, got: {err}"
        );
    }

    #[tokio::test]
    async fn manager_list_server_states_caches_emitted_states() {
        // 连接一个 server → CachingSink 应把 Connecting/Connected 写入 states 缓存。
        let (server, script) = make_server("state-test", "echo");
        let settings = settings_with(vec![server]);
        let manager = McpManager::new_with_sink(Arc::new(()), 5_000);

        // list_all_tools 会初始化 client 并握手，CachingSink 记录状态。
        let _ = manager.list_all_tools(&settings).await;

        // 触发一次 call_tool 确保 handshake 完成（list_tools 已握手，这里验证池化复用）。
        let _ = manager
            .call_tool("state-test", "echo", serde_json::json!({ "text": "x" }))
            .await;

        let states = manager.list_server_states().await;
        assert!(
            states.get("state-test") == Some(&McpServerState::Connected),
            "state-test should be Connected in cache, got: {:?}",
            states.get("state-test")
        );

        manager.disconnect_all().await;
        let _ = std::fs::remove_file(&script);
    }
}

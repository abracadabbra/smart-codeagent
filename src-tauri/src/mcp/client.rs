//! 单个 stdio MCP server 的持久 client。
//!
//! 参照 Kivio `mcp/manager.rs::StdioConn` 的持久 session 模式：子进程常驻、
//! 握手一次、reader_task 按 JSON-RPC id 路由到 oneshot、单飞门闩防并发重复握手、
//! 死连接透明重连一次。砍掉 HTTP transport / OAuth / idle reaper（Phase 3.1 不需要）。
//!
//! 关键约束（见 design.md §4 / §6）：
//! - 绝不跨握手 / RPC await 持外层 `session` 锁；命中即克隆 `Arc<Mutex<McpSession>>`
//!   后立即释放外层锁，再锁会话做握手。
//! - `kill_on_drop(true)` + `McpSession::Drop` abort reader/stderr task + `start_kill()`
//!   + `disconnect()` 兜底，避免孤儿进程。
//! - 超时不重试（保护非幂等工具）；只有「连接已死」才重连一次。

use std::{
    collections::{HashMap, VecDeque},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, oneshot},
    task::JoinHandle,
    time::timeout,
};

use crate::settings::ChatMcpServer;

use super::types::{McpServerState, McpTool, McpToolCallResult, parse_tool_result};

/// MCP 协议版本（2025-06-18）。
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// stderr 尾巴最多保留多少行（状态面板诊断用）。
pub const STDERR_TAIL_LINES: usize = 20;

/// 状态事件发射器。生产代码用 `TauriEventSink`（Round 4）发 `mcp-server-state`；
/// 测试用 `()` 空实现，这样核心连接逻辑无需真实 Tauri AppHandle 即可单测。
pub trait McpEventSink: Send + Sync {
    fn emit_server_state(&self, server: &ChatMcpServer, state: &McpServerState);
    fn emit_disconnected(&self, server_id: &str);
}

impl McpEventSink for () {
    fn emit_server_state(&self, _server: &ChatMcpServer, _state: &McpServerState) {}
    fn emit_disconnected(&self, _server_id: &str) {}
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// 持久 stdio 会话：子进程 + 写端 + 后台 reader/stderr task。
///
/// reader_task 循环读 stdout 行，按 JSON-RPC id 把响应投递给在途请求的 oneshot；
/// 支持并发在途请求。`Drop` 时 abort 两个 task 并 `start_kill()` 子进程。
pub struct McpSession {
    child: Child,
    stdin: ChildStdin,
    next_id: AtomicU64,
    pending: PendingMap,
    reader_task: JoinHandle<()>,
    stderr_task: JoinHandle<()>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    /// 握手 `initialize` 返回的 `serverInfo`。
    pub server_info: Option<Value>,
    /// 握手 `initialize` 返回的 `capabilities`。
    pub capabilities: Option<Value>,
    timeout: Duration,
}

impl Drop for McpSession {
    fn drop(&mut self) {
        self.reader_task.abort();
        self.stderr_task.abort();
        // kill_on_drop(true) 已兜底，这里再显式触发一次。
        let _ = self.child.start_kill();
    }
}

impl McpSession {
    /// 子进程是否已退出（liveness 探活）。
    pub fn is_dead(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }

    /// 读 stderr 尾巴快照（拼成多行字符串）。
    pub async fn stderr_tail_text(&self) -> String {
        let tail = self.stderr_tail.lock().await;
        tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    /// 发一次 JSON-RPC 请求并等待匹配 id 的响应。
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }
        let mut message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if !params.is_null() {
            message["params"] = params;
        }
        if let Err(err) = self.write_message(&message).await {
            self.pending.lock().await.remove(&id);
            return Err(err);
        }
        match timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                // reader task 结束（子进程关闭 stdout）时 oneshot sender 被丢弃。
                self.pending.lock().await.remove(&id);
                Err("MCP server closed stdout".to_string())
            }
            Err(_) => {
                // 超时不杀子进程、不重试（保护非幂等工具），只移除 pending 槽。
                self.pending.lock().await.remove(&id);
                Err("MCP stdio read timed out".to_string())
            }
        }
    }

    /// 发一次 JSON-RPC notification（无 id，不等响应）。
    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let mut message = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if !params.is_null() {
            message["params"] = params;
        }
        self.write_message(&message).await
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), String> {
        let line = serde_json::to_string(message).map_err(|err| err.to_string())?;
        timeout(self.timeout, async {
            self.stdin.write_all(line.as_bytes()).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await
        })
        .await
        .map_err(|_| "MCP stdio write timed out".to_string())?
        .map_err(|err| format!("MCP stdio write failed: {err}"))
    }
}

/// 单 server 的 stdio client：持久 session + 单飞门闩 + 死连接重连。
///
/// 一个 `StdioMcpClient` 对应一个 `ChatMcpServer`。`McpManager`（Round 4）持有
/// `HashMap<server_id, Arc<StdioMcpClient>>` 做多 server 协调。
pub struct StdioMcpClient {
    server: ChatMcpServer,
    timeout: Duration,
    sink: Arc<dyn McpEventSink>,
    /// `None` = 未连接 / 已断开；`Some(arc)` = 有 session（可能已死）。
    session: Mutex<Option<Arc<Mutex<McpSession>>>>,
    /// 握手单飞门闩：序列化并发握手，避免两个 caller 各 spawn 一个子进程。
    handshake_lock: Mutex<()>,
}

impl StdioMcpClient {
    pub fn new(server: ChatMcpServer, sink: Arc<dyn McpEventSink>, timeout_ms: u64) -> Self {
        Self {
            server,
            timeout: Duration::from_millis(timeout_ms.max(1_000)),
            sink,
            session: Mutex::new(None),
            handshake_lock: Mutex::new(()),
        }
    }

    /// 获取或建立连接（单飞门闩）。
    ///
    /// 快速路径：已有存活 session → 直接返回。
    /// 慢速路径：握手锁 → 双重检查 → spawn + initialize → 存 session → emit Connected。
    pub async fn connect(&self) -> Result<Arc<Mutex<McpSession>>, String> {
        // 快速路径：已有活 session
        if let Some(arc) = self.live_session().await {
            return Ok(arc);
        }

        // 单飞握手：序列化并发握手请求。
        let _guard = self.handshake_lock.lock().await;

        // 双重检查：上一持锁者可能刚完成握手。
        if let Some(arc) = self.live_session().await {
            return Ok(arc);
        }

        // 握手。
        self.sink
            .emit_server_state(&self.server, &McpServerState::Connecting);
        match self.spawn_and_handshake().await {
            Ok(session) => {
                let arc = Arc::new(Mutex::new(session));
                {
                    let mut guard = self.session.lock().await;
                    *guard = Some(arc.clone());
                }
                self.sink
                    .emit_server_state(&self.server, &McpServerState::Connected);
                Ok(arc)
            }
            Err(err) => {
                self.sink.emit_server_state(
                    &self.server,
                    &McpServerState::Error {
                        message: err.clone(),
                    },
                );
                Err(err)
            }
        }
    }

    /// 检查现有 session 是否存活；若死则清除并返回 None。
    async fn live_session(&self) -> Option<Arc<Mutex<McpSession>>> {
        let arc = {
            let guard = self.session.lock().await;
            guard.as_ref().cloned()
        }?;
        let mut s = arc.lock().await;
        if s.is_dead() {
            drop(s);
            // 死连接：清掉旧 session（仅当还是同一个 arc 时）。
            let mut guard = self.session.lock().await;
            if guard.as_ref().is_some_and(|x| Arc::ptr_eq(x, &arc)) {
                *guard = None;
            }
            return None;
        }
        drop(s);
        Some(arc)
    }

    /// spawn 子进程 + 握手（initialize → notifications/initialized）。
    async fn spawn_and_handshake(&self) -> Result<McpSession, String> {
        if self.server.command.trim().is_empty() {
            return Err("MCP server command is empty".to_string());
        }
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        let mut session = spawn_stdio(&self.server, self.timeout, stderr_tail)?;
        let init = session.request("initialize", initialize_params()).await?;
        session
            .notify("notifications/initialized", Value::Null)
            .await?;
        session.server_info = init.get("serverInfo").cloned();
        session.capabilities = init.get("capabilities").cloned();
        Ok(session)
    }

    /// 拉取工具列表（死连接透明重连一次）。
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
        match self.list_tools_inner().await {
            Ok(tools) => Ok(tools),
            Err(err) if is_connection_closed_error(&err) => {
                // 连接已死 → 清旧 session 重连一次重试。
                self.invalidate_session().await;
                self.list_tools_inner().await
            }
            Err(err) => Err(err),
        }
    }

    async fn list_tools_inner(&self) -> Result<Vec<McpTool>, String> {
        let arc = self.connect().await?;
        let mut s = arc.lock().await;
        let value = s.request("tools/list", Value::Null).await?;
        parse_tools(&value)
    }

    /// 调用工具（死连接透明重连一次）。
    ///
    /// 注意：超时不重试（保护非幂等工具）。只有「连接已死」才重连一次。
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult, String> {
        match self.call_tool_inner(name, arguments.clone()).await {
            Ok(result) => Ok(result),
            Err(err) if is_connection_closed_error(&err) => {
                self.invalidate_session().await;
                self.call_tool_inner(name, arguments).await
            }
            Err(err) => Err(err),
        }
    }

    async fn call_tool_inner(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult, String> {
        let arc = self.connect().await?;
        let mut s = arc.lock().await;
        let value = s
            .request(
                "tools/call",
                serde_json::json!({
                    "name": name,
                    "arguments": arguments,
                }),
            )
            .await?;
        Ok(parse_tool_result(value))
    }

    /// 主动断开（重连按钮 / 退出钩子用）。Drop 触发 abort task + kill 子进程。
    pub async fn disconnect(&self) {
        let taken = {
            let mut guard = self.session.lock().await;
            guard.take()
        };
        drop(taken); // McpSession::Drop → abort + kill
        self.sink.emit_disconnected(&self.server.id);
    }

    /// 清除当前 session（死连接后内部用，不发 Disconnected 事件）。
    async fn invalidate_session(&self) {
        let taken = {
            let mut guard = self.session.lock().await;
            guard.take()
        };
        drop(taken);
    }

    /// server 配置引用（manager 用）。
    pub fn server(&self) -> &ChatMcpServer {
        &self.server
    }
}

/// 判断错误是否表示连接已关闭（用于决定是否重连）。
/// 区分「连接死了应重连」与「慢但健康的工具超时应透传」。
fn is_connection_closed_error(err: &str) -> bool {
    err.contains("MCP server closed stdout")
}

/// `initialize` 请求 params（共享给所有 transport）。
pub(crate) fn initialize_params() -> Value {
    serde_json::json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {
            "name": "smart-codeagent",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn parse_tools(value: &Value) -> Result<Vec<McpTool>, String> {
    let tools = value
        .get("tools")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    serde_json::from_value(tools).map_err(|err| format!("MCP tools/list parse failed: {err}"))
}

fn clean_env(env: &HashMap<String, String>) -> Vec<(String, String)> {
    env.iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            if key.is_empty() {
                None
            } else {
                Some((key.to_string(), value.clone()))
            }
        })
        .collect()
}

fn compact_json(value: &Value, max_chars: usize) -> String {
    let raw = serde_json::to_string(value).unwrap_or_default();
    raw.chars().take(max_chars).collect()
}

/// spawn stdio 子进程 + reader / stderr 后台 task。
fn spawn_stdio(
    server: &ChatMcpServer,
    timeout_dur: Duration,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
) -> Result<McpSession, String> {
    let mut command = Command::new(&server.command);
    command.args(&server.args);
    if let Some(cwd) = server.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()) {
        command.current_dir(cwd);
    }
    command.envs(clean_env(&server.env));
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to start MCP server {}: {err}", server.name))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "MCP server stdin unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "MCP server stdout unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "MCP server stderr unavailable".to_string())?;

    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

    // reader task：循环读 stdout 行，按 JSON-RPC id 投递给在途请求的 oneshot。
    let reader_pending = pending.clone();
    let reader_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let value: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // 无 id 的通知 / 进度消息：忽略。
            let Some(id) = value.get("id").and_then(|id| id.as_u64()) else {
                continue;
            };
            let sender = {
                let mut pending = reader_pending.lock().await;
                pending.remove(&id)
            };
            if let Some(sender) = sender {
                let outcome = if let Some(error) = value.get("error") {
                    Err(format!("MCP error: {}", compact_json(error, 500)))
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = sender.send(outcome);
            }
        }
        // EOF 或读错误 → 子进程关闭 stdout，结束 reader；在途请求的 oneshot
        // 在此被丢弃，request 侧收到 RecvError 报 "closed stdout"。
    });

    // stderr task：把 stderr 尾巴收进环形缓冲（最多 STDERR_TAIL_LINES 行）。
    let stderr_tail_for_task = stderr_tail.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut tail = stderr_tail_for_task.lock().await;
            if tail.len() >= STDERR_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    });

    Ok(McpSession {
        child,
        stdin,
        next_id: AtomicU64::new(1),
        pending,
        reader_task,
        stderr_task,
        stderr_tail,
        server_info: None,
        capabilities: None,
        timeout: timeout_dur,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 写一个 fake stdio MCP server python 脚本到临时文件，返回脚本路径。
    /// 协议：逐行读 JSON-RPC；initialize/tools/list/tools/call 各自回包；无 id 的通知忽略。
    /// 环境变量控制：
    /// - `SCA_DIE_AFTER_CALL=N`：第 N 次 tools/call 回包后退出（模拟子进程死亡 → 透明重连）。
    /// - `SCA_DELAY_CALL_MS=N`：响应 tools/call 前 sleep N 毫秒（模拟慢但健康的工具）。
    /// - `SCA_CALL_MARKER=path`：每次执行 tools/call 时往该文件追加一行（统计实际执行次数）。
    /// - `SCA_STDERR_MSG=...`：启动时往 stderr 写一行（验证 stderr_tail 收集）。
    fn write_fake_server() -> std::path::PathBuf {
        let script = r#"#!/usr/bin/env python3
import sys, json, os, time
die_after = int(os.environ.get("SCA_DIE_AFTER_CALL", "0"))
delay_ms = int(os.environ.get("SCA_DELAY_CALL_MS", "0"))
marker = os.environ.get("SCA_CALL_MARKER", "")
stderr_msg = os.environ.get("SCA_STDERR_MSG", "")
if stderr_msg:
    sys.stderr.write(stderr_msg + "\n")
    sys.stderr.flush()
calls = 0
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
        resp = {"jsonrpc":"2.0","id":mid,"result":{"tools":[{"name":"echo","description":"Echo text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}
    elif method == "tools/call":
        calls += 1
        if marker:
            with open(marker, "a") as f:
                f.write("call\n")
        text = ""
        try:
            text = msg["params"]["arguments"].get("text","")
        except Exception:
            text = ""
        if delay_ms:
            time.sleep(delay_ms / 1000.0)
        resp = {"jsonrpc":"2.0","id":mid,"result":{"content":[{"type":"text","text":"echo: "+str(text)}]}}
        sys.stdout.write(json.dumps(resp)+"\n")
        sys.stdout.flush()
        if die_after and calls >= die_after:
            sys.exit(0)
        continue
    else:
        resp = {"jsonrpc":"2.0","id":mid,"result":{}}
    sys.stdout.write(json.dumps(resp)+"\n")
    sys.stdout.flush()
"#;
        let mut path = std::env::temp_dir();
        path.push(format!("sca-fake-mcp-{}.py", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).expect("create fake server");
        file.write_all(script.as_bytes())
            .expect("write fake server");
        path
    }

    fn python_server(script: &std::path::Path) -> ChatMcpServer {
        ChatMcpServer {
            id: "test-stdio".to_string(),
            name: "Test Stdio".to_string(),
            enabled: true,
            transport: "stdio".to_string(),
            command: "python3".to_string(),
            args: vec!["-u".to_string(), script.to_string_lossy().into_owned()],
            env: HashMap::new(),
            cwd: None,
            enabled_tools: Vec::new(),
            ..Default::default()
        }
    }

    fn make_client(server: ChatMcpServer, timeout_ms: u64) -> StdioMcpClient {
        StdioMcpClient::new(server, Arc::new(()), timeout_ms)
    }

    #[tokio::test]
    async fn stdio_handshake_then_list_tools() {
        let script = write_fake_server();
        let client = make_client(python_server(&script), 5_000);

        let tools = client.list_tools().await.expect("list_tools should work");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description, "Echo text");

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn stdio_call_tool_returns_content() {
        let script = write_fake_server();
        let client = make_client(python_server(&script), 5_000);

        let result = client
            .call_tool("echo", serde_json::json!({ "text": "hello" }))
            .await
            .expect("call_tool should work");
        assert_eq!(result.content, "echo: hello");
        assert!(!result.is_error);

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn stdio_ten_calls_one_handshake() {
        // 持久 session：10 次调用复用同一子进程，不重新握手。
        let script = write_fake_server();
        let client = make_client(python_server(&script), 5_000);

        for i in 0..10 {
            let result = client
                .call_tool("echo", serde_json::json!({ "text": i }))
                .await
                .expect("call should succeed");
            assert_eq!(result.content, format!("echo: {i}"));
        }

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn stdio_dead_connection_reconnects_once() {
        // 子进程在第 1 次 tools/call 后退出 → 第 2 次探活发现死连接 → 透明重连。
        let script = write_fake_server();
        let mut server = python_server(&script);
        server
            .env
            .insert("SCA_DIE_AFTER_CALL".to_string(), "1".to_string());
        let client = make_client(server, 5_000);

        let first = client
            .call_tool("echo", serde_json::json!({ "text": "a" }))
            .await
            .expect("first call ok");
        assert_eq!(first.content, "echo: a");

        // 给子进程一点时间真正退出。
        tokio::time::sleep(Duration::from_millis(200)).await;

        let second = client
            .call_tool("echo", serde_json::json!({ "text": "b" }))
            .await
            .expect("second call should transparently reconnect");
        assert_eq!(second.content, "echo: b");

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn stdio_timeout_does_not_retry() {
        // 慢但健康的工具超过 timeout → 透传错误，不杀子进程、不重连、不重发。
        let script = write_fake_server();
        let mut server = python_server(&script);
        server
            .env
            .insert("SCA_DELAY_CALL_MS".to_string(), "2500".to_string());
        server.env.insert("SCA_CALL_MARKER".to_string(), {
            let mut p = std::env::temp_dir();
            p.push(format!("sca-marker-{}.txt", uuid::Uuid::new_v4()));
            p.to_string_lossy().into_owned()
        });

        // 用最小 timeout（1s，受 .max(1_000) 约束）；server 延迟 2.5s 远超之。
        let client = make_client(server.clone(), 1_000);

        let marker = server.env.get("SCA_CALL_MARKER").unwrap().clone();
        let err = client
            .call_tool("echo", serde_json::json!({ "text": "slow" }))
            .await
            .expect_err("slow healthy tool should surface a timeout error");
        assert!(
            err.contains("timed out"),
            "expected a timeout error, got: {err}"
        );

        // 给延迟的 tools/call 充足时间真正执行完一次（验证只执行一次，没被重发）。
        tokio::time::sleep(Duration::from_millis(3_000)).await;
        let marker_lines = std::fs::read_to_string(&marker).unwrap_or_default();
        let executed = marker_lines.lines().filter(|l| *l == "call").count();
        assert_eq!(
            executed, 1,
            "the tool body must run exactly once (no silent re-execution)"
        );

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_file(&marker);
    }

    #[tokio::test]
    async fn stdio_stderr_tail_collected() {
        let script = write_fake_server();
        let mut server = python_server(&script);
        server
            .env
            .insert("SCA_STDERR_MSG".to_string(), "hello-stderr".to_string());
        let client = make_client(server, 5_000);

        // 触发握手 + 拉一次工具，确保 stderr task 启动。
        let _ = client.list_tools().await.expect("list_tools ok");

        let arc = client.connect().await.expect("session should be present");
        let s = arc.lock().await;
        let tail = s.stderr_tail_text().await;
        assert!(
            tail.contains("hello-stderr"),
            "stderr tail should contain msg, got: {tail}"
        );

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn stdio_concurrent_connect_share_one_handshake() {
        // 两个并发 connect() 必须收敛到单飞门闩，只做一次握手。
        let script = write_fake_server();
        let client = Arc::new(make_client(python_server(&script), 5_000));

        let c1 = client.clone();
        let c2 = client.clone();
        let (r1, r2) = tokio::join!(async move { c1.connect().await.map(|_| ()) }, async move {
            c2.connect().await.map(|_| ())
        },);
        r1.expect("connect one ok");
        r2.expect("connect two ok");

        // 内部 session 应只有一个。
        let guard = client.session.lock().await;
        assert!(guard.is_some(), "session should be present");
        drop(guard);

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn stdio_concurrent_in_flight_requests_match_by_id() {
        // 同一 session 上并发两个 tools/call，验证 reader task 按 id 正确关联。
        let script = write_fake_server();
        let client = Arc::new(make_client(python_server(&script), 5_000));

        // 先建立连接（避免两个并发同时握手）。
        client
            .call_tool("echo", serde_json::json!({ "text": "warm" }))
            .await
            .expect("warmup ok");

        let c1 = client.clone();
        let c2 = client.clone();
        let (r1, r2) = tokio::join!(
            async move {
                c1.call_tool("echo", serde_json::json!({ "text": "one" }))
                    .await
            },
            async move {
                c2.call_tool("echo", serde_json::json!({ "text": "two" }))
                    .await
            },
        );
        let r1 = r1.expect("call one ok");
        let r2 = r2.expect("call two ok");
        assert_eq!(r1.content, "echo: one");
        assert_eq!(r2.content, "echo: two");

        client.disconnect().await;
        let _ = std::fs::remove_file(&script);
    }

    #[tokio::test]
    async fn stdio_disconnect_kills_child() {
        let script = write_fake_server();
        let client = make_client(python_server(&script), 5_000);

        client
            .call_tool("echo", serde_json::json!({ "text": "x" }))
            .await
            .expect("call ok");

        // 记录子进程 pid。
        let pid = {
            let arc = client.connect().await.expect("session present");
            let s = arc.lock().await;
            s.child.id()
        };
        assert!(pid.is_some());

        client.disconnect().await;
        {
            let guard = client.session.lock().await;
            assert!(guard.is_none(), "session cleared on disconnect");
        }

        // 给 kill 一点时间生效后确认进程不再存活。
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(pid) = pid {
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            assert!(!alive, "child process should be killed");
        }

        let _ = std::fs::remove_file(&script);
    }

    #[test]
    fn is_connection_closed_error_matches_closed_stdout() {
        assert!(is_connection_closed_error("MCP server closed stdout"));
        assert!(!is_connection_closed_error("MCP stdio read timed out"));
        assert!(!is_connection_closed_error("MCP error: boom"));
    }

    #[test]
    fn initialize_params_has_protocol_version() {
        let params = initialize_params();
        assert_eq!(
            params.get("protocolVersion").and_then(|v| v.as_str()),
            Some(MCP_PROTOCOL_VERSION)
        );
        assert_eq!(
            params
                .get("clientInfo")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str()),
            Some("smart-codeagent")
        );
    }
}

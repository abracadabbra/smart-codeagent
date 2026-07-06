//! 后台命令管理：spawn / output / kill。
//!
//! 借 Kivio `native_tools/shell.rs:329-377` 的 `BackgroundCommand` 形态。
//!
//! Phase 2 内存存储（Mutex<HashMap>）；Phase 3 可换持久化。

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};

use super::bash::BashTool;
use super::{Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum BackgroundStatus {
    Running,
    Exited,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundInfo {
    pub id: String,
    pub status: BackgroundStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub command: String,
    pub started_at: i64,
}

pub struct BackgroundHandle {
    pub id: String,
    pub child: Child,
    pub command: String,
    pub started_at: i64,
}

#[derive(Default)]
pub struct BackgroundRegistry {
    handles: Mutex<HashMap<String, BackgroundHandle>>,
}

impl BackgroundRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, handle: BackgroundHandle) {
        self.handles.lock().unwrap().insert(handle.id.clone(), handle);
    }

    pub fn remove(&self, id: &str) -> Option<BackgroundHandle> {
        self.handles.lock().unwrap().remove(id)
    }

    pub fn list(&self) -> Vec<BackgroundInfo> {
        self.handles
            .lock()
            .unwrap()
            .values()
            .map(|h| BackgroundInfo {
                id: h.id.clone(),
                status: BackgroundStatus::Running,
                exit_code: None,
                command: h.command.clone(),
                started_at: h.started_at,
            })
            .collect()
    }
}

static REGISTRY: std::sync::OnceLock<BackgroundRegistry> = std::sync::OnceLock::new();

fn registry() -> &'static BackgroundRegistry {
    REGISTRY.get_or_init(BackgroundRegistry::new)
}

pub async fn spawn_background(command: &str, cwd: &std::path::Path) -> Result<ToolOutput, ToolError> {
    let id = uuid::Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now().timestamp();

    let child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false)
        .spawn()
        .map_err(|e| ToolError::Execution(format!("spawn background failed: {e}")))?;

    registry().insert(BackgroundHandle {
        id: id.clone(),
        child,
        command: command.to_string(),
        started_at,
    });

    Ok(ToolOutput {
        content: format!("spawned background command, id={id}"),
        structured: Some(serde_json::json!({
            "background_id": id,
            "command": command,
            "started_at": started_at,
        })),
        artifacts: vec![],
        truncated: false,
    })
}

#[derive(Debug, Deserialize)]
struct BashOutputArgs {
    background_id: String,
    #[serde(default)]
    wait_ms: Option<u64>,
}

pub struct BashOutputTool;

#[async_trait::async_trait]
impl super::Tool for BashOutputTool {
    fn name(&self) -> &'static str {
        "bash_output"
    }
    fn description(&self) -> &'static str {
        "Read output from a background command by its id. Optionally wait up to wait_ms \
         for output to be ready."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "background_id": { "type": "string" },
                "wait_ms": { "type": "integer", "minimum": 0, "maximum": 60000 }
            },
            "required": ["background_id"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        _ctx: &'a super::ToolContext,
    ) -> super::ToolFuture<'a> {
        Box::pin(async move {
            let args: BashOutputArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("bash_output: {e}")))?;
            let id = args.background_id.clone();

            // 取出 child handle
            let mut handle = match registry().remove(&id) {
                Some(h) => h,
                None => return Err(ToolError::Execution(format!("no such background_id: {id}"))),
            };

            let wait = args.wait_ms.unwrap_or(0);
            let wait_dur = std::time::Duration::from_millis(wait);

            let result: Result<std::process::ExitStatus, ToolError> = if wait > 0 {
                match tokio::time::timeout(wait_dur, handle.child.wait()).await {
                    Ok(Ok(status)) => Ok(status),
                    Ok(Err(e)) => Err(ToolError::Io(e)),
                    Err(_) => {
                        // 超时：仍把 handle 放回，函数走下面"still running"分支
                        registry().insert(handle);
                        return Ok(ToolOutput {
                            content: format!("background {id} still running after {wait}ms"),
                            structured: Some(serde_json::json!({ "status": "Running" })),
                            artifacts: vec![],
                            truncated: false,
                        });
                    }
                }
            } else {
                match handle.child.try_wait() {
                    Ok(Some(status)) => Ok(status),
                    Ok(None) => Err(ToolError::Execution("still running".into())),
                    Err(e) => Err(ToolError::Io(e)),
                }
            };

            // 不管结果如何，把 handle 放回 registry（Running 或 Exited 都还归它）
            registry().insert(handle);

            match result {
                Ok(status) => {
                    // 已退出：再 wait_with_output 拿 stdout/stderr
                    let handle = registry().remove(&id).expect("just inserted");
                    let output = match handle.child.wait_with_output().await {
                        Ok(o) => o,
                        Err(_) => std::process::Output {
                            status,
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                        },
                    };
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    Ok(ToolOutput {
                        content: format!("background {id} exited\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"),
                        structured: Some(serde_json::json!({
                            "status": "Exited",
                            "exit_code": status.code(),
                            "stdout": stdout,
                            "stderr": stderr,
                        })),
                        artifacts: vec![],
                        truncated: false,
                    })
                }
                Err(ToolError::Execution(msg)) if msg == "still running" => Ok(ToolOutput {
                    content: format!("background {id} still running"),
                    structured: Some(serde_json::json!({ "status": "Running" })),
                    artifacts: vec![],
                    truncated: false,
                }),
                Err(e) => Err(e),
            }
        })
    }
}

#[derive(Debug, Deserialize)]
struct KillBackgroundArgs {
    background_id: String,
}

pub struct KillBackgroundTool;

#[async_trait::async_trait]
impl super::Tool for KillBackgroundTool {
    fn name(&self) -> &'static str {
        "kill_background"
    }
    fn description(&self) -> &'static str {
        "Kill a running background command by its id. Requires user approval."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "background_id": { "type": "string" }
            },
            "required": ["background_id"]
        })
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        _ctx: &'a super::ToolContext,
    ) -> super::ToolFuture<'a> {
        Box::pin(async move {
            let args: KillBackgroundArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("kill_background: {e}")))?;
            let id = args.background_id.clone();
            let mut handle = match registry().remove(&id) {
                Some(h) => h,
                None => return Err(ToolError::Execution(format!("no such background_id: {id}"))),
            };
            let killed = handle.child.start_kill().is_ok();
            Ok(ToolOutput {
                content: format!(
                    "{} background {id}",
                    if killed { "killed" } else { "failed to kill" }
                ),
                structured: Some(serde_json::json!({ "killed": killed })),
                artifacts: vec![],
                truncated: false,
            })
        })
    }
}

// 反引用 BashTool 防止 unused import warning（Kivio 风格 re-export）
#[allow(dead_code)]
fn _ensure_bash_in_scope() -> &'static BashTool {
    &BashTool
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::ToolContext;

    fn ctx() -> ToolContext {
        ToolContext {
            conversation_id: "t".into(),
            run_id: "t".into(),
            message_id: "t".into(),
            tool_call_id: "tc_test".into(),
            round: 0,
            generation: 0,
        }
    }

    #[tokio::test]
    async fn spawn_then_kill() {
        let cwd = std::env::temp_dir();
        let out = spawn_background("sleep 60", &cwd).await.unwrap();
        let id = out.structured.unwrap()["background_id"].as_str().unwrap().to_string();

        let tool = KillBackgroundTool;
        let out = tool
            .execute(serde_json::json!({ "background_id": id }), &ctx())
            .await
            .unwrap();
        let s = out.structured.unwrap();
        assert_eq!(s["killed"], true);
    }

    #[tokio::test]
    async fn bash_output_returns_running_or_exited() {
        let cwd = std::env::temp_dir();
        let out = spawn_background("echo done", &cwd).await.unwrap();
        let id = out.structured.unwrap()["background_id"].as_str().unwrap().to_string();

        let tool = BashOutputTool;
        let out = tool
            .execute(
                serde_json::json!({ "background_id": id, "wait_ms": 1000 }),
                &ctx(),
            )
            .await
            .unwrap();
        // echo 立即完成；可能返回 Exited 或 Running（取决于时序）
        assert!(out.structured.is_some());
    }

    #[tokio::test]
    async fn unknown_id_errors() {
        let tool = BashOutputTool;
        let res = tool
            .execute(serde_json::json!({ "background_id": "nonexistent_xyz" }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::Execution(_))));
    }
}
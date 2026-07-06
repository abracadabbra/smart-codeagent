//! `run_command` / `bash` 工具：执行 shell 命令。前台执行带超时；后台执行 spawn 后立即返回。
//!
//! 借 Kivio `native_tools/shell.rs:63` 的 `run_command` 形态。
//!
//! 危险命令走 `deny_list` 兜底；host python install 默认拒，需 `allow_host_python_package_install: true`。

use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use tokio::process::Command;

use super::{deny_list, Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TIMEOUT_MS: u64 = 600_000;

pub struct BashTool;

#[derive(Debug, Deserialize)]
struct BashArgs {
    command: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    allow_host_python_package_install: bool,
    #[serde(default)]
    run_in_background: bool,
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self) -> &'static str {
        "Execute a shell command via /bin/sh -c. Returns stdout, stderr, exit_code, \
         duration. Supports foreground (with timeout_ms, default 30s, max 10min) \
         or background (returns a background_id for later bash_output / kill_background). \
         Requires user approval."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 600000,
                    "description": "Foreground timeout. Defaults to 30s."
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory. Defaults to project root."
                },
                "allow_host_python_package_install": {
                    "type": "boolean",
                    "default": false,
                    "description": "Opt-in for pip install / uv pip install."
                },
                "run_in_background": {
                    "type": "boolean",
                    "default": false,
                    "description": "Spawn and return immediately; poll with bash_output."
                }
            },
            "required": ["command"]
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: BashArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("run_command: {e}")))?;

            // 1. 黑名单
            if let Some(blocked) = deny_list::is_denied(&args.command) {
                return Err(ToolError::Denied(format!(
                    "matched deny list pattern: {blocked}"
                )));
            }

            // 2. host python install gate
            if !args.allow_host_python_package_install
                && deny_list::needs_host_python_opt_in(&args.command)
            {
                return Err(ToolError::Denied(
                    "host python install blocked; pass allow_host_python_package_install=true".into(),
                ));
            }

            // 3. cwd
            let cwd = match &args.cwd {
                Some(c) => super::path::resolve_tool_path(c)
                    .map_err(|e| ToolError::Path(format!("run_command cwd: {e}")))?,
                None => std::env::current_dir().map_err(ToolError::Io)?,
            };

            // 4. 后台：spawn + return
            if args.run_in_background {
                return super::background::spawn_background(&args.command, &cwd).await;
            }

            // 5. 前台执行带超时
            let timeout = args.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).min(MAX_TIMEOUT_MS);
            let started = std::time::Instant::now();

            let future = async {
                let child = Command::new("/bin/sh")
                    .arg("-c")
                    .arg(&args.command)
                    .current_dir(&cwd)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|e| {
                        ToolError::Execution(format!("spawn failed: {e}"))
                    })?;

                let output = child.wait_with_output().await.map_err(|e| {
                    ToolError::Execution(format!("wait failed: {e}"))
                })?;

                Ok::<_, ToolError>(output)
            };

            let result = tokio::time::timeout(std::time::Duration::from_millis(timeout), future).await;
            let duration_ms = started.elapsed().as_millis() as u64;

            match result {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let exit_code = output.status.code().unwrap_or(-1);
                    let combined = if stderr.is_empty() {
                        stdout.clone()
                    } else {
                        format!("{stdout}\n--- stderr ---\n{stderr}")
                    };
                    Ok(ToolOutput {
                        content: combined,
                        structured: Some(serde_json::json!({
                            "stdout": stdout,
                            "stderr": stderr,
                            "exit_code": exit_code,
                            "duration_ms": duration_ms,
                        })),
                        artifacts: vec![],
                        truncated: false,
                    })
                }
                Ok(Err(e)) => Err(e),
                Err(_) => Err(ToolError::Timeout(timeout)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ToolContext {
        ToolContext {
            conversation_id: "test".into(),
            run_id: "test".into(),
            message_id: "test".into(),
            tool_call_id: "tc_test".into(),
            round: 0,
            generation: 0,
        }
    }

    #[tokio::test]
    async fn runs_simple_command() {
        let tool = BashTool;
        let out = tool
            .execute(serde_json::json!({ "command": "echo hi" }), &ctx())
            .await
            .unwrap();
        let s = out.structured.unwrap();
        assert_eq!(s["exit_code"], 0);
        assert!(s["stdout"].as_str().unwrap().contains("hi"));
    }

    #[tokio::test]
    async fn captures_exit_code() {
        let tool = BashTool;
        let out = tool
            .execute(serde_json::json!({ "command": "exit 42" }), &ctx())
            .await
            .unwrap();
        let s = out.structured.unwrap();
        assert_eq!(s["exit_code"], 42);
    }

    #[tokio::test]
    async fn blocks_rm_rf_root() {
        let tool = BashTool;
        let res = tool
            .execute(serde_json::json!({ "command": "rm -rf /" }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::Denied(_))));
    }

    #[tokio::test]
    async fn blocks_pip_install_without_opt_in() {
        let tool = BashTool;
        let res = tool
            .execute(serde_json::json!({ "command": "pip install requests" }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::Denied(_))));
    }

    #[tokio::test]
    async fn allows_pip_install_with_opt_in() {
        // `echo` 在任何 shell 都存在；opt-in 仅绕过 deny_list 检查，与命令本身无关
        let tool = BashTool;
        let out = tool
            .execute(
                serde_json::json!({
                    "command": "echo pip would be allowed with opt-in",
                    "allow_host_python_package_install": true
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let s = out.structured.unwrap();
        assert_eq!(s["exit_code"], 0);
    }

    #[tokio::test]
    async fn timeout_works() {
        let tool = BashTool;
        let res = tool
            .execute(
                serde_json::json!({ "command": "sleep 5", "timeout_ms": 100 }),
                &ctx(),
            )
            .await;
        assert!(matches!(res, Err(ToolError::Timeout(100))));
    }
}
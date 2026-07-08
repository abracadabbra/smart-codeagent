//! `ls` / `list_dir` 工具：列出目录条目。
//!
//! 借 Kivio `native_tools/files.rs:1016` 的 `list_dir` 形态，砍掉了 hidden 文件过滤选项。

use async_trait::async_trait;
use serde::Deserialize;
use walkdir::WalkDir;

use super::{Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

pub struct LsTool;

#[derive(Debug, Deserialize)]
struct LsArgs {
    path: String,
}

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List the entries of a directory (non-recursive). Each entry reports name, \
         kind (file/dir), and size in bytes."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list. Relative paths resolve against cwd."
                }
            },
            "required": ["path"]
        })
    }

    fn execute<'a>(&'a self, args: serde_json::Value, _ctx: &'a ToolContext) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: LsArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("list_dir: {e}")))?;
            let resolved = super::path::resolve_tool_path(&args.path)
                .map_err(|e| ToolError::Path(format!("list_dir: {e}")))?;

            if !resolved.is_dir() {
                return Err(ToolError::Execution(format!(
                    "list_dir: not a directory: {}",
                    resolved.display()
                )));
            }

            let mut entries: Vec<serde_json::Value> = Vec::new();
            for entry in WalkDir::new(&resolved).min_depth(1).max_depth(1) {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        return Err(ToolError::Io(std::io::Error::other(format!(
                            "walk {}: {e}",
                            resolved.display()
                        ))));
                    }
                };
                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue, // skip symlinks we can't stat
                };
                let kind = if meta.is_dir() { "dir" } else { "file" };
                entries.push(serde_json::json!({
                    "name": entry.file_name().to_string_lossy(),
                    "kind": kind,
                    "size": meta.len(),
                }));
            }

            entries.sort_by(|a, b| {
                let ak = a["kind"].as_str().unwrap_or("");
                let bk = b["kind"].as_str().unwrap_or("");
                ak.cmp(bk).then(
                    a["name"]
                        .as_str()
                        .unwrap_or("")
                        .cmp(b["name"].as_str().unwrap_or("")),
                )
            });

            let total = entries.len();
            let text = entries
                .iter()
                .map(|e| {
                    let kind_marker = if e["kind"] == "dir" { "d" } else { "-" };
                    format!(
                        "{} {:>10}  {}",
                        kind_marker,
                        e["size"].as_u64().unwrap_or(0),
                        e["name"].as_str().unwrap_or("?")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            Ok(ToolOutput {
                content: if text.is_empty() {
                    "(empty directory)".to_string()
                } else {
                    text
                },
                structured: Some(serde_json::json!({
                    "entries": entries,
                    "total": total,
                })),
                artifacts: vec![resolved.to_string_lossy().to_string()],
                truncated: false,
            })
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
    async fn lists_directory_entries() {
        let dir = std::env::temp_dir().join(format!("ls_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();

        let tool = LsTool;
        let out = tool
            .execute(serde_json::json!({ "path": dir.to_string_lossy() }), &ctx())
            .await
            .unwrap();
        let s = out.structured.unwrap();
        let entries = s["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn not_a_directory_errors() {
        let f = std::env::temp_dir().join(format!("ls_file_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&f, "x").unwrap();

        let tool = LsTool;
        let res = tool
            .execute(serde_json::json!({ "path": f.to_string_lossy() }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::Execution(_))));

        std::fs::remove_file(&f).ok();
    }
}

//! `search_files` 工具：在文件内容中按正则搜索。
//!
//! 借 Kivio `native_tools/files.rs:1124` 的 `search_files` 形态。

use async_trait::async_trait;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use walkdir::WalkDir;

use super::{Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

pub struct GrepTool;

#[derive(Debug, Deserialize)]
struct GrepArgs {
    pattern: String,
    path: String,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    case_insensitive: Option<bool>,
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "search_files"
    }

    fn description(&self) -> &'static str {
        "Search file contents with a regex pattern. Returns matching lines with file path \
         and line number. Recurses into subdirectories."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Rust regex pattern." },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search. Recurses if directory."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Cap on matches. Defaults to 200."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "default": false
                }
            },
            "required": ["pattern", "path"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: GrepArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("search_files: {e}")))?;
            let base = super::path::resolve_tool_path(&args.path)
                .map_err(|e| ToolError::Path(format!("search_files: {e}")))?;

            let mut builder = regex::RegexBuilder::new(&args.pattern);
            if args.case_insensitive.unwrap_or(false) {
                builder.case_insensitive(true);
            }
            let re = builder
                .build()
                .map_err(|e| ToolError::InvalidArgs(format!("search_files: bad regex: {e}")))?;

            let max = args.max_results.unwrap_or(200);
            let mut matches = Vec::new();

            let walker = if base.is_file() {
                WalkDir::new(&base).min_depth(0).max_depth(0).into_iter().collect::<Vec<_>>()
            } else {
                WalkDir::new(&base).into_iter().collect::<Vec<_>>()
            };

            for entry in walker {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let f = match File::open(path) {
                    Ok(f) => f,
                    Err(_) => continue, // skip unreadable
                };
                let reader = BufReader::new(f);
                for (idx, line_res) in reader.lines().enumerate() {
                    let line = match line_res {
                        Ok(l) => l,
                        Err(_) => break, // binary file → stop at first non-utf8
                    };
                    if re.is_match(&line) {
                        matches.push(serde_json::json!({
                            "path": path.to_string_lossy(),
                            "line": idx + 1,
                            "content": line,
                        }));
                        if matches.len() >= max {
                            return Ok(ToolOutput {
                                content: format!(
                                    "(truncated at {max} matches)\n{}",
                                    matches
                                        .iter()
                                        .map(|m| format!(
                                            "{}:{}: {}",
                                            m["path"].as_str().unwrap_or("?"),
                                            m["line"].as_u64().unwrap_or(0),
                                            m["content"].as_str().unwrap_or("")
                                        ))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                ),
                                structured: Some(serde_json::json!({
                                    "matches": matches,
                                    "truncated": true,
                                })),
                                artifacts: vec![],
                                truncated: true,
                            });
                        }
                    }
                }
            }

            let _total = matches.len();
            let text = if matches.is_empty() {
                "(no matches)".to_string()
            } else {
                matches
                    .iter()
                    .map(|m| format!(
                        "{}:{}: {}",
                        m["path"].as_str().unwrap_or("?"),
                        m["line"].as_u64().unwrap_or(0),
                        m["content"].as_str().unwrap_or("")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            Ok(ToolOutput {
                content: text,
                structured: Some(serde_json::json!({
                    "matches": matches,
                    "truncated": false,
                })),
                artifacts: vec![],
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
    async fn finds_matching_lines() {
        let dir = std::env::temp_dir().join(format!("grep_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "fn main() {}\nfn helper() {}\n").unwrap();
        std::fs::write(dir.join("b.txt"), "no match here\n").unwrap();

        let tool = GrepTool;
        let out = tool
            .execute(
                serde_json::json!({
                    "pattern": "fn ",
                    "path": dir.to_string_lossy()
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let s = out.structured.unwrap();
        let matches = s["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn case_insensitive() {
        let dir = std::env::temp_dir().join(format!("grep_ci_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "Hello World\n").unwrap();

        let tool = GrepTool;
        let out = tool
            .execute(
                serde_json::json!({
                    "pattern": "hello",
                    "path": dir.to_string_lossy(),
                    "case_insensitive": true
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let s = out.structured.unwrap();
        assert_eq!(s["matches"].as_array().unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn invalid_regex_errors() {
        let tool = GrepTool;
        let res = tool
            .execute(
                serde_json::json!({
                    "pattern": "[unclosed",
                    "path": "/tmp"
                }),
                &ctx(),
            )
            .await;
        assert!(matches!(res, Err(ToolError::InvalidArgs(_))));
    }
}
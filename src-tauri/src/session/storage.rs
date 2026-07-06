//! Phase 3.2 文件存储原语。
//!
//! 照搬 Kivio `chat/storage.rs` 的 `atomic_write` 实现（3 次重试 + tmp + rename），
//! 加上 smart-codeagent 的文件布局 helpers（design.md §2）：
//!
//! ```text
//! <app_data_dir>/sessions/
//!   conv_abc123/
//!     meta.json          # Conversation 元数据（atomic_write）
//!     messages.jsonl     # 每行一条 ChatMessage（append-only）
//!   index.json           # 所有会话的 meta 摘要（atomic_write）
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// atomic_write 重试次数（照搬 Kivio `WRITE_RETRY_ATTEMPTS`）。
const WRITE_RETRY_ATTEMPTS: usize = 3;

/// 原子写文件：先写 tmp，再 rename 到目标路径。
///
/// 照搬 Kivio `chat/storage.rs:54-95`。重试 3 次，线性退避（20ms / 40ms / 60ms）。
/// rename 失败时如果目标已存在则 `remove_file` 后重试一次。
///
/// 用于写 `meta.json` 和 `index.json`（小文件，整文件原子更新）。
/// **不用于** `messages.jsonl`（那是 append-only，用 `append_message`）。
pub(crate) fn atomic_write(path: &Path, content: &str, label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("write {label}: path has no parent"))?;

    // 父目录不存在则创建
    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("write {label}: create parent dir: {e}"))?;
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("session");

    for attempt in 0..WRITE_RETRY_ATTEMPTS {
        // tmp 文件命名：.{file_name}.tmp.{attempt}
        let tmp_path = parent.join(format!(".{file_name}.tmp.{attempt}"));

        match fs::write(&tmp_path, content) {
            Ok(()) => {
                // 写成功，尝试 rename
                match fs::rename(&tmp_path, path) {
                    Ok(()) => return Ok(()),
                    Err(_e) => {
                        // rename 失败：如果目标已存在，先 remove 再重试一次
                        if path.exists() {
                            if let Err(rm_err) = fs::remove_file(path) {
                                tracing::warn!(
                                    "atomic_write({}): remove stale target failed: {rm_err}",
                                    path.display()
                                );
                            }
                        }
                        match fs::rename(&tmp_path, path) {
                            Ok(()) => return Ok(()),
                            Err(e2) => {
                                let _ = fs::remove_file(&tmp_path);
                                if attempt + 1 == WRITE_RETRY_ATTEMPTS {
                                    return Err(format!("write {label} file: {e2}"));
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                // 写 tmp 失败
                if e.kind() == std::io::ErrorKind::NotFound && !parent.exists() {
                    // 父目录可能在并发下被删了，重建
                    let _ = fs::create_dir_all(parent);
                }
                if attempt + 1 == WRITE_RETRY_ATTEMPTS {
                    return Err(format!("write {label} tmp file: {e}"));
                }
            }
        }

        // 线性退避：20ms / 40ms / 60ms
        std::thread::sleep(std::time::Duration::from_millis(
            20 * (attempt as u64 + 1),
        ));
    }

    Err(format!(
        "write {label}: exhausted {WRITE_RETRY_ATTEMPTS} retries"
    ))
}

/// `sessions/` 目录路径：`<app_data_dir>/sessions/`。
pub fn sessions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir 不可用: {e}"))?;
    Ok(base.join("sessions"))
}

/// 单个会话目录：`<app_data_dir>/sessions/<conv_id>/`。
pub fn session_dir(app: &AppHandle, conv_id: &str) -> Result<PathBuf, String> {
    validate_conversation_id(conv_id)?;
    Ok(sessions_dir(app)?.join(conv_id))
}

/// `meta.json` 路径：`<app_data_dir>/sessions/<conv_id>/meta.json`。
pub fn meta_path(app: &AppHandle, conv_id: &str) -> Result<PathBuf, String> {
    Ok(session_dir(app, conv_id)?.join("meta.json"))
}

/// `messages.jsonl` 路径：`<app_data_dir>/sessions/<conv_id>/messages.jsonl`。
pub fn messages_path(app: &AppHandle, conv_id: &str) -> Result<PathBuf, String> {
    Ok(session_dir(app, conv_id)?.join("messages.jsonl"))
}

/// `index.json` 路径：`<app_data_dir>/sessions/index.json`。
pub fn index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(sessions_dir(app)?.join("index.json"))
}

/// 校验 conversation_id 格式：必须以 `conv_` 开头。
///
/// 防止 path traversal 攻击（`../../etc/passwd` 会被拒绝）。
/// 照搬 Kivio `validate_conversation_id`（前缀校验）。
pub fn validate_conversation_id(conv_id: &str) -> Result<(), String> {
    if !conv_id.starts_with("conv_") || conv_id.len() <= "conv_".len() {
        return Err(format!(
            "invalid conversation_id: must start with 'conv_' and have a non-empty suffix (got '{conv_id}')"
        ));
    }
    // 额外校验：只允许字母数字 + 连字符 + 下划线，防止 path traversal
    if !conv_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "invalid conversation_id: only alphanumeric, '_', '-' allowed (got '{conv_id}')"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// 在临时目录下创建一个测试用的 atomic_write 目标路径。
    fn tmp_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn atomic_write_writes_file() {
        let dir = TempDir::new().unwrap();
        let path = tmp_path(&dir, "test.json");
        atomic_write(&path, r#"{"a":1}"#, "test").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, r#"{"a":1}"#);
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = tmp_path(&dir, "test.json");
        atomic_write(&path, r#"{"v":1}"#, "test").unwrap();
        atomic_write(&path, r#"{"v":2}"#, "test").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, r#"{"v":2}"#);
    }

    #[test]
    fn atomic_write_creates_parent_dir() {
        let dir = TempDir::new().unwrap();
        // parent 目录不存在
        let path = dir.path().join("nested/deep/test.json");
        atomic_write(&path, r#"{"a":1}"#, "test").unwrap();
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, r#"{"a":1}"#);
    }

    #[test]
    fn validate_conversation_id_accepts_valid() {
        assert!(validate_conversation_id("conv_abc123").is_ok());
        assert!(validate_conversation_id("conv_550e8400-e29b-41d4-a716-446655440000").is_ok());
        assert!(validate_conversation_id("conv_a").is_ok());
    }

    #[test]
    fn validate_conversation_id_rejects_invalid() {
        // 缺少前缀
        assert!(validate_conversation_id("abc123").is_err());
        assert!(validate_conversation_id("proj_xxx").is_err());
        // 前缀后为空
        assert!(validate_conversation_id("conv_").is_err());
        // 包含 path traversal 字符
        assert!(validate_conversation_id("conv_../etc").is_err());
        assert!(validate_conversation_id("conv_a/b").is_err());
        assert!(validate_conversation_id("conv_a;b").is_err());
        // 空字符串
        assert!(validate_conversation_id("").is_err());
    }
}

//! 路径沙箱（Phase 2）：相对路径相对 cwd 解析；绝对路径 canonicalize 后通过；
//! `..` 允许（无 workspace 边界，Phase 2 简化）。
//!
//! 设计参考 Kivio `native_tools/mod.rs:87-126` 的 `resolve_tool_*_path`，
//! 但砍掉了 workspace / project 概念——Phase 2 假设"项目根 = cwd"。

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("invalid path: {0}")]
    Invalid(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// 把用户输入的路径解析成 canonicalize 后的绝对路径。
///
/// 规则：
/// - 空字符串 → `Err(PathError::Empty)`
/// - 相对路径 → 相对 `std::env::current_dir()` 解析
/// - 绝对路径 → 直接用
/// - 路径不存在也允许（支持 Write 新文件）；canonicalize 走 `existing_or_self` 模式
pub fn resolve_tool_path(raw: &str) -> Result<PathBuf, PathError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(PathError::Empty);
    }

    let path = Path::new(trimmed);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    Ok(fs_canonicalize_existing_or_self(&candidate))
}

fn fs_canonicalize_existing_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_errors() {
        assert!(matches!(resolve_tool_path(""), Err(PathError::Empty)));
        assert!(matches!(resolve_tool_path("   "), Err(PathError::Empty)));
    }

    #[test]
    fn relative_path_resolves_against_cwd() {
        // tempfile crate 不在 dev-deps 里；用 std::env::temp_dir() + uuid 手工拼。
        // 不引 tempfile 是为了避免新增 dev-dep；uuid 也不在 deps 里，
        // 改用时间戳后缀做唯一化（够测试用）。
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let rel = format!("smart_codeagent_test_{stamp}");
        let resolved = resolve_tool_path(&rel).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert!(resolved.starts_with(&cwd), "should be under cwd");
        let expected = cwd.join(&rel);
        // canonicalize 对不存在的 path 走 fallback，但回退到原 path；
        // 所以 resolved 与 expected 可能只差 canonicalize 后的形式。
        assert_eq!(resolved.file_name().unwrap(), expected.file_name().unwrap());
    }

    #[test]
    fn absolute_path_passes_through() {
        let p = if cfg!(windows) { "C:\\Windows" } else { "/tmp" };
        let resolved = resolve_tool_path(p).unwrap();
        assert!(resolved.is_absolute());
    }

    #[test]
    fn double_dot_allowed() {
        // `..` 在 Phase 2 允许（无 workspace 边界）
        let resolved = resolve_tool_path("../").unwrap();
        assert!(resolved.is_absolute());
        assert!(resolved.starts_with(std::env::current_dir().unwrap().parent().unwrap()));
    }

    #[test]
    fn nonexistent_path_returns_self() {
        let p = "/this/path/should/not/exist/anywhere_xyz_42";
        let resolved = resolve_tool_path(p).unwrap();
        // canonicalize 失败 → 返回原路径
        assert_eq!(resolved.to_string_lossy(), p);
    }

    #[test]
    fn existing_path_canonicalizes() {
        let tmp = std::env::temp_dir();
        let resolved = resolve_tool_path(tmp.to_str().unwrap()).unwrap();
        // canonicalize 后路径应该等于原路径
        assert_eq!(resolved, tmp.canonicalize().unwrap());
    }
}
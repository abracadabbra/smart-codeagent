//! Bash 黑名单 + host python install 检测。
//!
//! 设计参考 Kivio `native_tools/shell.rs:18-30` 的 `COMMAND_DENYLIST`，
//! 砍掉了他独有的一些 pattern（himalaya 邮件），增加通用 shell 危险命令。

/// 命中即拒的危险命令（前缀匹配，大小写不敏感）。
///
/// 命中后返回 `Err("command blocked by safety policy")`，
/// loop 转 `agent:tool_rejected` 事件。
pub const COMMAND_DENYLIST: &[&str] = &[
    "sudo ",
    "rm -rf /",
    "rm -rf /*",
    ":(){ :|:& };:",
    ":(){:|:&};:",
    "mkfs.",
    "dd if=/dev/zero",
    "> /dev/sd",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "chmod 777 /",
];

/// pip / uv 等会修改全局 Python 环境的命令。
///
/// 默认拒；除非调用方显式传 `allow_host_python_package_install: true`。
pub const HOST_PYTHON_PACKAGE_INSTALL_PATTERNS: &[&str] = &[
    "pip install",
    "pip3 install",
    "python -m pip install",
    "python3 -m pip install",
    "uv pip install",
];

/// 检查 command 是否命中 deny list（大小写不敏感）。
pub fn is_denied(command: &str) -> Option<&'static str> {
    let lowered = command.to_ascii_lowercase();
    COMMAND_DENYLIST
        .iter()
        .find(|denied| lowered.contains(*denied))
        .copied()
}

/// 检查 command 是否需要 `allow_host_python_package_install` 标记。
pub fn needs_host_python_opt_in(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    HOST_PYTHON_PACKAGE_INSTALL_PATTERNS
        .iter()
        .any(|p| lowered.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_rm_rf_root() {
        assert!(is_denied("rm -rf /").is_some());
        assert!(is_denied("sudo rm -rf /tmp").is_some()); // sudo 也命中
        assert!(is_denied("RM -RF /").is_some()); // case insensitive
    }

    #[test]
    fn blocks_dd_zero() {
        assert!(is_denied("dd if=/dev/zero of=/dev/sda").is_some());
        assert!(is_denied("sudo dd if=/dev/zero").is_some());
    }

    #[test]
    fn blocks_mkfs() {
        assert!(is_denied("mkfs.ext4 /dev/sda1").is_some());
        assert!(is_denied("mkfs.xfs /dev/sdb").is_some());
    }

    #[test]
    fn blocks_fork_bomb_both_forms() {
        assert!(is_denied(":(){ :|:& };:").is_some());
        assert!(is_denied(":(){:|:&};:").is_some());
    }

    #[test]
    fn blocks_shutdown_reboot() {
        assert!(is_denied("shutdown -h now").is_some());
        assert!(is_denied("reboot").is_some());
        assert!(is_denied("halt").is_some());
        assert!(is_denied("poweroff").is_some());
    }

    #[test]
    fn blocks_chmod_777_root() {
        assert!(is_denied("chmod 777 / ").is_some());
    }

    #[test]
    fn allows_safe_commands() {
        assert!(is_denied("ls -la").is_none());
        assert!(is_denied("cargo build").is_none());
        assert!(is_denied("rm -rf ./build").is_none()); // 局部 rm 允许
        assert!(is_denied("echo hello").is_none());
    }

    #[test]
    fn host_python_needs_opt_in() {
        assert!(needs_host_python_opt_in("pip install requests"));
        assert!(needs_host_python_opt_in("python3 -m pip install foo"));
        assert!(needs_host_python_opt_in("uv pip install bar"));
        assert!(!needs_host_python_opt_in("pip list"));
        assert!(!needs_host_python_opt_in("python --version"));
    }
}
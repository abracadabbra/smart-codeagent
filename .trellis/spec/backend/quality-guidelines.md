# Quality Guidelines

> Code quality standards for backend development.

---

## Overview

This document covers linting, formatting, testing, and code review for
`src-tauri/`. The goal is **catch regressions in CI, not in code review**
— every rule below should be enforced by `cargo` or `clippy`, not by
humans arguing in PR threads.

---

## Required Patterns

- **`cargo fmt --check`** in CI. Run `cargo fmt` before committing.
- **`cargo clippy -- -D warnings`** in CI. Treat warnings as errors.
- **`#[serde(rename_all = "camelCase")]`** on every struct that crosses
  the Tauri IPC boundary (commands + event payloads). This is the
  single most-bug-prone serialization decision in this project;
  the test suite in `src-tauri/tests/ipc_payload_contract.rs` pins it.
- **`#[derive(Debug, Clone)]`** on every domain struct that may be
  cloned or formatted. Streams and trait objects are exceptions.
- **Async on `tokio` runtime** for I/O. Never wrap blocking calls
  inside `#[tauri::command]` without `tokio::task::spawn_blocking`.

---

## Forbidden Patterns

- **`unwrap()` / `expect()` in shared code paths.** Only acceptable in
  constructors that build once at startup or in tests.
- **`std::sync::Mutex` for long-held locks.** Use `tokio::sync::Mutex`
  for anything that crosses an `.await` point.
- **Hardcoded URLs, model names, or API keys** in code. Always read
  from `crate::config::AnthropicConfig::from_env()`.
- **`async-trait` with `dyn` for hot-path dispatch** in Phase 1+2.
  The trait is fine for Phase 1 (single impl). When adding a second
  provider, benchmark first; `Box<dyn Trait>` has measurable cost.

---

## Testing Requirements

- **Every IPC payload struct** gets a serialization contract test
  (see `tests/ipc_payload_contract.rs` as the template).
- **Provider parsing logic** must have a unit test that feeds real SSE
  bytes recorded from a successful run. Don't synthesize fake bytes.
- **No end-to-end Tauri tests yet** (no headless WebDriver setup).
  Manual UI verification before archiving a phase is required.
- **All tests pass** before `task.py archive`. A failed `cargo test`
  blocks the archive.

---

## Code Review Checklist

- [ ] New env var consumed? Search `ANTHROPIC_` and `LLM_` to make sure
      no old name lingers (see `cross-layer-thinking-guide`).
- [ ] New event payload? Mirrors a Rust struct with `rename_all`? Has a
      matching TS type in `src/types/`?
- [ ] New `async fn`? Holds no `std::sync::MutexGuard` across `.await`?
- [ ] New error variant? Plumbed through `From` impls, not just stringified?
- [ ] New public API? Documented in the relevant spec file
      (`error-handling.md`, `directory-structure.md`, etc.)?
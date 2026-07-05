# Error Handling

> How errors are handled in this project.

---

## Overview

This project uses a **layered error strategy**: domain enums at module
boundaries (`thiserror`), opaque `anyhow` at the top of the call stack,
and `String` at the Tauri IPC boundary. Every error must reach the
frontend with enough context to render in the status bar / chat region.

---

## Error Types

Use `thiserror::Error` enums per module — never `String` for domain errors.

```rust
// src-tauri/src/providers/mod.rs
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("SSE parse error: {0}")]
    SseParse(String),
    #[error("API error: {status} — {message}")]
    Api { status: u16, message: String },
    #[error("config error: {0}")]
    Config(String),
}
```

Convention: 1 enum per module, variants mirror failure modes the caller
can act on (HTTP vs SSE parse vs upstream API vs config).

---

## Propagation

- **Inside the module**: `Result<T, ModuleError>` via `?`.
- **Across modules**: convert via `From` impls on the receiving enum.
  Never `unwrap()` in shared code paths.
- **Top of the stack** (`AgentLoop::run_inner`): `anyhow::Result<()>`.
  `?` is fine here — final conversion happens at the IPC boundary.
- **Panics** are reserved for: missing required env vars on startup
  (PRD §AC9 — fail loud, do not silently default), and
  `expect("invariant that the type system already proves")` in
  constructors that build once.

---

## IPC Boundary

`#[tauri::command]` returns `Result<T, String>`. Convert by formatting:

```rust
// src-tauri/src/agent/loop_.rs
return Err(anyhow::anyhow!(e));  // collects chain
```

…and in the command itself, return the formatted chain to JS:

```rust
#[tauri::command]
pub async fn send_message(...) -> Result<(), String> {
    agent.spawn_run(text, assistant_id);
    Ok(())  // spawn_run errors surface via agent:error event, NOT here
}
```

Errors that occur mid-stream must be pushed via `emit_error` so the
frontend can attach them to the current assistant message; do **not**
return them from the command.

---

## Logging Errors

- `tracing::error!` for failures the user must see.
- `tracing::warn!` for degraded-but-recoverable paths
  (e.g. SSE line we couldn't parse — skip, don't kill stream).
- `tracing::debug!` for verbose parse / retry context.
- Never log secrets (API keys, full request bodies).

---

## Common Mistakes

- **Returning `Result<_, String>` from a domain module.** Loses type
  info and forces string-matching upstack.
- **Logging the error AND returning it.** Choose one: either log and
  swallow (recoverable, e.g. SSE skip), or return and let the
  boundary log it (actionable).
- **Mapping every error to `anyhow!` early.** Kills the ability to
  match on variants. Keep domain enums until the very top.

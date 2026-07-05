# Logging Guidelines

> How logging is done in this project.

---

## Overview

This project uses **`tracing`** with the `tracing-subscriber` registry.
The subscriber is initialized in `src-tauri/src/lib.rs::init_tracing()`
and reads `RUST_LOG` from env, defaulting to `info`. Logs go to stderr
(visible in `tauri dev` terminal); there is no file rotation in Phase 1.

---

## Log Levels

| Level | When to use |
|-------|-------------|
| `error` | User-visible failure: request failed, stream aborted, config invalid. Frontend must surface this. |
| `warn` | Degraded but recoverable: SSE line we couldn't parse, `emit!` failed because no AppHandle attached yet, retry attempt. |
| `info` | Lifecycle events: command invoked, agent state transition, stream completed. Useful for debugging user-reported flows. |
| `debug` | Verbose parse / retry context: SSE chunk received, JSON decode succeeded, content_block type seen. Off by default. |
| `trace` | Reserved for future use (request/response bodies, timing). Not currently emitted. |

---

## Structured Fields

Use the `%` (Display) or `?` (Debug) formatters consistently. Field
naming follows snake_case:

```rust
tracing::info!(
    msg_id = %id,
    bytes = chunk.len(),
    "received SSE chunk"
);
```

Avoid string interpolation in the message body — fields are easier to
filter and aggregate:

```rust
// BAD
tracing::info!("received SSE chunk for {id}: {} bytes", chunk.len());
// GOOD
tracing::info!(msg_id = %id, bytes = chunk.len(), "received SSE chunk");
```

---

## What to Log

- **Command entry/exit**: every `#[tauri::command]` should log on entry
  with the args (truncated if large) and on error.
- **Agent state transitions**: `Idle → Prepare → Stream → Stop → Idle`
  transitions, with the reason for any non-happy-path exit.
- **Provider lifecycle**: request sent, stream opened, stream ended,
  total tokens seen.
- **Recoverable failures**: skipped SSE lines, transient retry attempts.

---

## What NOT to Log

- **API keys** or any substring of `LLM_API_KEY`. Never include in a
  log field, even at `debug`.
- **Full request/response bodies** in production builds. They may
  contain user PII or proprietary code snippets.
- **SSE `thinking_delta` content** at `info` or above — it can be
  very large (1M context model). `debug` only.
- **Tauri AppHandle internals** beyond what `Emitter::emit` already
  gives you.

---

## Common Mistakes

- **Logging and returning the same error.** Decide: either log and
  swallow (recoverable), or return and let the boundary log it
  (actionable). See `error-handling.md`.
- **`println!` instead of `tracing::info!`.** Bypasses the level
  filter and shows up in `cargo test` output as noise.
- **Logging PII in test fixtures.** Pin fake payloads with synthetic
  IDs (`"asst-test"`) to avoid accidentally logging real data later.
- **`tracing::error!` for recoverable SSE skip paths.** Use `debug!`
  or `warn!` — `error!` triggers pager fatigue and misleads grep.
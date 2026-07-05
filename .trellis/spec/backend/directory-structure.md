# Directory Structure

> How backend code is organized in this project.

---

## Overview

`src-tauri/src/` is split into **four modules by concern**, plus two
top-level files for crate entry. The boundary between modules is the
direction of dependency: lower layers know nothing about upper layers.
Adding a new module requires placing it in the layer that matches its
dependency direction, not its feature name.

```
src-tauri/
├── Cargo.toml
├── tauri.conf.json
├── build.rs
└── src/
    ├── main.rs          # binary entry: load .env, init tracing, run lib
    ├── lib.rs           # library entry: pub mods + tauri::Builder setup
    ├── config.rs        # env → typed config (LLM_API_KEY, base_url, model)
    ├── agent/
    │   ├── mod.rs       # AgentState enum + Message struct + pub use loop_
    │   └── loop_.rs     # AgentLoop struct: 4-state state machine
    ├── ipc/
    │   ├── mod.rs       # re-exports commands + events
    │   ├── commands.rs  # #[tauri::command] entry points
    │   └── events.rs    # event payload structs + emit_* helpers
    └── providers/
        ├── mod.rs       # Provider trait + MessagesRequest + ProviderError
        └── anthropic.rs # Anthropic Messages API SSE impl (SenseNova-compatible)
```

---

## Module Boundaries

```
config ─────────► providers ─────────► agent ─────────► ipc ─────────► lib
  env vars          trait + impl         state machine    bridge to JS    composition root
```

| Module | Owns | Knows about | Forbidden from |
|--------|------|-------------|----------------|
| `config` | env → struct conversion | std env | touching `agent` / `ipc` / `providers` |
| `providers` | `Provider` trait, `MessagesRequest`, `ProviderError`, SSE parsing | `crate::agent::Message`, `crate::config` | touching `ipc` / tauri types |
| `agent` | `AgentState`, `Message`, `AgentLoop`, history | `providers`, `ipc::events` (emits) | defining new providers |
| `ipc` | Tauri command handlers + event payload structs | `agent`, tauri | provider impls |
| `lib` | composition root, tracing init | everything (above) | — |

The cycle `providers → agent → ipc → providers` would be a red flag;
guard with `cargo build` failing on circular deps.

---

## Where to Add New Code

- **New LLM provider** (e.g. OpenAI-compatible) → `providers/<name>.rs`,
  register in `providers/mod.rs::default_provider()`. Do not move
  existing files around.
- **New Tauri command** → `ipc/commands.rs`. Add `emit_*` helper to
  `ipc/events.rs` if you need a new event kind. Always mirror the
  payload struct in `src/types/` (frontend) with the same camelCase
  fields.
- **New agent state** (e.g. `Recover`, `ToolCall`) → edit
  `agent/mod.rs::AgentState` enum + add transition method on
  `agent/loop_.rs`. Update `src/types/agent.ts` to match.
- **New tool** (Phase 2) → `tools/<tool_name>.rs` as a new module,
  add to `lib.rs::pub mod tools`. Keep `loop_.rs` ignorant of
  specific tools — it should dispatch via a registry.

---

## File Naming

- `mod.rs` for module entry. **No** `lib.rs` inside submodules.
- Module names are singular when they own a single concern
  (`config`, `agent`); plural when they own a category
  (`providers`, `tools`, `commands`).
- `loop_.rs` uses a trailing underscore because `loop` is a Rust
  keyword. Do not rename — module-relative paths are stable.
- Test files live in `src-tauri/tests/` (integration) or
  `#[cfg(test)] mod tests` at the bottom of the source file (unit).

---

## Examples

The cleanest reference for "what a module should look like":

- **Smallest module**: `ipc/mod.rs` (5 lines, two re-exports).
  Use this shape when a module only exists to group submodules.
- **Trait + impls module**: `providers/mod.rs` (Provider trait,
  error enum, request struct, default constructor). Use this shape
  when introducing a new category of pluggable backends.
- **Single-purpose module**: `config.rs` (env → struct). One file,
  no submodule needed until there are 3+ independent config groups.

---

## Common Mistakes

- **Adding code to `lib.rs` that isn't composition root.**
  `lib.rs` should only: declare `pub mod`s, init tracing, build the
  Tauri runtime, register handlers.
- **Reaching across the module graph.** `providers/anthropic.rs`
  importing from `ipc::events` would invert the dependency direction.
  The trait's output type is opaque (`TokenStream`); emission
  happens in `agent` or `ipc`, not in providers.
- **Putting a `pub use` chain that hides where a type lives.**
  `pub use foo::Bar` is fine for re-export at module root, but do
  not stack 3 layers of re-export — `grep` will mislead reviewers.
- **Splitting a 60-line module into `mod.rs` + 1 file** prematurely.
  Submodules are for files >200 lines or for parallel ownership
  (e.g. multiple providers).
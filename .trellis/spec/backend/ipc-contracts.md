# Tauri 2 IPC Contracts

> Cross-layer contract spec for Tauri commands + events.
> Phase 2 (tool system) introduced the patterns here; future phases MUST follow them.

---

## Scenario: Tauri 2 Managed State + IPC Serde Contract

### 1. Scope / Trigger

Trigger: any task that adds a new `#[tauri::command]`, a new event payload in `src/ipc/events.rs`,
or shares state between commands and the agent loop. This is mandatory code-spec territory
because the bugs here are silent: TypeScript compiles, Rust compiles, but the runtime
contract breaks (commands can't find state, frontend payloads don't deserialize).

### 2. Signatures

Two kinds of Tauri 2 IPC surfaces:

**Commands (frontend → backend, `invoke`)**

```rust
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    agent: State<'_, Arc<AgentLoop>>,
    text: String,
    assistant_id: String,        // snake_case in Rust
    run_id: String,
    generation: u64,
) -> Result<(), String> { ... }
```

```typescript
// Frontend invoke — keys MUST be camelCase to match Rust param names
// after Tauri 2's automatic snake↔camel boundary.
// Tauri 2 does NOT convert; the JSON key must equal the Rust param name verbatim.
await invoke("send_message", {
  text,
  assistantId,   // ← camelCase
  runId,
  generation,
});
```

**Events (backend → frontend, `emit` / `listen`)**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]          // ← MANDATORY on every payload struct
pub struct AgentApprovalRequestPayload {
    pub approval_id: String,                // serializes as "approvalId"
    pub run_id: String,                     // serializes as "runId"
    pub tool_call_id: String,               // serializes as "toolCallId"
    pub sensitive: bool,
}
```

### 3. Contracts

**Shared state rule (CRITICAL)**

If a command resolves a pending oneshot channel held by the agent loop
(approval / ask_user / cancel), the `TauriHost` MUST be constructed ONCE in
`lib.rs::run()` `setup(|app| ...)` and registered via `app.manage(Arc<TauriHost>)`.

The agent loop retrieves it via `try_state`:

```rust
let host: Arc<TauriHost> = app_handle
    .try_state::<Arc<TauriHost>>()
    .map(|s| s.inner().clone())
    .ok_or_else(|| anyhow::anyhow!("TauriHost not managed"))?;
```

Commands retrieve the SAME instance:

```rust
#[tauri::command]
pub async fn approve_tool(app: AppHandle, args: ApproveToolArgs) -> Result<(), String> {
    let host = app
        .try_state::<Arc<TauriHost>>()
        .ok_or_else(|| "TauriHost not managed".to_string())?;
    host.resolve_approval(&args.approval_id, args.allow);
    Ok(())
}
```

**Serde rename rule**

| Direction | Struct attribute | Key style on the wire |
|---|---|---|
| Backend → frontend (event payload) | `#[serde(rename_all = "camelCase")]` | camelCase JSON |
| Frontend → backend (command args struct) | `#[serde(rename_all = "camelCase")]` | camelCase JSON |
| Frontend → backend (top-level command params) | none — Tauri 2 maps JSON key ↔ Rust param name 1:1 | camelCase JSON key == snake_case Rust param (Tauri 2 auto-maps) |

The third row is the trap: Tauri 2 DOES auto-convert for top-level command params
(`assistantId` ↔ `assistant_id`), but for nested structs in `args: SomeArgs`,
you must add `#[serde(rename_all = "camelCase")]` yourself or deserialization fails silently.

**Command registration rule**

Every `#[tauri::command]` fn MUST appear in `tauri::generate_handler![...]` in `lib.rs::run()`.
A command defined but not registered returns "command not found" at invoke time — TypeScript
won't catch it.

### 4. Validation & Error Matrix

| Condition | Symptom | Error |
|---|---|---|
| Command fn not in `generate_handler!` | `invoke` rejects with "command not found" | Frontend `console.error`, no Rust log |
| `TauriHost` constructed per-run instead of managed | `approve_tool` `try_state` returns `Some`, resolves a DIFFERENT instance's map → `None` | Loop's oneshot `rx` times out at 60s |
| `TauriHost` not managed at all | `try_state` returns `None` | `"TauriHost not managed"` Err string |
| Payload struct missing `#[serde(rename_all = "camelCase")]` | Frontend gets `runId: undefined`, sees `run_id` in JSON | TS type lies (compiles, runtime undefined) |
| Args struct missing `#[serde(rename_all = "camelCase")]` | Backend deserialization fails silently | `invoke` rejects with deserialization error |
| Top-level command param named `assistant_id` but frontend sends `assistant_id` (snake) | Tauri 2 does NOT auto-convert at top level in all versions | Param arrives as `undefined` / default |

### 5. Good / Base / Bad Cases

**Good** — managed singleton, both sides share:

```rust
// lib.rs
.setup(|app| {
    let host: Arc<TauriHost> = Arc::new(TauriHost::new(app.handle().clone()));
    app.manage(host);
    let agent: Arc<AgentLoop> = Arc::new(AgentLoop::new(cfg));
    app.manage(agent);
    Ok(())
})
```

```rust
// loop_.rs
let host: Arc<dyn AgentHost> = {
    let h: Arc<TauriHost> = handle
        .try_state::<Arc<TauriHost>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| anyhow::anyhow!("TauriHost not managed"))?;
    h.register_generation(&run_id, generation);
    h as Arc<dyn AgentHost>
};
```

**Base** — command args struct with explicit rename:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]        // ← don't forget this
pub struct ApproveToolArgs {
    pub approval_id: String,
    pub allow: bool,
}
```

**Bad** — per-run host construction (causes silent oneshot timeout):

```rust
// WRONG: each run gets its own TauriHost; commands can't reach this instance
let host = Arc::new(TauriHost::new(handle.clone()));
host.register_generation(&run_id, generation);
// ... later, approve_tool's try_state finds a DIFFERENT (managed) instance
// → resolve_approval returns false → loop's rx.await times out at 60s
```

### 6. Tests Required

Contract tests live in `src-tauri/tests/ipc_payload_contract.rs`. Each payload struct
gets one test that:

1. Constructs the struct with snake_case Rust fields
2. Serializes to `serde_json::Value`
3. Asserts camelCase keys exist (`json["runId"]`)
4. Asserts snake_case keys absent (`json.get("run_id").is_none()`)
5. For nested records (e.g. `ToolCallRecord` inside `AgentToolRecordPayload`), asserts
   nested camelCase too

For command args structs (frontend → backend), also assert camelCase JSON deserializes
AND snake_case JSON does NOT (to catch a future regression where someone removes the
`#[serde(rename_all = "camelCase")]` attribute).

```rust
#[test]
fn approve_tool_args_deserializes_camel_case() {
    use smart_codeagent_lib::ipc::commands::ApproveToolArgs;
    let json = serde_json::json!({ "approvalId": "appr-1", "allow": true });
    let args: ApproveToolArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.approval_id, "appr-1");

    let snake = serde_json::json!({ "approval_id": "x", "allow": false });
    assert!(serde_json::from_value::<ApproveToolArgs>(snake).is_err());
}
```

Phase 2 baseline: 13 contract tests (5 Phase 1 + 8 Phase 2). Add one per new payload.

### 7. Wrong vs Correct

**Wrong — payload struct without serde rename**

```rust
#[derive(Debug, Clone, Serialize)]
pub struct AgentStreamDeltaPayload {
    pub run_id: String,
    pub msg_id: String,
    pub text: String,
    pub reasoning_delta: Option<String>,
}
// Serializes as {"run_id": ..., "msg_id": ..., "reasoning_delta": ...}
// Frontend type says { runId, msgId, reasoningDelta } → all undefined at runtime
```

**Correct**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStreamDeltaPayload {
    pub run_id: String,
    pub msg_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_delta: Option<String>,   // None → field omitted
}
```

---

## Common Mistake: Per-Run TauriHost Construction

**Symptom**: User clicks "approve" in `<ApprovalDialog>`, the modal closes,
but the agent loop's `request_tool_approval` future hangs for 60 seconds then
returns `false` (timeout path).

**Cause**: The agent loop constructed its own `TauriHost::new(handle)` per run,
so its `pending_approvals` map is a different `HashMap` than the one `approve_tool`
command mutates via `try_state::<Arc<TauriHost>>()`.

**Fix**: Construct `TauriHost` ONCE in `lib.rs::run()` `setup`, `app.manage(Arc::new(host))`,
and have the loop retrieve it via `try_state` each run.

**Prevention**: The contract test `approve_tool_args_deserializes_camel_case` catches
the serde side; the managed-state side is enforced by the pattern "if a command and
the loop both touch type X, X is managed in `lib.rs` and never `X::new()` elsewhere."

---

## Convention: Event Payload Skip-None Fields

Optional payload fields SHOULD use:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub reasoning_delta: Option<String>,
```

so absent fields don't appear as `"reasoningDelta": null` on the wire. This keeps
frontend optional-field semantics (`reasoningDelta?: string`) honest.

Required fields (e.g. `runId`, `msgId`) MUST NOT skip — they always serialize.

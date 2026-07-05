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

---

## Scenario: LLM Provider Protocol — OpenAI Chat Completions Only

### 1. Scope / Trigger

Trigger: any task that adds or modifies the LLM provider (`src-tauri/src/providers/`),
changes the `Message` struct, or wires tool calling. This spec exists because we
burned ~3 hours debugging "tool_use as plain text" symptoms caused by using the
wrong API protocol for the wrong model family.

### 2. Decision

**Use OpenAI `/v1/chat/completions` exclusively, NOT Anthropic `/v1/messages`.**

This applies even when the provider is SenseNova (which exposes both endpoints).
The file is still named `anthropic.rs` for historical reasons (Phase 1 used the
Anthropic protocol), but the implementation talks OpenAI.

### 3. Why (the trap we fell into)

SenseNova's `/v1/messages` endpoint **only translates tool_use content blocks for
Claude-family models**. For DeepSeek / Qwen / GLM models (which is what we ship by
default — `deepseek-v4-flash`), `/v1/messages` silently degrades tool calling:

- LLM emits `tool_use` JSON as plain text (`[{"id":"call_xxx","input":{...},"name":"read_file","type":"tool_use"}]`)
- The `call_` ID prefix is OpenAI format (Anthropic would be `toolu_`)
- `content_block_start` / `input_json_delta` SSE events never arrive
- `delta.content` carries the whole JSON blob as text

Switching to `/v1/chat/completions` makes DeepSeek/Qwen/GLM use native OpenAI
tool calling (`delta.tool_calls[].function.arguments` streaming) and works correctly.

### 4. Contracts

**Endpoint**: `POST {base_url}/v1/chat/completions`

**Request body shape**:
```json
{
  "model": "deepseek-v4-flash",
  "max_tokens": 8192,
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": "..."},
    {"role": "assistant", "content": null, "tool_calls": [...]},
    {"role": "tool", "tool_call_id": "call_xxx", "content": "..."}
  ],
  "stream": true,
  "tools": [{"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}]
}
```

**SSE response shape** (OpenAI streaming):
- `delta.content` — text deltas (skip empty strings)
- `delta.tool_calls[].index` — distinguishes parallel tool_calls (0, 1, 2...)
- First `delta.tool_calls` block for an index carries `id` + `function.name` + (optional) `arguments`
- Subsequent blocks carry only `function.arguments` deltas (JSON string fragments)
- `finish_reason: ""` (empty string) on intermediate chunks — MUST be treated as `None`
- `finish_reason: "tool_calls"` — all tool_calls done, emit `ToolUseEnd` + `Done`
- `finish_reason: "stop"` / `"length"` / `"content_filter"` — final, emit `Done`
- `data: [DONE]` — stream end

**Critical quirk**: SenseNova (and many OpenAI-compatible providers) emit
`finish_reason: ""` on every intermediate chunk. Treat empty-string `finish_reason`
as `None`, otherwise the first chunk triggers `StreamChunk::Done` and the stream
ends immediately with no output.

### 5. Message struct serde contract

`agent::Message` has `#[serde(rename_all = "camelCase")]` at the struct level
(for internal consistency and because some downstream consumers expect camelCase).
**But two fields MUST be force-renamed to snake_case** because they go on the wire
to the LLM API, which strictly requires snake_case:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    /// OpenAI tool_calls — MUST be snake_case on the wire
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "tool_calls")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
    /// OpenAI tool_call_id — MUST be snake_case on the wire
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "tool_call_id")]
    pub tool_call_id: Option<String>,
}
```

If these `rename` attributes are removed, serde serializes `tool_calls` as
`toolCalls` and `tool_call_id` as `toolCallId`. The OpenAI API silently ignores
unknown fields, so:
- assistant message loses its `tool_calls` array
- the following `role: "tool"` message references a `tool_call_id` that the API
  no longer recognizes
- API returns `400 invalid tool_call_id` on every multi-round tool call

Regression test: `providers::anthropic::tests::message_serializes_openai_snake_case`
asserts both that `tool_calls` (snake) exists AND `toolCalls` (camel) does not.

### 6. Tool call flow (multi-round)

```
User text
   ↓
LLM stream → delta.tool_calls (id + name + arguments fragments)
   ↓ finish_reason="tool_calls"
emit ToolUseEnd → parse all accumulated input_raw → tool_uses vec
   ↓
dispatch_round: execute each tool_use
   ↓
append to history:
   Message::assistant_tool_calls(vec![OpenAiToolCall { id, type:"function", function:{name, arguments: JSON_string} }])
   Message::tool_result(tool_call_id, content)   // one per tool
   ↓
Next LLM round with full history
```

**Key invariant**: every `tool_call_id` in the assistant message's `tool_calls`
MUST have a corresponding `role: "tool"` message immediately after, with matching
`tool_call_id`. Missing or mismatched IDs cause `400 invalid tool_call_id`.

### 7. Validation & Error Matrix

| Condition | Symptom | Error |
|---|---|---|
| Using `/v1/messages` with DeepSeek model | LLM emits tool_use as plain text (`call_` IDs visible in chat) | No error; silent degradation |
| `finish_reason: ""` treated as `Some("")` | Stream ends after first chunk, no output | No error; silent |
| `tool_calls` field serialized as `toolCalls` | First round works, second round 400 | `400 invalid tool_call_id` |
| `tool_call_id` mismatched between assistant + tool message | 400 on next round | `400 invalid tool_call_id` |
| `arguments` not a JSON string (e.g. raw object) | 400 on request | `400 invalid_request_error` |

### 8. Tests Required

- `parses_text_delta` — `delta.content` produces `StreamChunk::Text`
- `parses_tool_call_start_and_args` — first block emits `ToolUseStart`, subsequent blocks emit `ToolUseInputDelta`
- `parses_finish_reason_tool_calls` — `tool_calls` emits `ToolUseEnd` + `Done`
- `parses_finish_reason_stop` — `stop` emits `Done` only
- `parses_done_marker` — `[DONE]` emits `Done`
- `handles_multiple_tool_calls_in_parallel` — two indices each get their own `ToolUseStart`
- `openai_tool_call_serializes_correctly` — `OpenAiToolCall` JSON has `id`/`type`/`function.name`/`function.arguments`
- `message_serializes_openai_snake_case` — `Message` serializes `tool_calls` (snake) not `toolCalls` (camel)

### 9. Wrong vs Correct

**Wrong — using `/v1/messages` with a non-Claude model**

```rust
let url = format!("{}/v1/messages", self.cfg.base_url);
// Works for Claude, silently breaks tool calling for DeepSeek/Qwen/GLM
```

**Correct — always use `/v1/chat/completions`**

```rust
let url = format!("{}/v1/chat/completions", self.cfg.base_url);
// Native OpenAI tool calling works for all model families
```

**Wrong — treating empty `finish_reason` as a real value**

```rust
if let Some(reason) = choice.finish_reason.as_deref() {
    // Some("") enters here! Triggers Done on every chunk.
    emit_done(reason);
}
```

**Correct — filter empty strings**

```rust
let reason_opt = choice
    .finish_reason
    .as_deref()
    .filter(|s| !s.is_empty());
if let Some(reason) = reason_opt { ... }
```

**Wrong — letting struct-level camelCase leak into API field names**

```rust
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub tool_calls: Option<Vec<...>>,       // serializes as "toolCalls" — WRONG
    pub tool_call_id: Option<String>,       // serializes as "toolCallId" — WRONG
}
```

**Correct — per-field rename override**

```rust
#[serde(rename_all = "camelCase")]
pub struct Message {
    #[serde(rename = "tool_calls")]
    pub tool_calls: Option<Vec<...>>,       // serializes as "tool_calls" — correct
    #[serde(rename = "tool_call_id")]
    pub tool_call_id: Option<String>,       // serializes as "tool_call_id" — correct
}
```

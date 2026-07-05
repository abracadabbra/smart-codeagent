# Type Safety

> Type safety patterns in this project.

---

## Overview

This project is **`tsc --strict` + zero runtime validation in Phase 1**.
The contract is enforced by the compiler and by `src-tauri/tests/`
(serialization round-trip tests). The single biggest type-safety risk
is the Tauri IPC boundary, where the Rust side controls field names —
the camelCase contract pinned in `ipc_payload_contract.rs` is the
defense.

---

## Type Organization

- **Domain types live in `src/types/`** when consumed by 2+ modules.
  Single-use types live next to their consumer (e.g. `TokenPayload`
  in `useAgentEvents.ts`).
- **Types mirror Rust structs**, not the other way around. When you
  change a Rust payload struct, the matching TS type changes with it
  in the same commit.
- **One file per domain**: `agent.ts`, `message.ts`. Don't group
  unrelated types under a generic `common.ts`.

```ts
// src/types/agent.ts
export type AgentState = "Idle" | "Prepare" | "Stream" | "Stop";
// Kept in sync with src-tauri/src/agent/mod.rs AgentState enum
```

The "kept in sync with …" comment is mandatory on every type that
mirrors a Rust enum or struct. Reviewers grep for it on Rust changes.

---

## IPC Contract (camelCase)

`#[serde(rename_all = "camelCase")]` is the single most-bug-prone
serialization decision in this project. Phase 1.2 had a regression
where the frontend read `p.msgId` but Rust emitted `msg_id` — the
event handler silently got `undefined`, so no token ever appended.

The contract is now pinned by **two** mechanisms:

1. **Rust side**: every payload struct in `src-tauri/src/ipc/events.rs`
   carries `#[serde(rename_all = "camelCase")]`. Verified by the 5
   tests in `tests/ipc_payload_contract.rs`.
2. **Frontend side**: the TS interfaces in `useAgentEvents.ts`
   (`TokenPayload`, `StatusPayload`, `ErrorPayload`, `DonePayload`)
   must use the camelCase names — `msgId`, not `msg_id`.

If you add a new event payload:

```ts
// In useAgentEvents.ts (or the equivalent hook)
interface MyNewPayload {
  msgId: string;     // mirrors msg_id in Rust
  someField: string; // mirrors some_field in Rust
}
```

And add a Rust-side test case to `ipc_payload_contract.rs`. Both
sides must be touched in the same commit, or one will silently
break the other.

---

## Type Narrowing for Tauri Payloads

`listen<unknown>` and `invoke` are intentionally typed loosely — the
runtime payload comes from Rust and TS cannot verify it statically.
Use a typed interface and **a single cast at the boundary**, not
per-field `as` chains:

```ts
// GOOD — one cast, then narrow access
const p = e.payload as TokenPayload;
prepareAssistantMessage(p.msgId);
appendToken(p.msgId, p.text);

// BAD — repeated casts leak the untyped shape into handlers
const msgId = (e.payload as { msgId?: string }).msgId;
const text = (e.payload as { text?: string }).text;
```

The second pattern duplicates the contract in every handler; if a
field is renamed, you must find every cast site. The first pattern
centralizes the contract in the interface.

---

## Forbidden Patterns

- **`any`** — disabled by `strict: true`. Use `unknown` and narrow.
- **`as unknown as T`** — double-cast to bypass checks. Means the
  type is lying; refactor the producer or add a runtime check.
- **Repeated `(payload as { field?: T }).field`** casts across
  multiple handlers — centralize in a single typed interface (see
  above).
- **Per-feature generic types** like `AgentEventMap<K>` that try to
  derive the payload type from the event name. This adds machinery
  without solving any real problem; just write 4 typed interfaces.
- **Optional fields used to express "may not be set yet"** in domain
  types (e.g. `content?: string` on `Message`). Prefer non-optional
  with empty-string default, or a discriminated union. Optional
  fields disable exhaustive checks.

---

## Runtime Validation

There is **no runtime validation** in Phase 1. Tauri commands trust
the frontend payload; the Rust side reads `String` parameters
without parsing. This is intentional — the frontend is the only
caller, in the same Tauri process.

When Phase 3 adds external input (e.g. file paths from disk, paste
from clipboard), introduce **Zod** or **`serde::Deserialize` + ts-type
generation**. Do not hand-roll validators — they drift from the
type definitions immediately.

---

## Adding a New TS Type

1. Decide: shared across modules → `src/types/<domain>.ts`; single-use
   → next to consumer.
2. If it mirrors a Rust struct, add a "kept in sync with …" comment.
3. If it's an IPC payload, add the matching `#[serde(rename_all =
   "camelCase")]` test case to `ipc_payload_contract.rs`.
4. Run `npm run type-check` — must pass with no errors and no
   warnings (the existing `tsconfig.json` already enables
   `noUnusedLocals` + `noUnusedParameters`).

---

## Common Mistakes

- **Inferring Rust field names from TS field names (or vice versa)**
  without verifying. The `ipc_payload_contract.rs` tests are the
  source of truth, not your memory.
- **Renaming a Rust field without touching TS.** The contract test
  will fail in CI if the test exists; if it doesn't, you've created
  a silent regression.
- **Adding `?` to a Rust field that the TS side doesn't have.**
  `Option<String>` in Rust serializes as `null | string` in JSON,
  not as a missing key — TS should mark the field optional, but the
  Rust side must still emit the key.
- **Using `enum` (closed) when a `union` of strings (open) is
  intended.** We use string unions (`"Idle" | "Prepare" | …`) so
  that adding a state in Rust doesn't require a parallel TS change
  to compile — the runtime guards catch the mismatch.
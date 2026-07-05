# State Management

> How state is managed in this project.

---

## Overview

This project uses **Zustand** for global state and **React local state**
(`useState`, `useRef`) for everything else. There is no Redux, no Context
Provider, no server-state cache (no React Query yet). The rule is simple:
if a value is consumed by more than one component tree branch, it goes
in a store; otherwise it stays local.

---

## State Categories

| Category | Mechanism | Examples |
|----------|-----------|----------|
| Component-local UI | `useState` | input text, open/closed popovers, textarea rows |
| Refs (mutable, no re-render) | `useRef` | `sendingRef` in InputBar, scroll DOM nodes |
| Shared chat data | Zustand `useChatStore` | message list, current assistant id, token append reducer |
| Shared agent state | Zustand `useAgentStore` | `AgentState` value, last error string |
| Cross-tab persistence | _(Phase 3)_ | session list, settings |

---

## Store Conventions

- **One store per concern**, not one store per feature. We have
  `chatStore` (data) and `agentStore` (state machine). Do not merge them.
- **Action functions over inline `set` calls.** Components call
  `appendToken(id, text)`, not `setState((s) => …)`. The store is the
  single place that knows the reducer logic — see
  `guides/code-reuse-thinking-guide.md` on reducers.
- **All actions return `void` or a typed return value.** No throwing.
- **Initial state is `useState`-equivalent default** in the
  `create<State>((set) => ({ … }))` factory. Never derive from props.

```ts
// src/stores/chatStore.ts
export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  currentAssistantId: null,
  appendToken: (id, text) =>
    set((s) => ({
      messages: s.messages.map((m) =>
        m.id === id
          ? { ...m, content: m.content + text, status: "streaming" }
          : m,
      ),
    })),
  // …
}));
```

---

## When to Promote to Global State

Promote a value to a store when **2 or more components** consume it
directly (props-drilling through 2+ levels is the smell). Examples:

- `agentState` is read by `InputBar` (to disable) and `ChatView`
  (status bar). → store.
- `messages` is read by `ChatView` (list) and `InputBar` (could clear
  after send in Phase 2). → store.
- `text` in `InputBar` is only used inside `InputBar`. → local.

Do **not** promote speculatively ("we might need it later"). Adding
a store is cheap; removing one is messy because components grow to
depend on its shape.

---

## Server State

There is no separate "server state cache" yet. The Rust Agent Loop is
the source of truth for in-flight generation; the frontend treats it as
an event stream, not as a fetchable resource. When Phase 3 adds session
persistence (SQLite), a thin read-through cache layer may be needed —
keep it isolated to the session feature folder.

---

## Common Mistakes

- **Reading store values inside render functions via `useStore(s => s)`**
  with an inline selector that returns a new object each call. This
  re-renders on every store change. Select narrow slices:
  `useChatStore((s) => s.messages)` not `(s) => ({ … })`.
- **Two stores owning overlapping state** (e.g. `chatStore.currentAssistantId`
  duplicating `agentStore.state`). Pick one owner; have the other
  derive or read.
- **Local state that should be global.** Symptom: callbacks bubble
  up 2+ levels. Promote to store.
- **Global state that should be local.** Symptom: a single component
  reads/writes 90% of a store. Push it down with `useState`.
- **Mutating store state outside `set`.** Always `set((s) => newState)`.
  Direct mutation breaks Zustand's referential-equality re-render.
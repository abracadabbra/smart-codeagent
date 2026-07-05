# Hook Guidelines

> How hooks are used in this project.

---

## Overview

Custom hooks in this project do **one of three things**: subscribe to a
Tauri event stream (`useAgentEvents`), gate side-effects on lifecycle
(`useEffect` inside the same hook), or expose a domain action that
calls `invoke` (`sendMessage`). They are **not** used for data fetching
(no React Query / SWR in Phase 1) and **not** used for global state —
that lives in Zustand stores.

---

## Custom Hook Patterns

### Lifecycle hooks

Use `useEffect` with a cleanup return. For Tauri event subscriptions,
collect unlisteners into an array and call them in the cleanup.

```ts
// src/hooks/useAgentEvents.ts
useEffect(() => {
  const unlisteners: UnlistenFn[] = [];
  void (async () => {
    for (const [name, handler] of handlers) {
      const unlisten = await listen<unknown>(name, handler);
      unlisteners.push(unlisten);
    }
  })();
  return () => {
    unlisteners.forEach((u) => u());
  };
}, [/* stable, action-shaped deps */]);
```

Two rules:

- **All `listen`/`invoke` work is inside an IIFE**. Tauri APIs return
  promises; the effect body itself stays synchronous.
- **The dependency array contains store action references**, not the
  raw selectors. This avoids re-subscribing on every store change.

---

### Action hooks

Hooks that expose a function (`sendMessage`) do **not** use `useCallback`
internally — they just declare an `async` function. Callers that need
stability wrap with `useCallback` themselves (see `InputBar.tsx`).

```ts
export async function sendMessage({ text, assistantId }: SendMessageArgs) {
  await invoke("send_message", { text, assistantId });
}
```

Why not a hook? Because there's no reactive state to subscribe to —
the function is the entire API.

---

## Data Fetching

There is no client-side data fetching in Phase 1. All "server" data
arrives as a Tauri event stream (`agent:token`, `agent:status`,
`agent:error`, `agent:done`). When Phase 3 adds session persistence,
fetching will use `invoke` commands, not `fetch` — the network is
in-process to the Rust side.

---

## Naming Conventions

- **One hook per file**, exported by name. Do not bundle multiple hooks
  in one module.
- **Hook filename matches the hook name**: `useAgentEvents.ts` exports
  `useAgentEvents`.
- **Action functions** (not hooks) that wrap `invoke` use a verb
  (`sendMessage`), not a `use*` prefix. They are not hooks.
- **Selectors** inside hooks/JSX use narrow accessors:
  `useChatStore((s) => s.messages)`, not the whole store.

---

## Co-location

- Hooks live in `src/hooks/` when reused by 2+ components.
- Single-use hooks may live next to their component until a second
  consumer appears — then promote.
- Domain types referenced only by a hook live next to the hook
  (e.g. `TokenPayload` in `useAgentEvents.ts`). Types consumed across
  modules go in `src/types/`.

---

## Common Mistakes

- **Returning objects from hooks without memoization.** A hook that
  returns `{ token, appendToken }` creates a new object every render,
  re-triggering downstream effects. Either `useMemo` or return the
  values as a tuple.
- **Calling `listen` outside `useEffect`.** Subscribes on every render
  and leaks handles.
- **Awaiting `listen` in the effect body.** The effect body must stay
  sync; use the IIFE pattern above.
- **Mixing hook and store responsibilities.** A hook that owns a
  Zustand store inside it ties the store's lifetime to the hook.
  Stores are module-level singletons, not hook-local state.
- **Selector that returns a new object** —
  `useStore((s) => ({ a: s.a, b: s.b }))` re-renders on every store
  change. Select one field, or `useShallow` if you need multiple.
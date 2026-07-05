# Directory Structure

> How frontend code is organized in this project.

---

## Overview

`src/` is split into **four folders by concern**, organized
feature-first (not type-first). The three-panel Qoder layout lives at
the App root; chat-specific components live under `components/chat/`.
New features get their own subfolder, not a new top-level directory.

```
src/
├── main.tsx                      # React 19 root + ReactDOM.createRoot
├── App.tsx                       # 3-column shell + AgentEventBridge
├── index.css                     # Tailwind base + custom CSS variables (ink-* scale)
├── components/
│   ├── AgentEventBridge.tsx      # side-effect-only wrapper around useAgentEvents
│   └── chat/                     # feature folder: chat panel
│       ├── ChatView.tsx          # container: scroll list + status bar + InputBar
│       ├── InputBar.tsx          # textarea + send button (gated on AgentState)
│       ├── MessageBubble.tsx     # one message (user right / assistant left)
│       └── StreamingText.tsx     # text + blinking caret (▍) during streaming
├── hooks/
│   └── useAgentEvents.ts         # Tauri event subscriptions + sendMessage action
├── stores/
│   ├── chatStore.ts              # Zustand: messages, currentAssistantId, reducers
│   └── agentStore.ts             # Zustand: AgentState value, lastError
└── types/
    ├── agent.ts                  # AgentState union (mirrors Rust enum, PascalCase)
    └── message.ts                # Message interface (id, role, content, status)
```

---

## Layer Boundaries

```
types  ◄──────  stores  ◄──────  hooks  ◄──────  components  ◄──────  App.tsx
  plain       reactive state     side effects      presentation       composition
  data        + actions          + event bridge     + layout           + bridge mount
```

| Folder | Owns | Knows about | Forbidden from |
|--------|------|-------------|----------------|
| `types/` | Plain interfaces / unions | nothing else | importing from `stores/`, `hooks/`, `components/` |
| `stores/` | Zustand stores, reducer logic | `types/` | importing from `hooks/`, `components/`, tauri APIs |
| `hooks/` | `useEffect`, `invoke`, `listen` | `stores/`, tauri APIs | rendering JSX |
| `components/` | JSX, layout, props | `stores/`, `hooks/`, `types/` | owning global state |
| `App.tsx` | 3-column shell | everything (above) | business logic |

Cycles are caught by ESLint rules and `tsc --noEmit`. The rule to
remember: **the folder furthest from `App.tsx` knows the least**.

---

## Where to Add New Code

- **New chat feature** (e.g. file preview pane, regenerate button)
  → `components/chat/<Feature>.tsx`. Co-locate with the chat panel;
  do not promote to `components/` until a second consumer appears.
- **New panel** (e.g. session list, settings) → `components/<panel>/`
  as a new feature folder. Mount in `App.tsx` next to `<ChatView />`.
- **New Tauri event subscription** → `hooks/use<Domain>Events.ts`,
  co-locate `send*` action in the same file (matches
  `useAgentEvents.ts` pattern).
- **New global state slice** → `stores/<domain>Store.ts`. One store
  per concern. Do not split `chatStore` into per-message stores.
- **New type shared by 2+ modules** → `types/<domain>.ts`. Single-use
  types stay next to their consumer.

---

## File Naming

- **Components**: PascalCase, default-export only if they are the
  primary export of the file. Named exports otherwise.
- **Hooks**: `use<Name>.ts`, single hook per file, named export.
- **Stores**: `<domain>Store.ts`, named export of the hook
  (`useChatStore`).
- **Types**: lowercase domain noun (`message.ts`, `agent.ts`),
  named exports of interfaces / unions.
- **Feature folders**: lowercase, plural only if the folder owns
  multiple parallel implementations (`chat/` is singular because
  there's only one chat panel).

---

## Examples

- **Single-purpose component**: `components/chat/StreamingText.tsx`
  (22 lines, one component, one concern). Use this shape when the
  component has one job and is unlikely to grow.
- **Container component**: `components/chat/ChatView.tsx` (63 lines,
  composes children + owns scroll ref + reads agent state). Use this
  shape at the top of each feature folder.
- **Bridge / invisible component**: `components/AgentEventBridge.tsx`
  (10 lines, returns `null`, just mounts `useAgentEvents`). Use this
  pattern when you need a side-effect but no UI.

---

## Common Mistakes

- **Grouping by type instead of feature**: a `components/buttons/`
  folder full of single-use buttons. Refactor to feature folders;
  extract to shared only after 2+ features use the same component.
- **Putting state in `App.tsx`** instead of a Zustand store.
  `App.tsx` is composition root, not state owner.
- **Reaching into a store directly from a parent that passes props
  down.** Two readers are a smell — promote the state to a store
  (see `state-management.md`).
- **Creating `utils/` for one-off helpers.** Helpers live next to
  their primary consumer until a second consumer appears.
- **Mixing JSX and event subscriptions.** `hooks/` is the only
  place that calls `listen` / `invoke`. Components consume the
  hook, they do not call Tauri APIs themselves.
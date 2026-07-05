# Quality Guidelines

> Code quality standards for frontend development.

---

## Overview

Frontend quality in this project is enforced by **compiler + linter + human review**, in that order. The rules below exist because some things cannot be expressed in TypeScript types — they are conventions, not syntax. Each section maps to either a tool config, an ESLint rule, or a checklist reviewers apply by hand.

---

## Required Patterns

- **`tsc --noEmit`** must pass with zero errors before every commit.
  `tsconfig.json` already enables `strict`, `noUnusedLocals`,
  `noUnusedParameters`, `noFallthroughCasesInSwitch`.
- **`npm run lint`** must pass with zero warnings — the script runs
  ESLint with `--max-warnings 0`. The hook dependency
  (`react-hooks`) catches missing deps and stale closures; treat its
  warnings as errors even when not in CI.
- **Path alias `@/`** for `src/` imports. Never use `../../..`
  relative paths that escape the `src/` directory.
- **Tailwind atomic classes** for styling. No CSS Modules, no
  styled-components, no inline `style={{ ... }}` for layout/colors.
- **Single named export per file** for components and hooks. Default
  exports are allowed only for the file's primary component.

---

## Forbidden Patterns

- **`any`**: disabled by `strict: true`. Use `unknown` and narrow.
- **`as unknown as T`**: double-cast. Means the type lies; refactor
  the producer or add a runtime check.
- **Repeated `(payload as { field?: T }).field` chains** across
  handlers — see `type-safety.md`. Centralize in one typed interface.
- **`console.log` for debugging**. Use `console.warn` for non-Tauri
  env warnings (see `useAgentEvents.ts`) or remove before commit.
  Lint rule should fail the build on `console.log`.
- **Magic strings for Tauri event names**. Use the constants from
  `useAgentEvents.ts` (`"agent:token"` etc.) — and if a new event
  is added, the constant goes in the same file as the listener.
- **`dangerouslySetInnerHTML`, `eval()`**: not in the codebase; keep
  it that way. Render user-provided text as text; never inject HTML.
- **Inline `style={{ ... }}`** for layout or theme colors. Tailwind
  classes only. Inline style is allowed only for dynamic values
  that Tailwind cannot express (e.g. computed transforms).
- **Props drilling through 2+ levels.** Promote the value to a store
  (see `state-management.md`) or a context.

---

## Testing Requirements

There is **no test framework in Phase 1**. The codebase is small
enough that manual UI verification has been sufficient. When Phase 2
adds non-trivial client-side logic (file preview, tool result
rendering, retry buttons), introduce **Vitest** for unit tests and
**Playwright** for E2E flows. The conventions below assume those
will land.

- **Unit tests**: Vitest + React Testing Library. Co-locate
  `*.test.ts(x)` next to the file under test.
- **E2E tests**: Playwright in `e2e/` at repo root. Each test maps
  to a PRD acceptance criterion (e.g. "send message → see streamed
  reply").
- **Coverage target**: 80% on `src/stores/` and `src/hooks/`
  (logic-bearing modules). Components are visual and exempt from
  the percentage target but must have at least one smoke test.

---

## Component Conventions

- **One component per file.** Co-located with siblings in a
  feature folder (`components/chat/`). File name matches the
  component name (`ChatView.tsx` exports `ChatView`).
- **Props interfaces are local to the file**. Do not export them
  unless a second component consumes them.
- **Default values via destructuring with `=`**, not via
  `defaultProps` (deprecated in React 19).
- **Hooks at the top of the component.** Conditional hooks are
  forbidden — extract a child component if you need branching.
- **Refs only for non-rendering values** (DOM nodes, mutable
  counters). Anything that should re-render goes in `useState`.

---

## Code Review Checklist

- [ ] New event listener? Constant in `useAgentEvents.ts`? Payload
      type defined? `as` cast happens exactly once at the handler
      boundary? (See `type-safety.md`.)
- [ ] New store or store action? Reducer logic in the store, not in
      the component? Action returns `void` or a typed value, not a
      Promise?
- [ ] New component file? Matches the file-name pattern? Single
      named export? Local props interface?
- [ ] New env / config value? Read via `import.meta.env` (Vite) and
      prefixed with `VITE_`? Documented in `.env.example`?
- [ ] `console.log` left over? `any` introduced? Two-level props
      drilling? Each is a lint-red, fix before review.

---

## Common Mistakes

- **Silencing ESLint with `// eslint-disable-next-line`**: the rule
  was there for a reason. Either fix the code or argue for the rule
  to change in a follow-up. Two disables in one file means the
  pattern is wrong, not the rule.
- **Adding `useMemo` / `useCallback` everywhere**: they have cost.
  Only add when the dependency array contains a new object/function
  reference every render.
- **Forgetting the cleanup function** in `useEffect` for Tauri
  `listen` calls. The unlisten pool pattern in `useAgentEvents.ts`
  is the only sanctioned shape.
- **Co-locating unrelated types** in `src/types/common.ts`. Use one
  file per domain; see `type-safety.md`.
- **Hardcoding Tauri event names** in components instead of
  importing from `useAgentEvents.ts`. If the event name ever changes,
  the constant is the single edit point.
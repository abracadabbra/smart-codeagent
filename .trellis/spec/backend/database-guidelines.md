# Database Guidelines

> Database patterns and conventions for this project.

---

## Status: Deferred to Phase 3

Phase 1–2 has **no database**. Session persistence (SQLite via `rusqlite`)
is a Phase 3 deliverable per the product PRD. This file exists so the
spec tree stays complete; the rules below will be filled when the
storage layer lands.

When Phase 3 starts, this file must cover:

- ORM choice (`rusqlite` raw SQL vs `sqlx` vs `diesel`) and the
  reasoning against the other two.
- Migration tool & directory layout (`src-tauri/migrations/`).
- Transaction boundaries and the convention for long-running async
  work (SQLite + Tokio: how to avoid blocking the runtime).
- Naming: snake_case tables, `id INTEGER PRIMARY KEY`,
  `created_at` / `updated_at` integers (unix ms, never strings).
- Soft-delete vs hard-delete decision for messages / sessions.

Do **not** open this file until then. Adding speculative rules now
would encode guesses that almost certainly change.
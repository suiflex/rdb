# DBM App — "All-4-Engine Usable" Design

**Date:** 2026-06-11
**Status:** Design approved, pending spec review
**Depends on:** all 5 implemented plans (foundation, connstore, driver-postgres, app-ui, drivers mysql/redis/mongo) — committed on `develop`.

## Problem

The app compiles, launches, and renders the 3-pane shell, but is **not usable**:

1. No way to create connections — the sidebar reads an empty `connections.json`, so a fresh launch shows nothing to click.
2. The query editor hardcodes `Query::Sql`, so Redis (`Command`) and Mongo (`MongoOp`) queries cannot be expressed in the UI at all.
3. `AnyDriver` wires only Postgres; the mysql/redis/mongo crates exist but are not app dependencies or dispatch arms.
4. No password-entry flow (connstore `set_password` exists but nothing calls it).
5. No connection edit/delete.

This design closes all five so the app is genuinely usable across the 4 MVP engines.

## Goal

From a fresh launch: create a connection (any of 4 engines) with a keychain-stored password, connect, browse schema, and run queries in that engine's paradigm — then edit or delete the connection.

Out of scope (deferred, noted not built): query history, data export, query cancel, SSH tunnels, a connection "test" button (connecting *is* the test), and any visual Mongo query builder.

## Components

### 1. Wire all 4 drivers (`app/src/dispatch.rs`, `app/Cargo.toml`)

- `app/Cargo.toml` adds `dbm-driver-mysql`, `dbm-driver-redis`, `dbm-driver-mongo` path deps.
- `AnyDriver` gains `Mysql`, `Redis`, `Mongo` variants. Every forwarding method (`ping`/`schema`/`query`/`close`) gets a one-line arm per variant.
- `is_supported` returns `true` for all four engines.
- `connect(engine, cfg)` constructs the matching concrete driver for all four.

### 2. Engine-aware query parsing (`app/src/query_parse.rs` — new, pure, unit-tested)

A single pure function is the entire seam between editor text and the typed `Query`:

```
parse_query(engine: Engine, text: &str) -> Result<Query, String>
```

- **Postgres / MySql** → `Query::Sql(text.to_string())`.
- **Redis** → split `text` on whitespace into tokens → `Query::Command(tokens)`. Empty input is an error. (MVP: no shell-style quoting; whitespace-delimited tokens only — documented.)
- **Mongo** → parse `text` as JSON of the form:
  ```json
  { "collection": "users", "op": "find", "body": { "age": { "$gte": 18 } } }
  ```
  - `op: "find"` → `MongoKind::Find(body)` (body is a JSON object filter).
  - `op: "insert"` → `MongoKind::Insert(body)` (body is the document object).
  - `op: "aggregate"` → `MongoKind::Aggregate(body)` (body is a JSON **array** of stages).
  - Missing/empty `collection`, unknown `op`, wrong `body` shape for the op, or invalid JSON → a clear `Err(String)` the UI surfaces in the result-status line.

`main.rs` stores the connected **engine** next to the driver (the shared slot becomes `Option<(Engine, AnyDriver)>`), and `run_query` calls `parse_query(engine, &sql)`; on `Err` it shows the message in `result_status` and runs nothing. The editor shows a per-engine hint (e.g. "SQL", "Redis: SET key val", "Mongo JSON: {collection, op, body}") via a status property.

### 3. Add / edit / delete connection modal (`app/src/ui/conn-form.slint` + `main.rs`)

- A modal overlay component (same scrim + single-elevation pattern as `palette.slint`), driven by a `form-open` bool and a `form-mode` ("add"/"edit") on `MainWindow`.
- Fields: name (LineEdit), engine (ComboBox of 4), host, port, user, database, password (LineEdit, `input-type: password`), sslmode (ComboBox of Disable/Prefer/Require), color (a row of ~8 preset swatch Rectangles; selected one is ringed).
- Selecting an engine sets the default port if the port field is empty/default: 5432 / 3306 / 6379 / 27017.
- Buttons: **Save**, **Cancel**, and (edit mode only) **Delete**.
- Triggers: a `+` button in the sidebar "Connections" header opens the modal in add mode; a per-row pencil/edit affordance opens it in edit mode pre-filled.

### 4. connstore lifetime + helper (`crates/connstore/src/store.rs`)

- Add `ConnStore::open_default() -> Result<ConnStore>` that wraps `default_path()` + `secret::select_backend(dir)` + `load(...)`. (Removes the duplicated open logic now scattered in `main.rs`.)
- The app holds one `Rc<RefCell<ConnStore>>` for its lifetime so form mutations (`add`/`update`/`remove`, `set_password`/`delete_password`) persist and the sidebar reloads from the same source of truth.

### 5. Save / delete flow

- **Save (add):** build `SavedConnection` from form fields (new uuid id, chosen color hex), `store.add(conn)`, then if a password was typed `store.set_password(&id, &pw)`. Rebuild the sidebar `ConnItem` model.
- **Save (edit):** `store.update(conn)` for the existing id; if password field non-empty `set_password`, else leave the stored secret untouched. Rebuild sidebar.
- **Delete:** `store.remove(&id)` + `store.delete_password(&id)`. Rebuild sidebar; if the deleted connection was selected, clear selection + schema.

## Data flow

```
add/edit modal ──build──> SavedConnection + password
      │                          │
      └── store.add/update ──────┴── store.set_password ──> connstore (JSON + keychain)
                          │
                          └── rebuild sidebar ConnItem model
click connection ──> AnyDriver::connect(engine, cfg)  [engine kept in slot]
                          └──> schema() ──> tree model
editor text + engine ──> parse_query(engine, text) ──Ok──> driver.query() ──> GridModel
                                              └──Err──> result_status message
```

## Error handling

- `parse_query` errors render in `result_status`; no driver call is made.
- Connect/query/driver errors continue to surface in the status line (existing pattern).
- Form validation is minimal-but-honest: empty name or host is rejected before save with an inline message; the rest is the engine's job to reject at connect time.

## Testing

- **`query_parse.rs`** — pure unit tests: SQL passthrough (PG + MySql), Redis tokenization (incl. empty-input error), Mongo find/insert/aggregate construction, and error cases (bad JSON, unknown op, aggregate body not an array, missing collection).
- **`dispatch.rs`** — extend: `is_supported` true for all four; `label` for all four.
- **`connstore`** — `open_default` smoke (path resolves or errors cleanly).
- **UI** — manual checklist (per the spec's "app/UI manual for MVP"): modal opens/saves/persists across relaunch, edit pre-fills, delete removes, engine sets default port, swatch selection tints the accent, per-engine editor hint shows.
- **Live** — end-to-end verified against the real Postgres the user supplies (create connection → connect → schema → SQL → grid). MySQL/Redis/Mongo are compile + unit verified unless live endpoints/Docker are available.

## Type consistency

Reuses the frozen `dbm-core` contract (`Query`, `MongoOp`, `MongoKind`, `ConnConfig`, `SslMode`, `ResultSet`, `Schema`) and the `dbm-connstore` types (`SavedConnection`, `Engine`, `ConnStore`). `parse_query` is the only new public surface in `app`; `ConnStore::open_default` the only new connstore surface. No core/driver types change.

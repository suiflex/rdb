# RDB — Agent Instructions

## Project

Native cross-platform database manager (PostgreSQL, MySQL, Redis, MongoDB, SQLite, Cassandra) built with Rust + Slint UI. Monorepo workspace.

Alongside the Rust workspace (`app/`, `crates/*`) the repo also holds: `website/`
(Astro marketing site, deployed by Cloudflare Pages' Git integration on every
push to `develop` — not part of the Rust build), `scripts/` (`install.sh` /
`install.ps1`, the curl/iwr entry points for a direct install), and
`npm/`/`packaging/` (the npm postinstall wrapper and package-manager
distribution files for Homebrew/Scoop).

## Build, Lint, Test

A root `Makefile` wraps the common cargo invocations and splits FE (the
`rdb` UI binary) from BE (the `crates/*` libraries) so each side builds and
tests independently. Run `make help` for the full target list.

```bash
make fe-build     # build the rdb UI (FE)
make fe-run       # run the UI
make be-build     # build backend crates only (no FE)
make be-test      # test backend crates only
make fmt-check    # format check   (make fmt to apply)
make lint         # clippy, warnings as errors
make test         # test the whole workspace
make all          # fmt-check + lint + test + build (CI gate)
cargo build --release -p rdb   # release binary
```

## CI

One GitHub Actions workflow per component in `.github/workflows/` — `rdb-app`
plus one per crate: `rdb-core`, `rdb-connstore`, `rdb-driver-postgres`,
`rdb-driver-mysql`, `rdb-driver-redis`, `rdb-driver-mongo`, `rdb-driver-mssql`,
`rdb-driver-clickhouse`.
Each has a `paths:` filter, so editing one component only runs that
component's CI (lean).

- **Known gap**: `rdb-driver-sqlite` and `rdb-driver-cassandra` have no
  dedicated workflow yet, and the root `Makefile`'s `BE_PKGS` (used by
  `be-build`/`be-test`/`be-check`) doesn't include them either — use
  `cargo build/test -p rdb-driver-sqlite` (or `-cassandra`) directly until
  that's wired up.
- Dependents also watch `crates/core/**`, so a `core` change fans out to retest
  core + all dependents (connstore, drivers, app). Other crates stay independent.
- Backend jobs run `cargo {fmt,clippy} -p <pkg>` and `cargo test -p <pkg> --lib`
  (scoped with `-p`, not the workspace-wide `make` targets). `--lib` runs unit
  tests only; the `tests/integration.rs` targets need Docker, so they stay out
  of CI and run locally via `make test-it`.
- The app job installs Slint system libs and runs `cargo build -p rdb`.
- `audit.yml` runs `cargo audit` on every `Cargo.toml`/`Cargo.lock` change plus
  a weekly sweep (new advisories land with no code change).
- `website.yml` is a CI check only for `website/**` — actual deploy is
  Cloudflare Pages' own Git integration on push to `develop`.

Releases are handled separately by `release-please.yml` (single workspace
release on `develop`): conventional commits drive an auto-maintained release
PR that bumps the version and `app/CHANGELOG.md`; merging it tags `vX.Y.Z`
and cuts a GitHub Release. The `app` package (`rdb`) is the tracked version.

`release-build.yml` does the actual packaging once that tag lands: builds
per-target native binaries (macOS `.dmg` with an ad-hoc-codesigned `.app`,
Windows bare `.exe`, Linux `.tar.gz`), attaches them to the GitHub Release,
and publishes to the `suiflex/homebrew-tap` formula/cask, the
`suiflex/scoop-bucket`, and npm (`@suiflex/rdb`, postinstall downloads the
matching asset). `scripts/install.sh` / `scripts/install.ps1` are the direct
(non-package-manager) install path and hit the same GitHub Releases API.

Release note sections are configured in `release-please-config.json` and are
triggered by conventional commit **type**:

- `feat(app): ...` -> **App Features**
- `feature(driver-<engine>): ...` or `feature(driver): ...` ->
  **Driver Features** (`release-please` treats `feature` like `feat` for minor
  version bumps)
- `fix(<scope>): ...` -> **Bug Fixes**
- `perf(<scope>): ...` -> **Performance Improvements**

Keep the scope specific (`app`, `driver-postgres`, `driver-mysql`, `core`,
`connstore`) so generated changelog lines stay readable.

## Architecture

- `app/` — Slint UI binary (main entry point)
  - `app/src/main.rs` — builds the state, builds the shared closures, calls the
    wiring modules, runs the event loop. It used to hold all of it; keep new
    callbacks out of it.
  - `app/src/wire/*.rs` — one module per area (connect, picker, query, runner,
    browse, tabs, edit, grid, split_pane, editor, find, schema, settings,
    conn_form, update). Each exposes `wire(&MainWindow, &AppState, …)` and
    installs that area's callbacks. `runner` and `editor` are the exceptions:
    they *build* closures (`run_sql`/`run_stream`, `sync_editor`/
    `load_editor_text`) that `main` hands on to the others.
  - `AppState` / `AppFns` (`main.rs`) — the shared `Rc`/`Arc` state, and the
    long-lived closures built from it. A wiring module destructures what it
    needs from these instead of `main` cloning handles per callback.
    `AppState` is deliberately `!Send`: anything crossing onto a tokio task
    clones the specific `Arc` it needs.
  - `app/src/pane.rs` — the `set_p_*`/`get_p_*` accessors. The window exposes a
    separate property per result pane (`cells` / `p1_cells`), so these wrap the
    `pane == 0` fork once each.
- `crates/core/` — `Driver` trait, `Query`, `ResultSet`, `Schema`, `RdbError`
- `crates/connstore/` — saved connections + OS keychain / AES-GCM
- `crates/driver-postgres/` — tokio-postgres
- `crates/driver-mysql/` — mysql_async
- `crates/driver-redis/` — redis crate
- `crates/driver-mongo/` — mongodb crate
- `crates/driver-sqlite/` — rusqlite (bundled)
- `crates/driver-cassandra/` — scylla crate (CQL, Cassandra/ScyllaDB)
- `crates/driver-mssql/` — tiberius (T-SQL, SQL-auth only in v1, no Windows/AD)
- `crates/driver-clickhouse/` — HTTP `clickhouse` crate, reads via `FORMAT JSON` (not the typed `Row` path), insert-only write-back

## Key Rules

- UI (`app/`) names a concrete driver crate only in `app/src/dispatch.rs` (the `AnyDriver` enum); the rest of the app depends on `rdb-core`.
- Adding new engine = new `driver-*` crate implementing `Driver` trait + a variant in `AnyDriver`.
  Query-tab behavior (completion, syntax highlighting, format) is driven by
  `Engine::language()` (`crates/connstore/src/model.rs`), which maps each
  `Engine` to a `QueryLanguage` (`Sql | Cql | Command | Mongo`) — that's the
  single fork point completion/lexer/format/`query_parse` all read from.

  | Engine | QueryLanguage | Query shape | Example |
  | --- | --- | --- | --- |
  | Postgres, MySQL, SQLite, SQL Server, ClickHouse | `Sql` | SQL text | `SELECT * FROM users` |
  | Cassandra | `Cql` | CQL text (no JOIN/subquery/HAVING) | `SELECT * FROM ks.t ALLOW FILTERING` |
  | Redis | `Command` | command tokens | `GET user:1` |
  | MongoDB | `Mongo` | structured op | `find({ age: { $gt: 20 } })` |

  Two cases:
  - **New driver, existing query language** (e.g. another SQL-family engine,
    or another Redis-like command store): after the driver crate + `AnyDriver`
    variant, add the engine to `Engine::language()`'s matching arm. Nothing
    else changes — completion/lexer/format/`query_parse` pick it up
    automatically.
  - **New driver, genuinely new query paradigm** (not SQL-shaped text,
    command tokens, or a Mongo-style structured op): add a new
    `QueryLanguage` variant, then:
    1. `crates/core/src/query.rs` — add a `Query` variant if the wire shape
       is new too (plain string follows `Sql`/`Cql`; structured op follows
       `Mongo`'s `Box<Op>`). Grep `Query::Sql(_) | Query::Command(_) | Query::Mongo(_)`
       for every driver's exhaustive rejection arm that needs the new case
       added (driver-mysql/redis/mongo use a wildcard `_` arm already and
       need no change).
    2. `app/src/editor/<lang>.rs` — keyword table + `is_keyword`, wired into
       `editor.rs`'s `keywords_for`.
    3. `app/src/completion/<lang>.rs` — bare-word + dot-context completion,
       wired into `completion::suggest`'s `match language`.
    4. If formattable text (not structured like Mongo): `app/src/format/<lang>.rs`
       supplying a `format::Spec`, wired into `format::dispatch`; add the
       language to `sql_capable` in `main.rs` so the Format button shows.
       Skip for structured/command-style languages — button stays hidden.
    5. `app/src/query_parse.rs::parse_query` — build the new `Query` variant
       for this language.

  **Full checklist for a new `Engine` variant** (driver-mssql's addition is
  the reference — a step here got missed and had to be backfilled twice):
  1. `crates/connstore/src/model.rs` — `Engine` variant **plus a row in
     `ENGINES`** (display label, badge key, URL scheme, default port, query
     language). `Engine::language()`, `display()`, `key()`, `scheme()` and
     `default_port()` all read that row, so this is the only place the strings
     live. `every_engine_has_a_row` fails if a variant has no row.
  2. `crates/connstore/src/conn_url.rs` — scheme(s) → engine in `scheme_to_engine`.
  3. New `driver-*` crate + `app/src/dispatch.rs` — `AnyDriver` variant (box it
     if the driver struct is large — `cargo clippy` catches this via
     `large_enum_variant`) + `write_statements`. Then run `cargo build -p rdb`
     and add an arm everywhere it complains — the compiler enumerates every
     exhaustive `Engine`/`Query` match site for you; don't hand-audit.
  4. Nothing. This step used to list six hand-written string tables
     (`label_to_engine`/`default_port` in the connection form, `label`/`badge`
     in `dispatch.rs`, label/scheme in `export.rs`) that had to be edited by
     hand and that no compiler checked — a missed entry silently routed a saved
     connection to Postgres. They all derive from the `ENGINES` row now.
  5. UI: `app/src/ui/conn-form.slint` (engine picker `model`, import-URL
     placeholder ternary, field-visibility `if` conditions e.g. SSL mode) and
     `app/src/ui/app-window.slint`'s Settings → About tab (static engine list
     string).
  6. **Icon assets — two separate tracks, both need the new engine**:
     - Monochrome: `app/src/ui/icons/db-<engine>.svg` (a single-color glyph —
       real brand SVGs work fine here too, `AppIcon`'s `colorize` flattens
       whatever's there to one flat tint), wired into `DbBadge`'s `known`
       list and `Tokens.db-color()` in `tokens.slint` (`components.slint`).
     - Full-color: `app/src/ui/icons/brand/<engine>.svg`, wired into
       `EngineLogo`'s ternary (`components.slint`, backs the Engine
       dropdown's `show-logo` and the empty-state "Works with" row in
       `picker.slint`).
     - **No official mark available → don't hand-draw one.** Fall back to
       text instead: `DbBadge`'s `fallback-text` (e.g. `"MS"` for SQL
       Server), `EngineLogo`'s empty-source case (renders nothing, row text
       still shows the name), and `website/Engines.astro`'s `icon: null`
       entries all do this — keep new engines with no real logo consistent
       with that pattern rather than improvising a glyph.
  7. `.github/ISSUE_TEMPLATE/bug_report.yml` — the "Database engine" dropdown
     `options:` list (missed for two engines in a row before this line was
     added — it's easy to forget since nothing enforces it, same class of
     gap as the docs/marketing list below).
  8. Docs/marketing surfaces that list engines by name — easy to forget since
     nothing enforces them: `README.md` (badge line, Features bullet,
     Supported Engines table, `Query` enum snippet, usage instructions,
     Project status line, Crate overview table), `npm/README.md` (near-dupe
     of the above), `VISION.md`, `website/src/components/Engines.astro`
     (icon + array — check `simple-icons` actually has the brand mark before
     assuming one exists), `website/src/pages/index.astro` (hero + meta
     description), `website/src/pages/open-source.astro` (crate list).
- Async I/O on tokio runtime, results bridge back to Slint main thread via `invoke_from_event_loop`.
- Release profile: `opt-level=z`, LTO, `panic=abort`, strip.
- In-app self-update (`app/src/self_update.rs`) only ever runs for
  `InstallMethod::Other` (direct curl/.dmg/.exe installs) — never for
  Homebrew/Scoop, which must keep showing the upgrade command + release-page
  link instead (`InstallMethod::self_update_supported`, `app/src/update.rs`).
  Don't change that gating without a deliberate reason; it's what stops the
  app from fighting the package manager on those installs.
- **A driver that reads `SslMode` must compile a TLS backend.** `mysql_async`
  and `scylla` **panic** (not `Err`) when asked for TLS with no TLS feature
  built in — `panic=abort` turns that into a process abort on a tokio worker,
  so it never reaches `RdbError` and shows up as a crash report with no app
  symbols. Both now carry `rustls-tls`. Wiring `SslMode` into a new driver
  means adding the crate's TLS feature in the same commit, and new connection
  forms default to `Disable` so an unconfigured server can't take the app
  down.
- **Saving a connection goes through `ConnStore::save_connection`**, not
  `add`/`update` + `set_password`. The split version is non-atomic: metadata
  flushed, secret write failed, and the connection came back later with
  `using password: NO`. `save_connection` reads the password back and rolls
  the metadata change back if it doesn't match; an empty password means
  "keep the existing secret", not "clear it".
- **A full-screen Slint overlay must gate `visible`, not just its children.**
  `ConnForm` is a root `Rectangle` covering the window; its backdrop
  `TouchArea` kept swallowing every click while the form was closed, leaving
  the picker rendered but dead. Hiding the inner content with `if` is not
  enough — the overlay itself needs `visible: <open>` and its dismiss
  `TouchArea` needs the same `if`.

## Driving the app (tests, screenshots, harnesses)

Everything here is env-var driven; there are no CLI flags.

| Variable | Effect |
| --- | --- |
| `RDB_STORE_DIR=<dir>` | Connection store, settings **and** query tabs move to `<dir>`. The isolation switch for any harness — without it a run reads and overwrites the developer's real store. |
| `RDB_MOCK=1` | Seeded in-memory data and an in-process driver, no network. Needs `--features mock`. |
| `RDB_WIN=WxH` | Fixed logical window size, for deterministic screenshots. |
| `RDB_SCREEN=<name>` | Auto-drives the UI to a named screen (mock mode only). |
| `RDB_SHOT=<path.bmp>` | Screenshot after `RDB_SHOT_DELAY_MS` (default 1200), then quit. Needs `--features mock`. |

**`RDB_MOCK` disables persistence.** `save_query_tabs` and the startup restore
both no-op under it, deliberately, so the screenshot harness never touches the
developer's tabs. A persistence test written in mock mode passes without testing
anything — use `RDB_STORE_DIR` with a real (SQLite is easiest) connection.

**Driving the UI from outside** — Slint 1.17 embeds an MCP server in the app:

    make fe-run-mcp                      # SLINT_EMIT_DEBUG_INFO=1 SLINT_MCP_PORT=8080

`SLINT_EMIT_DEBUG_INFO=1` is what keeps element ids (`Component::element-id`) in
the compiled UI; without it introspection finds nothing. Ids are
component-scoped, so `PrimaryButton::ta` matches every instance — address a
specific one by its accessible label. `find_elements_by_id` searches descendants
only, so a window's own root id never matches; go through
`get_window_properties`.

**A plain `cargo build -p rdb` silently replaces the MCP-enabled binary** and the
port stops opening, which looks like the app is broken. Rebuild with
`--features slint/mcp` (that is what `make fe-run-mcp` does).

**A failing test aborts instead of reporting.** `panic=abort` turns a failed
assertion into `SIGABRT` with no message and no test name. Re-run with
`cargo test -p rdb --bin rdb -- --test-threads=1` — the last test printed is the
one that failed.

## Demo and test data

Fixtures, doc examples and test queries use **neutral sample names**. This
repository is public: schema, database and group names, hostnames and example
connection strings have all leaked real deployment details before. Sample hosts
use the RFC 5737 documentation ranges (`203.0.113.0/24`), not real addresses.
Never paste a real connection string, schema name or dataset into a fixture,
even a passing one.

## Toolchain

Rust stable, components: rustfmt + clippy.

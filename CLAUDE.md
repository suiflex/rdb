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
`rdb-driver-mysql`, `rdb-driver-redis`, `rdb-driver-mongo`, `rdb-driver-mssql`.
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
- `crates/core/` — `Driver` trait, `Query`, `ResultSet`, `Schema`, `RdbError`
- `crates/connstore/` — saved connections + OS keychain / AES-GCM
- `crates/driver-postgres/` — tokio-postgres
- `crates/driver-mysql/` — mysql_async
- `crates/driver-redis/` — redis crate
- `crates/driver-mongo/` — mongodb crate
- `crates/driver-sqlite/` — rusqlite (bundled)
- `crates/driver-cassandra/` — scylla crate (CQL, Cassandra/ScyllaDB)
- `crates/driver-mssql/` — tiberius (T-SQL, SQL-auth only in v1, no Windows/AD)

## Key Rules

- UI (`app/`) names a concrete driver crate only in `app/src/dispatch.rs` (the `AnyDriver` enum); the rest of the app depends on `rdb-core`.
- Adding new engine = new `driver-*` crate implementing `Driver` trait + a variant in `AnyDriver`.
  Query-tab behavior (completion, syntax highlighting, format) is driven by
  `Engine::language()` (`crates/connstore/src/model.rs`), which maps each
  `Engine` to a `QueryLanguage` (`Sql | Cql | Command | Mongo`) — that's the
  single fork point completion/lexer/format/`query_parse` all read from.

  | Engine | QueryLanguage | Query shape | Example |
  | --- | --- | --- | --- |
  | Postgres, MySQL, SQLite, SQL Server | `Sql` | SQL text | `SELECT * FROM users` |
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
  1. `crates/connstore/src/model.rs` — `Engine` variant + `Engine::language()` arm.
  2. `crates/connstore/src/conn_url.rs` — scheme(s) → engine in `scheme_to_engine`.
  3. New `driver-*` crate + `app/src/dispatch.rs` — `AnyDriver` variant (box it
     if the driver struct is large — `cargo clippy` catches this via
     `large_enum_variant`) + `write_statements`. Then run `cargo build -p rdb`
     and add an arm everywhere it complains — the compiler enumerates every
     exhaustive `Engine`/`Query` match site for you; don't hand-audit.
  4. **String-keyed lookups the compiler can NOT catch** (grep the engine's
     display label, e.g. `"SQL Server"`, across `app/src/main.rs`): the
     connection-form's `label_to_engine`/`default_port` — `label_to_engine`
     has a wildcard arm that silently defaults an unmatched label to
     `Engine::Postgres`, so a missed entry here misroutes a new-connection
     save to the wrong driver instead of failing to compile.
  5. UI: `app/src/ui/conn-form.slint` (engine picker `model`, import-URL
     placeholder ternary, field-visibility `if` conditions e.g. SSL mode) and
     `app/src/ui/app-window.slint`'s Settings → About tab (static engine list
     string).
  6. Docs/marketing surfaces that list engines by name — easy to forget since
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

## Toolchain

Rust stable, components: rustfmt + clippy.

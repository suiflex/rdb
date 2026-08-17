# Contributing to RDB

Thanks for your interest in RDB — a native, cross-platform database manager
(PostgreSQL, MySQL, Redis, MongoDB, SQLite, Cassandra, SQL Server and
ClickHouse) built with Rust and Slint.

This guide covers how to build, test, and submit changes. By participating you
agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Quick links

- **Website** — https://rdb.suiflex.dev
- **Where RDB is headed** — [VISION.md](VISION.md)
- **Security policy** — [SECURITY.md](SECURITY.md)
- **Code of Conduct** — [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- **Bugs & feature requests** — [Issues](https://github.com/suiflex/rdb/issues/new/choose)
- **Questions & setup help** — [Discussions](https://github.com/suiflex/rdb/discussions)

Reading [VISION.md](VISION.md) first is worth the two minutes — it explains
what RDB is deliberately *not* trying to be, which is the fastest way to tell
whether an idea fits before you spend time on it.

## How to contribute

Start here, before opening anything:

1. **Bug or small fix** → open a pull request directly.
2. **New database engine, or an architectural change** → open an
   [issue](https://github.com/suiflex/rdb/issues/new/choose) first. Adding an
   engine touches more places than it looks (see
   [Adding a database engine](#adding-a-database-engine)); agreeing on the
   approach up front saves a rewrite.
3. **Question, setup trouble, or "is this a bug?"** →
   [Discussions](https://github.com/suiflex/rdb/discussions), not an issue.
4. **Security vulnerability** → **do not** open a public issue. Follow
   [SECURITY.md](SECURITY.md).

## Getting started

RDB is a Cargo workspace:

- `app/` — the Slint UI binary (`rdb`), the only crate that names a concrete
  driver (in `app/src/dispatch.rs`).
- `crates/core/` — the `Driver` trait, `Query`, `ResultSet`, `Schema`, errors.
- `crates/connstore/` — saved connections + OS keychain / AES-GCM.
- `crates/driver-*/` — one crate per engine, each implementing `Driver`:
  `postgres`, `mysql`, `redis`, `mongo`, `sqlite`, `cassandra`, `mssql`,
  `clickhouse`.

The repo also holds a few things that are not part of the Rust build:
`website/` (the Astro marketing site), `scripts/` (the `install.sh` /
`install.ps1` entry points), and `npm/` + `packaging/` (the npm wrapper and
Homebrew/Scoop distribution files).

### From zero to a running app

1. Install a stable Rust toolchain, then the components CI checks against:

   ```bash
   rustup component add rustfmt clippy
   ```

2. Install the Slint system libraries for your platform — see the
   [Slint prerequisites](https://releases.slint.dev/latest/docs/rust/slint/#prerequisites).
   Backend-only work (`crates/*`) does not need these; the UI build does.

3. Build and launch the UI:

   ```bash
   make fe-run
   ```

If you only want to poke at the UI, `make fe-run-mock` starts it with seeded
in-memory data and no database at all — see
[Running and testing the app](#running-and-testing-the-app).

## Build, lint, test

A root `Makefile` wraps the common Cargo invocations and splits the frontend
(the `rdb` UI binary) from the backend (the `crates/*` libraries). Run
`make help` for the full list. The most common targets:

```bash
make fe-build     # build the rdb UI
make fe-run       # run the UI
make fe-run-mock  # run the UI with seeded mock data, no database needed
make fe-run-mcp   # run the UI with Slint's MCP server (for driving it in tests)
make be-build     # build backend crates only
make be-check     # type-check backend crates only
make be-test      # test backend crates only
make fmt-check    # format check (make fmt to apply)
make lint         # clippy, warnings treated as errors
make test         # test the whole workspace
make all          # fmt-check + lint + test + build (the CI gate)
```

Run `make all` before opening a pull request. Each driver crate also carries
`tests/integration.rs`, which needs Docker and runs locally via `make test-it`;
those are intentionally kept out of CI.

### What CI covers

Every backend crate has its own workflow in `.github/workflows/`, and all ten
are in the `Makefile`'s `BE_PKGS`, so `make be-test` and CI agree on scope.
Each workflow also watches `crates/core/**`, so a change to `core` fans out and
retests its dependents.

Backend jobs run `cargo fmt -p <pkg> --check`, `cargo clippy -p <pkg>
--all-targets -- -D warnings`, and `cargo test -p <pkg> --lib`. The `--lib`
matters: the Docker-backed integration tests are not part of that run.

## Running and testing the app

Everything that drives the app is environment-variable driven; there are no
CLI flags.

| Variable | Effect |
| --- | --- |
| `RDB_STORE_DIR=<dir>` | Moves the connection store, settings **and** query tabs to `<dir>`. Without it, a test run reads and overwrites your real saved connections. |
| `RDB_MOCK=1` | Seeded in-memory data and an in-process driver, no network. Needs `--features mock`. |
| `RDB_WIN=WxH` | Fixed logical window size, for deterministic screenshots. |
| `RDB_SCREEN=<name>` | Drives the UI to a named screen (mock mode only). |
| `RDB_SHOT=<path.bmp>` | Screenshot after `RDB_SHOT_DELAY_MS` (default 1200), then quit. Needs `--features mock`. |

Three things that will cost you an afternoon if you hit them cold:

- **`RDB_MOCK` disables persistence.** Saving query tabs and restoring them on
  startup both no-op under it, deliberately, so the screenshot harness never
  touches your real tabs. A persistence test written in mock mode passes
  without testing anything — use `RDB_STORE_DIR` with a real connection
  (SQLite is easiest).
- **A plain `cargo build -p rdb` silently replaces the MCP-enabled binary.**
  The port stops opening and it looks like the app is broken. Rebuild through
  `make fe-run-mcp`, which passes `--features slint/mcp`.
- **A failing test aborts instead of reporting.** The release profile uses
  `panic=abort`, so a failed assertion becomes `SIGABRT` with no message and no
  test name. Re-run with
  `cargo test -p rdb --bin rdb -- --test-threads=1` — the last test printed is
  the one that failed.

For end-to-end scenarios the workspace tests do not cover, our sibling project
[`suiflex/suitest`](https://github.com/suiflex/suitest) can drive the app: it
ships a desktop target that bundles `slint-mcp` in-process, the same mechanism
`make fe-run-mcp` exposes. Entirely optional — it is not required to
contribute, and no PR is held up for lacking it.

## Adding a database engine

New engines are welcome. **Open an issue before you start writing one.** It's
not a gate to keep contributions out — it's that an engine touches more of the
codebase than the two obvious steps below suggest, and a short conversation up
front settles the scope (which engine, which auth modes, whether TLS is in the
first pass) while it's still cheap to change.

The two structural pieces are:

1. A new `crates/driver-<engine>/` crate implementing the `Driver` trait from
   `rdb-core`.
2. A variant in the `AnyDriver` enum in `app/src/dispatch.rs`, forwarding the
   trait methods.

Once `AnyDriver` has the variant, run `cargo build -p rdb` and fix every error
it reports. The compiler enumerates the exhaustive `Engine` / `Query` match
sites for you — do not hand-audit them.

The compiler will **not** catch these, and they are what previous engine
additions actually missed:

- The `ENGINES` row in `crates/connstore/src/model.rs` — display label, badge
  key, URL scheme, default port, query language all come from that one row.
- `scheme_to_engine` in `crates/connstore/src/conn_url.rs`.
- Slint UI: the engine picker and field visibility in
  `app/src/ui/conn-form.slint`, plus the engine list in Settings → About.
- Icons, two separate tracks: a monochrome `app/src/ui/icons/db-<engine>.svg`
  and a full-color `app/src/ui/icons/brand/<engine>.svg`. If no official mark
  exists, **don't hand-draw one** — use the text fallback other engines use.
- The "Database engine" dropdown in `.github/ISSUE_TEMPLATE/bug_report.yml`.
- Engine lists in `README.md`, `VISION.md` and `website/`.

[CLAUDE.md](CLAUDE.md) carries the full step-by-step checklist and stays the
source of truth — follow it rather than this summary when you actually do the
work.

**If your driver reads `SslMode`, it must compile a TLS backend in the same
change.** `mysql_async` and `scylla` **panic** rather than returning an error
when asked for TLS with no TLS feature built in, and `panic=abort` turns that
into a process abort on a worker thread — it never reaches `RdbError` and
surfaces as a crash with no app symbols. New connection forms default to
`Disable` so an unconfigured server cannot take the app down.

Keep engine-specific logic (SQL strings, protocol quirks) inside the driver
crate. The app layer must stay engine-agnostic — the only place it names a
concrete driver is `AnyDriver`.

## Commit conventions

We follow [Conventional Commits](https://www.conventionalcommits.org/); the
`release-please` workflow parses them to drive the changelog and version bumps.

- Types: `feat`, `feature`, `fix`, `perf`, `chore`, `refactor`, `docs`,
  `test`, `build`, `ci`.
- Use `feat(app): ...` for user-facing application/UI features. These appear
  under **App Features** in the generated changelog.
- Use `feature(driver-<engine>): ...` or `feature(driver): ...` for database
  driver capabilities. These appear under **Driver Features** in the generated
  changelog. `release-please` treats `feature` the same as `feat` for version
  bumping.
- Use `fix(<scope>): ...` for bug fixes and `perf(<scope>): ...` for
  performance improvements. These keep the standard release-please sections.
- Subject line ≤ 72 chars, imperative mood, no trailing period.
- Wrap the body at 72 chars and explain **why** the change exists — the diff
  already shows the what.
- One logical change per commit. Each commit should leave the tree in a
  buildable, testable state so `git revert` stays safe.
- Do **not** hand-edit the `release-please`-managed sections of any changelog.

Example release notes from commit headers:

```markdown
### App Features

* **app:** add saved-query folders

### Driver Features

* **driver-postgres:** introspect materialized views
* **driver-redis:** support key TTL editing

### Bug Fixes

* **app:** keep tabs after reconnect
* **driver-mysql:** parse unsigned bigint columns

### Performance Improvements

* **app:** virtualize large result grids
```

## Branching & pull requests

- Branch off `develop` using a name that matches the leading commit type:
  `feat/…`, `fix/…`, `refactor/…`, `chore/…`, `docs/…`.
- Fill out the pull request template (Summary / Changes / Test plan).
- Keep PRs focused on one logical change — smaller PRs are easier to review.
- Fill the test plan honestly: tick what you actually ran, leave the rest
  unchecked. An unchecked box is information; a wrongly ticked one wastes a
  review cycle.
- CI runs a scoped workflow per component — see [What CI covers](#what-ci-covers).
  Make sure `make all` passes locally first.

### AI-assisted pull requests

AI-assisted PRs are welcome — plenty of good contributions start that way. We
don't ask you to label them. We do ask for two things:

- **Evidence.** Say which commands you ran and what they produced. "`make all`
  passes", plus `make test-it` if you touched a driver's wire protocol, is
  enough. The code and CI get reviewed either way; the PR body is where you
  make that easy to follow.
- **Understand what you're submitting.** If a reviewer asks why a line is
  there, you should be able to answer. PRs whose author can't are the ones that
  stall.

## Reporting bugs & requesting features

Use the issue forms under **Issues → New issue**. Security vulnerabilities must
**not** be filed as public issues — see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE) that covers this project.

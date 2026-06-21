# RDBS — Agent Instructions

## Project

Native cross-platform database manager (PostgreSQL, MySQL, Redis, MongoDB) built with Rust + Slint UI. Monorepo workspace.

## Build, Lint, Test

A root `Makefile` wraps the common cargo invocations and splits FE (the
`storix` UI binary) from BE (the `crates/*` libraries) so each side builds and
tests independently. Run `make help` for the full target list.

```bash
make fe-build     # build the storix UI (FE)
make fe-run       # run the UI
make be-build     # build backend crates only (no FE)
make be-test      # test backend crates only
make fmt-check    # format check   (make fmt to apply)
make lint         # clippy, warnings as errors
make test         # test the whole workspace
make all          # fmt-check + lint + test + build (CI gate)
cargo build --release -p storix   # release binary
```

## Architecture

- `app/` — Slint UI binary (main entry point)
- `crates/core/` — `Driver` trait, `Query`, `ResultSet`, `Schema`, `DbmError`
- `crates/connstore/` — saved connections + OS keychain / AES-GCM
- `crates/driver-postgres/` — tokio-postgres
- `crates/driver-mysql/` — mysql_async
- `crates/driver-redis/` — redis crate
- `crates/driver-mongo/` — mongodb crate

## Key Rules

- UI (`app/`) names a concrete driver crate only in `app/src/dispatch.rs` (the `AnyDriver` enum); the rest of the app depends on `dbm-core`.
- Adding new engine = new `driver-*` crate implementing `Driver` trait + a variant in `AnyDriver`.
- Async I/O on tokio runtime, results bridge back to Slint main thread via `invoke_from_event_loop`.
- Release profile: `opt-level=z`, LTO, `panic=abort`, strip.

## Toolchain

Rust stable, components: rustfmt + clippy.

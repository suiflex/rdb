//! Design-mock mode gate. `mock_mode()` is always compiled so the call sites
//! in `main.rs`/`dispatch.rs` need no `#[cfg]`; without the `mock` feature it
//! folds to `false` and the seeded store + `MockDriver` are dropped from the
//! build entirely (see `imp`).

/// True when the app runs in design-mock mode. Only ever true when built with
/// the `mock` feature *and* `RDB_MOCK=1`.
pub fn mock_mode() -> bool {
    cfg!(feature = "mock") && std::env::var("RDB_MOCK").is_ok_and(|v| v == "1")
}

#[cfg(feature = "mock")]
mod imp;

#[cfg(feature = "mock")]
pub use imp::{mock_store, MockDriver};

// ponytail: stub so the (statically dead) mock_store call in main.rs still
// resolves without the feature; mock_mode() is const-false there, so unreachable.
#[cfg(not(feature = "mock"))]
pub fn mock_store(_dir: std::path::PathBuf) -> rdb_connstore::ConnStore {
    unreachable!("mock_store requires the `mock` feature")
}

//! Per-area UI wiring, split out of `main`.
//!
//! Each submodule exposes a single `wire(&MainWindow, &AppState, …)` that
//! installs the callbacks for one part of the window. `main` builds the state
//! and calls them in order; the handler bodies themselves are unchanged.

pub(crate) mod browse;
pub(crate) mod conn_form;
pub(crate) mod connect;
pub(crate) mod edit;
pub(crate) mod find;
pub(crate) mod grid;
pub(crate) mod picker;
pub(crate) mod query;
pub(crate) mod schema;
pub(crate) mod settings;
pub(crate) mod split_pane;
pub(crate) mod tabs;
pub(crate) mod update;

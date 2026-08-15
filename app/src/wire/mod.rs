//! Per-area UI wiring, split out of `main`.
//!
//! Each submodule exposes a single `wire(&MainWindow, &AppState, …)` that
//! installs the callbacks for one part of the window. `main` builds the state
//! and calls them in order; the handler bodies themselves are unchanged.

pub(crate) mod conn_form;
pub(crate) mod grid;
pub(crate) mod settings;
pub(crate) mod update;

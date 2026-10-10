//! Where a query tab's Run goes.
//!
//! One answer, read by the Run path and the topbar alike. Before this the
//! runner and the chrome each worked it out on their own, and an offline tab
//! showed one connection in the topbar while its query silently ran on
//! whichever connection happened to be live.

use std::collections::HashSet;

use rdb_connstore::{Engine, QueryLanguage, TabFocus, TabRebind, UiState};

/// The three settings collapsed into the one behaviour they pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabMode {
    /// "Query tabs follow connection": each connection has its own tabs, and a
    /// tab always runs on the connection it belongs to.
    Follow,
    /// The sidebar's connection is where a tab runs, as long as it speaks the
    /// tab's query language. `rebind_on_open` moves the tab's own connection
    /// over on opening instead of on its first run there.
    Sidebar { rebind_on_open: bool },
    /// Opening a tab moves the sidebar to the tab's connection.
    Tab,
}

impl TabMode {
    pub(crate) fn from_ui(ui: &UiState) -> Self {
        if ui.query_tabs_by_connection {
            return TabMode::Follow;
        }
        match ui.tab_focus {
            TabFocus::Tab => TabMode::Tab,
            TabFocus::Sidebar => TabMode::Sidebar {
                rebind_on_open: ui.tab_rebind == TabRebind::OnOpen,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RunTarget {
    /// Run on this open connection.
    Ready(String),
    /// Run on this connection once it is open: Run reads "Connect & Run" and
    /// nothing connects on its own.
    NeedsConnect(String),
    /// The sidebar's connection speaks another query language than the tab
    /// (a Mongo tab while Postgres is selected). Never run across families.
    Mismatch { origin: Option<String> },
    /// Nothing to run on until a connection is picked.
    NoConnection,
}

/// Query language of a badge key; `None` for a tab bound to nothing yet.
pub(crate) fn family(engine_key: &str) -> Option<QueryLanguage> {
    Engine::from_key(engine_key).map(Engine::language)
}

/// One query tab's side of the question.
pub(crate) struct TabSide<'a> {
    pub origin: Option<&'a str>,
    pub family: Option<QueryLanguage>,
}

/// The sidebar's side: its connection and that connection's query language.
pub(crate) struct SidebarSide<'a> {
    pub active: Option<&'a str>,
    pub family: Option<QueryLanguage>,
}

pub(crate) fn resolve(
    mode: TabMode,
    tab: TabSide,
    sidebar: SidebarSide,
    live: &HashSet<String>,
) -> RunTarget {
    let on = |id: &str| {
        if live.contains(id) {
            RunTarget::Ready(id.to_string())
        } else {
            RunTarget::NeedsConnect(id.to_string())
        }
    };
    match mode {
        TabMode::Follow => match tab.origin.or(sidebar.active) {
            Some(id) => on(id),
            None => RunTarget::NoConnection,
        },
        TabMode::Tab => match tab.origin {
            Some(id) => on(id),
            None => RunTarget::NoConnection,
        },
        TabMode::Sidebar { .. } => {
            let Some(active) = sidebar.active else {
                return RunTarget::NoConnection;
            };
            match (tab.family, sidebar.family) {
                (Some(t), Some(s)) if t != s => RunTarget::Mismatch {
                    origin: tab.origin.map(str::to_string),
                },
                _ => on(active),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQL: Option<QueryLanguage> = Some(QueryLanguage::Sql);
    const MONGO: Option<QueryLanguage> = Some(QueryLanguage::Mongo);
    const SIDEBAR: TabMode = TabMode::Sidebar {
        rebind_on_open: false,
    };

    fn live(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn run(
        mode: TabMode,
        origin: Option<&str>,
        tab_family: Option<QueryLanguage>,
        active: Option<&str>,
        active_family: Option<QueryLanguage>,
        open: &[&str],
    ) -> RunTarget {
        resolve(
            mode,
            TabSide {
                origin,
                family: tab_family,
            },
            SidebarSide {
                active,
                family: active_family,
            },
            &live(open),
        )
    }

    #[test]
    fn sidebar_mode_runs_a_same_family_tab_on_the_sidebar_connection() {
        let t = run(
            SIDEBAR,
            Some("mysql"),
            SQL,
            Some("pg"),
            SQL,
            &["pg", "mysql"],
        );
        assert_eq!(t, RunTarget::Ready("pg".into()));
    }

    #[test]
    fn sidebar_mode_never_runs_across_families() {
        let t = run(
            SIDEBAR,
            Some("mongo"),
            MONGO,
            Some("pg"),
            SQL,
            &["pg", "mongo"],
        );
        assert_eq!(
            t,
            RunTarget::Mismatch {
                origin: Some("mongo".into())
            }
        );
    }

    #[test]
    fn sidebar_mode_ignores_the_tab_connection_being_offline() {
        let t = run(SIDEBAR, Some("stg"), SQL, Some("pg"), SQL, &["pg"]);
        assert_eq!(t, RunTarget::Ready("pg".into()));
    }

    #[test]
    fn sidebar_mode_takes_an_unbound_tab() {
        let t = run(SIDEBAR, None, None, Some("pg"), SQL, &["pg"]);
        assert_eq!(t, RunTarget::Ready("pg".into()));
    }

    #[test]
    fn sidebar_mode_without_a_sidebar_connection_has_nowhere_to_run() {
        let t = run(SIDEBAR, Some("pg"), SQL, None, None, &["pg"]);
        assert_eq!(t, RunTarget::NoConnection);
    }

    /// Regression: an offline tab ran on whatever connection was live, which
    /// could be production, while the topbar named the offline one.
    #[test]
    fn tab_mode_waits_for_the_tab_connection_instead_of_borrowing_one() {
        let t = run(TabMode::Tab, Some("stg"), SQL, Some("prod"), SQL, &["prod"]);
        assert_eq!(t, RunTarget::NeedsConnect("stg".into()));
    }

    #[test]
    fn tab_mode_runs_an_unbound_tab_nowhere() {
        let t = run(TabMode::Tab, None, None, Some("pg"), SQL, &["pg"]);
        assert_eq!(t, RunTarget::NoConnection);
    }

    #[test]
    fn follow_mode_runs_on_the_owning_connection() {
        let t = run(TabMode::Follow, Some("a"), SQL, Some("b"), SQL, &["a", "b"]);
        assert_eq!(t, RunTarget::Ready("a".into()));
        let t = run(TabMode::Follow, None, None, Some("b"), SQL, &["b"]);
        assert_eq!(t, RunTarget::Ready("b".into()));
    }

    #[test]
    fn mode_comes_from_the_three_settings() {
        let mut ui = UiState::default();
        assert_eq!(TabMode::from_ui(&ui), SIDEBAR);
        ui.tab_rebind = TabRebind::OnOpen;
        assert_eq!(
            TabMode::from_ui(&ui),
            TabMode::Sidebar {
                rebind_on_open: true
            }
        );
        ui.tab_focus = TabFocus::Tab;
        assert_eq!(TabMode::from_ui(&ui), TabMode::Tab);
        ui.query_tabs_by_connection = true;
        assert_eq!(TabMode::from_ui(&ui), TabMode::Follow);
    }
}

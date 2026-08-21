//! Connection-form wiring: add, edit, import-URL, test, save, delete.
//!
//! Split out of `main` so the form's handlers sit next to each other instead of
//! in the middle of eight thousand lines of unrelated callbacks. The bodies are
//! unchanged; they take their state out of [`AppState`] at the top instead of
//! capturing whatever `main` happened to have in scope.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

/// Which password a Test should actually use.
///
/// The typed one wins. An empty box means "unchanged" — the same thing it means
/// on save (`ConnStore::save_connection` skips an empty password rather than
/// clearing the secret) — so Test falls back to the stored secret and exercises
/// what Connect would really use. Without this, testing an existing connection
/// authenticates with no password at all and fails on a connection that works.
///
/// `stored` is lazy so the add-connection form, which has no id yet, never
/// reaches into the store for some other connection's secret.
fn test_password(typed: Option<String>, stored: impl FnOnce() -> Option<String>) -> Option<String> {
    match typed {
        Some(p) if !p.is_empty() => Some(p),
        _ => stored(),
    }
}

/// How the password box should describe a saved connection's secret, as
/// `(has_password, unreadable)`.
///
/// Kept separate from `.ok()` on purpose. Collapsing the error case into "no
/// password" makes an unreadable store look exactly like an empty one — a blank
/// field either way — and leaves the user with nothing to act on.
fn password_state(read: rdb_connstore::Result<Option<String>>) -> (bool, bool) {
    match read {
        Ok(pw) => (pw.is_some_and(|s| !s.is_empty()), false),
        Err(_) => (false, true),
    }
}

pub(crate) fn wire(window: &MainWindow, state: &AppState) {
    let AppState {
        rt,
        store,
        collapsed,
        conn_filter,
        editing_id,
        ..
    } = state.clone();

    // ----- connection form (add / edit / delete) -----

    // open add form
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        window.on_open_add_form(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *editing_id.borrow_mut() = String::new();
            open_add_form(&w, &store.borrow(), None);
        });
    }
    // open add form, pre-nested under an existing top-level group ("New
    // Subgroup" on the picker's group context menu, only offered on
    // top-level headers — see picker.slint).
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        window.on_open_add_form_in_group(move |parent| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *editing_id.borrow_mut() = String::new();
            open_add_form(&w, &store.borrow(), Some(parent.as_ref()));
        });
    }
    // open edit form
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        window.on_open_edit_form(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let st = store.borrow();
            let Some(sc) = st.list().get(idx as usize).cloned() else {
                return;
            };
            *editing_id.borrow_mut() = sc.id.clone();
            // The stored secret is never shown; expose only whether one exists so
            // the form can prompt "leave blank to keep" instead of looking empty.
            //
            // A failed read is reported separately rather than folded in. `.ok()`
            // alone would turn "the store errored" into "there is no password",
            // which looks identical to an empty field and leaves the user with no
            // way to tell a missing secret from an unreadable one.
            let (has_pw, pw_unreadable) = password_state(st.get_password(&sc.id));
            let has_ssh_sec = st
                .get_ssh_secret(&sc.id)
                .ok()
                .flatten()
                .is_some_and(|s| !s.is_empty());
            w.set_form_edit_mode(true);
            w.set_f_name(SharedString::from(sc.name));
            w.set_f_engine(SharedString::from(AnyDriver::label(sc.engine)));
            w.set_f_host(SharedString::from(sc.host));
            w.set_f_port(SharedString::from(sc.port.to_string()));
            w.set_f_user(SharedString::from(sc.user));
            w.set_f_database(SharedString::from(sc.database.unwrap_or_default()));
            w.set_f_password(SharedString::default());
            w.set_f_has_password(has_pw);
            w.set_f_password_unreadable(pw_unreadable);
            w.set_f_sslmode(SharedString::from(match sc.sslmode {
                rdb_core::conn::SslMode::Disable => "Disable",
                rdb_core::conn::SslMode::Prefer => "Prefer",
                rdb_core::conn::SslMode::Require => "Require",
            }));
            w.set_f_params(SharedString::from(sc.params.unwrap_or_default()));
            w.set_f_color(SharedString::from(
                sc.color.unwrap_or_else(|| "#2c5fd8".into()),
            ));
            w.set_f_env_tag(SharedString::from(sc.env_tag.as_str()));
            w.set_f_ssh_enabled(sc.ssh_enabled);
            w.set_f_ssh_host(SharedString::from(sc.ssh_host.unwrap_or_default()));
            w.set_f_ssh_port(SharedString::from(sc.ssh_port.unwrap_or(22).to_string()));
            w.set_f_ssh_user(SharedString::from(sc.ssh_user.unwrap_or_default()));
            w.set_f_ssh_auth_mode(SharedString::from(sc.ssh_auth_mode.as_str()));
            w.set_f_ssh_key_path(SharedString::from(sc.ssh_key_path.unwrap_or_default()));
            w.set_f_ssh_password(SharedString::default());
            w.set_f_ssh_passphrase(SharedString::default());
            w.set_f_has_ssh_secret(has_ssh_sec);
            w.set_f_new_group_text(SharedString::default());
            match sc.group.as_deref() {
                None => {
                    w.set_f_group_display(SharedString::from("None"));
                    w.set_f_subgroup_display(SharedString::from("None"));
                    w.set_f_new_subgroup_text(SharedString::default());
                    w.set_subgroup_options(ModelRc::from(Rc::new(VecModel::from(vec![
                        SharedString::from("None"),
                        SharedString::from("+ New subgroup…"),
                    ]))));
                }
                Some(g) => {
                    let top = g.split('/').next().unwrap_or(g).to_string();
                    let rest = g[top.len()..].trim_start_matches('/').to_string();
                    let sub_opts = subgroup_picker_options(&st, &top);
                    w.set_f_group_display(SharedString::from(top));
                    if rest.is_empty() {
                        w.set_f_subgroup_display(SharedString::from("None"));
                        w.set_f_new_subgroup_text(SharedString::default());
                    } else if sub_opts.iter().any(|o| o == &rest) {
                        w.set_f_subgroup_display(SharedString::from(rest));
                        w.set_f_new_subgroup_text(SharedString::default());
                    } else {
                        // A deeper/irregular path than the guided picker
                        // covers — surface it as free text so editing never
                        // silently truncates it.
                        w.set_f_subgroup_display(SharedString::from("+ New subgroup…"));
                        w.set_f_new_subgroup_text(SharedString::from(rest));
                    }
                    w.set_subgroup_options(ModelRc::from(Rc::new(VecModel::from(
                        sub_opts
                            .into_iter()
                            .map(SharedString::from)
                            .collect::<Vec<_>>(),
                    ))));
                }
            }
            w.set_group_options(ModelRc::from(Rc::new(VecModel::from(
                group_picker_options(&st)
                    .into_iter()
                    .map(SharedString::from)
                    .collect::<Vec<_>>(),
            ))));
            w.set_f_import_url(SharedString::default());
            w.set_form_error(SharedString::default());
            w.set_test_result(SharedString::default());
            w.set_test_ok(false);
            w.set_test_busy(false);
            w.set_form_open(true);
        });
    }
    // Group dropdown changed -> recompute the Subgroup dropdown's options
    // for the newly-picked parent.
    {
        let weak = window.as_weak();
        let store = store.clone();
        window.on_form_group_changed(move |top| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let opts = if top == "None" || top == "+ New group…" {
                vec!["None".to_string(), "+ New subgroup…".to_string()]
            } else {
                subgroup_picker_options(&store.borrow(), &top)
            };
            w.set_subgroup_options(ModelRc::from(Rc::new(VecModel::from(
                opts.into_iter().map(SharedString::from).collect::<Vec<_>>(),
            ))));
        });
    }
    // engine changed -> default port if port empty/default-ish
    {
        let weak = window.as_weak();
        window.on_form_engine_changed(move |label| {
            if let Some(w) = weak.upgrade() {
                let cur = w.get_f_port().to_string();
                // "did the user customize the port?" — any engine's default
                // counts as untouched, read off ENGINES so a new engine's
                // default is included automatically. The old hardcoded list
                // had gone stale and missed Cassandra/SQL Server/ClickHouse.
                let is_a_default = rdb_connstore::ENGINES.iter().any(|m| m.default_port == cur);
                if cur.is_empty() || is_a_default {
                    w.set_f_port(SharedString::from(default_port(&label)));
                }
            }
        });
    }
    // import URL -> parse and fill form fields for review.
    {
        let weak = window.as_weak();
        window.on_form_import_url(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let raw = w.get_f_import_url().to_string();
            match rdb_connstore::parse_conn_url(&raw) {
                Ok(parsed) => {
                    if let Some(engine) = parsed.engine {
                        w.set_f_engine(SharedString::from(AnyDriver::label(engine)));
                    }
                    if let Some(host) = parsed.host {
                        w.set_f_host(SharedString::from(host));
                    }
                    // Port from the URL wins; otherwise apply the engine default
                    // (mirrors form_engine_changed) without clobbering a URL port.
                    if let Some(port) = parsed.port {
                        w.set_f_port(SharedString::from(port.to_string()));
                    } else if let Some(engine) = parsed.engine {
                        w.set_f_port(SharedString::from(default_port(AnyDriver::label(engine))));
                    }
                    if let Some(user) = parsed.user {
                        w.set_f_user(SharedString::from(user));
                    }
                    if let Some(password) = parsed.password {
                        w.set_f_password(SharedString::from(password));
                    }
                    if let Some(database) = parsed.database {
                        w.set_f_database(SharedString::from(database));
                    }
                    if let Some(sslmode) = parsed.sslmode {
                        w.set_f_sslmode(SharedString::from(match sslmode {
                            rdb_core::conn::SslMode::Disable => "Disable",
                            rdb_core::conn::SslMode::Prefer => "Prefer",
                            rdb_core::conn::SslMode::Require => "Require",
                        }));
                    }
                    w.set_form_error(SharedString::default());
                }
                Err(e) => {
                    w.set_form_error(SharedString::from(format!("import failed: {e}")));
                }
            }
        });
    }
    // cancel
    {
        let weak = window.as_weak();
        window.on_form_cancel(move || {
            if let Some(w) = weak.upgrade() {
                // Clear any in-flight test state so the form is never stuck on
                // "Testing connection…" when reopened.
                w.set_test_busy(false);
                w.set_test_result(SharedString::default());
                w.set_form_open(false);
            }
        });
    }
    // export saved connections. The URL embeds the real (percent-encoded)
    // password so the file is a re-usable backup — it is sensitive. 0=JSON, 1=CSV.
    {
        let weak = window.as_weak();
        let store = store.clone();
        window.on_export_conns(move |fmt| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let st = store.borrow();
            // Real passwords are embedded so the export can be re-imported.
            let pw_for = |c: &rdb_connstore::SavedConnection| st.get_password(&c.id).ok().flatten();
            let (ext, contents) = if fmt == 1 {
                ("csv", export::conns_to_csv(st.list(), pw_for))
            } else {
                ("json", export::conns_to_json(st.list(), pw_for))
            };
            save_via_dialog(
                &w,
                format!("rdb-connections.{ext}"),
                ext.to_uppercase(),
                ext.to_string(),
                contents,
                |w, msg| w.set_sel_footer(SharedString::from(msg)),
            );
        });
    }
    // quick test from the picker detail pane: saved config, result in the
    // detail footer line.
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let store = store.clone();
        window.on_test_conn_quick(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (engine, cfg) = {
                let st = store.borrow();
                let Some(sc) = st.list().get(idx.max(0) as usize) else {
                    return;
                };
                match st.conn_config_for(&sc.id) {
                    Ok(c) => (sc.engine, c),
                    Err(e) => {
                        w.set_sel_footer(SharedString::from(format!("connection failed: {e}")));
                        return;
                    }
                }
            };
            w.set_sel_footer(SharedString::from("Testing connection…"));
            let weak2 = weak.clone();
            rt.spawn(async move {
                let result = try_connect(engine, cfg).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_sel_footer(SharedString::from(match result {
                            Ok(ms) => format!("connection ok · {}", model::format_latency(ms)),
                            // `RdbError`'s own `Display` already reads
                            // "connection failed: …" — don't prefix it again.
                            Err(e) => format!("{e}"),
                        }));
                    }
                });
            });
        });
    }
    // browse SSH private key file
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        window.on_form_browse_ssh_key(move || {
            let Some(_w) = weak.upgrade() else {
                return;
            };
            let weak2 = weak.clone();
            rt.spawn(async move {
                let dialog = rfd::AsyncFileDialog::new().set_title("Select SSH Private Key");
                if let Some(file) = dialog.pick_file().await {
                    let path = file.path().to_string_lossy().to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = weak2.upgrade() {
                            w.set_f_ssh_key_path(SharedString::from(path));
                        }
                    });
                }
            });
        });
    }
    // test connection: build a config straight from the form fields (not the
    // store) so unsaved edits are exercised, open a real connection, then drop
    // it. Result reported in the form's test-result line.
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let store = store.clone();
        let editing_id = editing_id.clone();
        window.on_form_test_conn(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let f = match read_conn_form(&w) {
                Ok(f) => f,
                Err(msg) => {
                    w.set_test_ok(false);
                    w.set_test_result(SharedString::from(msg));
                    return;
                }
            };
            let engine = f.engine;
            let ssh = if f.ssh_enabled && f.engine != rdb_connstore::Engine::Sqlite {
                if let (Some(host), Some(user)) = (&f.ssh_host, &f.ssh_user) {
                    let (ssh_pw, ssh_passphrase) = match f.ssh_auth_mode {
                        rdb_core::conn::SshAuthMode::Password => {
                            let pw = f.ssh_password.or_else(|| {
                                let id = editing_id.borrow();
                                if !id.is_empty() {
                                    store.borrow().get_ssh_secret(&id).ok().flatten()
                                } else {
                                    None
                                }
                            });
                            (pw, None)
                        }
                        rdb_core::conn::SshAuthMode::KeyFile => {
                            let pass = f.ssh_passphrase.or_else(|| {
                                let id = editing_id.borrow();
                                if !id.is_empty() {
                                    store.borrow().get_ssh_secret(&id).ok().flatten()
                                } else {
                                    None
                                }
                            });
                            (None, pass)
                        }
                        rdb_core::conn::SshAuthMode::Agent => (None, None),
                    };
                    Some(rdb_core::conn::SshTunnelConfig {
                        host: host.clone(),
                        port: f.ssh_port.unwrap_or(22),
                        user: user.clone(),
                        auth_mode: f.ssh_auth_mode,
                        key_path: f.ssh_key_path.clone(),
                        password: ssh_pw,
                        passphrase: ssh_passphrase,
                    })
                } else {
                    None
                }
            } else {
                None
            };

            // Same fallback the SSH branches above already do — this field was
            // the one that missed it.
            let typed_password = f.password.clone();
            let password = test_password(f.password, || {
                let id = editing_id.borrow();
                if id.is_empty() {
                    None
                } else {
                    store.borrow().get_password(&id).ok().flatten()
                }
            });
            // A pass is ambiguous otherwise: the user cannot tell whether their
            // typing or the saved secret was exercised.
            let used_saved = typed_password.is_none() && password.is_some();

            let cfg = rdb_core::conn::ConnConfig {
                host: f.host,
                port: f.port,
                user: f.user,
                database: f.database,
                password,
                sslmode: f.sslmode,
                params: f.params,
                ssh,
            };

            w.set_test_busy(true);
            w.set_test_ok(false);
            w.set_test_result(SharedString::default());
            w.set_form_error(SharedString::default());

            let weak2 = weak.clone();
            rt.spawn(async move {
                let result = try_connect(engine, cfg).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_test_busy(false);
                        match result {
                            Ok(_) => {
                                w.set_test_ok(true);
                                w.set_test_result(SharedString::from(if used_saved {
                                    "connection ok — used the saved password"
                                } else {
                                    "connection ok"
                                }));
                            }
                            Err(e) => {
                                w.set_test_ok(false);
                                // `RdbError`'s own `Display` already reads
                                // "connection failed: …" — don't prefix it again.
                                w.set_test_result(SharedString::from(format!("{e}")));
                            }
                        }
                    }
                });
            });
        });
    }
    // save (add or update)
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        let rebuild = {
            let weak = window.as_weak();
            let store = store.clone();
            let collapsed = collapsed.clone();
            let conn_filter = conn_filter.clone();
            move || {
                if let Some(w) = weak.upgrade() {
                    w.set_connections(build_sidebar_model(
                        &store.borrow(),
                        &collapsed.borrow(),
                        &conn_filter.borrow(),
                    ));
                }
            }
        };
        window.on_form_save(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let name = w.get_f_name().to_string();
            if name.trim().is_empty() {
                w.set_form_error(SharedString::from("name is required"));
                return;
            }
            let f = match read_conn_form(&w) {
                Ok(f) => f,
                Err(msg) => {
                    w.set_form_error(SharedString::from(msg));
                    return;
                }
            };
            let FormConn {
                engine,
                host,
                port,
                user,
                database,
                password,
                params,
                sslmode,
                ssh_enabled,
                ssh_host,
                ssh_port,
                ssh_user,
                ssh_auth_mode,
                ssh_key_path,
                ssh_password,
                ssh_passphrase,
            } = f;
            // An empty box means "keep the stored secret" — see
            // `ConnStore::save_connection`.
            let password = password.unwrap_or_default();
            let color = Some(w.get_f_color().to_string());
            let env_tag = rdb_connstore::EnvTag::parse(w.get_f_env_tag().as_ref());
            let group = {
                let g = w.get_f_group().to_string().trim().to_string();
                if g.is_empty() {
                    None
                } else {
                    Some(g)
                }
            };
            // A freshly-typed group ("+ New group…") that only differs in case
            // from an existing one joins it instead of spawning a duplicate.
            let group = group.map(|g| {
                existing_groups(&store.borrow())
                    .into_iter()
                    .find(|e| e.eq_ignore_ascii_case(&g))
                    .unwrap_or(g)
            });
            let id = editing_id.borrow().clone();

            let result: rdb_connstore::Result<()> = (|| {
                let mut st = store.borrow_mut();
                let mut sc = if id.is_empty() {
                    rdb_connstore::SavedConnection::new(
                        name.clone(),
                        engine,
                        host.clone(),
                        port,
                        user.clone(),
                    )
                } else {
                    st.get(&id)
                        .cloned()
                        .ok_or_else(|| rdb_connstore::ConnStoreError::NotFound(id.clone()))?
                };
                sc.name = name;
                sc.engine = engine;
                sc.host = host;
                sc.port = port;
                sc.user = user;
                sc.database = database;
                sc.sslmode = sslmode;
                sc.color = color;
                sc.env_tag = env_tag;
                sc.group = group;
                sc.params = params;
                sc.ssh_enabled = ssh_enabled;
                sc.ssh_host = ssh_host;
                sc.ssh_port = ssh_port;
                sc.ssh_user = ssh_user;
                sc.ssh_auth_mode = ssh_auth_mode;
                sc.ssh_key_path = ssh_key_path;

                let ssh_secret = match ssh_auth_mode {
                    rdb_core::conn::SshAuthMode::Password => ssh_password,
                    rdb_core::conn::SshAuthMode::KeyFile => ssh_passphrase,
                    rdb_core::conn::SshAuthMode::Agent => None,
                };

                st.save_connection_with_ssh(sc, Some(&password), ssh_secret.as_deref())
            })();

            match result {
                Ok(()) => {
                    w.set_form_open(false);
                    rebuild();
                }
                Err(e) => {
                    w.set_form_error(SharedString::from(format!("save failed: {e}")));
                }
            }
        });
    }
    // delete
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_form_delete_confirmed(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let id = editing_id.borrow().clone();
            if id.is_empty() {
                w.set_form_open(false);
                return;
            }
            {
                let mut st = store.borrow_mut();
                let _ = st.delete_password(&id);
                let _ = st.remove(&id);
            }
            w.set_form_open(false);
            w.set_selected_conn(-1);
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::<TreeNode>::default())));
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{password_state, test_password};
    use std::cell::Cell;

    #[test]
    fn a_stored_secret_reads_as_present() {
        assert_eq!(password_state(Ok(Some("hunter2".into()))), (true, false));
    }

    #[test]
    fn no_secret_reads_as_absent_not_broken() {
        assert_eq!(password_state(Ok(None)), (false, false));
        // An empty string is not a password.
        assert_eq!(password_state(Ok(Some(String::new()))), (false, false));
    }

    #[test]
    fn an_unreadable_store_is_not_reported_as_no_password() {
        // The whole point: this used to be indistinguishable from Ok(None), so
        // a broken secret store showed the same blank box as a connection that
        // genuinely has no password.
        let err = rdb_connstore::ConnStoreError::Secret("keychain denied".into());
        assert_eq!(password_state(Err(err)), (false, true));
    }

    #[test]
    fn typed_password_wins_over_the_stored_one() {
        let got = test_password(Some("typed".into()), || Some("stored".into()));
        assert_eq!(got.as_deref(), Some("typed"));
    }

    #[test]
    fn empty_box_falls_back_to_the_stored_secret() {
        // The reported bug: an existing connection tested with no password at
        // all, so a connection that works failed its own Test button.
        let got = test_password(None, || Some("stored".into()));
        assert_eq!(got.as_deref(), Some("stored"));
    }

    #[test]
    fn a_blank_string_counts_as_empty_not_as_a_password() {
        // read_conn_form normalises "" to None, but the rule must not depend on
        // that happening upstream — an empty string is "unchanged" everywhere
        // else, including ConnStore::save_connection.
        let got = test_password(Some(String::new()), || Some("stored".into()));
        assert_eq!(got.as_deref(), Some("stored"));
    }

    #[test]
    fn nothing_typed_and_nothing_stored_stays_none() {
        let got = test_password(None, || None);
        assert_eq!(got, None);
    }

    #[test]
    fn the_store_is_not_consulted_when_a_password_was_typed() {
        // Laziness matters: reading the secret hits the keychain or decrypts a
        // file, and the add form has no id to look up in the first place.
        let looked_up = Cell::new(false);
        let got = test_password(Some("typed".into()), || {
            looked_up.set(true);
            Some("stored".into())
        });
        assert_eq!(got.as_deref(), Some("typed"));
        assert!(!looked_up.get(), "store was read despite a typed password");
    }
}

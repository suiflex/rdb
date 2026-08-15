//! Update wiring: the "newer version is out" reminder, the in-app
//! swap-and-relaunch flow, the manual "Check now" button, and the once-a-day
//! background check.
//!
//! Split out of `main`; the handler bodies are unchanged.

use slint::{ComponentHandle, SharedString};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState) {
    let AppState { settings, .. } = state.clone();

    // Whether the in-app swap-and-relaunch flow applies to this exact
    // install; false in mock mode so screenshot tests never depend on the
    // build machine's binary path. Homebrew/Scoop are never eligible — see
    // `InstallMethod::self_update_supported`.
    fn self_update_supported_now() -> bool {
        !mock::mock_mode()
            && update::InstallMethod::detect()
                .self_update_supported(std::env::current_exe().ok().as_deref())
    }

    // ----- update reminder: open the release page / dismiss -----
    {
        window.on_update_open(move || {
            let _ = open::that(update::release_page());
        });
    }
    {
        let weak = window.as_weak();
        window.on_update_dismiss(move || {
            if let Some(w) = weak.upgrade() {
                w.set_update_available(false);
            }
        });
    }
    // ----- restart to update: idle/error -> download, ready -> swap + relaunch -----
    {
        let weak = window.as_weak();
        let staged_path: Arc<std::sync::Mutex<Option<std::path::PathBuf>>> =
            Arc::new(std::sync::Mutex::new(None));
        window.on_restart_to_update(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let stage = w.get_update_stage();
            match stage.as_str() {
                "idle" | "error" => {
                    w.set_update_stage(SharedString::from("downloading"));
                    w.set_update_progress(0.0);
                    w.set_update_error(SharedString::default());
                    let weak2 = weak.clone();
                    let staged_path = staged_path.clone();
                    std::thread::spawn(move || {
                        let last_reported = std::sync::atomic::AtomicI32::new(-1);
                        let result = self_update::fetch_latest_assets()
                            .map_err(|e| e.to_string())
                            .and_then(|assets| {
                                self_update::pick_asset(&assets).ok_or_else(|| {
                                    "no matching release asset for this platform".to_string()
                                })
                            })
                            .and_then(|asset| {
                                let weak3 = weak2.clone();
                                self_update::download_asset(&asset, |frac| {
                                    let pct = (frac * 100.0) as i32;
                                    if pct
                                        == last_reported
                                            .swap(pct, std::sync::atomic::Ordering::Relaxed)
                                    {
                                        return;
                                    }
                                    let weak4 = weak3.clone();
                                    let _ = slint::invoke_from_event_loop(move || {
                                        if let Some(w) = weak4.upgrade() {
                                            w.set_update_progress(pct as f32 / 100.0);
                                        }
                                    });
                                })
                                .map_err(|e| e.to_string())
                            });
                        match result {
                            Ok(path) => {
                                *staged_path.lock().unwrap() = Some(path);
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(w) = weak2.upgrade() {
                                        w.set_update_progress(1.0);
                                        w.set_update_stage(SharedString::from("ready"));
                                    }
                                });
                            }
                            Err(e) => {
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(w) = weak2.upgrade() {
                                        w.set_update_error(SharedString::from(e));
                                        w.set_update_stage(SharedString::from("error"));
                                    }
                                });
                            }
                        }
                    });
                }
                "ready" => {
                    let Some(path) = staged_path.lock().unwrap().clone() else {
                        w.set_update_stage(SharedString::from("idle"));
                        return;
                    };
                    w.set_update_stage(SharedString::from("restarting"));
                    let weak2 = weak.clone();
                    std::thread::spawn(move || match self_update::perform_swap(&path) {
                        Ok(()) => {
                            let _ = slint::invoke_from_event_loop(|| {
                                let _ = slint::quit_event_loop();
                            });
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(w) = weak2.upgrade() {
                                    w.set_update_error(SharedString::from(msg));
                                    w.set_update_stage(SharedString::from("error"));
                                }
                            });
                        }
                    });
                }
                _ => {} // "downloading"/"restarting": ignore repeat clicks
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_update_copy_hint(move || {
            if let Some(w) = weak.upgrade() {
                clip_set(&w.get_update_hint());
            }
        });
    }

    // ----- manual "Check now" from settings: bypasses the daily throttle and
    // reports the outcome inline so the toggle no longer feels like a no-op -----
    {
        let weak = window.as_weak();
        window.on_check_updates_now(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            w.set_update_check_status(SharedString::from("Checking…"));
            let weak = weak.clone();
            std::thread::spawn(move || {
                let current = env!("CARGO_PKG_VERSION");
                let tag = update::fetch_latest_tag();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        match tag {
                            Some(t) if update::is_newer(&t, current) => {
                                let version = t.trim_start_matches('v').to_string();
                                let hint =
                                    update::InstallMethod::detect().upgrade_hint().to_string();
                                w.set_update_check_status(SharedString::from(format!(
                                    "Update available — v{version}"
                                )));
                                w.set_update_version(version.into());
                                w.set_update_hint(hint.into());
                                w.set_update_self_update_supported(self_update_supported_now());
                                w.set_update_stage(SharedString::from("idle"));
                                w.set_update_available(true);
                            }
                            Some(_) => w.set_update_check_status(SharedString::from(format!(
                                "Up to date (v{current})"
                            ))),
                            None => w.set_update_check_status(SharedString::from(
                                "Check failed — try again",
                            )),
                        }
                    }
                });
            });
        });
    }

    // ----- update check: once/day, gated by the setting, off the UI thread -----
    // Skip in mock mode so the reference screenshots stay deterministic.
    if !mock::mock_mode() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (enabled, last) = {
            let s = settings.borrow();
            (s.get().update_check, s.get().last_update_check)
        };
        if enabled && update::due_for_check(last, now) {
            // Persist "checked now" up front on the UI thread — the Rc settings
            // store cannot cross into the worker thread. Worst case a failed
            // check simply waits a day, which is the intended throttle.
            let _ = settings
                .borrow_mut()
                .update(|s| s.last_update_check = Some(now));
            let weak = window.as_weak();
            std::thread::spawn(move || {
                let Some(tag) = update::fetch_latest_tag() else {
                    return;
                };
                if !update::is_newer(&tag, env!("CARGO_PKG_VERSION")) {
                    return;
                }
                let version = tag.trim_start_matches('v').to_string();
                let hint = update::InstallMethod::detect().upgrade_hint().to_string();
                // Native nudge for when the window isn't in focus at launch; the
                // in-app banner still shows for when it is.
                update::notify_update(&version, &hint);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_update_version(version.into());
                        w.set_update_hint(hint.into());
                        w.set_update_self_update_supported(self_update_supported_now());
                        w.set_update_stage(SharedString::from("idle"));
                        w.set_update_available(true);
                    }
                });
            });
        }
    }
}

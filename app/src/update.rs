//! Startup "a newer version is out" check.
//!
//! Network and version comparison live here as small pure-ish helpers;
//! `main.rs` orchestrates: it reads the throttle/toggle off the settings store
//! on the UI thread, does the HTTP GET on a background thread, then hops back
//! via `invoke_from_event_loop` to show the banner and persist the timestamp.

use std::path::Path;

use semver::Version;

const RELEASES_LATEST: &str = "https://api.github.com/repos/suiflex/rdb/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/suiflex/rdb/releases/latest";
const DAY_SECS: i64 = 86_400;

/// How the running binary was installed, which decides the upgrade advice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    Homebrew,
    Scoop,
    /// curl installer, manual copy, or unknown.
    Other,
}

impl InstallMethod {
    pub fn detect() -> Self {
        Self::from_exe_path(std::env::current_exe().ok().as_deref())
    }

    fn from_exe_path(path: Option<&Path>) -> Self {
        let Some(p) = path else {
            return InstallMethod::Other;
        };
        let s = p.to_string_lossy().to_lowercase();
        if s.contains("/cellar/") || s.contains("/homebrew/") {
            InstallMethod::Homebrew
        } else if s.contains("/scoop/apps/") || s.contains("\\scoop\\apps\\") {
            InstallMethod::Scoop
        } else {
            InstallMethod::Other
        }
    }

    /// One-line upgrade instruction to show in the banner.
    pub fn upgrade_hint(self) -> &'static str {
        match self {
            InstallMethod::Homebrew => "Run: brew upgrade rdb",
            InstallMethod::Scoop => "Run: scoop update rdb",
            InstallMethod::Other => "Open the download page",
        }
    }

    /// Whether the in-app "Restart to Update" flow (`self_update.rs`) can run
    /// for this install. Homebrew/Scoop are never eligible — self-updating a
    /// package-manager-owned install would fight `brew upgrade`/`scoop
    /// update`, so those keep the plain hint + release-page link below.
    /// `Other` still needs `self_update::is_swappable` to confirm this
    /// specific binary is actually a layout we know how to swap (a real
    /// `.app` bundle on macOS, a recognized target on Windows) — a dev build
    /// or unsupported platform falls back to the same hint-only behavior.
    pub fn self_update_supported(self, exe: Option<&std::path::Path>) -> bool {
        self == InstallMethod::Other && crate::self_update::is_swappable(exe)
    }
}

/// Clicking the banner opens the release page — still the only action for
/// Homebrew/Scoop (fighting the package manager) and for any `Other` install
/// this build can't safely swap in place. Installs where
/// `InstallMethod::self_update_supported` is true get the "Restart to
/// Update" flow instead (see `self_update.rs`).
pub fn release_page() -> &'static str {
    RELEASES_PAGE
}

/// Fire a native desktop notification inviting the user to update. Best-effort:
/// any platform/permission failure is swallowed so it never disrupts startup.
pub fn notify_update(version: &str, hint: &str) {
    let _ = notify_rust::Notification::new()
        .summary(&format!("RDB {version} is available"))
        .body(&format!("A new version is out — {hint}."))
        .show();
}

/// True if `latest_tag` (e.g. "v1.2.0") is a strictly newer semver than the
/// running `current` version. Unparseable input is treated as "not newer" so a
/// malformed tag never nags the user.
pub fn is_newer(latest_tag: &str, current: &str) -> bool {
    let latest = latest_tag.trim_start_matches('v');
    match (Version::parse(latest), Version::parse(current)) {
        (Ok(l), Ok(c)) => l > c,
        _ => false,
    }
}

/// Throttle: check at most once per day.
pub fn due_for_check(last_check: Option<i64>, now: i64) -> bool {
    match last_check {
        Some(t) => now.saturating_sub(t) >= DAY_SECS,
        None => true,
    }
}

/// Blocking GitHub API call returning the latest release tag. Returns `None` on
/// any network/parse error — a failed check is silent, never fatal.
pub fn fetch_latest_tag() -> Option<String> {
    let mut response = ureq::get(RELEASES_LATEST)
        .header("User-Agent", "rdb-update-check")
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(8)))
        .build()
        .call()
        .ok()?;
    let body = response.body_mut().read_to_string().ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    json.get("tag_name")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_homebrew_and_scoop_from_path() {
        let brew = PathBuf::from("/opt/homebrew/Cellar/rdb/1.0.0/bin/rdb");
        assert_eq!(
            InstallMethod::from_exe_path(Some(&brew)),
            InstallMethod::Homebrew
        );
        let scoop = PathBuf::from(r"C:\Users\me\scoop\apps\rdb\current\rdb.exe");
        assert_eq!(
            InstallMethod::from_exe_path(Some(&scoop)),
            InstallMethod::Scoop
        );
        let other = PathBuf::from("/usr/local/bin/rdb");
        assert_eq!(
            InstallMethod::from_exe_path(Some(&other)),
            InstallMethod::Other
        );
        assert_eq!(InstallMethod::from_exe_path(None), InstallMethod::Other);
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("v1.2.0", "1.1.0"));
        assert!(is_newer("1.2.0", "1.1.9"));
        assert!(!is_newer("v1.0.0", "1.0.0")); // equal is not newer
        assert!(!is_newer("v0.9.0", "1.0.0")); // older
        assert!(!is_newer("garbage", "1.0.0")); // unparseable -> no nag
    }

    #[test]
    fn throttle_window() {
        assert!(due_for_check(None, 1_000_000));
        assert!(due_for_check(Some(0), DAY_SECS));
        assert!(!due_for_check(Some(1000), 1000 + DAY_SECS - 1));
        assert!(due_for_check(Some(1000), 1000 + DAY_SECS));
    }
}

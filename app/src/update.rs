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
            InstallMethod::Homebrew => "Run: brew upgrade rdbs",
            InstallMethod::Scoop => "Run: scoop update rdbs",
            InstallMethod::Other => "Open the download page",
        }
    }
}

/// Clicking the banner opens the release page for every install method —
/// self-updating a brew/scoop install from inside the app would fight the
/// package manager, and the banner text already shows the exact command.
/// ponytail: no self-update machinery until asked.
pub fn release_page() -> &'static str {
    RELEASES_PAGE
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
    let body = ureq::get(RELEASES_LATEST)
        .set("User-Agent", "rdbs-update-check")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    json.get("tag_name")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_homebrew_and_scoop_from_path() {
        let brew = PathBuf::from("/opt/homebrew/Cellar/rdbs/1.0.0/bin/rdbs");
        assert_eq!(
            InstallMethod::from_exe_path(Some(&brew)),
            InstallMethod::Homebrew
        );
        let scoop = PathBuf::from(r"C:\Users\me\scoop\apps\rdbs\current\rdbs.exe");
        assert_eq!(
            InstallMethod::from_exe_path(Some(&scoop)),
            InstallMethod::Scoop
        );
        let other = PathBuf::from("/usr/local/bin/rdbs");
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

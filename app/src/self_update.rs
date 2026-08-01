//! In-app "Restart to Update": download the matching release asset, swap it
//! into place, relaunch. Only ever reached for `InstallMethod::Other` installs
//! (see `update.rs`) — Homebrew/Scoop keep the existing hint-only behavior.
//!
//! Integrity note: release assets have no published checksum today, so the
//! only check here is "did we receive the number of bytes the server said we
//! would" (catches truncated downloads, not tampering). Publishing a
//! `checksums.txt` release asset would let this verify a real hash; until
//! then, don't claim more than size-matching actually proves.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

// Some variants are only ever constructed inside the macOS/Windows-specific
// swap code below — legitimately unconstructed (not dead) on Linux builds,
// which never reach a swap flow (`is_swappable` is always false there).
#[derive(Debug)]
#[allow(dead_code)]
pub enum SelfUpdateError {
    Network(String),
    NoMatchingAsset,
    SizeMismatch { expected: u64, actual: u64 },
    Io(std::io::Error),
    NotSwappable,
    ExternalTool(String),
    RelaunchFailed(String),
}

impl std::fmt::Display for SelfUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::NoMatchingAsset => write!(f, "no matching release asset for this platform"),
            Self::SizeMismatch { expected, actual } => {
                write!(f, "download incomplete: got {actual} of {expected} bytes")
            }
            Self::Io(e) => write!(f, "{e}"),
            Self::NotSwappable => write!(f, "this install can't be updated in place"),
            Self::ExternalTool(e) => write!(f, "{e}"),
            Self::RelaunchFailed(e) => write!(f, "update installed, but relaunch failed: {e}"),
        }
    }
}

impl From<std::io::Error> for SelfUpdateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

const RELEASES_LATEST: &str = "https://api.github.com/repos/suiflex/rdb/releases/latest";

/// `None` on any OS/arch this feature doesn't cover — the caller's signal to
/// fall back to the existing "open the release page" behavior.
fn target_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        _ => None,
    }
}

fn wanted_asset_name() -> Option<String> {
    let triple = target_triple()?;
    Some(match std::env::consts::OS {
        "macos" => format!("rdb-{triple}.dmg"),
        "windows" => format!("rdb-{triple}.exe"),
        _ => return None,
    })
}

/// Full release JSON (including assets), fetched only when the user actually
/// starts the update flow — the daily background check (`update::fetch_latest_tag`)
/// stays a separate, lighter call that only reads the tag name.
pub fn fetch_latest_assets() -> Result<Vec<ReleaseAsset>, SelfUpdateError> {
    let mut response = ureq::get(RELEASES_LATEST)
        .header("User-Agent", "rdb-self-update")
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .call()
        .map_err(|e| SelfUpdateError::Network(e.to_string()))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| SelfUpdateError::Network(e.to_string()))?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| SelfUpdateError::Network(e.to_string()))?;
    let assets = json
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or(SelfUpdateError::NoMatchingAsset)?;
    Ok(assets
        .iter()
        .filter_map(|a| {
            Some(ReleaseAsset {
                name: a.get("name")?.as_str()?.to_string(),
                browser_download_url: a.get("browser_download_url")?.as_str()?.to_string(),
            })
        })
        .collect())
}

pub fn pick_asset(assets: &[ReleaseAsset]) -> Option<ReleaseAsset> {
    let name = wanted_asset_name()?;
    assets.iter().find(|a| a.name == name).cloned()
}

fn work_dir() -> PathBuf {
    std::env::temp_dir().join("rdb-self-update")
}

/// Downloads `asset` into the self-update work dir, reporting 0.0..=1.0
/// progress as it goes, and returns the local path once fully verified.
/// Writes to a `.part` file first and only renames to the final name once
/// the byte count matches `Content-Length` — a leftover `.part` file always
/// means "don't trust this download." This is the "idle -> downloading"
/// step; swapping it into place is a separate, later step (`perform_swap`)
/// so the user gets an explicit second confirmation before anything is
/// actually replaced.
pub fn download_asset(
    asset: &ReleaseAsset,
    mut on_progress: impl FnMut(f32),
) -> Result<PathBuf, SelfUpdateError> {
    let work = work_dir();
    std::fs::create_dir_all(&work)?;
    let dest = work.join(&asset.name);

    let mut response = ureq::get(&asset.browser_download_url)
        .header("User-Agent", "rdb-self-update")
        .config()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .call()
        .map_err(|e| SelfUpdateError::Network(e.to_string()))?;
    let content_length = response.body_mut().content_length();
    let mut reader = response.into_body().into_reader();

    let tmp = dest.with_extension("part");
    let mut file = std::fs::File::create(&tmp)?;
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        total += n as u64;
        on_progress(
            content_length
                .map(|t| total as f32 / t as f32)
                .unwrap_or(0.0),
        );
    }
    file.sync_all()?;
    drop(file);

    if let Some(expected) = content_length {
        if total != expected {
            let _ = std::fs::remove_file(&tmp);
            return Err(SelfUpdateError::SizeMismatch {
                expected,
                actual: total,
            });
        }
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    /// `exe` = `.../RDB.app/Contents/MacOS/rdb` -> `.../RDB.app`. `None` for
    /// anything that isn't a real bundle install (dev build, `cargo run`) —
    /// the caller falls back to opening the release page instead of
    /// attempting a swap it can't reason about.
    pub fn bundle_root(exe: &Path) -> Option<PathBuf> {
        let macos_dir = exe.parent()?;
        if macos_dir.file_name()? != "MacOS" {
            return None;
        }
        let contents_dir = macos_dir.parent()?;
        if contents_dir.file_name()? != "Contents" {
            return None;
        }
        let bundle_root = contents_dir.parent()?;
        if bundle_root.extension()? != "app" {
            return None;
        }
        Some(bundle_root.to_path_buf())
    }

    fn find_dot_app(dir: &Path) -> Option<PathBuf> {
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "app"))
    }

    fn run_checked(cmd: &str, args: &[&str]) -> Result<(), SelfUpdateError> {
        let output = std::process::Command::new(cmd)
            .args(args)
            .output()
            .map_err(|e| SelfUpdateError::ExternalTool(format!("{cmd}: {e}")))?;
        if !output.status.success() {
            return Err(SelfUpdateError::ExternalTool(format!(
                "{cmd} {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    /// Mounts the already-downloaded `.dmg`, stages the `.app` next to the
    /// running bundle, swaps it in (never deleting the old one until the new
    /// one is confirmed in place, rolling back on any failure), clears
    /// quarantine, and relaunches. On success the caller quits the event
    /// loop right after.
    pub fn perform_update(dmg_path: &Path) -> Result<(), SelfUpdateError> {
        let exe = std::env::current_exe()?;
        let bundle_root = bundle_root(&exe).ok_or(SelfUpdateError::NotSwappable)?;
        let bundle_name = bundle_root
            .file_name()
            .ok_or(SelfUpdateError::NotSwappable)?
            .to_string_lossy()
            .to_string();

        let work = work_dir();
        let mnt = work.join("mnt");
        std::fs::create_dir_all(&mnt)?;
        let mnt_str = mnt.to_string_lossy().to_string();
        let dmg_str = dmg_path.to_string_lossy().to_string();
        run_checked(
            "hdiutil",
            &[
                "attach",
                "-nobrowse",
                "-readonly",
                "-mountpoint",
                &mnt_str,
                &dmg_str,
            ],
        )?;
        let mounted_app = find_dot_app(&mnt).ok_or(SelfUpdateError::NoMatchingAsset)?;

        let staged = bundle_root.with_file_name(format!("{bundle_name}.new"));
        let _ = std::fs::remove_dir_all(&staged);
        let staged_result = run_checked(
            "cp",
            &[
                "-R",
                &mounted_app.to_string_lossy(),
                &staged.to_string_lossy(),
            ],
        );
        let _ = run_checked("hdiutil", &["detach", &mnt_str]);
        staged_result?;
        let _ = run_checked(
            "xattr",
            &["-dr", "com.apple.quarantine", &staged.to_string_lossy()],
        );

        let backup = bundle_root.with_file_name(format!("{bundle_name}.old"));
        let _ = std::fs::remove_dir_all(&backup);
        std::fs::rename(&bundle_root, &backup)?;
        match std::fs::rename(&staged, &bundle_root) {
            Ok(()) => {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Err(e) => {
                // Old bundle wasn't touched beyond the rename-out above — put
                // it back so the app is never left in a broken state.
                let _ = std::fs::rename(&backup, &bundle_root);
                return Err(SelfUpdateError::Io(e));
            }
        }

        std::process::Command::new("open")
            .args(["-n", &bundle_root.to_string_lossy()])
            .spawn()
            .map_err(|e| SelfUpdateError::RelaunchFailed(e.to_string()))?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    const FINISH_SCRIPT: &str = r#"
param([int]$Pid, [string]$OldPath, [string]$NewPath)
try { Wait-Process -Id $Pid -Timeout 15 -ErrorAction SilentlyContinue } catch {}
$ok = $false
for ($i = 0; $i -lt 10; $i++) {
    try { Move-Item -Path $NewPath -Destination $OldPath -Force; $ok = $true; break }
    catch { Start-Sleep -Milliseconds 500 }
}
if ($ok) { Start-Process -FilePath $OldPath }
Remove-Item -Path $NewPath -ErrorAction SilentlyContinue
Remove-Item -Path $PSCommandPath -ErrorAction SilentlyContinue
"#;

    /// Spawns a detached helper script that waits for this process to exit
    /// (Windows can't overwrite a running exe), swaps the already-downloaded
    /// exe into place, and relaunches. Our process must quit right after this
    /// returns `Ok` — the swap only happens once we're actually gone.
    pub fn perform_update(new_exe: &Path) -> Result<(), SelfUpdateError> {
        let exe = std::env::current_exe()?;
        let work = work_dir();
        std::fs::create_dir_all(&work)?;

        let mut head = [0u8; 2];
        std::fs::File::open(new_exe)?.read_exact(&mut head)?;
        if &head != b"MZ" {
            let _ = std::fs::remove_file(new_exe);
            return Err(SelfUpdateError::NoMatchingAsset);
        }

        let script_path = work.join("finish-update.ps1");
        std::fs::write(&script_path, FINISH_SCRIPT)?;

        let pid = std::process::id();
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
            ])
            .arg(&script_path)
            .args([
                "-Pid",
                &pid.to_string(),
                "-OldPath",
                &exe.to_string_lossy(),
                "-NewPath",
                &new_exe.to_string_lossy(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| SelfUpdateError::RelaunchFailed(e.to_string()))?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use macos::bundle_root as macos_bundle_root;

/// Whether this specific install can be updated in place: `Other` install
/// method (checked by the caller) on a platform/layout we know how to swap.
pub fn is_swappable(_exe: Option<&Path>) -> bool {
    #[cfg(target_os = "macos")]
    {
        _exe.and_then(macos_bundle_root).is_some()
    }
    #[cfg(target_os = "windows")]
    {
        target_triple().is_some()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Swaps the already-downloaded asset (from `download_asset`) into place and
/// relaunches. On success the caller should quit the event loop right after
/// — the old process's job is done. Never reached in practice on a platform
/// where `is_swappable` is false, but stays a plain cross-platform function
/// (rather than `#[cfg]`-gated) so callers don't need their own per-platform
/// branching.
pub fn perform_swap(downloaded: &Path) -> Result<(), SelfUpdateError> {
    #[cfg(target_os = "macos")]
    {
        macos::perform_update(downloaded)
    }
    #[cfg(target_os = "windows")]
    {
        windows::perform_update(downloaded)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = downloaded;
        Err(SelfUpdateError::NotSwappable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_asset_never_partial_matches() {
        // A name that is a superstring of any real target's asset name (e.g.
        // a published checksum sidecar file) must never be picked in place
        // of the exact match — regardless of which platform runs this test.
        let bogus = vec![ReleaseAsset {
            name: "rdb-aarch64-apple-darwin.dmg.sha256".into(),
            browser_download_url: "https://example.com/c".into(),
        }];
        assert!(pick_asset(&bogus).is_none());
    }

    #[test]
    fn size_mismatch_display_is_readable() {
        let err = SelfUpdateError::SizeMismatch {
            expected: 100,
            actual: 40,
        };
        assert_eq!(err.to_string(), "download incomplete: got 40 of 100 bytes");
    }
}

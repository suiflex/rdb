//! File-export helpers: timestamped paths in ~/Downloads (temp-dir fallback).
//! No chrono/time dependency — the civil-date math is a dozen lines.

use std::path::PathBuf;

/// `~/Downloads/<prefix>-YYYYMMDD-HHMMSS.<ext>`; temp dir when $HOME is unset
/// or Downloads does not exist.
pub fn export_path(prefix: &str, ext: &str) -> PathBuf {
    let dir = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .filter(|d| d.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("{prefix}-{}.{ext}", timestamp()))
}

/// Current UTC time as "YYYYMMDD-HHMMSS".
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fmt_timestamp(secs)
}

fn fmt_timestamp(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let tod = secs % 86_400;
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_epoch_and_leap() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2024-02-29 is day 19782
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn timestamp_format() {
        assert_eq!(fmt_timestamp(0), "19700101-000000");
        assert_eq!(fmt_timestamp(86_399), "19700101-235959");
    }
}

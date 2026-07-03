//! Dev-only window capture: when `RDBS_SHOT=<path.bmp>` is set, snapshot the
//! window after `RDBS_SHOT_DELAY_MS` (default 1200) and quit. Used by the
//! design-parity screenshot loop; inert in normal runs.

use slint::ComponentHandle;

pub fn install<W: ComponentHandle + 'static>(window: &W) {
    let Ok(path) = std::env::var("RDBS_SHOT") else {
        return;
    };
    let delay: u64 = std::env::var("RDBS_SHOT_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1200);
    // Nudge a redraw shortly before capturing: property changes made from
    // background tasks may not repaint an idle window, and take_snapshot
    // would return the stale first frame.
    {
        let weak = window.as_weak();
        let redraw = Box::leak(Box::new(slint::Timer::default()));
        redraw.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(delay.saturating_sub(150)),
            move || {
                if let Some(w) = weak.upgrade() {
                    w.window().request_redraw();
                }
            },
        );
    }
    let weak = window.as_weak();
    let timer = Box::leak(Box::new(slint::Timer::default()));
    timer.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(delay),
        move || {
            if let Some(w) = weak.upgrade() {
                match w.window().take_snapshot() {
                    Ok(buf) => {
                        let (wd, ht) = (buf.width(), buf.height());
                        if let Err(e) = write_bmp(&path, wd, ht, buf.as_bytes()) {
                            eprintln!("RDBS_SHOT write failed: {e}");
                        } else {
                            eprintln!("RDBS_SHOT saved {path} ({wd}x{ht})");
                        }
                    }
                    Err(e) => eprintln!("RDBS_SHOT snapshot failed: {e}"),
                }
            }
            let _ = slint::quit_event_loop();
        },
    );
}

/// Minimal 24-bit BMP writer (no deps); input is RGBA8.
fn write_bmp(path: &str, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let row_len = (w * 3).div_ceil(4) * 4; // rows padded to 4 bytes
    let data_len = row_len * h;
    let mut out = Vec::with_capacity(54 + data_len as usize);
    // BITMAPFILEHEADER
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data_len).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]); // compression..colors, all zero
    for y in (0..h).rev() {
        let row_start = (y * w * 4) as usize;
        let mut written = 0;
        for x in 0..w {
            let p = row_start + (x * 4) as usize;
            out.extend_from_slice(&[rgba[p + 2], rgba[p + 1], rgba[p]]);
            written += 3;
        }
        while written % 4 != 0 {
            out.push(0);
            written += 1;
        }
    }
    std::fs::write(path, out)
}

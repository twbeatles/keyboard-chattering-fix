//! Diagnostic file log for the GUI build.
//!
//! The GUI sets `windows_subsystem = "windows"`, so there is no console and
//! `tracing` output would be lost. Slow-tick warnings and hook-restart
//! notices are appended here instead, so the next "not responding" report
//! comes with evidence. One rotating generation, size-capped; never called
//! from the hook hot path.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Cap of one generation; the previous one is kept as `.1`.
const MAX_BYTES: u64 = 256 * 1024;

fn log_path() -> PathBuf {
    std::env::temp_dir().join("kcfix-diag.log")
}

static WRITE_LOCK: Mutex<()> = Mutex::new(());

fn stamp_hh_mm_ss() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}

/// Append one line to the diagnostic log (size-capped, one rotation).
/// Best effort: logging must never fail the caller.
pub(crate) fn log_line(msg: &str) {
    let _lock = WRITE_LOCK.lock().unwrap();
    write_line(&log_path(), &format!("{} {msg}\n", stamp_hh_mm_ss()));
}

fn write_line(path: &Path, line: &str) {
    if path.metadata().map(|m| m.len()).unwrap_or(0) >= MAX_BYTES {
        let _ = std::fs::remove_file(path.with_extension("log.1"));
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(line.as_bytes())
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kcfix_diag_{}_{}_{tag}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lines_accumulate_in_order() {
        let dir = scratch_dir("order");
        let path = dir.join("d.log");
        write_line(&path, "first\n");
        write_line(&path, "second\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\nsecond\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversize_log_rotates_once() {
        let dir = scratch_dir("rotate");
        let path = dir.join("d.log");
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        write_line(&path, "fresh\n");
        let rotated = path.with_extension("log.1");
        assert!(rotated.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

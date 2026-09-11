//! Minimal file logger (F-CF-04, design §7/§9).
//!
//! Appends to `logs/ezStreamer-YYYY-MM-DD.log` under the config dir
//! (`%APPDATA%/ezStreamer` on Windows, `~/.config/ezStreamer` on Linux).
//! No logger framework: the event volume is low (bus errors, retries,
//! capture failures) and every call opens the file briefly, so there is no
//! global state to poison tests. Files rotate at 10MB (design §10) by
//! renaming to `<name>.old`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

pub fn log_dir() -> PathBuf {
    ezstreamer_core::config::config_dir().join("logs")
}

fn log_path_in(dir: &Path) -> PathBuf {
    let date = chrono::Local::now().format("%Y-%m-%d");
    dir.join(format!("ezStreamer-{date}.log"))
}

fn rotate_if_needed(path: &Path) {
    let too_big = fs::metadata(path)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if too_big {
        let old = path.with_extension("log.old");
        let _ = fs::remove_file(&old);
        let _ = fs::rename(path, old);
    }
}

fn append(dir: &Path, level: &str, msg: &str) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = log_path_in(dir);
    rotate_if_needed(&path);
    let ts = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%z");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{ts}] [{level}] {msg}");
    }
}

/// Append one line to today's log. Errors are swallowed: logging must never
/// take the stream down.
pub fn log(level: &str, msg: &str) {
    append(&log_dir(), level, msg);
}

pub fn info(msg: &str) {
    log("info", msg);
}

pub fn error(msg: &str) {
    log("error", msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ezstreamer-log-{tag}-{}", std::process::id()))
    }

    #[test]
    fn writes_dated_line_and_rotates() {
        let dir = temp_dir("write");
        let _ = fs::remove_dir_all(&dir);
        append(&dir, "info", "hello");
        let path = log_path_in(&dir);
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("[info] hello"), "body: {body}");

        // Oversized file rotates to .old, not truncated in place.
        let huge = vec![b'x'; (MAX_LOG_BYTES + 1) as usize];
        fs::write(&path, huge).unwrap();
        append(&dir, "error", "after-rotate");
        assert!(fs::metadata(&path).unwrap().len() <= MAX_LOG_BYTES);
        assert!(path.with_extension("log.old").is_file());
        assert!(fs::read_to_string(&path).unwrap().contains("after-rotate"));
        let _ = fs::remove_dir_all(&dir);
    }
}

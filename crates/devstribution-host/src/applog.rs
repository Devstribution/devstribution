//! The launcher's own log: what failed and when, in a file a user can find and send.
//!
//! One line per event: a start, a command that failed, a panic, a run that could not be
//! recorded. Never what a command was given (secrets, inputs, paths the user chose): only what
//! went wrong. It stops at about 512 KB and keeps one older file, `launcher.log.1`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::time::{date, now_secs};

const MAX_BYTES: u64 = 512 * 1024;
const MAX_LINE: usize = 2000;

static FILE: OnceLock<PathBuf> = OnceLock::new();
static WRITING: Mutex<()> = Mutex::new(());

/// Sets the file the launcher logs to. The first call wins.
pub fn init(path: PathBuf) {
    let _ = FILE.set(path);
}

/// Adds a line to the log, if `init` was called. A log that cannot be written is not an error
/// worth another one.
pub fn write(message: &str) {
    if let Some(path) = FILE.get() {
        let _guard = WRITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        append(path, message);
    }
}

fn append(path: &Path, message: &str) {
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let old = path.with_extension("log.1");
        let _ = fs::remove_file(&old);
        let _ = fs::rename(path, old);
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let secs = now_secs();
    let rem = secs % 86_400;
    let text: String = message
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
        .chars()
        .take(MAX_LINE)
        .collect();
    let line = format!(
        "{} {:02}:{:02}:{:02}Z {text}\n",
        date(secs),
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    );
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// The last `lines` lines of the log, oldest first; empty when there is no log.
pub fn tail(path: &Path, lines: usize) -> Vec<String> {
    let text = fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|l| (*l).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_one_per_event_and_the_tail_is_the_latest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("launcher.log");
        assert!(tail(&path, 5).is_empty());
        append(&path, "first");
        append(&path, "failed:\n  could not read\n\n  the file");
        append(&path, "third");
        let last = tail(&path, 2);
        assert_eq!(last.len(), 2);
        assert!(
            last[0].ends_with("failed: | could not read | the file"),
            "{last:?}"
        );
        assert!(last[1].ends_with("Z third"));
    }

    #[test]
    fn a_big_log_moves_aside_and_a_new_one_starts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("launcher.log");
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&path, "after");
        assert_eq!(tail(&path, 10).len(), 1);
        assert!(dir.path().join("launcher.log.1").is_file());
    }
}

//! The journal: an append-only file that says what the launcher did to the run history.
//!
//! Every finished run adds a line with the SHA-256 of its `run.json`, and every cleanup adds a
//! line with what it removed. Each line holds the hash of the line before it, so a line edited
//! or removed in the middle breaks the chain. When results are gone, the journal tells apart
//! what the launcher did on request (a cleanup line names them) from what it did not.
//!
//! It lives in the data folder, which anything the user runs can write: it makes accidents and
//! edits visible, it is not proof against a program set on hiding them. Cutting lines off the
//! end cannot be seen without a copy kept elsewhere.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::time::now_secs;

/// Appends are one at a time within the process: runs finish on their own threads.
static APPEND: Mutex<()> = Mutex::new(());

/// The hash a first line chains to.
const START: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    pub at: u64,
    /// `run` (a run finished) or `cleanup`.
    pub kind: String,
    pub data: Value,
    /// Hash of the previous line.
    pub prev: String,
    /// Hash of this line's other fields.
    pub hash: String,
}

/// What a line's hash covers.
#[derive(Serialize)]
struct Body<'a> {
    seq: u64,
    at: u64,
    kind: &'a str,
    data: &'a Value,
    prev: &'a str,
}

fn hash_of(body: &Body) -> String {
    let json = serde_json::to_vec(body).expect("a journal line serializes");
    hex::encode(Sha256::digest(json))
}

/// The SHA-256 of a file, hex, or `None` if it cannot be read.
pub fn file_sha256(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|data| hex::encode(Sha256::digest(data)))
}

/// What reading the journal found.
#[derive(Debug, Clone, Serialize)]
pub struct Reading {
    pub entries: Vec<Entry>,
    /// The first line that does not follow from the one before it, or cannot be read.
    pub broken_at: Option<u64>,
}

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(path: PathBuf) -> Journal {
        Journal { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every line, up to the first one that breaks the chain.
    pub fn read(&self) -> Reading {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return Reading {
                entries: Vec::new(),
                broken_at: None,
            };
        };
        let mut entries: Vec<Entry> = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let expected_prev = entries.last().map_or(START, |e| e.hash.as_str());
            let ok = serde_json::from_str::<Entry>(line).ok().filter(|e| {
                e.seq == i as u64
                    && e.prev == expected_prev
                    && e.hash
                        == hash_of(&Body {
                            seq: e.seq,
                            at: e.at,
                            kind: &e.kind,
                            data: &e.data,
                            prev: &e.prev,
                        })
            });
            match ok {
                Some(e) => entries.push(e),
                None => {
                    return Reading {
                        entries,
                        broken_at: Some(i as u64),
                    };
                }
            }
        }
        Reading {
            entries,
            broken_at: None,
        }
    }

    /// Adds a line after the last one. Refuses when the chain is already broken: new lines
    /// would only hide where it broke.
    pub fn append(&self, kind: &str, data: Value) -> Result<Entry> {
        let _guard = APPEND
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reading = self.read();
        if let Some(at) = reading.broken_at {
            anyhow::bail!("the journal is broken at line {}", at + 1);
        }
        let (seq, prev) = reading
            .entries
            .last()
            .map_or((0, START.to_string()), |e| (e.seq + 1, e.hash.clone()));
        let at = now_secs();
        let hash = hash_of(&Body {
            seq,
            at,
            kind,
            data: &data,
            prev: &prev,
        });
        let entry = Entry {
            seq,
            at,
            kind: kind.to_string(),
            data,
            prev,
            hash,
        };
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn journal() -> (tempfile::TempDir, Journal) {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::new(dir.path().join("journal.jsonl"));
        (dir, j)
    }

    #[test]
    fn lines_chain_and_read_back() {
        let (_dir, j) = journal();
        assert!(j.read().entries.is_empty());
        let a = j.append("run", json!({"run_id": "a"})).unwrap();
        let b = j.append("cleanup", json!({"runs": 2})).unwrap();
        assert_eq!((a.seq, b.seq), (0, 1));
        assert_eq!(b.prev, a.hash);
        let reading = j.read();
        assert_eq!(reading.entries.len(), 2);
        assert_eq!(reading.broken_at, None);
    }

    #[test]
    fn an_edited_or_removed_line_breaks_the_chain() {
        let (_dir, j) = journal();
        for i in 0..3 {
            j.append("run", json!({ "run_id": i })).unwrap();
        }
        let original = fs::read_to_string(j.path()).unwrap();

        fs::write(
            j.path(),
            original.replacen("\"run_id\":1", "\"run_id\":9", 1),
        )
        .unwrap();
        assert_eq!(j.read().broken_at, Some(1));
        assert!(j.append("run", json!({})).is_err());

        let without_middle: Vec<&str> = original
            .lines()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| l)
            .collect();
        fs::write(j.path(), without_middle.join("\n") + "\n").unwrap();
        assert_eq!(j.read().broken_at, Some(1));
    }
}

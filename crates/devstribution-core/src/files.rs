//! `FILES.sha256`: the list of every file in a package with its SHA-256.
//!
//! Canonical form: UTF-8, LF line endings, one line `<sha256 hex>  <path>` per file, paths with
//! `/`, sorted by bytes, ending with a newline. Anything else is refused, so that a given set of
//! files has exactly one valid list and therefore one signature.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::paths::validate_relative_path;
use crate::{Error, FILES_LIST, FILES_SIGNATURE, Result};

/// File path → SHA-256. `BTreeMap<String, _>` iterates in byte order, as the format requires.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilesList {
    entries: BTreeMap<String, [u8; 32]>,
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

impl FilesList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &str, data: &[u8]) -> Result<()> {
        validate_relative_path(path).map_err(Error::Package)?;
        if is_reserved(path) {
            return Err(Error::Package(format!("`{path}` is a reserved file name")));
        }
        if self.entries.insert(path.to_owned(), sha256(data)).is_some() {
            return Err(Error::Package(format!("`{path}` appears twice")));
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, path: &str) -> Option<&[u8; 32]> {
        self.entries.get(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn to_canonical_string(&self) -> String {
        let mut out = String::new();
        for (path, hash) in &self.entries {
            out.push_str(&hex::encode(hash));
            out.push_str("  ");
            out.push_str(path);
            out.push('\n');
        }
        out
    }

    /// Parses a list and refuses it unless it is byte-for-byte canonical.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let bad = |msg: String| Error::Package(format!("{FILES_LIST}: {msg}"));
        let text = std::str::from_utf8(data).map_err(|_| bad("not valid UTF-8".into()))?;
        let mut list = FilesList::new();
        for (n, line) in text.split_terminator('\n').enumerate() {
            let (hash, path) = line
                .split_once("  ")
                .ok_or_else(|| bad(format!("line {} is not `<sha256>  <path>`", n + 1)))?;
            if hash.len() != 64 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return Err(bad(format!(
                    "line {}: hash is not 64 lowercase hex digits",
                    n + 1
                )));
            }
            let mut digest = [0u8; 32];
            hex::decode_to_slice(hash, &mut digest).expect("checked above");
            validate_relative_path(path).map_err(bad)?;
            if is_reserved(path) {
                return Err(bad(format!("lists the reserved file `{path}`")));
            }
            list.entries.insert(path.to_owned(), digest);
        }
        if list.to_canonical_string().as_bytes() != data {
            return Err(bad(
                "not in canonical form (sorted, unique, LF line endings, final newline)".into(),
            ));
        }
        Ok(list)
    }
}

fn is_reserved(path: &str) -> bool {
    path == FILES_LIST || path == FILES_SIGNATURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_is_sorted_by_bytes() {
        let mut list = FilesList::new();
        list.insert("main.py", b"print(1)").unwrap();
        list.insert("Zeta.txt", b"").unwrap();
        list.insert("a/b.py", b"x").unwrap();
        let text = list.to_canonical_string();
        let paths: Vec<_> = text
            .lines()
            .map(|l| l.split_once("  ").unwrap().1)
            .collect();
        assert_eq!(paths, ["Zeta.txt", "a/b.py", "main.py"]);
        assert_eq!(FilesList::parse(text.as_bytes()).unwrap(), list);
    }

    #[test]
    fn refuses_non_canonical_lists() {
        let mut list = FilesList::new();
        list.insert("b.py", b"b").unwrap();
        list.insert("a.py", b"a").unwrap();
        let text = list.to_canonical_string();
        let lines: Vec<_> = text.lines().collect();
        let unsorted = format!("{}\n{}\n", lines[1], lines[0]);
        let crlf = text.replace('\n', "\r\n");
        let no_final_newline = text.trim_end().to_owned();
        let duplicated = format!("{text}{}\n", lines[1]);
        let upper = text.to_uppercase();
        for bad in [unsorted, crlf, no_final_newline, duplicated, upper] {
            assert!(FilesList::parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_reserved_and_duplicate_names() {
        let mut list = FilesList::new();
        assert!(list.insert(FILES_LIST, b"").is_err());
        list.insert("a.py", b"").unwrap();
        assert!(list.insert("a.py", b"").is_err());
    }
}

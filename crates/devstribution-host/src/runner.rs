//! The Python runner, embedded in the binary and written to the data folder.
//!
//! The data folder can be written by any program the user runs, apps included, and the runner
//! folder comes first on every app's import path. So before each run the folder is compared
//! with the embedded copy, file by file, and rewritten if anything differs or was added.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

const FILES: &[(&str, &str)] = &[
    ("bootstrap.py", include_str!("../../../runner/bootstrap.py")),
    (
        "devstribution_runner/__init__.py",
        include_str!("../../../runner/devstribution_runner/__init__.py"),
    ),
    (
        "devstribution_runner/_com.py",
        include_str!("../../../runner/devstribution_runner/_com.py"),
    ),
    (
        "devstribution_runner/_context.py",
        include_str!("../../../runner/devstribution_runner/_context.py"),
    ),
    (
        "devstribution_runner/_runner.py",
        include_str!("../../../runner/devstribution_runner/_runner.py"),
    ),
    (
        "devstribution_tools/check_app.py",
        include_str!("../../../runner/devstribution_tools/check_app.py"),
    ),
];

/// Folder holding this build's runner, named after its content so versions never mix,
/// checked (and rewritten if needed) on every call.
pub fn install(paths: &crate::paths::Paths) -> Result<PathBuf> {
    let mut hasher = Sha256::new();
    for (name, content) in FILES {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(content.as_bytes());
    }
    let hash = hex::encode(&hasher.finalize()[..6]);
    let dir = paths.root.join("runner").join(hash);
    if is_intact(&dir) {
        return Ok(dir);
    }
    let tmp = paths
        .root
        .join("runner")
        .join(format!("{}.tmp", crate::time::random_hex(6)));
    for (name, content) in FILES {
        let path = tmp.join(name);
        fs::create_dir_all(path.parent().expect("has a parent"))?;
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    }
    let _ = fs::remove_dir_all(&dir);
    if let Err(e) = fs::rename(&tmp, &dir) {
        let _ = fs::remove_dir_all(&tmp);
        // Another launcher may have written it at the same moment.
        if !is_intact(&dir) {
            return Err(e).context("installing the runner");
        }
    }
    Ok(dir)
}

/// Whether `dir` holds exactly the embedded runner files with their exact content.
fn is_intact(dir: &Path) -> bool {
    let mut found = Vec::new();
    if !list_files(dir, dir, &mut found) || found.len() != FILES.len() {
        return false;
    }
    FILES.iter().all(|(name, content)| {
        found.iter().any(|f| f == name)
            && fs::read(dir.join(name)).is_ok_and(|d| d == content.as_bytes())
    })
}

fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for e in entries.flatten() {
        let Ok(kind) = e.file_type() else {
            return false;
        };
        if kind.is_dir() {
            if !list_files(root, &e.path(), out) {
                return false;
            }
        } else if kind.is_file() {
            let rel = e.path().strip_prefix(root).map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            });
            match rel {
                Ok(rel) => out.push(rel),
                Err(_) => return false,
            }
        } else {
            return false;
        }
    }
    true
}

//! The Python runner, embedded in the binary and written to the data folder on first use.
//! The launcher ships the same files in its install folder.

use std::fs;
use std::path::PathBuf;

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

/// Folder holding this build's runner, named after its content so versions never mix.
pub fn install() -> Result<PathBuf> {
    let mut hasher = Sha256::new();
    for (name, content) in FILES {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(content.as_bytes());
    }
    let hash = hex::encode(&hasher.finalize()[..6]);
    let dir = crate::env::data_dir()?.join("runner").join(hash);
    if !dir.join("bootstrap.py").is_file() {
        let tmp = dir.with_extension("tmp");
        let _ = fs::remove_dir_all(&tmp);
        for (name, content) in FILES {
            let path = tmp.join(name);
            fs::create_dir_all(path.parent().expect("has a parent"))?;
            fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        }
        if let Err(e) = fs::rename(&tmp, &dir) {
            // Another process may have installed it at the same time.
            if !dir.join("bootstrap.py").is_file() {
                return Err(e).context("installing the runner");
            }
            let _ = fs::remove_dir_all(&tmp);
        }
    }
    Ok(dir)
}

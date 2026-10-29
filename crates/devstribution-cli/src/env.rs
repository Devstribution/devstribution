//! Folders and external tools used by the CLI.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use devstribution_host::Paths;

pub fn paths() -> Result<Paths> {
    Paths::from_env()
}

/// Publisher keys. `DEVSTRIBUTION_HOME` overrides it so tests and CI never touch the profile.
pub fn keys_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("DEVSTRIBUTION_HOME") {
        return Ok(PathBuf::from(home).join("keys"));
    }
    Ok(dirs::config_dir()
        .context("cannot find the configuration folder")?
        .join("devstribution")
        .join("keys"))
}

/// Runs `uv` and fails if it fails (its output goes to the terminal).
pub fn run_uv(args: &[&str], dir: &Path) -> Result<()> {
    if !uv_succeeds(args, dir)? {
        bail!("`uv {}` failed", args.join(" "));
    }
    Ok(())
}

/// Runs `uv` and says whether it succeeded; an error only when uv cannot be started, so the
/// caller does not blame its own check for a missing uv.
pub fn uv_succeeds(args: &[&str], dir: &Path) -> Result<bool> {
    let status = devstribution_host::uv::command()
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| {
            anyhow::anyhow!(
                "cannot start uv ({e}): install it from https://docs.astral.sh/uv/ or set DEVSTRIBUTION_UV"
            )
        })?;
    Ok(status.success())
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path =
        std::path::absolute(path).with_context(|| format!("invalid path {}", path.display()))?;
    Ok(dunce_canonical(&path).unwrap_or(path))
}

/// `canonicalize` without the `\\?\` prefix on Windows, which Python and uv handle poorly.
fn dunce_canonical(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    #[cfg(windows)]
    {
        let s = canonical.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\")
            && !stripped.starts_with("UNC\\")
        {
            return Some(PathBuf::from(stripped));
        }
    }
    Some(canonical)
}

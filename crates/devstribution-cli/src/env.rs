//! Folders and external tools used by the CLI.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Root for runs, development environments and the runner. `DEVSTRIBUTION_HOME` overrides it
/// (and the keys folder) so tests and CI never touch the real profile.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("DEVSTRIBUTION_HOME") {
        return Ok(PathBuf::from(home));
    }
    Ok(dirs::data_local_dir()
        .context("cannot find the local data folder")?
        .join("devstribution"))
}

pub fn keys_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("DEVSTRIBUTION_HOME") {
        return Ok(PathBuf::from(home).join("keys"));
    }
    Ok(dirs::config_dir()
        .context("cannot find the configuration folder")?
        .join("devstribution")
        .join("keys"))
}

pub fn uv() -> OsString {
    std::env::var_os("DEVSTRIBUTION_UV").unwrap_or_else(|| "uv".into())
}

pub fn uv_command() -> Command {
    Command::new(uv())
}

/// Runs `uv` and fails with its output if it fails.
pub fn run_uv(args: &[&str], dir: &Path, extra_env: &[(&str, &Path)]) -> Result<()> {
    let mut cmd = uv_command();
    cmd.args(args).current_dir(dir);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let status = cmd.status().map_err(|e| {
        anyhow::anyhow!("cannot start uv ({e}); install it from https://docs.astral.sh/uv/ or set DEVSTRIBUTION_UV")
    })?;
    if !status.success() {
        bail!("`uv {}` failed", args.join(" "));
    }
    Ok(())
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
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            if !stripped.starts_with("UNC\\") {
                return Some(PathBuf::from(stripped));
            }
        }
    }
    Some(canonical)
}

/// `YYYYMMDD-HHMMSS` in UTC, for run folder names.
pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil-from-days (Howard Hinnant), valid for all dates after 1970.
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_shape() {
        let t = super::timestamp();
        assert_eq!(t.len(), 15);
        assert!(t.starts_with("20"));
    }
}

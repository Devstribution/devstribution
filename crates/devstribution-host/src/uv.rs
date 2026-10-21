//! Preparing app environments with `uv`: wheels only, from the lockfile, outside the app folder.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// `DEVSTRIBUTION_UV`, else `uv` next to the running executable (the launcher's sidecar), else
/// `uv` from PATH.
pub fn uv_path() -> OsString {
    if let Some(p) = std::env::var_os("DEVSTRIBUTION_UV") {
        return p;
    }
    let exe = if cfg!(windows) { "uv.exe" } else { "uv" };
    if let Some(sidecar) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(exe)))
        .filter(|p| p.is_file())
    {
        return sidecar.into_os_string();
    }
    "uv".into()
}

pub fn command() -> Command {
    let mut cmd = Command::new(uv_path());
    hide_window(&mut cmd);
    cmd
}

#[cfg(windows)]
pub(crate) fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn hide_window(_cmd: &mut Command) {}

pub fn version() -> Option<String> {
    let out = command().arg("--version").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// Installed apps: use the lockfile exactly as signed.
    Frozen,
    /// Development: fail if the lockfile no longer matches pyproject.toml.
    Locked,
}

/// `uv sync` the app's locked dependencies into `env_dir`, reporting uv's progress lines.
pub fn sync(
    app_dir: &Path,
    env_dir: &Path,
    mode: LockMode,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let lock_flag = match mode {
        LockMode::Frozen => "--frozen",
        LockMode::Locked => "--locked",
    };
    let mut child = command()
        .args([
            "sync",
            lock_flag,
            "--no-build",
            "--no-dev",
            "--no-install-project",
        ])
        .args(["--color", "never", "--no-progress"])
        .current_dir(app_dir)
        .env("UV_PROJECT_ENVIRONMENT", env_dir)
        // Always uv's own Python builds, never whatever Python (conda, system) is on PATH:
        // the same app then gets the same interpreter on every machine.
        .env("UV_PYTHON_PREFERENCE", "only-managed")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context(
            "cannot start uv; install it from https://docs.astral.sh/uv/ or set DEVSTRIBUTION_UV",
        )?;
    let mut tail: Vec<String> = Vec::new();
    for line in BufReader::new(child.stderr.take().expect("piped")).lines() {
        let line = line.unwrap_or_default();
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        on_line(line);
        tail.push(line.to_string());
        if tail.len() > 12 {
            tail.remove(0);
        }
    }
    let status = child.wait()?;
    if !status.success() {
        bail!(
            "preparing the Python environment failed:\n{}",
            tail.join("\n")
        );
    }
    Ok(())
}

/// The environment's interpreter. GUI apps on Windows use `pythonw.exe` (no console window).
pub fn python_in(env_dir: &Path, gui: bool) -> PathBuf {
    if cfg!(windows) {
        let exe = if gui { "pythonw.exe" } else { "python.exe" };
        env_dir.join("Scripts").join(exe)
    } else {
        env_dir.join("bin").join("python")
    }
}

//! Preparing app environments with `uv`: wheels only, from the lockfile, outside the app folder.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use devstribution_core::manifest::Manifest;

static FIXED_UV: OnceLock<PathBuf> = OnceLock::new();

/// Makes every later call use exactly this uv. The released launcher calls it with the uv it
/// ships (pinned and hash-checked), so neither `DEVSTRIBUTION_UV` nor PATH can swap it.
pub fn use_only(uv: PathBuf) {
    let _ = FIXED_UV.set(uv);
}

/// The uv fixed by [`use_only`]; otherwise `DEVSTRIBUTION_UV`, else `uv` next to the running
/// executable (the launcher's sidecar), else `uv` from PATH (the developer CLI).
pub fn uv_path() -> OsString {
    if let Some(p) = FIXED_UV.get() {
        return p.clone().into_os_string();
    }
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
    let mut cmd = command();
    cmd.args(["sync", "--no-build", "--no-dev", "--no-install-project"])
        .args(["--color", "never", "--no-progress"])
        .current_dir(app_dir);
    match mode {
        LockMode::Frozen => installed_app_settings(&mut cmd, app_dir)?,
        // `devstribution dev` on the developer's own machine keeps their uv settings.
        LockMode::Locked => {
            cmd.arg("--locked")
                .env("UV_PYTHON_PREFERENCE", "only-managed");
        }
    }
    // After the settings above, which clear the environment.
    cmd.env("UV_PROJECT_ENVIRONMENT", env_dir);
    let mut child = cmd
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

/// Environment variables uv may still see when it prepares an installed app: what Windows and
/// the network need (proxies, certificates), nothing that configures uv or Python.
const KEPT_ENV: &[&str] = &[
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PATH",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "USERNAME",
    "USERDOMAIN",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "HOMEDRIVE",
    "HOMEPATH",
    "HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "LANG",
    "LC_ALL",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// What makes preparing an installed app depend only on its signed lockfile:
/// - `--frozen`: the lockfile as signed, never re-resolved;
/// - `--no-config`: no `uv.toml` and no `[tool.uv]` settings from the package (they could move
///   uv's cache, point it at other Python downloads or indexes) nor from the user's folders;
/// - a clean environment: no `UV_*`, `PIP_*` or `PYTHON*` variable from the user's session;
/// - `--managed-python` with an explicit version request: always uv's own Python builds, never
///   a `.python-version` from the package or whatever Python is on PATH (conda, system);
/// - `--system-certs`: the system's certificate store, so company proxies that inspect HTTPS
///   keep working.
///
/// The lockfile's sources were checked before (`lock::check_sources`): only wheels from PyPI.
fn installed_app_settings(cmd: &mut Command, app_dir: &Path) -> Result<()> {
    let pyproject = std::fs::read_to_string(app_dir.join("pyproject.toml"))
        .context("reading the app's pyproject.toml")?;
    let manifest = Manifest::parse(&pyproject)?;
    let python = manifest.requires_python.as_deref().unwrap_or(">=3.11");
    cmd.args([
        "--frozen",
        "--no-config",
        "--managed-python",
        "--system-certs",
    ])
    .args(["--python", python])
    .env_clear();
    for (k, v) in std::env::vars_os() {
        if k.to_str()
            .is_some_and(|k| KEPT_ENV.iter().any(|kept| kept.eq_ignore_ascii_case(k)))
        {
            cmd.env(k, v);
        }
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

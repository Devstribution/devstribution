//! Preparing app environments with `uv`: wheels only, from the lockfile, outside the app folder.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use devstribution_core::manifest::Manifest;

use crate::run::platform;

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

/// How long uv may take to prepare an environment before it is stopped. Large dependencies
/// (PyTorch) take minutes on a slow line; a proxy that holds the connection open forever must
/// not leave the install screen waiting forever.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Preparing the environment was stopped because the user asked to.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("preparing the Python environment was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// `uv sync` the app's locked dependencies into `env_dir`, reporting uv's progress lines.
/// `stop` is asked a few times a second; when it says yes, or after [`TIMEOUT`], uv and
/// everything it started are ended.
pub fn sync(
    app_dir: &Path,
    env_dir: &Path,
    mode: LockMode,
    on_line: &mut dyn FnMut(&str),
    stop: &dyn Fn() -> bool,
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
    supervise(cmd, on_line, stop, TIMEOUT)
}

/// Runs uv in a Job Object, like an app's run, so that Cancel, the timeout and closing the
/// launcher end it together with every process it started (Python downloads, extractors).
fn supervise(
    mut cmd: Command,
    on_line: &mut dyn FnMut(&str),
    stop: &dyn Fn() -> bool,
    timeout: Duration,
) -> Result<()> {
    platform::prepare(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context(
            "cannot start uv, the tool that sets up an app's Python: reinstall devstribution, or ask your IT department whether it is blocked (developers can set DEVSTRIBUTION_UV)",
        )?;
    let job = match platform::Job::assign(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    // Lines arrive on a thread of their own, so that a uv that prints nothing can still be
    // stopped.
    let stderr = child.stderr.take().expect("piped");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            if tx.send(line.unwrap_or_default()).is_err() {
                break;
            }
        }
    });
    let started = Instant::now();
    let mut tail: Vec<String> = Vec::new();
    let stopped = loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                let line = line.trim_end();
                if !line.is_empty() {
                    on_line(line);
                    tail.push(line.to_string());
                    if tail.len() > 12 {
                        tail.remove(0);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break None,
        }
        if stop() {
            break Some(anyhow::Error::new(Cancelled));
        }
        if started.elapsed() > timeout {
            let minutes = timeout.as_secs().div_ceil(60).max(1);
            break Some(anyhow!(
                "preparing the Python environment took more than {minutes} minute{} and was \
                 stopped. This is usually a slow connection, or a proxy that holds the download: \
                 check it and try again.",
                if minutes == 1 { "" } else { "s" }
            ));
        }
    };
    if let Some(e) = stopped {
        job.terminate();
        let _ = child.kill();
        let _ = child.wait();
        return Err(e);
    }
    let status = child.wait()?;
    if !status.success() {
        bail!(
            "preparing the Python environment failed. This is usually the internet connection or a proxy blocking the download: check it and try again.
{}",
            tail.join("
")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A command that prints a line, then waits about a minute without printing anything.
    fn slow() -> Command {
        if cfg!(windows) {
            let mut cmd = Command::new("cmd");
            cmd.args(["/c", "echo started 1>&2 && ping -n 60 127.0.0.1 >nul"]);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "echo started >&2; sleep 60"]);
            cmd
        }
    }

    #[test]
    fn cancel_ends_uv_and_what_it_started() {
        let cancel = Arc::new(AtomicBool::new(false));
        let later = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            later.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let mut lines = Vec::new();
        let err = supervise(
            slow(),
            &mut |l| lines.push(l.to_string()),
            &|| cancel.load(Ordering::SeqCst),
            TIMEOUT,
        )
        .unwrap_err();
        assert!(err.is::<Cancelled>(), "{err:#}");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(lines, ["started"]);
    }

    #[test]
    fn a_silent_uv_is_stopped_at_the_timeout() {
        let started = Instant::now();
        let err = supervise(slow(), &mut |_| {}, &|| false, Duration::from_millis(500))
            .unwrap_err()
            .to_string();
        assert!(err.contains("took more than 1 minute and was"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}

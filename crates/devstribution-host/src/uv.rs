//! Preparing app environments with `uv`: wheels only, from the lockfile, outside the app folder.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
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
pub fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_window(_cmd: &mut Command) {}

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

/// Where an installed app's downloads come from, when IT names somewhere other than the
/// defaults (managed mode's policy).
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// Replaces `https://github.com/astral-sh/python-build-standalone/releases/download`, where
    /// uv downloads Python itself. uv checks every Python download against the hash built into
    /// it, so a mirror can make a download fail but cannot change what is installed.
    pub python_mirror: Option<String>,
}

/// `uv sync` the app's locked dependencies into `env_dir`, reporting uv's progress lines.
/// `stop` is asked a few times a second; when it says yes, or after [`TIMEOUT`], uv and
/// everything it started are ended.
pub fn sync(
    app_dir: &Path,
    env_dir: &Path,
    mode: LockMode,
    sources: &Sources,
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
    if let Some(mirror) = &sources.python_mirror {
        cmd.env("UV_PYTHON_INSTALL_MIRROR", mirror);
    }
    supervise(cmd, on_line, stop, TIMEOUT).map_err(|e| match e.downcast::<Failed>() {
        Ok(failed) => anyhow!(failed.explain(sources, &proxy_in_use())),
        Err(e) => e,
    })
}

/// uv ended with an error; its last lines.
#[derive(Debug)]
struct Failed(Vec<String>);

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "preparing the Python environment failed. This is usually the internet connection or \
             a proxy blocking the download: check it and try again.\n{}",
            self.0.join("\n")
        )
    }
}

impl std::error::Error for Failed {}

/// What went wrong with a download, as far as uv's error lines tell. Windows translates its
/// own error texts, so the codes (`os error 11001`) are matched, not the words.
#[derive(Debug, PartialEq, Eq)]
enum Cause {
    /// The name of the host could not be found.
    Dns,
    /// The proxy wants a login (NTLM, Kerberos), which uv cannot give.
    ProxyLogin,
    /// The proxy could not be reached, or would not open the connection.
    Proxy,
    /// The certificate the connection presented is not trusted.
    Certificate,
    /// The host or a firewall refused (403), or the connection was refused or timed out.
    Blocked,
    /// The host does not have the file (404).
    Missing,
}

impl Failed {
    fn cause(&self) -> Option<Cause> {
        let text = self.0.join("\n").to_ascii_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
        Some(
            if has(&[
                "proxy authentication required",
                "proxy authorization required",
                " 407",
            ]) {
                Cause::ProxyLogin
            } else if has(&[
                "certificate",
                "unknownissuer",
                "unknown issuer",
                "os error -2146762487",
            ]) {
                Cause::Certificate
            } else if has(&["tunnel error", "proxy"]) {
                Cause::Proxy
            } else if has(&[
                "dns error",
                "os error 11001",
                "os error 11004",
                "failed to lookup address",
            ]) {
                Cause::Dns
            } else if has(&[
                "403 forbidden",
                "os error 10061",
                "os error 10060",
                "os error 10054",
                "timed out",
            ]) {
                Cause::Blocked
            } else if has(&["404 not found"]) {
                Cause::Missing
            } else {
                return None;
            },
        )
    }

    /// The host of the download that failed: the URL in uv's last lines.
    fn host(&self) -> Option<String> {
        self.0.iter().rev().find_map(|line| {
            let start = line.find("https://").or_else(|| line.find("http://"))?;
            let rest = &line[start..];
            let rest = &rest[rest.find("//")? + 2..];
            let end = rest.find(['/', ')', '`', ' ', ':']).unwrap_or(rest.len());
            let host = &rest[..end];
            let host = host.rsplit('@').next().unwrap_or(host);
            (!host.is_empty()).then(|| host.to_ascii_lowercase())
        })
    }

    /// The message the user sees: what could not be downloaded, why, and what to ask IT for,
    /// then uv's own lines for IT.
    fn explain(&self, sources: &Sources, proxy: &str) -> String {
        let (Some(cause), Some(host)) = (self.cause(), self.host()) else {
            return self.to_string();
        };
        let mirror_host = sources
            .python_mirror
            .as_deref()
            .and_then(|m| Failed(vec![m.to_string()]).host());
        let what = if mirror_host.as_deref() == Some(host.as_str()) {
            "the Python mirror your organisation set"
        } else if host == "github.com" || host.ends_with(".githubusercontent.com") {
            "Python itself"
        } else if host == "pypi.org" || host == "files.pythonhosted.org" {
            "the app's packages"
        } else {
            "the app's Python or packages"
        };
        let why = match cause {
            Cause::Dns => format!(
                "{host} could not be found: the computer may be offline, or this network only \
                 reaches the internet through a proxy that is not set"
            ),
            Cause::ProxyLogin => format!(
                "the proxy asks for a Windows login, which devstribution cannot give. Ask IT to \
                 let {host} through the proxy without one"
            ),
            Cause::Proxy => format!("the proxy ({proxy}) did not open the connection to {host}"),
            Cause::Certificate => format!(
                "the connection to {host} presented a certificate Windows does not trust. A \
                 company proxy that inspects HTTPS needs its certificate installed in Windows"
            ),
            Cause::Blocked => format!("{host} or a firewall refused the connection"),
            Cause::Missing => format!("{host} does not have the file: a mirror may be incomplete"),
        };
        let ask = match (cause, what) {
            (Cause::Missing, _) | (_, "the Python mirror your organisation set") => {
                format!("Ask your IT department to check the mirror at {host}.")
            }
            (_, "Python itself") => format!(
                "Ask your IT department to allow downloads from {host}, or to set a Python \
                 mirror (the PythonMirror policy)."
            ),
            _ => format!("Ask your IT department to allow downloads from {host}."),
        };
        format!(
            "Could not download {what} from {host}: {why}. {ask}\nProxy: {proxy}.\n{}",
            self.0.join("\n")
        )
    }
}

/// The proxy uv uses, as the user and IT may see it: credentials in the address are hidden.
pub fn proxy_in_use() -> String {
    let proxy = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()));
    match proxy {
        Some(p) => hide_login(p.trim()),
        None => "none set (HTTPS_PROXY)".into(),
    }
}

fn hide_login(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((_, host)) => format!("{scheme}://…@{host}"),
            None => url.to_string(),
        },
        None => match url.split_once('@') {
            Some((_, host)) => format!("…@{host}"),
            None => url.to_string(),
        },
    }
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
        return Err(Failed(tail).into());
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

/// Compiles the app's code and its dependencies' into `cache` (the app's own bytecode folder,
/// which its runs use), so the first run does not spend seconds compiling (pandas: 2–3 s).
/// `-S`: nothing of the environment runs (no `.pth` file), only the compiler reads it. Best
/// effort: a file that does not compile (a package's test data) leaves it to the first run;
/// only Cancel is an error.
pub fn compile_bytecode(
    env_dir: &Path,
    app_dir: &Path,
    cache: &Path,
    stop: &dyn Fn() -> bool,
) -> Result<()> {
    let mut prefix = OsString::from("pycache_prefix=");
    prefix.push(cache);
    let mut cmd = Command::new(python_in(env_dir, false));
    cmd.args(["-I", "-S", "-X"])
        .arg(prefix)
        .args(["-m", "compileall", "-q", "-j", "0"])
        .arg(env_dir)
        .arg(app_dir)
        .env_clear();
    for (k, v) in std::env::vars_os() {
        if k.to_str()
            .is_some_and(|k| KEPT_ENV.iter().any(|kept| kept.eq_ignore_ascii_case(k)))
        {
            cmd.env(k, v);
        }
    }
    match supervise(cmd, &mut |_| {}, stop, Duration::from_secs(10 * 60)) {
        Err(e) if e.is::<Cancelled>() => Err(e),
        _ => Ok(()),
    }
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

    fn failed(lines: &str) -> Failed {
        Failed(lines.lines().map(String::from).collect())
    }

    /// uv's last lines for a download that failed, as uv 0.13 prints them on Windows (in
    /// Italian: Windows translates its own error texts).
    const DNS: &str = "error: Failed to install cpython-3.10.4-windows-x86_64-none
  cause: Failed to download `https://github.com/astral-sh/python-build-standalone/releases/download/20220528/cpython.tar.gz`
  cause: error sending request for url (https://github.com/astral-sh/python-build-standalone/releases/download/20220528/cpython.tar.gz)
  cause: client error (Connect)
  cause: dns error
  cause: Host sconosciuto. (os error 11001)";

    const PROXY_DOWN: &str = "  cause: error sending request for url (https://files.pythonhosted.org/packages/pandas.whl)
  cause: client error (Connect)
  cause: tunnel error: failed to create underlying connection
  cause: tcp connect error
  cause: Impossibile stabilire la connessione. (os error 10061)";

    const NOT_FOUND: &str = "  cause: Failed to download `https://mirror.example.com/pbs/20220528/cpython.tar.gz`
  cause: HTTP status client error (404 Not Found) for url (https://mirror.example.com/pbs/20220528/cpython.tar.gz)";

    #[test]
    fn a_failed_download_says_what_to_ask_for() {
        let none = Sources::default();
        let dns = failed(DNS);
        assert_eq!(dns.cause(), Some(Cause::Dns));
        assert_eq!(dns.host().as_deref(), Some("github.com"));
        let text = dns.explain(&none, "none set (HTTPS_PROXY)");
        assert!(
            text.starts_with("Could not download Python itself from github.com"),
            "{text}"
        );
        assert!(text.contains("PythonMirror"), "{text}");
        assert!(text.contains("Proxy: none set"), "{text}");
        assert!(
            text.contains("os error 11001"),
            "uv's lines stay for IT: {text}"
        );

        let proxy = failed(PROXY_DOWN);
        assert_eq!(proxy.cause(), Some(Cause::Proxy));
        let text = proxy.explain(&none, "http://proxy.corp:8080");
        assert!(
            text.contains("the app's packages from files.pythonhosted.org"),
            "{text}"
        );
        assert!(
            text.contains("the proxy (http://proxy.corp:8080)"),
            "{text}"
        );

        let mirror = Sources {
            python_mirror: Some("https://mirror.example.com/pbs".into()),
        };
        let missing = failed(NOT_FOUND);
        assert_eq!(missing.cause(), Some(Cause::Missing));
        let text = missing.explain(&mirror, "none");
        assert!(
            text.contains("the Python mirror your organisation set"),
            "{text}"
        );
        assert!(
            text.contains("check the mirror at mirror.example.com"),
            "{text}"
        );

        let login = failed(
            "  cause: tunnel error: proxy authorization required\n  for url (https://pypi.org/simple)",
        );
        assert_eq!(login.cause(), Some(Cause::ProxyLogin));
        let cert =
            failed("  cause: invalid peer certificate: UnknownIssuer\n  url (https://pypi.org/x)");
        assert_eq!(cert.cause(), Some(Cause::Certificate));

        // What is not a download keeps the general message.
        let other = failed("error: No solution found");
        assert!(
            other
                .explain(&none, "none")
                .starts_with("preparing the Python environment failed")
        );
    }

    #[test]
    fn a_proxy_login_is_not_shown() {
        assert_eq!(hide_login("http://me:pw@proxy:8080"), "http://…@proxy:8080");
        assert_eq!(hide_login("proxy:8080"), "proxy:8080");
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

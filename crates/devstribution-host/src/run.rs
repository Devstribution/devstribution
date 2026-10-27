//! Starting a run and supervising it: messages from the runner, the run log, Cancel, timeout,
//! and the hard kill (Job Object on Windows, plus the COM servers the app registered).

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use devstribution_core::manifest::{InputType, Kind, Manifest, OutputType};

use crate::guard::{self, GuardHit, MAX_HITS};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// After Cancel, how long the app has to stop by itself before it is killed.
pub const GRACE: Duration = Duration::from_secs(10);

/// Default time limit for kinds that are expected to finish (`form`, `cli`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

/// Longest line kept from the app's output; the rest of a longer line is dropped.
const MAX_LINE: usize = 16 * 1024;

/// Size at which `app.log` stops growing for the run.
const MAX_LOG_BYTES: u64 = 20 << 20;

/// The variables an app gets from the launcher's environment; every other one is left out.
/// What Windows programs, COM and Python need to find the system, the user's folders, the
/// proxy and the display. Anything else a session may hold (cloud keys, `GITHUB_TOKEN`, API
/// keys) is a credential the app did not declare: secrets reach an app only as declared.
const RUN_ENV: &[&str] = &[
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
    "USERDNSDOMAIN",
    "LOGONSERVER",
    "COMPUTERNAME",
    "SESSIONNAME",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "ONEDRIVE",
    "ONEDRIVECOMMERCIAL",
    "ONEDRIVECONSUMER",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)",
    "COMMONPROGRAMW6432",
    "HOMEDRIVE",
    "HOMEPATH",
    "HOME",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_IDENTIFIER",
    "NUMBER_OF_PROCESSORS",
    "LANG",
    "LC_ALL",
    "TZ",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    // Outside Windows (development): the display, for `gui` apps.
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
];

fn kept_for_run(name: &str) -> bool {
    RUN_ENV.iter().any(|kept| kept.eq_ignore_ascii_case(name))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Progress {
        value: Option<f64>,
        message: String,
    },
    Log {
        level: String,
        message: String,
    },
    /// A line the app wrote to stdout/stderr (it also goes to the run log).
    Output {
        line: String,
    },
    Cancelling,
    /// The runtime guard stopped the run, blocked a call, or reports what the app did.
    Guard(GuardHit),
    Finished {
        result: RunResult,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Ok,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub status: RunStatus,
    #[serde(default)]
    pub outputs: Map<String, Value>,
    pub error: Option<String>,
    pub traceback: Option<String>,
    pub duration_ms: u64,
    /// What the runtime guard did; a `stop` among them ended the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guard: Vec<GuardHit>,
}

impl RunResult {
    /// The guard report that ended the run, if one did.
    pub fn guard_stop(&self) -> Option<&GuardHit> {
        self.guard.iter().find(|h| h.is_stop())
    }
}

/// Everything needed to start one run.
pub struct RunSpec {
    pub manifest: Manifest,
    pub app_dir: PathBuf,
    pub python: PathBuf,
    pub runner_dir: PathBuf,
    /// The run's folder: working directory, `app.log`; outputs go to `output_dir`.
    pub run_dir: PathBuf,
    pub output_dir: PathBuf,
    pub pycache_dir: PathBuf,
    /// devstribution's data folder: no app may change it, or read other runs' results in it.
    pub data_dir: PathBuf,
    /// Input values after [`resolve_inputs`].
    pub inputs: Map<String, Value>,
    pub secrets: Map<String, Value>,
    pub args: Vec<String>,
    /// Guard rules the user allowed for this app after a stop ("Allow and run again").
    pub guard_allow: Vec<String>,
}

/// Fills defaults and checks that every required input has a value. Secret inputs are left
/// out: they travel with the secrets.
pub fn resolve_inputs(
    manifest: &Manifest,
    given: &Map<String, Value>,
) -> Result<Map<String, Value>> {
    for name in given.keys() {
        if !manifest.inputs.iter().any(|i| &i.name == name) {
            bail!("unknown input `{name}`");
        }
    }
    let mut out = Map::new();
    let mut missing = Vec::new();
    for input in &manifest.inputs {
        if input.kind == InputType::Secret {
            continue;
        }
        let value = match given.get(&input.name) {
            Some(v) if !v.is_null() && v.as_str() != Some("") => v.clone(),
            _ => match &input.default {
                Some(d) => serde_json::to_value(d)?,
                None if input.required => {
                    missing.push(input.label.clone().unwrap_or_else(|| input.name.clone()));
                    continue;
                }
                None => Value::Null,
            },
        };
        out.insert(input.name.clone(), value);
    }
    if !missing.is_empty() {
        bail!("missing input: {}", missing.join(", "));
    }
    Ok(out)
}

/// Names of the secrets an app may receive: declared permissions plus `secret` inputs.
pub fn secret_names(manifest: &Manifest) -> Vec<String> {
    let mut names = manifest.permissions.secrets.clone();
    for input in &manifest.inputs {
        if input.kind == InputType::Secret && !names.contains(&input.name) {
            names.push(input.name.clone());
        }
    }
    names
}

fn start_message(spec: &RunSpec) -> Value {
    let m = &spec.manifest;
    let input_types: Map<String, Value> = m
        .inputs
        .iter()
        .map(|i| {
            (
                i.name.clone(),
                serde_json::to_value(i.kind).unwrap_or_default(),
            )
        })
        .collect();
    json!({
        "protocol": 1,
        "kind": m.kind.as_str(),
        "app_dir": spec.app_dir,
        "entry": m.entry.to_string(),
        "output_dir": spec.output_dir,
        "inputs": spec.inputs,
        "input_types": input_types,
        "outputs": m.outputs.iter().map(|o| json!({"name": o.name, "type": o.kind})).collect::<Vec<_>>(),
        "secrets": spec.secrets,
        "allowed_secrets": secret_names(m),
        "args": spec.args,
        "pycache_dir": spec.pycache_dir,
        "guard": {
            "network": m.permissions.network,
            "sap": m.permissions.sap,
            "allow": spec.guard_allow,
            "own": [spec.run_dir, spec.pycache_dir],
            "protected": [spec.data_dir],
            "private": [spec.data_dir.join("runs")],
            "child_dir": child_dir(&spec.run_dir),
        },
    })
}

/// Where the runner puts the settings and reports of the other Pythons the app starts: a folder
/// the guard seals (`runner/devstribution_runner/_child.py`).
fn child_dir(run_dir: &Path) -> PathBuf {
    run_dir.join("guard")
}

enum Control {
    Cancel,
    Kill,
}

/// A run in progress.
pub struct RunHandle {
    control: Sender<Control>,
    join: JoinHandle<RunResult>,
}

impl RunHandle {
    /// Asks the app to stop (cooperative); it is killed if it has not stopped after [`GRACE`].
    pub fn cancel(&self) {
        let _ = self.control.send(Control::Cancel);
    }

    /// Stops the app now.
    pub fn kill(&self) {
        let _ = self.control.send(Control::Kill);
    }

    pub fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    pub fn wait(self) -> RunResult {
        self.join.join().unwrap_or_else(|_| RunResult {
            status: RunStatus::Error,
            outputs: Map::new(),
            error: Some("the run supervisor crashed".into()),
            traceback: None,
            duration_ms: 0,
            guard: Vec::new(),
        })
    }
}

/// A COM server process registered by the app: (pid, creation time).
type ComProcess = (u32, Option<u64>);

/// Whether a command line is that of a program Windows started as a COM server: COM adds
/// `-Embedding` (or `/Embedding`). A program the user opened has no such flag, so a process
/// the app names (the runner shares the app's process, and can be told anything) is ended
/// only if COM started it, never because the app asked.
#[cfg_attr(not(windows), allow(dead_code))]
fn started_by_com(command_line: &str) -> bool {
    command_line
        .split_whitespace()
        .any(|arg| arg.eq_ignore_ascii_case("-embedding") || arg.eq_ignore_ascii_case("/embedding"))
}

pub type EventSink = Arc<dyn Fn(RunEvent) + Send + Sync>;

pub fn start(spec: RunSpec, on_event: EventSink) -> Result<RunHandle> {
    if !matches!(spec.manifest.kind, Kind::Form | Kind::Gui | Kind::Cli) {
        bail!(
            "apps of kind \"{}\" are not supported by this version yet",
            spec.manifest.kind.as_str()
        );
    }
    fs::create_dir_all(&spec.output_dir)
        .with_context(|| format!("creating {}", spec.output_dir.display()))?;
    let timeout = match spec.manifest.kind {
        Kind::Form | Kind::Cli => Some(
            spec.manifest
                .timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or(DEFAULT_TIMEOUT),
        ),
        _ => spec.manifest.timeout_seconds.map(Duration::from_secs),
    };

    let mut cmd = Command::new(&spec.python);
    // `-X utf8`: the app's output reaches the launcher as UTF-8 whatever the Windows code page.
    // `-S`: the runner loads `site` itself, after the runtime guard, so the code dependencies
    // run from `.pth` files when Python starts is checked too.
    // `-B`: no `__pycache__` in the runner's folder, which is checked to hold exactly the
    // runner's files before each run; the runner turns caching back on for the app's code.
    cmd.args(["-I", "-S", "-B", "-X", "utf8"])
        .arg(spec.runner_dir.join("bootstrap.py"))
        .current_dir(&spec.run_dir)
        .env_clear()
        .envs(std::env::vars_os().filter(|(k, _)| k.to_str().is_some_and(kept_for_run)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // `gui` apps run with pythonw.exe; the others write only to pipes, so a console window
    // would stay empty.
    platform::prepare(&mut cmd);
    let mut child = cmd.spawn().with_context(|| {
        format!(
            "cannot start {}: use Repair on the app, or reinstall it",
            spec.python.display()
        )
    })?;
    // The process starts suspended (Windows): it joins the Job Object before it can start
    // anything, so every process it creates is stopped with the run.
    let job = match platform::Job::assign(&child) {
        Ok(job) => Arc::new(job),
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let started_at = std::time::SystemTime::now();

    let mut stdin = child.stdin.take().expect("piped");
    writeln!(stdin, "{}", start_message(&spec))?;
    stdin.flush()?;

    let result_msg: Arc<Mutex<Option<Value>>> = Arc::default();
    let com_processes: Arc<Mutex<Vec<ComProcess>>> = Arc::default();
    let guard_hits: Arc<Mutex<Vec<GuardHit>>> = Arc::default();
    // Reports the guard's work; a stop also ends the whole process tree at once.
    let report: Arc<dyn Fn(GuardHit) + Send + Sync> = {
        let (on_event, guard_hits, job) = (on_event.clone(), guard_hits.clone(), job.clone());
        Arc::new(move |hit: GuardHit| {
            if hit.is_stop() {
                job.terminate();
            }
            {
                let mut hits = guard_hits.lock().unwrap();
                // The run's closing summary of the files it changed is always kept.
                if hits.len() >= MAX_HITS && !hit.is_stop() && hit.rule != "files" {
                    return;
                }
                hits.push(hit.clone());
            }
            on_event(RunEvent::Guard(hit));
        })
    };
    // Every process the run starts, whatever started it (the app cannot hide it from the Job
    // Object): one that malware would start ends the run.
    {
        let report = report.clone();
        let allowed = spec.guard_allow.clone();
        let pythons = guard::PythonCheck::new(
            &spec.runner_dir,
            &child_dir(&spec.run_dir).join("config.json"),
        );
        job.watch(child.id(), move |event| match event {
            JobEvent::NewProcess { command } => {
                // Another Python the guard is not in: started through a shell, `ctypes` or
                // the `py` launcher, which the runner could not rewrite.
                if pythons.outside_guard(&command) {
                    let allowed = allowed.iter().any(|r| r == "unguarded-python");
                    report(GuardHit {
                        action: if allowed { "observe" } else { "stop" }.into(),
                        rule: "unguarded-python".into(),
                        message: "started Python in a way devstribution cannot check".into(),
                        detail: command.chars().take(500).collect(),
                    });
                }
                if let Some(rule) = guard::match_process(&command, guard::process_rules()) {
                    let allowed = allowed.contains(&rule.id);
                    report(GuardHit {
                        action: if allowed { "observe" } else { "stop" }.into(),
                        rule: rule.id.clone(),
                        message: rule.message.clone(),
                        detail: command.chars().take(500).collect(),
                    });
                }
            }
            JobEvent::ProcessLimit => report(GuardHit {
                action: "block".into(),
                rule: "process-limit".into(),
                message: format!(
                    "tried to run more than {} programs at once",
                    platform::MAX_PROCESSES
                ),
                detail: String::new(),
            }),
        });
    }

    let stdout = child.stdout.take().expect("piped");
    let stdout_thread = {
        let (on_event, result_msg, com, report) = (
            on_event.clone(),
            result_msg.clone(),
            com_processes.clone(),
            report.clone(),
        );
        let run_start = platform::filetime(started_at);
        std::thread::spawn(move || {
            for_each_line(stdout, |line| {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    on_event(RunEvent::Output { line });
                    return;
                };
                let text = |k: &str| msg[k].as_str().unwrap_or("").to_string();
                match msg["type"].as_str() {
                    Some("progress") => on_event(RunEvent::Progress {
                        value: msg["value"].as_f64(),
                        message: text("message"),
                    }),
                    Some("log") => on_event(RunEvent::Log {
                        level: text("level"),
                        message: text("message"),
                    }),
                    // Only processes started during this run: the app (which runs in the
                    // same process as the runner) could otherwise name any process of the
                    // user's and have the launcher end it.
                    Some("com_process") => {
                        let created = msg["created"].as_u64();
                        if let (Some(pid), Some(c)) = (msg["pid"].as_u64(), created)
                            && u32::try_from(pid).is_ok()
                            && c + 20_000_000 >= run_start
                        {
                            com.lock().unwrap().push((pid as u32, created));
                        }
                    }
                    Some("guard") => {
                        if let Ok(hit) = serde_json::from_value::<GuardHit>(msg) {
                            report(hit);
                        }
                    }
                    Some("result") => *result_msg.lock().unwrap() = Some(msg),
                    _ => {}
                }
            });
        })
    };

    let stderr = child.stderr.take().expect("piped");
    let log_path = spec.run_dir.join("app.log");
    let stderr_thread = {
        let on_event = on_event.clone();
        std::thread::spawn(move || {
            let mut log = fs::File::create(&log_path).ok();
            let mut written = 0u64;
            for_each_line(stderr, |line| {
                if let Some(f) = log.as_mut() {
                    written += line.len() as u64 + 1;
                    if written > MAX_LOG_BYTES {
                        let _ = writeln!(f, "[the log is too long: the rest was not kept]");
                        log = None;
                    } else {
                        let _ = writeln!(f, "{line}");
                    }
                }
                on_event(RunEvent::Output { line });
            });
        })
    };

    let (control, control_rx) = mpsc::channel();
    let outputs_declared: Vec<(String, OutputType)> = spec
        .manifest
        .outputs
        .iter()
        .map(|o| (o.name.clone(), o.kind))
        .collect();
    let result_folders = result_folders(&spec.manifest, &spec.inputs, &spec.output_dir);
    let join = std::thread::spawn(move || {
        let started = Instant::now();
        let mut stdin = Some(stdin);
        let mut cancel_at: Option<Instant> = None;
        let mut killed = false;
        let kill = |child: &mut Child, stdin: &mut Option<ChildStdin>| {
            job.terminate();
            let _ = child.kill();
            for (pid, created) in com_processes.lock().unwrap().iter() {
                platform::terminate_if_same(*pid, *created);
            }
            stdin.take();
        };
        let exit = loop {
            if let Ok(Some(status)) = child.try_wait() {
                break Some(status);
            }
            let request = match control_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => None,
            };
            if !killed && guard_hits.lock().unwrap().iter().any(GuardHit::is_stop) {
                kill(&mut child, &mut stdin);
                killed = true;
            }
            let timed_out = timeout.is_some_and(|t| started.elapsed() > t);
            let wants_cancel = matches!(request, Some(Control::Cancel)) || timed_out;
            if matches!(request, Some(Control::Kill)) {
                kill(&mut child, &mut stdin);
                killed = true;
            } else if wants_cancel && cancel_at.is_none() {
                on_event(RunEvent::Cancelling);
                if let Some(s) = stdin.as_mut() {
                    let _ = writeln!(s, "{}", json!({"type": "cancel"}));
                    let _ = s.flush();
                }
                cancel_at = Some(Instant::now());
            }
            if !killed && cancel_at.is_some_and(|at| at.elapsed() > GRACE) {
                kill(&mut child, &mut stdin);
                killed = true;
            }
            if killed {
                break child.wait().ok();
            }
        };
        drop(stdin);
        // The app may have left child processes holding the pipes open; don't wait on them
        // forever once the app itself is gone.
        job.terminate();
        join_for(stdout_thread, Duration::from_secs(2));
        join_for(stderr_thread, Duration::from_secs(2));

        let duration_ms = started.elapsed().as_millis() as u64;
        let msg = result_msg.lock().unwrap().take();
        let mut result = match msg {
            Some(m) => RunResult {
                status: match m["status"].as_str() {
                    Some("ok") => RunStatus::Ok,
                    Some("cancelled") => RunStatus::Cancelled,
                    _ => RunStatus::Error,
                },
                outputs: m["outputs"].as_object().cloned().unwrap_or_default(),
                error: m["error"].as_str().map(String::from),
                traceback: m["traceback"].as_str().map(String::from),
                duration_ms,
                guard: Vec::new(),
            },
            None if killed || cancel_at.is_some() => RunResult {
                status: RunStatus::Cancelled,
                outputs: Map::new(),
                error: Some("the app was stopped".into()),
                traceback: None,
                duration_ms,
                guard: Vec::new(),
            },
            None => RunResult {
                status: RunStatus::Error,
                outputs: Map::new(),
                error: Some(match exit {
                    Some(s) => format!("the app ended without a result ({s})"),
                    None => "the app ended without a result".into(),
                }),
                traceback: None,
                duration_ms,
                guard: Vec::new(),
            },
        };
        result.guard = std::mem::take(&mut *guard_hits.lock().unwrap());
        if let Some(stop) = result.guard_stop().cloned() {
            result.status = RunStatus::Error;
            result.error = Some(stop.stop_error());
            result.traceback = None;
            result.outputs.clear();
        }
        // The result comes from the app's own process, so it is checked here like any other
        // input: only declared outputs, and files only from the run's output folder or a
        // folder the user chose for this run.
        for problem in
            keep_declared_outputs(&outputs_declared, &result_folders, &mut result.outputs)
        {
            on_event(RunEvent::Log {
                level: "warning".into(),
                message: problem,
            });
        }
        // An app that crashed or was stopped cannot close the Excel (or other COM server)
        // it started; do it for it.
        if result.status != RunStatus::Ok {
            for (pid, created) in com_processes.lock().unwrap().iter() {
                platform::terminate_if_same(*pid, *created);
            }
        }
        on_event(RunEvent::Finished {
            result: result.clone(),
        });
        result
    });
    Ok(RunHandle { control, join })
}

/// Calls `f` with every line read from `r`, decoded leniently (an invalid byte becomes `�`
/// instead of ending the reading) and cut at [`MAX_LINE`] bytes, until the end of the stream.
fn for_each_line(r: impl Read, mut f: impl FnMut(String)) {
    let mut reader = BufReader::new(r);
    let mut line: Vec<u8> = Vec::new();
    let mut overflow = false;
    loop {
        let (consumed, end_of_line) = match reader.fill_buf() {
            Ok([]) | Err(_) => break,
            Ok(buf) => match buf.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    let room = MAX_LINE.saturating_sub(line.len());
                    line.extend_from_slice(&buf[..i.min(room)]);
                    (i + 1, true)
                }
                None => {
                    let room = MAX_LINE.saturating_sub(line.len());
                    overflow |= buf.len() > room;
                    line.extend_from_slice(&buf[..buf.len().min(room)]);
                    (buf.len(), false)
                }
            },
        };
        reader.consume(consumed);
        if end_of_line {
            emit_line(&mut line, &mut overflow, &mut f);
        }
    }
    if !line.is_empty() {
        emit_line(&mut line, &mut overflow, &mut f);
    }
}

fn emit_line(line: &mut Vec<u8>, overflow: &mut bool, f: &mut impl FnMut(String)) {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    let mut text = String::from_utf8_lossy(line).into_owned();
    if *overflow {
        text.push_str(" […]");
    }
    f(text);
    line.clear();
    *overflow = false;
}

/// Where a run's `file` outputs may be: its output folder, and the folders the user chose for
/// its `folder` inputs (apps that keep their work between runs write there, not in a new
/// folder per run).
pub fn result_folders(
    manifest: &Manifest,
    inputs: &Map<String, Value>,
    output_dir: &Path,
) -> Vec<PathBuf> {
    let mut folders = vec![output_dir.to_path_buf()];
    for input in &manifest.inputs {
        if input.kind == InputType::Folder
            && let Some(path) = inputs.get(&input.name).and_then(Value::as_str)
            && !path.is_empty()
        {
            folders.push(PathBuf::from(path));
        }
    }
    folders
}

/// Keeps only outputs the manifest declares, and `file` outputs only when they name a file
/// inside one of `folders` (see [`result_folders`]). Returns what was dropped, for the run log.
fn keep_declared_outputs(
    declared: &[(String, OutputType)],
    folders: &[PathBuf],
    outputs: &mut Map<String, Value>,
) -> Vec<String> {
    let mut problems = Vec::new();
    outputs.retain(|name, value| {
        let Some((_, kind)) = declared.iter().find(|(n, _)| n == name) else {
            problems.push(format!(
                "the app returned `{name}`, which it does not declare: ignored"
            ));
            return false;
        };
        if *kind == OutputType::File && !value.is_null() {
            let inside = value.as_str().is_some_and(|p| {
                folders.iter().any(|dir| is_inside(Path::new(p), dir)) && Path::new(p).is_file()
            });
            if !inside {
                problems.push(format!(
                    "the file output `{name}` is not a file in the run's output folder or in a                      folder chosen for the run: ignored"
                ));
                return false;
            }
        }
        true
    });
    problems
}

fn join_for(handle: JoinHandle<()>, limit: Duration) {
    let until = Instant::now() + limit;
    while !handle.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}

/// Whether `path` is inside `dir` (both are resolved first).
pub fn is_inside(path: &Path, dir: &Path) -> bool {
    match (path.canonicalize(), dir.canonicalize()) {
        (Ok(p), Ok(d)) => p.starts_with(d),
        _ => false,
    }
}

/// What the run's Job Object reports.
pub enum JobEvent {
    /// A process joined the run (not the app's own): its command line, or its program when the
    /// command line cannot be read.
    NewProcess { command: String },
    /// The app tried to have more than `MAX_PROCESSES` processes at once.
    ProcessLimit,
}

#[cfg(windows)]
mod platform {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Result, bail};
    use windows_sys::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, UNICODE_STRING,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::IO::{
        CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_UILIMIT_DESKTOP,
        JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
        JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOBOBJECT_ASSOCIATE_COMPLETION_PORT,
        JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectAssociateCompletionPortInformation, JobObjectBasicUIRestrictions,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::SystemServices::{
        JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT, JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO,
        JOB_OBJECT_MSG_NEW_PROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, GetProcessTimes, OpenProcess, OpenThread,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        QueryFullProcessImageNameW, ResumeThread, THREAD_SUSPEND_RESUME, TerminateProcess,
    };

    use super::JobEvent;

    /// Most processes a run may have at once: a pool of workers fits, a fork bomb does not.
    pub const MAX_PROCESSES: u32 = 128;

    /// No console window, and suspended until [`Job::assign`] has put it in its Job Object.
    pub fn prepare(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    /// A time as a Windows FILETIME (100 ns since 1601), the unit of process creation times.
    pub fn filetime(t: SystemTime) -> u64 {
        let unix = t.duration_since(UNIX_EPOCH).unwrap_or_default();
        (unix.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(unix.subsec_nanos() / 100)
    }

    /// The run's Job Object: the app and every process it starts. Closing the launcher closes
    /// the handle, which kills them (KILL_ON_JOB_CLOSE).
    pub struct Job {
        job: HANDLE,
        /// Where the job reports new processes; read by [`Job::watch`]'s thread.
        port: HANDLE,
        closed: Arc<AtomicBool>,
    }

    // The handles are only used through thread-safe kernel calls.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// Puts the suspended `child` in a new Job Object, then lets it run. A run that cannot
        /// be put in a job is not started: Cancel and closing the launcher could not stop
        /// what it starts.
        pub fn assign(child: &Child) -> Result<Job> {
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    bail!("cannot create a Job Object for the run");
                }
                let port = CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1);
                let job = Job {
                    job: handle,
                    port,
                    closed: Arc::default(),
                };
                if port.is_null() {
                    bail!("cannot watch the app's processes (no completion port)");
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
                info.BasicLimitInformation.ActiveProcessLimit = MAX_PROCESSES;
                // No switching desktops, changing display or system settings, or logging off.
                // Not the clipboard: apps that paste (tkinter, SAP scripts) need it.
                let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
                    UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                        | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                        | JOB_OBJECT_UILIMIT_EXITWINDOWS
                        | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
                };
                let port_info = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
                    CompletionKey: std::ptr::null_mut(),
                    CompletionPort: port,
                };
                let set = |class, data: *const std::ffi::c_void, size: usize| {
                    SetInformationJobObject(job.job, class, data, size as u32) != 0
                };
                let ok = set(
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    std::mem::size_of_val(&info),
                ) && set(
                    JobObjectBasicUIRestrictions,
                    (&raw const ui).cast(),
                    std::mem::size_of_val(&ui),
                ) && set(
                    JobObjectAssociateCompletionPortInformation,
                    (&raw const port_info).cast(),
                    std::mem::size_of_val(&port_info),
                );
                if !ok || AssignProcessToJobObject(job.job, child.as_raw_handle() as HANDLE) == 0 {
                    bail!("cannot put the app in its Job Object");
                }
                resume(child.id())?;
                Ok(job)
            }
        }

        pub fn terminate(&self) {
            if !self.job.is_null() {
                unsafe { TerminateJobObject(self.job, 1) };
            }
        }

        /// Calls `on_event`, from a thread of its own, for every process that joins the job
        /// apart from the app's own (`app_pid`), until the job is empty or dropped.
        pub fn watch(&self, app_pid: u32, on_event: impl Fn(JobEvent) + Send + 'static) {
            let port = self.port as usize;
            let closed = self.closed.clone();
            std::thread::spawn(move || {
                let port = port as HANDLE;
                while !closed.load(Ordering::Relaxed) {
                    let (mut code, mut key) = (0u32, 0usize);
                    let mut overlapped: *mut OVERLAPPED = std::ptr::null_mut();
                    let got = unsafe {
                        GetQueuedCompletionStatus(port, &mut code, &mut key, &mut overlapped, 250)
                    };
                    if got == 0 {
                        continue; // nothing within 250 ms
                    }
                    // For job messages, the "overlapped" pointer carries the process id.
                    let pid = overlapped as usize as u32;
                    match code {
                        JOB_OBJECT_MSG_NEW_PROCESS if pid != app_pid => {
                            if let Some(command) = command_line(pid) {
                                on_event(JobEvent::NewProcess { command });
                            }
                        }
                        JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT => on_event(JobEvent::ProcessLimit),
                        JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO => break,
                        _ => {}
                    }
                }
                unsafe { CloseHandle(port) };
            });
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::Relaxed);
            if !self.job.is_null() {
                unsafe { CloseHandle(self.job) };
            }
            // The watching thread owns the port: it stops within 250 ms and closes it.
        }
    }

    /// The command line of `pid`, or its program when the command line cannot be read.
    fn command_line(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None; // already gone
            }
            // A UNICODE_STRING followed by its text.
            let mut buffer = vec![0u64; 4096];
            let mut len = 0u32;
            let status = NtQueryInformationProcess(
                h,
                ProcessCommandLineInformation,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                &mut len,
            );
            let mut text = None;
            if status >= 0 {
                let us = &*(buffer.as_ptr() as *const UNICODE_STRING);
                if !us.Buffer.is_null() && us.Length > 0 {
                    let chars = std::slice::from_raw_parts(us.Buffer, us.Length as usize / 2);
                    text = Some(String::from_utf16_lossy(chars));
                }
            }
            if text.is_none() {
                let mut name = vec![0u16; 32768];
                let mut size = name.len() as u32;
                if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, name.as_mut_ptr(), &mut size)
                    != 0
                {
                    text = Some(String::from_utf16_lossy(&name[..size as usize]));
                }
            }
            CloseHandle(h);
            text
        }
    }

    /// Resumes the threads of a process created suspended (it has exactly one).
    fn resume(pid: u32) -> Result<()> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                bail!("cannot start the app (no thread snapshot)");
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut resumed = 0;
            let mut more = Thread32First(snapshot, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        if ResumeThread(thread) != u32::MAX {
                            resumed += 1;
                        }
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            if resumed == 0 {
                bail!("cannot start the app (its thread could not be resumed)");
            }
            Ok(())
        }
    }

    /// Terminates `pid` only if it is still the process created at `created` (PIDs are reused)
    /// and COM started it ([`super::started_by_com`]).
    pub fn terminate_if_same(pid: u32, created: Option<u64>) {
        if !command_line(pid).is_some_and(|c| super::started_by_com(&c)) {
            return;
        }
        let Some(created) = created else { return };
        unsafe {
            let h = OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            );
            if h.is_null() {
                return;
            }
            let zero = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
            if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0 {
                let actual = (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime);
                if actual == created {
                    TerminateProcess(h, 1);
                }
            }
            CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::process::{Child, Command};
    use std::time::SystemTime;

    use anyhow::Result;

    pub fn prepare(_cmd: &mut Command) {}

    pub fn filetime(_t: SystemTime) -> u64 {
        0
    }

    pub const MAX_PROCESSES: u32 = 0;

    /// Outside Windows the app is killed directly; Job Objects do not exist.
    pub struct Job;

    impl Job {
        pub fn assign(_child: &Child) -> Result<Job> {
            Ok(Job)
        }

        pub fn terminate(&self) {}

        pub fn watch(&self, _app_pid: u32, _on_event: impl Fn(super::JobEvent) + Send + 'static) {}
    }

    pub fn terminate_if_same(_pid: u32, _created: Option<u64>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_com_servers_are_ended() {
        let excel = r#""C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE""#;
        assert!(started_by_com(&format!("{excel} /automation -Embedding")));
        assert!(started_by_com(r"C:\x\WINWORD.EXE /Automation /EMBEDDING"));
        assert!(!started_by_com(&format!(
            r#"{excel} "C:\docs\clients.xlsx""#
        )));
        assert!(!started_by_com(&format!(
            r"{excel} C:\docs\-Embedding-notes.xlsx"
        )));
    }

    #[test]
    fn runs_get_only_the_listed_environment() {
        for kept in [
            "Path",
            "SystemRoot",
            "APPDATA",
            "ProgramFiles(x86)",
            "https_proxy",
        ] {
            assert!(kept_for_run(kept), "{kept}");
        }
        for left in [
            "AWS_SECRET_ACCESS_KEY",
            "GITHUB_TOKEN",
            "OPENAI_API_KEY",
            "PYTHONPATH",
            "VIRTUAL_ENV",
            "UV_INDEX_URL",
        ] {
            assert!(!kept_for_run(left), "{left}");
        }
    }

    #[test]
    fn forged_results_are_trimmed_to_what_the_manifest_declares() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("ok.txt"), "x").unwrap();
        let elsewhere = dir.path().join("elsewhere.exe");
        fs::write(&elsewhere, "x").unwrap();
        let chosen = dir.path().join("chosen");
        fs::create_dir_all(&chosen).unwrap();
        fs::write(chosen.join("work.xlsx"), "x").unwrap();
        let declared = vec![
            ("report".to_string(), OutputType::File),
            ("other".to_string(), OutputType::File),
            ("work".to_string(), OutputType::File),
            ("gone".to_string(), OutputType::File),
            ("summary".to_string(), OutputType::Text),
        ];
        let mut outputs = Map::new();
        outputs.insert("report".into(), json!(out.join("ok.txt")));
        outputs.insert("other".into(), json!(elsewhere));
        outputs.insert("work".into(), json!(chosen.join("work.xlsx")));
        outputs.insert("gone".into(), json!(chosen.join("missing.xlsx")));
        outputs.insert("summary".into(), json!("fine"));
        outputs.insert("extra".into(), json!("C:/Windows/System32/calc.exe"));
        let problems = keep_declared_outputs(&declared, &[out, chosen], &mut outputs);
        assert_eq!(problems.len(), 3, "{problems:?}");
        let mut kept: Vec<&str> = outputs.keys().map(String::as_str).collect();
        kept.sort_unstable();
        assert_eq!(kept, ["report", "summary", "work"]);
    }

    #[test]
    fn lines_survive_invalid_bytes_and_are_capped() {
        let mut data = b"caf\xe9\r\n".to_vec();
        data.extend(std::iter::repeat_n(b'x', MAX_LINE * 3));
        data.extend(b"\nlast");
        let mut lines = Vec::new();
        for_each_line(&data[..], |l| lines.push(l));
        assert_eq!(lines[0], "caf\u{fffd}");
        assert!(lines[1].len() < MAX_LINE + 10 && lines[1].ends_with("[…]"));
        assert_eq!(lines[2], "last");
    }
}

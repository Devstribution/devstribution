//! Starting a run and supervising it: messages from the runner, the run log, Cancel, timeout,
//! and the hard kill (Job Object on Windows, plus the COM servers the app registered).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use devstribution_core::manifest::{InputType, Kind, Manifest};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// After Cancel, how long the app has to stop by itself before it is killed.
pub const GRACE: Duration = Duration::from_secs(10);

/// Default time limit for kinds that are expected to finish (`form`, `cli`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

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
    /// Input values after [`resolve_inputs`].
    pub inputs: Map<String, Value>,
    pub secrets: Map<String, Value>,
    pub args: Vec<String>,
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
    })
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
        })
    }
}

/// A COM server process registered by the app: (pid, creation time).
type ComProcess = (u32, Option<u64>);

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
    cmd.arg("-I")
        .arg(spec.runner_dir.join("bootstrap.py"))
        .current_dir(&spec.run_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if spec.manifest.kind == Kind::Form {
        crate::uv::hide_window(&mut cmd);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("cannot start {}", spec.python.display()))?;
    let job = platform::Job::assign(&child);

    let mut stdin = child.stdin.take().expect("piped");
    writeln!(stdin, "{}", start_message(&spec))?;
    stdin.flush()?;

    let result_msg: Arc<Mutex<Option<Value>>> = Arc::default();
    let com_processes: Arc<Mutex<Vec<ComProcess>>> = Arc::default();

    let stdout = child.stdout.take().expect("piped");
    let stdout_thread = {
        let (on_event, result_msg, com) =
            (on_event.clone(), result_msg.clone(), com_processes.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    on_event(RunEvent::Output { line });
                    continue;
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
                    Some("com_process") => {
                        if let Some(pid) = msg["pid"].as_u64() {
                            com.lock()
                                .unwrap()
                                .push((pid as u32, msg["created"].as_u64()));
                        }
                    }
                    Some("result") => *result_msg.lock().unwrap() = Some(msg),
                    _ => {}
                }
            }
        })
    };

    let stderr = child.stderr.take().expect("piped");
    let log_path = spec.run_dir.join("app.log");
    let stderr_thread = {
        let on_event = on_event.clone();
        std::thread::spawn(move || {
            let mut log = fs::File::create(&log_path).ok();
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if let Some(f) = log.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
                on_event(RunEvent::Output { line });
            }
        })
    };

    let (control, control_rx) = mpsc::channel();
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
        let result = match msg {
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
            },
            None if killed || cancel_at.is_some() => RunResult {
                status: RunStatus::Cancelled,
                outputs: Map::new(),
                error: Some("the app was stopped".into()),
                traceback: None,
                duration_ms,
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
            },
        };
        on_event(RunEvent::Finished {
            result: result.clone(),
        });
        result
    });
    Ok(RunHandle { control, join })
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

#[cfg(windows)]
mod platform {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };

    /// The run's Job Object: the app and every process it starts. Closing the launcher closes
    /// the handle, which kills them (KILL_ON_JOB_CLOSE).
    pub struct Job(HANDLE);

    // The handle is only used through thread-safe kernel calls.
    unsafe impl Send for Job {}

    impl Job {
        pub fn assign(child: &Child) -> Job {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Job(job);
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
                Job(job)
            }
        }

        pub fn terminate(&self) {
            if !self.0.is_null() {
                unsafe { TerminateJobObject(self.0, 1) };
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// Terminates `pid` only if it is still the process created at `created` (PIDs are reused).
    pub fn terminate_if_same(pid: u32, created: Option<u64>) {
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
    use std::process::Child;

    /// Outside Windows the app is killed directly; Job Objects do not exist.
    pub struct Job;

    impl Job {
        pub fn assign(_child: &Child) -> Job {
            Job
        }

        pub fn terminate(&self) {}
    }

    pub fn terminate_if_same(_pid: u32, _created: Option<u64>) {}
}

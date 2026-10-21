//! `devstribution dev`: run an app folder exactly as the launcher would, minus the signature.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use devstribution_core::manifest::{InputType, Kind, Manifest};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{checks, env, runner_files};

/// After Cancel, how long the app gets to stop by itself before it is killed.
const GRACE: Duration = Duration::from_secs(10);

pub fn dev(dir: &Path, inputs: &[String], args: &[String], json_output: bool) -> Result<u8> {
    let app_dir = env::absolute(dir)?;
    let runner_dir = runner_files::install()?;
    let checked = checks::check_app(&app_dir, &runner_dir)?;
    checks::print_warnings(&checked.warnings);
    let manifest = checked.manifest;
    if !matches!(manifest.kind, Kind::Form | Kind::Gui | Kind::Cli) {
        bail!(
            "kind = \"{}\" is not supported yet (form, gui and cli are)",
            manifest.kind.as_str()
        );
    }
    let here = current_platform();
    if !manifest.platforms.iter().any(|p| p == here) {
        eprintln!(
            "warning: the app declares platforms {:?}, not {here}",
            manifest.platforms
        );
    }

    let python = sync_environment(&app_dir, &manifest)?;
    let start_inputs = collect_inputs(&manifest, inputs)?;
    let secrets = collect_secrets(&manifest)?;

    let run_dir =
        env::data_dir()?
            .join("runs")
            .join(format!("{}-{}", env::timestamp(), manifest.id.app));
    let output_dir = run_dir.join("output");
    std::fs::create_dir_all(&output_dir)?;

    let mut input_types = Map::new();
    for input in &manifest.inputs {
        input_types.insert(input.name.clone(), serde_json::to_value(input.kind)?);
    }
    let start = json!({
        "protocol": 1,
        "kind": manifest.kind.as_str(),
        "app_dir": app_dir,
        "entry": manifest.entry.to_string(),
        "output_dir": output_dir,
        "inputs": start_inputs,
        "input_types": input_types,
        "outputs": manifest.outputs.iter().map(|o| json!({"name": o.name, "type": o.kind})).collect::<Vec<_>>(),
        "secrets": secrets,
        "allowed_secrets": allowed_secrets(&manifest),
        "args": args,
        "pycache_dir": env::data_dir()?.join("pycache"),
    });

    eprintln!("Running {} {} ...", manifest.id, manifest.version);
    let mut child = Command::new(&python)
        .arg("-I")
        .arg(runner_dir.join("bootstrap.py"))
        .current_dir(&run_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("cannot start {}", python.display()))?;
    let mut stdin = child.stdin.take().expect("piped");
    writeln!(stdin, "{start}")?;
    stdin.flush()?;

    let result = supervise(
        &mut child,
        stdin,
        manifest.timeout_seconds.map(Duration::from_secs),
    )?;
    report(&result, &output_dir, json_output)
}

fn current_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// `uv sync` into a per-folder environment outside the app folder, as the launcher does.
fn sync_environment(app_dir: &Path, manifest: &Manifest) -> Result<PathBuf> {
    let key = hex::encode(&Sha256::digest(app_dir.to_string_lossy().as_bytes())[..8]);
    let env_dir = env::data_dir()?
        .join("dev-envs")
        .join(format!("{}-{key}", manifest.id.app));
    let args = [
        "sync",
        "--locked",
        "--no-build",
        "--no-dev",
        "--no-install-project",
        "--quiet",
    ];
    env::run_uv(&args, app_dir, &[("UV_PROJECT_ENVIRONMENT", &env_dir)])
        .context("preparing the environment (if uv.lock is out of date, run `uv lock`)")?;
    let python = if cfg!(windows) {
        let exe = if manifest.kind == Kind::Gui {
            "pythonw.exe"
        } else {
            "python.exe"
        };
        env_dir.join("Scripts").join(exe)
    } else {
        env_dir.join("bin").join("python")
    };
    if !python.exists() {
        bail!("the environment has no Python at {}", python.display());
    }
    Ok(python)
}

fn collect_inputs(manifest: &Manifest, given: &[String]) -> Result<Map<String, Value>> {
    let mut values: BTreeMap<&str, &str> = BTreeMap::new();
    for item in given {
        let (name, value) = item
            .split_once('=')
            .with_context(|| format!("`--input {item}`: use name=value"))?;
        if !manifest.inputs.iter().any(|i| i.name == name) {
            let names: Vec<&str> = manifest.inputs.iter().map(|i| i.name.as_str()).collect();
            bail!("unknown input `{name}` (the app declares {names:?})");
        }
        values.insert(name, value);
    }
    let mut out = Map::new();
    let mut missing = Vec::new();
    for input in &manifest.inputs {
        if input.kind == InputType::Secret {
            continue;
        }
        let value = match (values.get(input.name.as_str()), &input.default) {
            (Some(v), _) => match input.kind {
                InputType::File | InputType::Folder => json!(env::absolute(Path::new(v))?),
                _ => json!(v),
            },
            (None, Some(default)) => serde_json::to_value(default)?,
            (None, None) if input.required => {
                missing.push(format!(
                    "{}=<{}>",
                    input.name,
                    serde_json::to_value(input.kind)?.as_str().unwrap_or("")
                ));
                continue;
            }
            (None, None) => Value::Null,
        };
        out.insert(input.name.clone(), value);
    }
    if !missing.is_empty() {
        bail!(
            "missing inputs; pass them as {}",
            missing
                .iter()
                .map(|m| format!("-i {m}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    Ok(out)
}

fn allowed_secrets(manifest: &Manifest) -> Vec<String> {
    let mut names = manifest.permissions.secrets.clone();
    for input in &manifest.inputs {
        if input.kind == InputType::Secret && !names.contains(&input.name) {
            names.push(input.name.clone());
        }
    }
    names
}

/// Secrets come from `DEVSTRIBUTION_SECRET_<NAME>` or a prompt; the launcher uses the keyring.
fn collect_secrets(manifest: &Manifest) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for name in allowed_secrets(manifest) {
        let var = format!("DEVSTRIBUTION_SECRET_{}", name.to_uppercase());
        let value = match std::env::var(&var) {
            Ok(v) => v,
            Err(_) => rpassword::prompt_password(format!("Secret `{name}` (or set {var}): "))
                .with_context(|| format!("secret `{name}` needed: set {var}"))?,
        };
        out.insert(name, json!(value));
    }
    Ok(out)
}

/// Relays the runner's messages, turns Ctrl-C into Cancel and kills the run after the grace
/// period. Returns the result message.
fn supervise(child: &mut Child, mut stdin: ChildStdin, timeout: Option<Duration>) -> Result<Value> {
    let interrupts = Arc::new(AtomicUsize::new(0));
    {
        let interrupts = interrupts.clone();
        let _ = ctrlc::set_handler(move || {
            interrupts.fetch_add(1, Ordering::SeqCst);
        });
    }
    let stdout = child.stdout.take().expect("piped");
    let (tx, rx) = mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            match serde_json::from_str(&line) {
                Ok(msg) => {
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
                Err(_) => eprintln!("{line}"),
            }
        }
    });

    let started = Instant::now();
    let mut cancelled_at: Option<Instant> = None;
    let mut result = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => match msg["type"].as_str() {
                Some("progress") => {
                    let pct = msg["value"]
                        .as_f64()
                        .map(|v| format!("{:3.0}% ", v * 100.0))
                        .unwrap_or_default();
                    eprintln!("[progress] {pct}{}", msg["message"].as_str().unwrap_or(""));
                }
                Some("log") => eprintln!(
                    "[{}] {}",
                    msg["level"].as_str().unwrap_or("info"),
                    msg["message"].as_str().unwrap_or("")
                ),
                Some("com_process") => eprintln!("[com] tracking process {}", msg["pid"]),
                Some("result") => {
                    result = Some(msg);
                    break;
                }
                _ => {}
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let timed_out = timeout.is_some_and(|t| started.elapsed() > t);
        let wants_cancel = interrupts.load(Ordering::SeqCst) > 0 || timed_out;
        match cancelled_at {
            None if wants_cancel => {
                eprintln!(
                    "{}; cancelling (Ctrl-C again to stop immediately)",
                    if timed_out {
                        "Timed out"
                    } else {
                        "Cancel requested"
                    }
                );
                let _ = writeln!(stdin, "{}", json!({"type": "cancel"}));
                let _ = stdin.flush();
                cancelled_at = Some(Instant::now());
            }
            Some(at) if at.elapsed() > GRACE || interrupts.load(Ordering::SeqCst) > 1 => {
                eprintln!("The app did not stop; killing it");
                let _ = child.kill();
                break;
            }
            _ => {}
        }
    }
    drop(stdin);
    let status = child.wait()?;
    Ok(result.unwrap_or_else(|| {
        json!({"type": "result", "status": "error", "error": format!("the app process ended without a result ({status})")})
    }))
}

fn report(result: &Value, output_dir: &Path, json_output: bool) -> Result<u8> {
    if json_output {
        println!("{result}");
    }
    match result["status"].as_str() {
        Some("ok") => {
            if !json_output {
                if let Some(outputs) = result["outputs"].as_object() {
                    for (name, value) in outputs {
                        match value {
                            Value::String(s) => println!("{name}: {s}"),
                            other => println!("{name}: {other}"),
                        }
                    }
                }
            }
            eprintln!("Done. Result folder: {}", output_dir.display());
            Ok(0)
        }
        Some("cancelled") => {
            eprintln!("Cancelled.");
            Ok(2)
        }
        _ => {
            eprintln!(
                "error: {}",
                result["error"].as_str().unwrap_or("unknown error")
            );
            Ok(1)
        }
    }
}

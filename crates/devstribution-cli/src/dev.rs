//! `devstribution dev`: run an app folder exactly as the launcher would, minus the signature.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use devstribution_core::manifest::{InputType, Kind};
use devstribution_host::run::{self, RunEvent, RunResult, RunSpec, RunStatus};
use devstribution_host::store::current_platform;
use devstribution_host::time::{now_secs, stamp};
use devstribution_host::uv::{self, LockMode};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{checks, env};

pub fn dev(dir: &Path, inputs: &[String], args: &[String], json_output: bool) -> Result<u8> {
    let paths = env::paths()?;
    let app_dir = env::absolute(dir)?;
    let runner_dir = devstribution_host::runner::install(&paths)?;
    let checked = checks::check_app(&app_dir, &runner_dir)?;
    checks::print_warnings(&checked.warnings);
    let manifest = checked.manifest;
    if !manifest.platforms.iter().any(|p| p == current_platform()) {
        eprintln!(
            "warning: the app declares platforms {:?}, not {}",
            manifest.platforms,
            current_platform()
        );
    }

    // One environment per app folder, outside it, as the launcher does for installed apps.
    let key = hex::encode(&Sha256::digest(app_dir.to_string_lossy().as_bytes())[..8]);
    let env_dir = paths.dev_envs().join(format!("{}-{key}", manifest.id.app));
    let mut quiet = |_: &str| {};
    uv::sync(&app_dir, &env_dir, LockMode::Locked, &mut quiet, &|| false)
        .context("preparing the environment (if uv.lock is out of date, run `uv lock`)")?;

    let given = parse_inputs(&manifest, inputs)?;
    let resolved = run::resolve_inputs(&manifest, &given)
        .map_err(|e| anyhow::anyhow!("{e}; pass inputs as -i name=value"))?;
    let secrets = collect_secrets(&run::secret_names(&manifest))?;

    let run_dir = paths
        .runs()
        .join(format!("{}-{}", stamp(now_secs()), manifest.id.app));
    let output_dir = run_dir.join("output");
    eprintln!("Running {} {} ...", manifest.id, manifest.version);
    let spec = RunSpec {
        python: uv::python_in(&env_dir, manifest.kind == Kind::Gui),
        manifest,
        app_dir,
        runner_dir,
        run_dir,
        output_dir: output_dir.clone(),
        pycache_dir: paths.pycache(),
        data_dir: paths.root.clone(),
        inputs: resolved,
        secrets,
        args: args.to_vec(),
        guard_allow: Vec::new(),
    };
    let handle = run::start(spec, Arc::new(print_event))?;

    // Ctrl-C: first press cancels (the app can clean up), second press kills.
    let presses = Arc::new(AtomicUsize::new(0));
    {
        let presses = presses.clone();
        let _ = ctrlc::set_handler(move || {
            presses.fetch_add(1, Ordering::SeqCst);
        });
    }
    let mut handled = 0;
    while !handle.is_finished() {
        std::thread::sleep(Duration::from_millis(100));
        let n = presses.load(Ordering::SeqCst);
        if n > handled {
            handled = n;
            if n == 1 {
                eprintln!("Cancelling (Ctrl-C again to stop immediately)");
                handle.cancel();
            } else {
                handle.kill();
            }
        }
    }
    report(&handle.wait(), &output_dir, json_output)
}

/// `-i name=value` → values; file and folder paths are made absolute (the run's working
/// directory is its own folder).
fn parse_inputs(
    manifest: &devstribution_core::manifest::Manifest,
    given: &[String],
) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for item in given {
        let (name, value) = item
            .split_once('=')
            .with_context(|| format!("`--input {item}`: use name=value"))?;
        let input = manifest
            .inputs
            .iter()
            .find(|i| i.name == name)
            .with_context(|| {
                let names: Vec<&str> = manifest.inputs.iter().map(|i| i.name.as_str()).collect();
                format!("unknown input `{name}` (the app declares {names:?})")
            })?;
        let value = match input.kind {
            InputType::File | InputType::Folder => json!(env::absolute(Path::new(value))?),
            _ => json!(value),
        };
        out.insert(name.to_string(), value);
    }
    Ok(out)
}

/// Secrets come from `DEVSTRIBUTION_SECRET_<NAME>` or a prompt; the launcher uses the
/// system credential store.
fn collect_secrets(names: &[String]) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for name in names {
        let var = format!("DEVSTRIBUTION_SECRET_{}", name.to_uppercase());
        let value = match std::env::var(&var) {
            Ok(v) => v,
            Err(_) => rpassword::prompt_password(format!("Secret `{name}` (or set {var}): "))
                .with_context(|| format!("secret `{name}` needed: set {var}"))?,
        };
        out.insert(name.clone(), json!(value));
    }
    Ok(out)
}

fn print_event(event: RunEvent) {
    match event {
        RunEvent::Progress { value, message } => {
            let pct = value
                .map(|v| format!("{:3.0}% ", v * 100.0))
                .unwrap_or_default();
            eprintln!("[progress] {pct}{message}");
        }
        RunEvent::Log { level, message } => eprintln!("[{level}] {message}"),
        RunEvent::Output { line } => eprintln!("{line}"),
        // The launcher would quarantine the app; in development the run just stops.
        RunEvent::Guard(hit) => match hit.action.as_str() {
            "observe" => eprintln!("[guard] {}: {}", hit.rule, hit.message),
            "block" => eprintln!("[guard] blocked: the app {} ({})", hit.message, hit.detail),
            _ => eprintln!("[guard] STOPPED: the app {} ({})", hit.message, hit.detail),
        },
        RunEvent::Cancelling | RunEvent::Finished { .. } => {}
    }
}

fn report(result: &RunResult, output_dir: &Path, json_output: bool) -> Result<u8> {
    if json_output {
        let status = serde_json::to_value(result.status)?;
        println!(
            "{}",
            json!({"status": status, "outputs": result.outputs, "error": result.error})
        );
    }
    match result.status {
        RunStatus::Ok => {
            if !json_output {
                for (name, value) in &result.outputs {
                    match value {
                        Value::String(s) => println!("{name}: {s}"),
                        other => println!("{name}: {other}"),
                    }
                }
            }
            eprintln!("Done. Result folder: {}", output_dir.display());
            Ok(0)
        }
        RunStatus::Cancelled => {
            eprintln!("Cancelled.");
            Ok(2)
        }
        _ => {
            eprintln!(
                "error: {}",
                result.error.as_deref().unwrap_or("unknown error")
            );
            Ok(1)
        }
    }
}

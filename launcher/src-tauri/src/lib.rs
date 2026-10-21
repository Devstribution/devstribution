//! The launcher backend: Tauri commands over `devstribution-host`. The UI never touches files or
//! processes itself; it asks for actions, and every decision about trust happens here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use devstribution_core::manifest::{Input, Output, Permissions};
use devstribution_host::run::{self, RunEvent};
use devstribution_host::store::{
    InstallStep, InstalledApp, PendingInstall, RunRecord, Stats, Store,
};
use devstribution_host::zone::Origin;
use devstribution_host::{Paths, runner, secrets, uv};
use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    store: Store,
    runner_dir: PathBuf,
    /// Serializes changes to installed apps (state.json, app and environment folders).
    write_lock: Mutex<()>,
    runs: Mutex<HashMap<String, Arc<run::RunHandle>>>,
    /// `.dsapp` files the launcher was started with (file association), until the UI asks.
    pending_files: Mutex<Vec<String>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    // `{:#}` keeps anyhow's context chain: "what we were doing: why it failed".
    format!("{e:#}")
}

#[derive(Serialize)]
struct SecretView {
    name: String,
    set: bool,
}

#[derive(Serialize)]
struct AppView {
    id: String,
    title: String,
    description: Option<String>,
    version: String,
    kind: String,
    namespace: String,
    key_id: String,
    trust: devstribution_host::store::Trust,
    icon: Option<String>,
    inputs: Vec<Input>,
    outputs: Vec<Output>,
    permissions: Permissions,
    requires: Vec<String>,
    installed_at: u64,
    origin: Option<Origin>,
    last_inputs: Map<String, Value>,
    secrets: Vec<SecretView>,
}

impl From<InstalledApp> for AppView {
    fn from(app: InstalledApp) -> AppView {
        let m = app.manifest;
        let id = m.id.to_string();
        let secrets = run::secret_names(&m)
            .into_iter()
            .map(|name| SecretView {
                set: secrets::get(&id, &name).ok().flatten().is_some(),
                name,
            })
            .collect();
        AppView {
            title: m.title,
            description: m.description,
            version: app.record.version,
            kind: m.kind.as_str().into(),
            namespace: m.id.namespace,
            key_id: app.record.key_id,
            trust: app.trust,
            icon: app.icon,
            inputs: m.inputs,
            outputs: m.outputs,
            permissions: m.permissions,
            requires: m.requires,
            installed_at: app.record.installed_at,
            origin: app.record.origin,
            last_inputs: app.record.last_inputs,
            secrets,
            id,
        }
    }
}

#[derive(Serialize)]
struct Overview {
    apps: Vec<AppView>,
    stats: Stats,
    recent_runs: Vec<RunRecord>,
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(err)?
        .map_err(err)
}

fn state(app: &AppHandle) -> State<'_, AppState> {
    app.state::<AppState>()
}

#[tauri::command]
async fn overview(app: AppHandle) -> CmdResult<Overview> {
    blocking(move || {
        let s = state(&app);
        Ok(Overview {
            apps: s
                .store
                .installed()?
                .into_iter()
                .map(AppView::from)
                .collect(),
            stats: s.store.stats(14)?,
            recent_runs: s.store.runs(None, 8)?,
        })
    })
    .await
}

#[tauri::command]
async fn get_app(app: AppHandle, id: String) -> CmdResult<AppView> {
    blocking(move || Ok(state(&app).store.app(&id)?.into())).await
}

#[tauri::command]
async fn inspect_package(app: AppHandle, path: String) -> CmdResult<PendingInstall> {
    blocking(move || state(&app).store.inspect(Path::new(&path))).await
}

#[derive(Clone, Serialize)]
struct StepEvent {
    key: String,
    step: &'static str,
    message: String,
}

fn step_sink(app: &AppHandle, key: &str) -> impl FnMut(InstallStep) + use<> {
    let (app, key) = (app.clone(), key.to_string());
    move |s: InstallStep| {
        let _ = app.emit(
            "install-step",
            StepEvent {
                key: key.clone(),
                step: s.step,
                message: s.message,
            },
        );
    }
}

#[tauri::command]
async fn confirm_install(app: AppHandle, token: String) -> CmdResult<AppView> {
    blocking(move || {
        let s = state(&app);
        let _guard = s.write_lock.lock().unwrap();
        let mut sink = step_sink(&app, &token);
        Ok(s.store.install(&token, &mut sink)?.into())
    })
    .await
}

#[tauri::command]
fn discard_install(app: AppHandle, token: String) {
    state(&app).store.discard(&token);
}

#[tauri::command]
async fn uninstall_app(app: AppHandle, id: String) -> CmdResult<()> {
    blocking(move || {
        let s = state(&app);
        let _guard = s.write_lock.lock().unwrap();
        s.store.uninstall(&id)
    })
    .await
}

#[tauri::command]
async fn repair_app(app: AppHandle, id: String) -> CmdResult<()> {
    blocking(move || {
        let s = state(&app);
        let _guard = s.write_lock.lock().unwrap();
        let mut sink = step_sink(&app, &id);
        s.store.repair(&id, &mut sink)
    })
    .await
}

#[derive(Clone, Serialize)]
struct RunEventPayload {
    run_id: String,
    event: RunEvent,
}

#[tauri::command]
async fn start_run(app: AppHandle, id: String, inputs: Map<String, Value>) -> CmdResult<RunRecord> {
    blocking(move || {
        let s = state(&app);
        let (spec, record) = s.store.prepare_run(&id, &inputs, &s.runner_dir)?;
        let run_id = record.id.clone();
        let sink = {
            let (app, run_id) = (app.clone(), run_id.clone());
            Arc::new(move |event: RunEvent| {
                let _ = app.emit(
                    "run-event",
                    RunEventPayload {
                        run_id: run_id.clone(),
                        event,
                    },
                );
            })
        };
        let handle = Arc::new(run::start(spec, sink)?);
        s.runs
            .lock()
            .unwrap()
            .insert(run_id.clone(), handle.clone());

        // Record the result when the run ends, then tell the UI to refresh its lists.
        let app2 = app.clone();
        let mut finished = record.clone();
        std::thread::spawn(move || {
            while !handle.is_finished() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let s = state(&app2);
            // Drop the table's reference so this thread holds the only one, then collect.
            s.runs.lock().unwrap().remove(&finished.id);
            if let Ok(handle) = Arc::try_unwrap(handle) {
                let result = handle.wait();
                let _ = s.store.finish_run(&mut finished, &result);
            }
            let _ = app2.emit("runs-changed", finished.id.clone());
        });
        Ok(record)
    })
    .await
}

#[tauri::command]
fn cancel_run(app: AppHandle, run_id: String) {
    if let Some(h) = state(&app).runs.lock().unwrap().get(&run_id) {
        h.cancel();
    }
}

#[tauri::command]
async fn list_runs(
    app: AppHandle,
    app_id: Option<String>,
    limit: Option<usize>,
) -> CmdResult<Vec<RunRecord>> {
    blocking(move || {
        state(&app)
            .store
            .runs(app_id.as_deref(), limit.unwrap_or(200))
    })
    .await
}

#[tauri::command]
async fn run_log(app: AppHandle, run_id: String) -> CmdResult<String> {
    blocking(move || {
        let record = state(&app).store.run(&run_id)?;
        let log = record
            .output_dir
            .parent()
            .unwrap_or(&record.output_dir)
            .join("app.log");
        let text = std::fs::read_to_string(log).unwrap_or_default();
        let start = text.len().saturating_sub(200_000);
        let start = (start..text.len())
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(0);
        Ok(text[start..].to_string())
    })
    .await
}

/// Opens a run's result (a file it produced, or its output folder). Only paths inside that
/// run's output folder can be opened: a compromised page cannot launch arbitrary files.
#[tauri::command]
async fn open_output(
    app: AppHandle,
    run_id: String,
    path: Option<String>,
    reveal: bool,
) -> CmdResult<()> {
    blocking(move || {
        let record = state(&app).store.run(&run_id)?;
        let target = path
            .map(PathBuf::from)
            .unwrap_or_else(|| record.output_dir.clone());
        if !run::is_inside(&target, &record.output_dir) {
            anyhow::bail!("this file is not a result of the run");
        }
        let opener = app.opener();
        if reveal {
            opener.reveal_item_in_dir(&target)?;
        } else {
            opener.open_path(target.to_string_lossy(), None::<&str>)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_secret(app: AppHandle, id: String, name: String, value: String) -> CmdResult<()> {
    blocking(move || {
        let installed = state(&app).store.app(&id)?;
        if !run::secret_names(&installed.manifest).contains(&name) {
            anyhow::bail!("`{name}` is not a secret this app declares");
        }
        secrets::set(&id, &name, &value)
    })
    .await
}

#[tauri::command]
async fn clear_secret(app: AppHandle, id: String, name: String) -> CmdResult<()> {
    blocking(move || {
        let installed = state(&app).store.app(&id)?;
        if !run::secret_names(&installed.manifest).contains(&name) {
            anyhow::bail!("`{name}` is not a secret this app declares");
        }
        secrets::delete(&id, &name)
    })
    .await
}

#[derive(Serialize)]
struct PinView {
    namespace: String,
    key_id: String,
    pinned_at: u64,
    source: String,
}

#[derive(Serialize)]
struct SettingsView {
    version: &'static str,
    data_dir: String,
    platform: &'static str,
    mode: &'static str,
    uv_version: Option<String>,
    publishers: Vec<PinView>,
}

#[tauri::command]
async fn settings(app: AppHandle) -> CmdResult<SettingsView> {
    blocking(move || {
        let s = state(&app);
        Ok(SettingsView {
            version: env!("CARGO_PKG_VERSION"),
            data_dir: s.store.paths.root.display().to_string(),
            platform: devstribution_host::store::current_platform(),
            mode: "free",
            uv_version: uv::version(),
            publishers: s
                .store
                .pins()?
                .into_iter()
                .map(|(namespace, p)| PinView {
                    namespace,
                    key_id: p.key_id,
                    pinned_at: p.pinned_at,
                    source: p.source,
                })
                .collect(),
        })
    })
    .await
}

#[tauri::command]
fn take_pending_files(app: AppHandle) -> Vec<String> {
    std::mem::take(&mut *state(&app).pending_files.lock().unwrap())
}

fn dsapp_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    args.into_iter()
        .filter(|a| a.to_ascii_lowercase().ends_with(".dsapp"))
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = Paths::from_env().expect("local data folder");
    let store = Store::open(paths).expect("opening the devstribution data folder");
    let runner_dir = runner::install(&store.paths).expect("installing the runner");

    tauri::Builder::default()
        // A second launch (double-click on another .dsapp) hands its files to this window.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let files = dsapp_args(args.into_iter().skip(1));
            if !files.is_empty() {
                let _ = app.emit("open-files", files);
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            store,
            runner_dir,
            write_lock: Mutex::new(()),
            runs: Mutex::new(HashMap::new()),
            pending_files: Mutex::new(dsapp_args(std::env::args().skip(1))),
        })
        .invoke_handler(tauri::generate_handler![
            overview,
            get_app,
            inspect_package,
            confirm_install,
            discard_install,
            uninstall_app,
            repair_app,
            start_run,
            cancel_run,
            list_runs,
            run_log,
            open_output,
            set_secret,
            clear_secret,
            settings,
            take_pending_files,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the launcher");
}

//! The launcher backend: Tauri commands over `devstribution-host`. The UI never touches files or
//! processes itself; it asks for actions, and every decision about trust happens here.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use devstribution_core::manifest::{Input, Output, Permissions};
use devstribution_host::policy::{self, Policy, ScanDanger};
use devstribution_host::run::{self, RunEvent};
use devstribution_host::store::{
    CleanupPlan, CleanupResult, InstallStep, InstalledApp, JournalReport, PendingInstall,
    Quarantine, RunRecord, Stats, Store,
};
use devstribution_host::zone::Origin;
use devstribution_host::{Paths, runner, secrets, uv};
use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    store: Store,
    /// Serializes changes to installed apps (state.json, app and environment folders).
    write_lock: Mutex<()>,
    runs: Mutex<HashMap<String, Arc<run::RunHandle>>>,
    /// `.dsapp` files the launcher was started with (file association), until the UI asks.
    pending_files: Mutex<Vec<String>>,
}

type CmdResult<T> = Result<T, String>;

/// Locks a mutex even if a thread panicked while holding it: the data it guards is only a
/// marker or a table of handles, and refusing every later install until a restart would be
/// worse.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// File types Windows runs, or that point somewhere else, when "opened": a result of one of
/// these types is shown in its folder instead.
const RUNNABLE: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "msi",
    "msp",
    "msc",
    "scr",
    "cpl",
    "lnk",
    "url",
    "pif",
    "reg",
    "jar",
    "appref-ms",
    "application",
    "settingcontent-ms",
    "library-ms",
    "website",
    "py",
    "pyw",
    "pyz",
];

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
    quarantine: Option<Quarantine>,
    guard_allow: Vec<String>,
    /// Whether the user may let the app run past a guard rule that stopped it (managed mode
    /// can keep that decision for IT).
    may_allow_guard_rules: bool,
}

impl AppView {
    fn new(app: InstalledApp, store: &Store) -> AppView {
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
            quarantine: app.record.quarantine,
            may_allow_guard_rules: store.users_may_allow_guard_rules(),
            guard_allow: app.record.guard_allow,
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
                .map(|a| AppView::new(a, &s.store))
                .collect(),
            // A month: the dashboard shows its last 14 days, "often" uses all of it.
            stats: s.store.stats(30)?,
            recent_runs: s.store.runs(None, 8)?,
        })
    })
    .await
}

#[tauri::command]
async fn get_app(app: AppHandle, id: String) -> CmdResult<AppView> {
    blocking(move || {
        let store = &state(&app).store;
        Ok(AppView::new(store.app(&id)?, store))
    })
    .await
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
        let _guard = lock(&s.write_lock);
        let mut sink = step_sink(&app, &token);
        Ok(AppView::new(s.store.install(&token, &mut sink)?, &s.store))
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
        let _guard = lock(&s.write_lock);
        s.store.uninstall(&id)
    })
    .await
}

#[tauri::command]
async fn repair_app(app: AppHandle, id: String) -> CmdResult<()> {
    blocking(move || {
        let s = state(&app);
        let _guard = lock(&s.write_lock);
        let mut sink = step_sink(&app, &id);
        s.store.repair(&id, &mut sink)
    })
    .await
}

/// Ends an app's quarantine after the runtime guard stopped it: `allow` lets the rule that
/// stopped it pass from now on ("Allow and run again"); without, runs are checked as before.
#[tauri::command]
async fn release_quarantine(app: AppHandle, id: String, allow: bool) -> CmdResult<AppView> {
    blocking(move || {
        let s = state(&app);
        let _guard = lock(&s.write_lock);
        Ok(AppView::new(
            s.store.release_quarantine(&id, allow)?,
            &s.store,
        ))
    })
    .await
}

#[derive(Clone, Serialize)]
struct RunEventsPayload {
    run_id: String,
    events: Vec<RunEvent>,
}

/// Sends a run's events to the window in batches, at most every 100 ms, so an app printing
/// thousands of lines a second cannot flood the window with messages.
fn batched_sink(app: &AppHandle, run_id: &str) -> run::EventSink {
    let (tx, rx) = std::sync::mpsc::channel::<RunEvent>();
    let (app, run_id) = (app.clone(), run_id.to_string());
    std::thread::spawn(move || {
        let mut done = false;
        while !done {
            let Ok(first) = rx.recv() else { break };
            let mut events = vec![first];
            let until = std::time::Instant::now() + Duration::from_millis(100);
            loop {
                done = matches!(events.last(), Some(RunEvent::Finished { .. }));
                let left = until.saturating_duration_since(std::time::Instant::now());
                if done || left.is_zero() {
                    break;
                }
                match rx.recv_timeout(left) {
                    Ok(e) => events.push(e),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        done = true;
                        break;
                    }
                }
            }
            let _ = app.emit(
                "run-events",
                RunEventsPayload {
                    run_id: run_id.clone(),
                    events,
                },
            );
        }
    });
    let tx = Mutex::new(tx);
    Arc::new(move |event: RunEvent| {
        let _ = lock(&tx).send(event);
    })
}

#[tauri::command]
async fn start_run(app: AppHandle, id: String, inputs: Map<String, Value>) -> CmdResult<RunRecord> {
    blocking(move || {
        let s = state(&app);
        // The runner is checked against the copy built into the launcher before every run,
        // not once at start: anything the user runs can write the data folder in between.
        let runner_dir = runner::install(&s.store.paths)?;
        let (spec, record) = s.store.prepare_run(&id, &inputs, &runner_dir)?;
        let run_id = record.id.clone();
        let sink = batched_sink(&app, &run_id);
        let handle = match run::start(spec, sink) {
            Ok(h) => Arc::new(h),
            Err(e) => {
                let mut failed = record.clone();
                let _ = s.store.finish_run(
                    &mut failed,
                    &run::RunResult {
                        status: run::RunStatus::Error,
                        outputs: Map::new(),
                        error: Some(format!("{e:#}")),
                        traceback: None,
                        duration_ms: 0,
                        guard: Vec::new(),
                    },
                );
                return Err(e);
            }
        };
        lock(&s.runs).insert(run_id.clone(), handle.clone());

        // Record the result when the run ends, then tell the UI to refresh its lists.
        let app2 = app.clone();
        let mut finished = record.clone();
        std::thread::spawn(move || {
            while !handle.is_finished() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let s = state(&app2);
            // Drop the table's reference so this thread holds the only one, then collect.
            lock(&s.runs).remove(&finished.id);
            if let Ok(handle) = Arc::try_unwrap(handle) {
                let result = handle.wait();
                // Rare (disk full, a file held by an antivirus), and the user must hear of it:
                // a guard stop that is not recorded only holds while the launcher stays open.
                if let Err(e) = s.store.finish_run(&mut finished, &result) {
                    let _ = app2.emit("run-not-recorded", format!("{e:#}"));
                }
            }
            let _ = app2.emit("runs-changed", finished.id.clone());
        });
        Ok(record)
    })
    .await
}

#[tauri::command]
fn cancel_run(app: AppHandle, run_id: String) {
    if let Some(h) = lock(&state(&app).runs).get(&run_id) {
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
/// The end of a run's log; `None` when no log file exists (an empty one is `Some("")`).
async fn run_log(app: AppHandle, run_id: String) -> CmdResult<Option<String>> {
    blocking(move || {
        let record = state(&app).store.run(&run_id)?;
        let log = record
            .output_dir
            .parent()
            .unwrap_or(&record.output_dir)
            .join("app.log");
        // The end of the log only, decoded leniently: it can be large, and not all UTF-8.
        let Ok(mut file) = std::fs::File::open(log) else {
            return Ok(None);
        };
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(200_000)))?;
        let mut bytes = Vec::new();
        file.take(200_000).read_to_end(&mut bytes)?;
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    })
    .await
}

/// Opens a run's result: its output folder, or a file the run returned as a result. Nothing
/// else can be opened, whatever the page asks: the path must be one of the run's recorded
/// results and resolve inside its output folder or a folder chosen for the run. With no file
/// given, it opens the output folder, or the chosen folder when the app left its own empty.
/// Files Windows would run (programs, scripts, shortcuts) are only shown in their folder.
#[tauri::command]
async fn open_output(
    app: AppHandle,
    run_id: String,
    path: Option<String>,
    reveal: bool,
) -> CmdResult<()> {
    blocking(move || {
        let record = state(&app).store.run(&run_id)?;
        if record.output_removed.is_some() {
            return Err(anyhow::anyhow!(
                "this run's results were removed by a cleanup"
            ));
        }
        // `dunce` gives `C:\...` rather than `\\?\C:\...`, which Explorer cannot always open.
        let output_dir = dunce::canonicalize(&record.output_dir)?;
        let target = match path {
            // An app that works in a folder the user chose usually leaves its own output folder
            // empty: then the result is in the chosen folder, so open that one.
            None if is_empty_dir(&output_dir) => record
                .result_folders
                .iter()
                .skip(1)
                .find_map(|dir| dunce::canonicalize(dir).ok().filter(|d| d.is_dir()))
                .unwrap_or_else(|| output_dir.clone()),
            None => output_dir.clone(),
            Some(p) => {
                if !record
                    .outputs
                    .values()
                    .any(|v| v.as_str() == Some(p.as_str()))
                {
                    anyhow::bail!("this file is not a result of the run");
                }
                // The resolved path is the one opened, so it cannot change after the check.
                let resolved = dunce::canonicalize(&p)?;
                let allowed = std::iter::once(&record.output_dir)
                    .chain(&record.result_folders)
                    .filter_map(|dir| dunce::canonicalize(dir).ok())
                    .any(|dir| resolved.starts_with(dir));
                if !allowed || !resolved.is_file() {
                    anyhow::bail!("this file is not a result of the run");
                }
                resolved
            }
        };
        let runnable = target
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| RUNNABLE.iter().any(|r| r.eq_ignore_ascii_case(e)));
        let opener = app.opener();
        if reveal || (runnable && target != output_dir) {
            opener.reveal_item_in_dir(&target)?;
        } else {
            opener.open_path(target.to_string_lossy(), None::<&str>)?;
        }
        Ok(())
    })
    .await
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
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
struct AuthorView {
    namespace: String,
    name: Option<String>,
    fingerprints: Vec<String>,
}

/// Managed mode's policy, as Settings shows it.
#[derive(Serialize)]
struct PolicyView {
    source: String,
    authors: Vec<AuthorView>,
    contact: Option<String>,
    dry_run: bool,
    scan_danger: ScanDanger,
    users_may_allow_guard_rules: bool,
    guard_report_only: Vec<String>,
    min_launcher_version: Option<String>,
    problems: Vec<String>,
    log_file: String,
}

impl PolicyView {
    fn new(p: &Policy, store: &Store) -> PolicyView {
        PolicyView {
            source: p.source.clone(),
            authors: p
                .authors
                .iter()
                .map(|a| AuthorView {
                    namespace: a.namespace.clone(),
                    name: a.name.clone(),
                    fingerprints: a.fingerprints(),
                })
                .collect(),
            contact: p.contact.clone(),
            dry_run: p.dry_run,
            scan_danger: p.scan_danger,
            users_may_allow_guard_rules: p.users_may_allow_guard_rules,
            guard_report_only: p.guard_report_only.clone(),
            min_launcher_version: p.min_launcher_version.clone(),
            problems: p.problems.clone(),
            log_file: store.policy_log().display().to_string(),
        }
    }
}

#[derive(Serialize)]
struct SettingsView {
    version: &'static str,
    data_dir: String,
    platform: &'static str,
    mode: &'static str,
    policy: Option<PolicyView>,
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
            mode: if s.store.policy().is_some() {
                "managed"
            } else {
                "free"
            },
            policy: s.store.policy().map(|p| PolicyView::new(p, &s.store)),
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

/// The text of "Copy diagnostics" (see `Store::diagnostics`).
#[tauri::command]
async fn diagnostics(app: AppHandle) -> CmdResult<String> {
    blocking(move || {
        Ok(state(&app)
            .store
            .diagnostics(env!("CARGO_PKG_VERSION"), uv::version().as_deref()))
    })
    .await
}

/// Saves a `.trust` file for the author of a package the policy refused, for the user to send
/// to IT ("ask your IT to trust X"). The backend asks where to save it, so the page cannot
/// choose a path; returns whether a file was written.
#[tauri::command]
async fn save_trust_request(app: AppHandle, token: String) -> CmdResult<bool> {
    blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let pending = state(&app).store.pending(&token)?;
        let text = policy::trust_request(&pending.namespace, &pending.publisher_key)?;
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Save a request for your IT department")
            .set_file_name(format!("{}.trust", pending.namespace))
            .add_filter("Trust file", &["trust"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let mut path = path.into_path()?;
        if path
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("trust"))
        {
            path.set_extension("trust");
        }
        std::fs::write(&path, text)?;
        app.opener().reveal_item_in_dir(&path)?;
        Ok(true)
    })
    .await
}

/// Opens devstribution's data folder in the file manager, for support and for IT. It takes no
/// path: the only folder it can open is the launcher's own root, whatever the page asks.
/// What a cleanup would remove: nothing is deleted by asking.
#[tauri::command]
async fn cleanup_preview(
    app: AppHandle,
    older_than_days: Option<u32>,
    app_id: Option<String>,
    remove_records: bool,
) -> CmdResult<CleanupPlan> {
    blocking(move || {
        state(&app)
            .store
            .cleanup_plan(older_than_days, app_id.as_deref(), remove_records)
    })
    .await
}

/// Removes the results of the runs the user confirmed, by id: the page never names a path.
#[tauri::command]
async fn cleanup_runs(
    app: AppHandle,
    run_ids: Vec<String>,
    remove_records: bool,
) -> CmdResult<CleanupResult> {
    blocking(move || state(&app).store.cleanup(&run_ids, remove_records)).await
}

#[tauri::command]
async fn cleanup_log(app: AppHandle) -> CmdResult<JournalReport> {
    blocking(move || Ok(state(&app).store.journal_report())).await
}

#[tauri::command]
async fn open_data_folder(app: AppHandle) -> CmdResult<()> {
    blocking(move || {
        let root = dunce::canonicalize(&state(&app).store.paths.root)?;
        app.opener()
            .open_path(root.to_string_lossy(), None::<&str>)?;
        Ok(())
    })
    .await
}

/// Starts the installer's own uninstaller (`uninstall.exe` next to the launcher), which asks
/// whether to delete app data, then closes the launcher so its files can be removed. Running
/// apps end with it. A launcher that was not installed by the setup program has no uninstaller.
#[tauri::command]
fn uninstall_launcher(app: AppHandle) -> CmdResult<()> {
    let uninstaller = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("uninstall.exe");
    if !cfg!(windows) || !uninstaller.is_file() {
        return Err(
            "This copy of devstribution was not installed with the setup program, so \
                    there is no uninstaller: delete its folder instead."
                .into(),
        );
    }
    std::process::Command::new(&uninstaller)
        .spawn()
        .map_err(|e| format!("Could not start the uninstaller: {e}"))?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn take_pending_files(app: AppHandle) -> Vec<String> {
    std::mem::take(&mut *lock(&state(&app).pending_files))
}

/// The packages among the arguments, made absolute against `cwd` (a second launch from a
/// terminal has its own working folder). Arguments that are not valid Unicode are skipped.
/// `.zip` counts too: browsers, mail and file sharing often rename a downloaded `.dsapp` (a zip
/// inside), and "Open with" then passes the renamed file. Its content is checked the same way.
fn dsapp_args(args: impl IntoIterator<Item = OsString>, cwd: &Path) -> Vec<String> {
    args.into_iter()
        .filter_map(|a| a.into_string().ok())
        .filter(|a| {
            let a = a.to_ascii_lowercase();
            a.ends_with(".dsapp") || a.ends_with(".zip")
        })
        .map(|a| cwd.join(a).to_string_lossy().into_owned())
        .collect()
}

/// Removes the secrets of every installed app from the credential store. The uninstaller runs
/// this (`devstribution.exe --forget-secrets`) when the user asks to delete app data, because
/// the credential store is outside the folders the uninstaller removes.
fn forget_secrets() {
    let Ok(paths) = Paths::from_env() else { return };
    let Ok(store) = Store::open(paths) else {
        return;
    };
    for app in store.installed().unwrap_or_default() {
        let id = app.manifest.id.to_string();
        for name in run::secret_names(&app.manifest) {
            let _ = secrets::delete(&id, &name);
        }
    }
}

/// Makes Windows load DLLs only from System32 and the launcher's own folder, never from the
/// current folder or PATH, where anyone could place a DLL for the signed launcher to load.
#[cfg(windows)]
fn restrict_dll_search() {
    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, SetDefaultDllDirectories,
    };
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) };
}

#[cfg(not(windows))]
fn restrict_dll_search() {}

/// The data folder, the runner, and the uv to use: set up once the launcher knows it is the
/// only one running (a second launch must not touch the first one's data).
fn open_store() -> anyhow::Result<Store> {
    let paths = Paths::from_env()?;
    paths.migrate_old_location()?;
    if !cfg!(debug_assertions) {
        // A release launcher uses only the uv it ships, which was pinned and hash-checked when
        // it was built; neither DEVSTRIBUTION_UV nor PATH can replace it.
        let exe = std::env::current_exe()?;
        let uv = exe
            .parent()
            .unwrap_or(Path::new("."))
            .join(if cfg!(windows) { "uv.exe" } else { "uv" });
        anyhow::ensure!(
            uv.is_file(),
            "the launcher's uv is missing: reinstall devstribution"
        );
        uv::use_only(uv);
    }
    let store = Store::open(paths)?;
    // Here too, so a data folder the launcher cannot write fails at start.
    runner::install(&store.paths)?;
    Ok(store)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    restrict_dll_search();
    if std::env::args_os().any(|a| a == "--forget-secrets") {
        forget_secrets();
        return;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let initial_files = dsapp_args(std::env::args_os().skip(1), &cwd);

    let result = tauri::Builder::default()
        // First: a second launch (double-click on another .dsapp) hands its files to this
        // window and exits before anything else happens.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let files = dsapp_args(
                args.into_iter().skip(1).map(OsString::from),
                Path::new(&cwd),
            );
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
        .setup(move |app| match open_store() {
            Ok(store) => {
                app.manage(AppState {
                    store,
                    write_lock: Mutex::new(()),
                    runs: Mutex::new(HashMap::new()),
                    pending_files: Mutex::new(initial_files),
                });
                Ok(())
            }
            Err(e) => {
                use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                app.dialog()
                    .message(format!("devstribution cannot start.\n\n{e:#}"))
                    .kind(MessageDialogKind::Error)
                    .title("devstribution")
                    .blocking_show();
                Err(e.into())
            }
        })
        .invoke_handler(tauri::generate_handler![
            overview,
            get_app,
            inspect_package,
            confirm_install,
            discard_install,
            uninstall_app,
            repair_app,
            release_quarantine,
            start_run,
            cancel_run,
            list_runs,
            run_log,
            open_output,
            open_data_folder,
            cleanup_preview,
            cleanup_runs,
            cleanup_log,
            diagnostics,
            set_secret,
            clear_secret,
            settings,
            uninstall_launcher,
            take_pending_files,
            save_trust_request,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("devstribution: {e}");
        std::process::exit(1);
    }
}

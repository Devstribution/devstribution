//! Installed apps and their state: the install flow (stage → confirm → environment → ready),
//! pinned publisher keys, anti-downgrade, uninstall, repair, and the history of runs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use devstribution_core::manifest::{AppId, Kind, Manifest, Version};
use devstribution_core::package::{
    self, Limits, PYPROJECT, SignatureStatus, VerifiedPackage, VerifyOptions,
};
use devstribution_core::scan::{self, ScanReport};
use devstribution_core::signing::{parse_public_key, public_key_base64, public_key_id};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::paths::Paths;
use crate::run::{RunResult, RunSpec, RunStatus};
use crate::time::{date, now_secs, random_hex, stamp};
use crate::uv::{self, LockMode};
use crate::zone::{self, Origin};

/// A publisher key pinned for a namespace: later packages must be signed by it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pin {
    pub key: String,
    pub key_id: String,
    pub pinned_at: u64,
    /// How the key became trusted: "first-use" for now; "catalog" and "github" in Phase 2.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRecord {
    pub id: String,
    pub version: String,
    pub content_hash: String,
    /// Full SHA-256 of the content (records from before it existed have it empty).
    #[serde(default)]
    pub content_sha256: String,
    pub key_id: String,
    pub installed_at: u64,
    pub origin: Option<Origin>,
    /// Last values typed in the form (never secrets), to prefill the next run.
    #[serde(default)]
    pub last_inputs: Map<String, Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    apps: BTreeMap<String, AppRecord>,
    #[serde(default)]
    pins: BTreeMap<String, Pin>,
    /// Highest version ever installed per app: older packages are refused (anti-downgrade).
    #[serde(default)]
    highest: BTreeMap<String, String>,
}

/// What the UI shows for an installed app.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub record: AppRecord,
    pub manifest: Manifest,
    pub icon: Option<String>,
    pub trust: Trust,
    #[serde(skip)]
    pub app_dir: PathBuf,
    #[serde(skip)]
    pub env_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// Key pinned on first install: files are intact and from the same key as before, but the
    /// publisher's name is not verified by a catalog or download source ("unverified name").
    FirstUse,
    /// First time this publisher's key is seen.
    NewPublisher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallAction {
    Install,
    Update,
    Reinstall,
}

/// A verified package waiting for the user's confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingInstall {
    pub token: String,
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub version: String,
    pub kind: String,
    pub namespace: String,
    pub key_id: String,
    pub trust: String,
    pub icon: Option<String>,
    pub permissions: devstribution_core::manifest::Permissions,
    pub requires: Vec<String>,
    pub platforms: Vec<String>,
    pub platform_supported: bool,
    pub input_count: usize,
    pub file_count: usize,
    pub size_bytes: u64,
    pub origin: Option<Origin>,
    pub installed_version: Option<String>,
    pub action: String,
    pub warnings: Vec<String>,
    /// What the code scan found in the verified files (`devstribution_core::scan`).
    #[serde(default)]
    pub scan: ScanReport,
    // Kept for the install step, not shown.
    pub publisher_key: String,
    pub content_hash: String,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallStep {
    pub step: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub app_id: String,
    pub app_title: String,
    pub version: String,
    pub kind: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: RunStatus,
    pub duration_ms: u64,
    #[serde(default)]
    pub outputs: Map<String, Value>,
    pub error: Option<String>,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayCount {
    pub date: String,
    pub ok: u32,
    pub error: u32,
    pub cancelled: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppCount {
    pub app_id: String,
    pub title: String,
    pub runs: u32,
}

/// How an app has been used: what the launcher's home ("recent", "often") and sorting read.
#[derive(Debug, Clone, Serialize)]
pub struct AppUsage {
    pub app_id: String,
    /// Finished runs within the stats window.
    pub runs: u32,
    /// Start of the latest run, whenever it was.
    pub last_run_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub apps_installed: usize,
    pub runs: u32,
    pub ok: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub success_rate: Option<f64>,
    pub total_duration_ms: u64,
    pub per_day: Vec<DayCount>,
    pub top_apps: Vec<AppCount>,
    /// Every app that ever ran, latest first.
    pub usage: Vec<AppUsage>,
}

pub fn current_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub struct Store {
    pub paths: Paths,
    /// Held for every read-modify-write of `state.json`, so that an install confirmed while
    /// a run starts (or two installs) cannot overwrite each other's changes.
    state_lock: Mutex<()>,
}

impl Store {
    pub fn open(paths: Paths) -> Result<Store> {
        for dir in [
            paths.apps(),
            paths.envs(),
            paths.runs(),
            paths.staging(),
            paths.state(),
        ] {
            fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        // Packages left on the confirmation screen by a previous session.
        if let Ok(entries) = fs::read_dir(paths.staging()) {
            for e in entries.flatten() {
                let _ = fs::remove_dir_all(e.path());
            }
        }
        let store = Store {
            paths,
            state_lock: Mutex::new(()),
        };
        store.close_interrupted_runs();
        Ok(store)
    }

    /// Runs still marked as running belong to a launcher that ended during the run (its Job
    /// Object then stopped the app): record them as stopped.
    fn close_interrupted_runs(&self) {
        let Ok(runs) = self.runs(None, 50) else {
            return;
        };
        for mut r in runs.into_iter().filter(|r| r.status == RunStatus::Running) {
            r.status = RunStatus::Cancelled;
            r.error = Some("the launcher closed during the run".into());
            r.finished_at = Some(now_secs());
            let _ = self.save_run(&r);
        }
    }

    fn state_file(&self) -> PathBuf {
        self.paths.state().join("state.json")
    }

    fn load(&self) -> Result<State> {
        match fs::read(self.state_file()) {
            Ok(data) => serde_json::from_slice(&data).context("reading state.json"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e).context("reading state.json"),
        }
    }

    fn save(&self, state: &State) -> Result<()> {
        let path = self.state_file();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
        fs::rename(&tmp, &path).context("saving state.json")
    }

    /// Loads the state, lets `f` change it and saves it, all under the state lock. Nothing is
    /// saved if `f` fails.
    fn update_state<T>(&self, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let _guard = self
            .state_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self.load()?;
        let out = f(&mut state)?;
        self.save(&state)?;
        Ok(out)
    }

    /// Whether a run of the app is in progress (its record says so).
    pub fn is_running(&self, id: &str) -> bool {
        self.runs(Some(id), 20)
            .is_ok_and(|runs| runs.iter().any(|r| r.status == RunStatus::Running))
    }

    fn refuse_if_running(&self, id: &str, what: &str) -> Result<()> {
        if self.is_running(id) {
            bail!("`{id}` is running: stop it before you {what} it");
        }
        Ok(())
    }

    // -- installed apps ----------------------------------------------------------------------

    pub fn installed(&self) -> Result<Vec<InstalledApp>> {
        let state = self.load()?;
        let mut apps: Vec<InstalledApp> = state
            .apps
            .values()
            .filter_map(|r| self.describe(r, &state).ok())
            .collect();
        apps.sort_by(|a, b| {
            a.manifest
                .title
                .to_lowercase()
                .cmp(&b.manifest.title.to_lowercase())
        });
        Ok(apps)
    }

    pub fn app(&self, id: &str) -> Result<InstalledApp> {
        let state = self.load()?;
        let record = state
            .apps
            .get(id)
            .ok_or_else(|| anyhow!("`{id}` is not installed"))?;
        self.describe(record, &state)
    }

    fn describe(&self, record: &AppRecord, state: &State) -> Result<InstalledApp> {
        let id: AppId = record.id.parse().map_err(|e: String| anyhow!(e))?;
        let app_dir = self.paths.app_dir(&id, &record.content_hash);
        let manifest = Manifest::parse(&fs::read_to_string(app_dir.join(PYPROJECT))?)?;
        let pin = state.pins.get(&id.namespace);
        let trust = if pin.is_some() {
            Trust::FirstUse
        } else {
            Trust::NewPublisher
        };
        let mut record = record.clone();
        // Records and pins from older versions hold minisign's key number; show the fingerprint.
        if let Some(fp) = pin.and_then(|p| fingerprint_of(&p.key)) {
            record.key_id = fp;
        }
        Ok(InstalledApp {
            icon: icon_data_url(&app_dir, &manifest),
            env_dir: self.paths.env_dir(&id, &record.content_hash),
            record,
            manifest,
            trust,
            app_dir,
        })
    }

    // -- install flow ------------------------------------------------------------------------

    /// Verifies a package into the staging area and describes it for the confirmation screen.
    pub fn inspect(&self, package_path: &Path) -> Result<PendingInstall> {
        let origin = zone::origin_of(package_path);
        let state = self.load()?;
        let token = random_hex(12);
        let stage = self.paths.staging().join(&token);
        fs::create_dir_all(&stage)?;
        let result = self.inspect_into(package_path, &stage, &token, origin, &state);
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }

    fn inspect_into(
        &self,
        package_path: &Path,
        stage: &Path,
        token: &str,
        origin: Option<Origin>,
        state: &State,
    ) -> Result<PendingInstall> {
        let size_bytes = fs::metadata(package_path)
            .with_context(|| format!("cannot open {}", package_path.display()))?
            .len();
        // The package is read exactly once: the publisher's key and the app id checked below
        // are those of the very files that get installed. (Reading it a second time to check
        // against the pinned key would let a file swapped in between pass as another app.)
        let opts = VerifyOptions {
            trusted_key: None,
            limits: Limits::default(),
        };
        let verified: VerifiedPackage =
            package::extract_and_verify(package_path, &stage.join("app"), &opts)?;
        let m = &verified.manifest;
        let id = m.id.to_string();
        if !m.resources.is_empty() {
            bail!(
                "this app needs downloadable resources, which this version of the launcher does not support yet"
            );
        }
        let (action, trust) = admit(
            state,
            &m.id,
            &m.version,
            &verified.publisher_key,
            &verified.content_sha256,
            &verified.content_hash,
        )?;
        let installed = state.apps.get(&id);
        let key_id = match &verified.signature {
            SignatureStatus::Trusted { key_id } | SignatureStatus::NewKey { key_id } => {
                key_id.clone()
            }
        };
        let trust = match trust {
            Trust::FirstUse => "first_use",
            Trust::NewPublisher => "new_publisher",
        };
        let mut warnings = Vec::new();
        if trust == "new_publisher" {
            warnings.push(format!(
                "First app from `{}` on this computer. Its name is not verified: install it only if \
                 you trust where you got it. Future versions must come from the same key.",
                m.id.namespace
            ));
        }
        if origin.as_ref().is_some_and(Origin::is_internet) {
            warnings.push("This file was downloaded from the internet.".into());
        }
        let platform_supported = m.platforms.iter().any(|p| p == current_platform());
        if !platform_supported {
            warnings.push(format!(
                "The publisher does not support {} (only {}).",
                current_platform(),
                m.platforms.join(", ")
            ));
        }

        let scan = scan::scan(&stage.join("app"), verified.files.paths(), &m.permissions);

        let pending = PendingInstall {
            token: token.to_string(),
            id,
            title: m.title.clone(),
            description: m.description.clone(),
            version: m.version.to_string(),
            kind: m.kind.as_str().to_string(),
            namespace: m.id.namespace.clone(),
            key_id,
            trust: trust.to_string(),
            icon: icon_data_url(&stage.join("app"), m),
            permissions: m.permissions.clone(),
            requires: m.requires.clone(),
            platforms: m.platforms.clone(),
            platform_supported,
            input_count: m.inputs.len(),
            file_count: verified.files.len(),
            size_bytes,
            origin,
            installed_version: installed.map(|r| r.version.clone()),
            action: serde_json::to_value(action)?
                .as_str()
                .unwrap_or("install")
                .to_string(),
            warnings,
            scan,
            publisher_key: verified.publisher_key.clone(),
            content_hash: verified.content_hash.clone(),
            content_sha256: verified.content_sha256.clone(),
        };
        fs::write(stage.join("pending.json"), serde_json::to_vec(&pending)?)?;
        Ok(pending)
    }

    pub fn discard(&self, token: &str) {
        if is_token(token) {
            let _ = fs::remove_dir_all(self.paths.staging().join(token));
        }
    }

    /// Installs a staged package after the user confirmed it.
    pub fn install(
        &self,
        token: &str,
        on_step: &mut dyn FnMut(InstallStep),
    ) -> Result<InstalledApp> {
        if !is_token(token) {
            bail!("invalid install token");
        }
        let stage = self.paths.staging().join(token);
        let pending: PendingInstall = serde_json::from_slice(
            &fs::read(stage.join("pending.json")).context("this install is no longer pending")?,
        )?;
        let id: AppId = pending.id.parse().map_err(|e: String| anyhow!(e))?;
        let version: Version = pending.version.parse().map_err(|e: String| anyhow!(e))?;
        // The checks of the confirmation screen again, against the state as it is now: another
        // install confirmed in the meantime may have pinned a different key for this publisher
        // or installed a newer version.
        admit(
            &self.load()?,
            &id,
            &version,
            &pending.publisher_key,
            &pending.content_sha256,
            &pending.content_hash,
        )
        .context("this install is no longer valid")?;
        self.refuse_if_running(&pending.id, "update")?;
        let app_dir = self.paths.app_dir(&id, &pending.content_hash);
        let env_dir = self.paths.env_dir(&id, &pending.content_hash);
        // Installed before with the same content (a reinstall): its files are already there.
        let same_content_installed = self.load()?.apps.get(&pending.id).map(|r| &r.content_hash)
            == Some(&pending.content_hash);

        on_step(InstallStep {
            step: "files",
            message: "Copying the verified files".into(),
        });
        if app_dir.exists() {
            fs::remove_dir_all(&app_dir)?;
        }
        fs::create_dir_all(app_dir.parent().expect("has a parent"))?;
        fs::rename(stage.join("app"), &app_dir).context("moving the app into place")?;
        let _ = fs::remove_dir_all(&stage);

        on_step(InstallStep {
            step: "environment",
            message: "Preparing the Python environment".into(),
        });
        let mut forward = |line: &str| {
            on_step(InstallStep {
                step: "environment",
                message: line.to_string(),
            })
        };
        if let Err(e) = build_env(&app_dir, &env_dir, &mut forward) {
            // The installed version, if any, is untouched: only new files go.
            if !same_content_installed {
                let _ = fs::remove_dir_all(&app_dir);
            }
            return Err(e);
        }

        let replaced = self
            .update_state(|state| {
                admit(
                    state,
                    &id,
                    &version,
                    &pending.publisher_key,
                    &pending.content_sha256,
                    &pending.content_hash,
                )
                .context("this install is no longer valid")?;
                let last_inputs = state
                    .apps
                    .get(&pending.id)
                    .map(|r| r.last_inputs.clone())
                    .unwrap_or_default();
                state
                    .pins
                    .entry(id.namespace.clone())
                    .or_insert_with(|| Pin {
                        key: pending.publisher_key.clone(),
                        key_id: pending.key_id.clone(),
                        pinned_at: now_secs(),
                        source: "first-use".into(),
                    });
                let replaced = state.apps.insert(
                    pending.id.clone(),
                    AppRecord {
                        id: pending.id.clone(),
                        version: pending.version.clone(),
                        content_hash: pending.content_hash.clone(),
                        content_sha256: pending.content_sha256.clone(),
                        key_id: pending.key_id.clone(),
                        installed_at: now_secs(),
                        origin: pending.origin.clone(),
                        last_inputs,
                    },
                );
                let newer = state
                    .highest
                    .get(&pending.id)
                    .and_then(|h| h.parse::<Version>().ok())
                    .is_none_or(|h| version > h);
                if newer {
                    state
                        .highest
                        .insert(pending.id.clone(), pending.version.clone());
                }
                Ok(replaced)
            })
            .inspect_err(|_| {
                if !same_content_installed {
                    let _ = fs::remove_dir_all(&app_dir);
                    let _ = fs::remove_dir_all(&env_dir);
                }
            })?;

        if let Some(old) = replaced.filter(|r| r.content_hash != pending.content_hash) {
            let _ = fs::remove_dir_all(self.paths.app_dir(&id, &old.content_hash));
            let _ = fs::remove_dir_all(self.paths.env_dir(&id, &old.content_hash));
        }
        on_step(InstallStep {
            step: "ready",
            message: "Ready".into(),
        });
        self.app(&pending.id)
    }

    /// Removes an app, its environment and its secrets. Its publisher stays pinned, so a later
    /// reinstall must still come from the same key; run history is kept.
    pub fn uninstall(&self, id: &str) -> Result<()> {
        self.refuse_if_running(id, "uninstall")?;
        let app_id: AppId = id.parse().map_err(|e: String| anyhow!(e))?;
        // Secrets first, while the app's manifest (which names them) is still there.
        if let Ok(app) = self.app(id) {
            for name in crate::run::secret_names(&app.manifest) {
                let _ = crate::secrets::delete(id, &name);
            }
        }
        self.update_state(|state| {
            state
                .apps
                .remove(id)
                .ok_or_else(|| anyhow!("`{id}` is not installed"))
        })?;
        let _ = fs::remove_dir_all(self.paths.apps().join(&app_id.namespace).join(&app_id.app));
        let _ = fs::remove_dir_all(self.paths.envs().join(&app_id.namespace).join(&app_id.app));
        let _ = fs::remove_dir_all(
            self.paths
                .pycache()
                .join(&app_id.namespace)
                .join(&app_id.app),
        );
        Ok(())
    }

    /// Rebuilds an app's environment from its lockfile.
    pub fn repair(&self, id: &str, on_step: &mut dyn FnMut(InstallStep)) -> Result<()> {
        self.refuse_if_running(id, "repair")?;
        let app = self.app(id)?;
        on_step(InstallStep {
            step: "environment",
            message: "Rebuilding the Python environment".into(),
        });
        let mut forward = |line: &str| {
            on_step(InstallStep {
                step: "environment",
                message: line.to_string(),
            })
        };
        // The old environment stays until the new one is complete.
        build_env(&app.app_dir, &app.env_dir, &mut forward)?;
        on_step(InstallStep {
            step: "ready",
            message: "Ready".into(),
        });
        Ok(())
    }

    pub fn pins(&self) -> Result<BTreeMap<String, Pin>> {
        Ok(self.load()?.pins)
    }

    // -- runs --------------------------------------------------------------------------------

    /// Builds the run of an installed app and records it as running.
    pub fn prepare_run(
        &self,
        id: &str,
        inputs: &Map<String, Value>,
        runner_dir: &Path,
    ) -> Result<(RunSpec, RunRecord)> {
        let app = self.app(id)?;
        let resolved = crate::run::resolve_inputs(&app.manifest, inputs)?;
        let mut secrets = Map::new();
        for name in crate::run::secret_names(&app.manifest) {
            let value = crate::secrets::get(id, &name)?
                .ok_or_else(|| anyhow!("the secret `{name}` is not set"))?;
            secrets.insert(name, Value::String(value));
        }
        let python = uv::python_in(&app.env_dir, app.manifest.kind == Kind::Gui);
        if !python.exists() {
            bail!("the app's environment is missing; use Repair");
        }

        self.update_state(|state| {
            if let Some(r) = state.apps.get_mut(id) {
                r.last_inputs = resolved.clone();
            }
            Ok(())
        })?;

        let started_at = now_secs();
        let run_id = format!(
            "{}-{}-{}",
            stamp(started_at),
            app.manifest.id.app,
            random_hex(2)
        );
        let run_dir = self.paths.runs().join(&run_id);
        let output_dir = run_dir.join("output");
        fs::create_dir_all(&output_dir)?;
        let record = RunRecord {
            id: run_id,
            app_id: id.to_string(),
            app_title: app.manifest.title.clone(),
            version: app.record.version.clone(),
            kind: app.manifest.kind.as_str().to_string(),
            started_at,
            finished_at: None,
            status: RunStatus::Running,
            duration_ms: 0,
            outputs: Map::new(),
            error: None,
            output_dir: output_dir.clone(),
        };
        self.save_run(&record)?;
        // One bytecode cache per installed version: an app cannot leave compiled code that
        // another app would load.
        let pycache_dir = self
            .paths
            .pycache()
            .join(&app.manifest.id.namespace)
            .join(&app.manifest.id.app)
            .join(&app.record.content_hash);
        let spec = RunSpec {
            manifest: app.manifest,
            app_dir: app.app_dir,
            python,
            runner_dir: runner_dir.to_path_buf(),
            run_dir,
            output_dir,
            pycache_dir,
            inputs: resolved,
            secrets,
            args: Vec::new(),
        };
        Ok((spec, record))
    }

    pub fn finish_run(&self, record: &mut RunRecord, result: &RunResult) -> Result<()> {
        record.status = result.status;
        record.finished_at = Some(now_secs());
        record.duration_ms = result.duration_ms;
        record.outputs = result.outputs.clone();
        record.error = result.error.clone();
        self.save_run(record)
    }

    fn save_run(&self, record: &RunRecord) -> Result<()> {
        let path = self.paths.runs().join(&record.id).join("run.json");
        fs::write(path, serde_json::to_vec_pretty(record)?).context("saving run.json")
    }

    pub fn run(&self, run_id: &str) -> Result<RunRecord> {
        if run_id.contains(['/', '\\', '.']) {
            bail!("invalid run id");
        }
        let data = fs::read(self.paths.runs().join(run_id).join("run.json"))
            .with_context(|| format!("no run `{run_id}`"))?;
        Ok(serde_json::from_slice(&data)?)
    }

    /// Most recent runs first.
    pub fn runs(&self, app_id: Option<&str>, limit: usize) -> Result<Vec<RunRecord>> {
        let mut names: Vec<String> = fs::read_dir(self.paths.runs())?
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort_unstable_by(|a, b| b.cmp(a));
        let mut out = Vec::new();
        for name in names {
            let Ok(record) = self.run(&name) else {
                continue;
            };
            if app_id.is_some_and(|id| id != record.app_id) {
                continue;
            }
            out.push(record);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    pub fn stats(&self, days: u32) -> Result<Stats> {
        let now = now_secs();
        let since = now.saturating_sub(u64::from(days) * 86_400);
        let runs = self.runs(None, 5_000)?;
        let mut per_day: BTreeMap<String, DayCount> = (0..days)
            .rev()
            .map(|d| {
                let day = date(now - u64::from(d) * 86_400);
                (
                    day.clone(),
                    DayCount {
                        date: day,
                        ok: 0,
                        error: 0,
                        cancelled: 0,
                    },
                )
            })
            .collect();
        let mut by_app: BTreeMap<String, AppCount> = BTreeMap::new();
        let mut usage: BTreeMap<String, AppUsage> = BTreeMap::new();
        for r in &runs {
            let u = usage.entry(r.app_id.clone()).or_insert_with(|| AppUsage {
                app_id: r.app_id.clone(),
                runs: 0,
                last_run_at: 0,
            });
            u.last_run_at = u.last_run_at.max(r.started_at);
            if r.started_at >= since && r.status != RunStatus::Running {
                u.runs += 1;
            }
        }
        let (mut ok, mut failed, mut cancelled, mut total_ms) = (0, 0, 0, 0);
        for r in runs.iter().filter(|r| r.started_at >= since) {
            let Some(day) = per_day.get_mut(&date(r.started_at)) else {
                continue;
            };
            match r.status {
                RunStatus::Ok => (day.ok += 1, ok += 1),
                RunStatus::Error => (day.error += 1, failed += 1),
                RunStatus::Cancelled => (day.cancelled += 1, cancelled += 1),
                RunStatus::Running => continue,
            };
            total_ms += r.duration_ms;
            by_app
                .entry(r.app_id.clone())
                .or_insert_with(|| AppCount {
                    app_id: r.app_id.clone(),
                    title: r.app_title.clone(),
                    runs: 0,
                })
                .runs += 1;
        }
        let finished = ok + failed;
        let mut top_apps: Vec<AppCount> = by_app.into_values().collect();
        top_apps.sort_by_key(|a| std::cmp::Reverse(a.runs));
        top_apps.truncate(5);
        let mut usage: Vec<AppUsage> = usage.into_values().collect();
        usage.sort_by_key(|u| std::cmp::Reverse(u.last_run_at));
        Ok(Stats {
            apps_installed: self.load()?.apps.len(),
            runs: ok + failed + cancelled,
            ok,
            failed,
            cancelled,
            success_rate: (finished > 0).then(|| f64::from(ok) / f64::from(finished)),
            total_duration_ms: total_ms,
            per_day: per_day.into_values().collect(),
            top_apps,
            usage,
        })
    }
}

/// Builds an app's environment next to `env_dir` and moves it into place only when uv has
/// finished: a failed or interrupted build never leaves a half-made environment that looks
/// ready, and the environment it replaces keeps working until then.
fn build_env(app_dir: &Path, env_dir: &Path, on_line: &mut dyn FnMut(&str)) -> Result<()> {
    let parent = env_dir.parent().expect("has a parent");
    fs::create_dir_all(parent)?;
    let name = env_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("env");
    let building = parent.join(format!("{name}.building-{}", random_hex(4)));
    if let Err(e) = uv::sync(app_dir, &building, LockMode::Frozen, on_line) {
        let _ = fs::remove_dir_all(&building);
        return Err(e);
    }
    if env_dir.exists() {
        fs::remove_dir_all(env_dir).context("replacing the old environment")?;
    }
    fs::rename(&building, env_dir).context("moving the environment into place")
}

/// Whether a verified package may be installed given what is installed and pinned: the
/// publisher's key must be the pinned one, and the version must not go back (nor stay the same
/// with different content). Run when the package is inspected and again when it is installed.
fn admit(
    state: &State,
    id: &AppId,
    version: &Version,
    publisher_key: &str,
    content_sha256: &str,
    content_hash: &str,
) -> Result<(InstallAction, Trust)> {
    let trust = match state.pins.get(&id.namespace) {
        Some(pin) if same_key(&pin.key, publisher_key) => Trust::FirstUse,
        Some(pin) => bail!(
            "this package is signed with key {}, but the apps from `{}` you installed before were \
             signed with key {}: it may not come from the same publisher, so it was refused",
            fingerprint_of(publisher_key).unwrap_or_default(),
            id.namespace,
            fingerprint_of(&pin.key).unwrap_or_else(|| pin.key_id.clone()),
        ),
        None => Trust::NewPublisher,
    };
    let key = id.to_string();
    let action = match state.apps.get(&key) {
        None => InstallAction::Install,
        Some(r) if r.content_sha256.is_empty() && r.content_hash == content_hash => {
            InstallAction::Reinstall
        }
        Some(r) if !r.content_sha256.is_empty() && r.content_sha256 == content_sha256 => {
            InstallAction::Reinstall
        }
        Some(_) => InstallAction::Update,
    };
    if let Some(highest) = state.highest.get(&key) {
        let highest: Version = highest.parse().map_err(|e: String| anyhow!(e))?;
        if *version < highest {
            bail!(
                "version {version} is older than version {highest}, which was already installed; \
                 older versions are refused (they may have known problems)"
            );
        }
        if *version == highest && action == InstallAction::Update {
            bail!(
                "this package says version {version} but its content differs from the installed \
                 {highest}: the publisher must give every release a new version number"
            );
        }
    }
    Ok((action, trust))
}

fn same_key(a: &str, b: &str) -> bool {
    match (parse_public_key(a), parse_public_key(b)) {
        (Ok(a), Ok(b)) => public_key_base64(&a) == public_key_base64(&b),
        _ => false,
    }
}

fn fingerprint_of(key: &str) -> Option<String> {
    parse_public_key(key).ok().map(|k| public_key_id(&k))
}

fn is_token(token: &str) -> bool {
    token.len() == 24 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The app's icon as a data URL (PNG, JPEG, SVG or ICO up to 512 KB), or `None`.
fn icon_data_url(app_dir: &Path, manifest: &Manifest) -> Option<String> {
    let icon = manifest.icon.as_ref()?;
    let mime = match Path::new(icon)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => return None,
    };
    let data = fs::read(app_dir.join(icon)).ok()?;
    if data.len() > 512 * 1024 {
        return None;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    Some(format!("data:{mime};base64,{b64}"))
}

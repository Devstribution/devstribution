//! Installed apps and their state: the install flow (stage → confirm → environment → ready),
//! pinned publisher keys, anti-downgrade, uninstall, repair, and the history of runs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use devstribution_core::diff::{self, AppDiff, AppFolder};
use devstribution_core::files::{FilesList, sha256};
use devstribution_core::manifest::{AppId, Kind, Manifest, Version};
use devstribution_core::package::{
    self, Limits, PYPROJECT, SignatureStatus, VerifiedPackage, VerifyOptions,
};
use devstribution_core::scan::{self, ScanReport};
use devstribution_core::signing::{parse_public_key, public_key_base64, public_key_id};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::guard::GuardHit;
use crate::journal::{self, Journal};
use crate::paths::Paths;
use crate::policy::{self, Policy, ScanDanger};
use crate::run::{self, RunResult, RunSpec, RunStatus};
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
    /// Set when the runtime guard stopped a run: the app does not run again until the user
    /// decides. Kept across updates, so a new version cannot clear it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<Quarantine>,
    /// Guard rules the user allowed for this app after a stop.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guard_allow: Vec<String>,
    /// The publisher's full key, base64 (empty in records from before it was kept).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher_key: String,
    /// Installed in managed mode, by an author the policy approved. Lets it run while the
    /// approved authors cannot be read (a laptop away from the network share).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub policy_approved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quarantine {
    pub run_id: String,
    pub at: u64,
    pub hit: GuardHit,
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
    /// Managed mode: the policy approves this author and key.
    Approved,
    /// Managed mode: the policy does not approve this author and key.
    NotApproved,
}

impl Trust {
    pub fn as_str(self) -> &'static str {
        match self {
            Trust::FirstUse => "first_use",
            Trust::NewPublisher => "new_publisher",
            Trust::Approved => "approved",
            Trust::NotApproved => "not_approved",
        }
    }
}

/// Why the policy refuses a package, for the confirmation screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blocked {
    pub reasons: Vec<String>,
    pub contact: Option<String>,
    /// The author or its key is not approved: a `.trust` request for IT can help.
    pub author_not_approved: bool,
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
    /// For an update: what changed since the installed version (`devstribution_core::diff`).
    #[serde(default)]
    pub changes: Option<AppDiff>,
    /// Set when the organisation's policy refuses the package: it cannot be installed.
    #[serde(default)]
    pub blocked: Option<Blocked>,
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
    /// What the user can try about the error ([`RunResult::hint`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// What the runtime guard did during the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guard: Vec<GuardHit>,
    pub output_dir: PathBuf,
    /// Where the run's file results may be: `output_dir` and the folders chosen for the run
    /// (see [`run::result_folders`]). "Open result" opens nothing outside them. Empty in
    /// records from before it existed, which means `output_dir` only.
    #[serde(default)]
    pub result_folders: Vec<PathBuf>,
    /// When a cleanup removed the run's output folder and log. The run stays in the history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_removed: Option<u64>,
}

/// A run a cleanup could remove the results of.
#[derive(Debug, Clone, Serialize)]
pub struct CleanupRun {
    pub run_id: String,
    pub app_id: String,
    pub app_title: String,
    pub started_at: u64,
    pub status: RunStatus,
    /// What would be freed.
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupPlan {
    pub runs: Vec<CleanupRun>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupResult {
    pub runs: usize,
    pub bytes: u64,
    /// Runs whose files could not all be removed (open in another program, for example).
    pub failed: Vec<String>,
    /// Set when the journal could not be written: the results are removed all the same.
    pub journal_error: Option<String>,
}

/// What the journal says now, next to what is on disk.
#[derive(Debug, Clone, Serialize)]
pub struct JournalReport {
    pub file: PathBuf,
    pub lines: usize,
    /// The first line that breaks the chain (1-based), if any.
    pub broken_at: Option<u64>,
    /// Runs whose `run.json` is not what the journal last recorded.
    pub changed: Vec<String>,
    /// Runs whose folder is gone, and no cleanup line says it was removed.
    pub missing: Vec<String>,
    /// The cleanups, newest first.
    pub cleanups: Vec<journal::Entry>,
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
    /// Managed mode's policy, read when the store opens; `None` in free mode.
    policy: Option<Policy>,
    /// Guard stops that `state.json` could not record (disk full, a file held by an
    /// antivirus): the app stays blocked from memory, and each later state change tries again.
    unsaved_stops: Mutex<BTreeMap<String, Quarantine>>,
    /// Packages on the confirmation screen, by token: the SHA-256 of their `pending.json` and
    /// their verified file list. The staging folder can be written by anything the user runs,
    /// so install checks both against these before trusting what is there.
    staged: Mutex<BTreeMap<String, ([u8; 32], FilesList)>>,
}

/// This launcher's version, compared with the policy's minimum.
pub const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

impl Store {
    pub fn open(paths: Paths) -> Result<Store> {
        for dir in [
            paths.apps(),
            paths.envs(),
            paths.runs(),
            paths.staging(),
            paths.state(),
        ] {
            fs::create_dir_all(&dir).with_context(|| {
                format!(
                    "cannot create {}: check that the disk has free space",
                    dir.display()
                )
            })?;
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
            policy: Policy::from_system(),
            unsaved_stops: Mutex::new(BTreeMap::new()),
            staged: Mutex::new(BTreeMap::new()),
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

    /// The policy of managed mode, `None` in free mode.
    pub fn policy(&self) -> Option<&Policy> {
        self.policy.as_ref()
    }

    /// Replaces the policy read from the system: for tests, and to check a policy file
    /// before rolling it out.
    pub fn set_policy(&mut self, policy: Option<Policy>) {
        self.policy = policy;
    }

    /// Why the policy refuses `app` signed by `key`; empty when it does not (or in free mode).
    /// `scan` is given for installs, where malware-like findings count.
    fn policy_reasons(
        &self,
        namespace: &str,
        key: &str,
        scan: Option<&ScanReport>,
        approved_before: bool,
    ) -> Vec<String> {
        let Some(p) = &self.policy else {
            return Vec::new();
        };
        let mut reasons = Vec::new();
        if let Some(r) = p.launcher_refusal(LAUNCHER_VERSION) {
            reasons.push(r);
        }
        if !p.approves(namespace, key) {
            // The approved authors could not all be read (a network share out of reach): an
            // app installed under the policy keeps running, nothing new installs.
            let tolerated = approved_before && !p.problems.is_empty() && scan.is_none();
            if !tolerated {
                reasons.push(if p.knows(namespace) {
                    format!(
                        "your organisation approves `{namespace}`, but not the key this was signed                          with ({})",
                        fingerprint_of(key).unwrap_or_default()
                    )
                } else {
                    format!("your organisation has not approved the author `{namespace}`")
                });
            }
        }
        if let Some(scan) = scan
            && p.scan_danger == ScanDanger::Refuse
            && scan
                .findings
                .iter()
                .any(|f| f.severity == scan::Severity::Danger)
        {
            reasons.push(
                "the code does things malware does, and your organisation does not install such \
                 packages"
                    .into(),
            );
        }
        reasons
    }

    /// Applies the policy's verdict: logs it, and in a dry run turns a refusal into a warning.
    /// Returns the reasons when the action is refused.
    fn policy_verdict(
        &self,
        action: &str,
        app: &str,
        version: &str,
        key: &str,
        reasons: Vec<String>,
        warnings: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        let p = self.policy.as_ref()?;
        if reasons.is_empty() {
            return None;
        }
        let fp = fingerprint_of(key).unwrap_or_default();
        let decision = if p.dry_run { "would_refuse" } else { "refused" };
        policy::log(
            &self.policy_log(),
            &policy::log_now(decision, action, app, version, &fp, &reasons),
        );
        if p.dry_run {
            warnings.push(format!(
                "Your organisation is trying out a policy that would refuse this: {}.",
                reasons.join("; ")
            ));
            return None;
        }
        Some(reasons)
    }

    /// What the policy refused, or would have refused in a dry run (JSON lines).
    pub fn policy_log(&self) -> PathBuf {
        self.paths.state().join("policy.log")
    }

    fn state_file(&self) -> PathBuf {
        self.paths.state().join("state.json")
    }

    fn load(&self) -> Result<State> {
        match fs::read(self.state_file()) {
            Ok(data) => serde_json::from_slice(&data).context(STATE_UNREADABLE),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e).context(STATE_UNREADABLE),
        }
    }

    fn save(&self, state: &State) -> Result<()> {
        let path = self.state_file();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
        fs::rename(&tmp, &path).context(
            "cannot save the list of installed apps (state.json): check that the data folder can be written and has free space",
        )
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
        let trust = match &self.policy {
            Some(p) if p.approves(&id.namespace, &self.publisher_key(record, &app_dir)) => {
                Trust::Approved
            }
            Some(_) => Trust::NotApproved,
            None if pin.is_some() => Trust::FirstUse,
            None => Trust::NewPublisher,
        };
        let mut record = record.clone();
        if record.quarantine.is_none() {
            record.quarantine = self.unsaved_stops().get(&record.id).cloned();
        }
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

    /// The full key that signed an installed app: from its record, or for records from before
    /// it was kept, from the key file in the app's folder.
    fn publisher_key(&self, record: &AppRecord, app_dir: &Path) -> String {
        if !record.publisher_key.is_empty() {
            return record.publisher_key.clone();
        }
        fs::read_to_string(app_dir.join(devstribution_core::FILES_PUBLIC_KEY))
            .ok()
            .and_then(|t| parse_public_key(&t).ok())
            .map(|k| public_key_base64(&k))
            .unwrap_or_default()
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
            .with_context(|| {
                format!(
                    "cannot open {}: check that the file is still there and not open in another program",
                    package_path.display()
                )
            })?
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
            self.policy.as_ref(),
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
        let trust = trust.as_str();
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
        // Compared with the installed version's files, which were verified when installed. A
        // comparison that fails (an app folder damaged since) only leaves the summary out.
        let changes = installed
            .filter(|r| r.content_sha256 != verified.content_sha256)
            .and_then(|r| {
                let old = AppFolder::all_files(&self.paths.app_dir(&m.id, &r.content_hash)).ok()?;
                let new = AppFolder::new(&stage.join("app"), verified.files.paths());
                diff::diff(&old, &new).ok()
            });
        let reasons =
            self.policy_reasons(&m.id.namespace, &verified.publisher_key, Some(&scan), false);
        let blocked = self
            .policy_verdict(
                "install",
                &id,
                &m.version.to_string(),
                &verified.publisher_key,
                reasons,
                &mut warnings,
            )
            .map(|reasons| Blocked {
                reasons,
                contact: self.policy.as_ref().and_then(|p| p.contact.clone()),
                author_not_approved: trust == "not_approved",
            });

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
            changes,
            blocked,
            publisher_key: verified.publisher_key.clone(),
            content_hash: verified.content_hash.clone(),
            content_sha256: verified.content_sha256.clone(),
        };
        let data = serde_json::to_vec(&pending)?;
        fs::write(stage.join("pending.json"), &data)?;
        self.staged()
            .insert(token.to_string(), (sha256(&data), verified.files.clone()));
        Ok(pending)
    }

    /// A package waiting on the confirmation screen, as `inspect` described it.
    pub fn pending(&self, token: &str) -> Result<PendingInstall> {
        if !is_token(token) {
            bail!("this install is no longer valid: open the package again");
        }
        let data = fs::read(self.paths.staging().join(token).join("pending.json"))
            .context("this install is no longer pending: open the package again")?;
        Ok(serde_json::from_slice(&data)?)
    }

    pub fn discard(&self, token: &str) {
        if is_token(token) {
            self.staged().remove(token);
            let _ = fs::remove_dir_all(self.paths.staging().join(token));
        }
    }

    fn staged(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, ([u8; 32], FilesList)>> {
        self.staged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Installs a staged package after the user confirmed it.
    pub fn install(
        &self,
        token: &str,
        on_step: &mut dyn FnMut(InstallStep),
    ) -> Result<InstalledApp> {
        if !is_token(token) {
            bail!("this install is no longer valid: open the package again");
        }
        let stage = self.paths.staging().join(token);
        let (pending_sha256, files) =
            self.staged().get(token).cloned().ok_or_else(|| {
                anyhow!("this install is no longer pending: open the package again")
            })?;
        let data = fs::read(stage.join("pending.json"))
            .context("this install is no longer pending: open the package again")?;
        if sha256(&data) != pending_sha256 {
            bail!("the install was changed after the confirmation screen: open the package again");
        }
        let pending: PendingInstall = serde_json::from_slice(&data)?;
        let id: AppId = pending.id.parse().map_err(|e: String| anyhow!(e))?;
        let version: Version = pending.version.parse().map_err(|e: String| anyhow!(e))?;
        // The checks of the confirmation screen again, against the state as it is now: another
        // install confirmed in the meantime may have pinned a different key for this publisher
        // or installed a newer version.
        if pending.blocked.is_some() {
            bail!("your organisation's policy does not allow this app");
        }
        admit(
            &self.load()?,
            self.policy.as_ref(),
            &id,
            &version,
            &pending.publisher_key,
            &pending.content_sha256,
            &pending.content_hash,
        )
        .context("this install is no longer valid")?;
        if let Some(p) = &self.policy
            && !p.dry_run
        {
            let reasons = self.policy_reasons(
                &id.namespace,
                &pending.publisher_key,
                Some(&pending.scan),
                false,
            );
            if !reasons.is_empty() {
                bail!(
                    "your organisation's policy does not allow this app: {}",
                    reasons.join("; ")
                );
            }
        }
        let policy_approved = self
            .policy
            .as_ref()
            .is_some_and(|p| p.approves(&id.namespace, &pending.publisher_key));
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
        // The files as the confirmation screen showed them, checked again just before the move.
        package::verify_extracted(&stage.join("app"), &files)?;
        if app_dir.exists() {
            fs::remove_dir_all(&app_dir)?;
        }
        fs::create_dir_all(app_dir.parent().expect("has a parent"))?;
        fs::rename(stage.join("app"), &app_dir).context(
            "cannot move the app into place: close programs that may have its files open (antivirus scans included) and install it again",
        )?;
        let _ = fs::remove_dir_all(&stage);
        self.staged().remove(token);

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
                    self.policy.as_ref(),
                    &id,
                    &version,
                    &pending.publisher_key,
                    &pending.content_sha256,
                    &pending.content_hash,
                )
                .context("this install is no longer valid")?;
                let previous = state.apps.get(&pending.id);
                let last_inputs = previous.map(|r| r.last_inputs.clone()).unwrap_or_default();
                let quarantine = previous.and_then(|r| r.quarantine.clone());
                let guard_allow = previous.map(|r| r.guard_allow.clone()).unwrap_or_default();
                let pin = Pin {
                    key: pending.publisher_key.clone(),
                    key_id: pending.key_id.clone(),
                    pinned_at: now_secs(),
                    source: if policy_approved {
                        "policy"
                    } else {
                        "first-use"
                    }
                    .into(),
                };
                if policy_approved {
                    // The policy decides in managed mode; the pin follows it, so that the
                    // namespace keeps the approved key if the policy is ever removed.
                    let same = state
                        .pins
                        .get(&id.namespace)
                        .is_some_and(|p| same_key(&p.key, &pin.key));
                    if !same {
                        state.pins.insert(id.namespace.clone(), pin);
                    }
                } else {
                    state.pins.entry(id.namespace.clone()).or_insert(pin);
                }
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
                        quarantine,
                        guard_allow,
                        publisher_key: pending.publisher_key.clone(),
                        policy_approved,
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
        // A stop still in memory blocks the app either way (`describe`); keep trying to save it.
        let _ = self.save_unsaved_stops();
        let app = self.app(id)?;
        let key = self.publisher_key(&app.record, &app.app_dir);
        let reasons = self.policy_reasons(
            &app.manifest.id.namespace,
            &key,
            None,
            app.record.policy_approved,
        );
        if let Some(reasons) = self.policy_verdict(
            "run",
            id,
            &app.record.version,
            &key,
            reasons,
            &mut Vec::new(),
        ) {
            bail!(
                "{} cannot run: {}. {}",
                app.manifest.title,
                reasons.join("; "),
                self.policy
                    .as_ref()
                    .map(Policy::ask_whom)
                    .unwrap_or_default()
            );
        }
        if let Some(q) = &app.record.quarantine {
            bail!(
                "{} is blocked: in an earlier run it {}. Allow it or uninstall it first.",
                app.manifest.title,
                q.hit.message
            );
        }
        let resolved = run::resolve_inputs(&app.manifest, inputs)?;
        let mut secrets = Map::new();
        for name in crate::run::secret_names(&app.manifest) {
            let value = crate::secrets::get(id, &name)?
                .ok_or_else(|| anyhow!("the secret `{name}` is not set: enter it when the launcher asks, or in the app's panel"))?;
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
            hint: None,
            guard: Vec::new(),
            output_dir: output_dir.clone(),
            result_folders: run::result_folders(&app.manifest, &resolved, &output_dir),
            output_removed: None,
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
            data_dir: self.paths.root.clone(),
            inputs: resolved,
            secrets,
            args: Vec::new(),
            guard_allow: self.guard_allow(&app.record),
        };
        Ok((spec, record))
    }

    pub fn finish_run(&self, record: &mut RunRecord, result: &RunResult) -> Result<()> {
        record.status = result.status;
        record.finished_at = Some(now_secs());
        record.duration_ms = result.duration_ms;
        record.outputs = result.outputs.clone();
        record.error = result.error.clone();
        record.hint = result.hint.clone();
        record.guard = result.guard.clone();
        let mut stop_unsaved = None;
        if let Some(hit) = result.guard_stop() {
            let quarantine = Quarantine {
                run_id: record.id.clone(),
                at: now_secs(),
                hit: hit.clone(),
            };
            self.unsaved_stops()
                .insert(record.app_id.clone(), quarantine);
            stop_unsaved = self.save_unsaved_stops().err();
        }
        // The run is recorded even when the stop could not be: it says what the app did.
        let saved = self.save_run(record);
        // The journal is a record beside the run, not part of it: a run is never lost to it.
        if saved.is_ok() {
            self.journal_run(&record.id);
        }
        match stop_unsaved {
            Some(e) => Err(e.context(
                "the app was stopped, but the launcher could not record it;                  it stays blocked until the launcher is closed",
            )),
            None => saved,
        }
    }

    fn unsaved_stops(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Quarantine>> {
        self.unsaved_stops
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Writes the guard stops not yet in `state.json`, trying a few times: nothing may run the
    /// app before its stop is recorded. Those still unsaved stay blocked from memory.
    fn save_unsaved_stops(&self) -> Result<()> {
        let pending = self.unsaved_stops().clone();
        if pending.is_empty() {
            return Ok(());
        }
        let mut last = None;
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            match self.update_state(|state| {
                for (id, q) in &pending {
                    if let Some(app) = state.apps.get_mut(id) {
                        app.quarantine = Some(q.clone());
                    }
                }
                Ok(())
            }) {
                Ok(()) => {
                    let mut unsaved = self.unsaved_stops();
                    unsaved.retain(|id, q| pending.get(id) != Some(q));
                    return Ok(());
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.expect("at least one attempt"))
    }

    /// The guard rules that only report for this app: the ones the user allowed after a stop,
    /// unless the policy keeps that decision for IT, and the ones the policy makes report-only.
    fn guard_allow(&self, record: &AppRecord) -> Vec<String> {
        match &self.policy {
            None => record.guard_allow.clone(),
            Some(p) => {
                let mut rules = p.guard_report_only.clone();
                if p.users_may_allow_guard_rules {
                    rules.extend(record.guard_allow.iter().cloned());
                }
                rules.sort();
                rules.dedup();
                rules
            }
        }
    }

    /// Whether the user may let an app run past the guard rule that stopped it.
    pub fn users_may_allow_guard_rules(&self) -> bool {
        self.policy
            .as_ref()
            .is_none_or(|p| p.users_may_allow_guard_rules)
    }

    /// Ends an app's quarantine. With `allow`, the rule that stopped it no longer stops this
    /// app (it is still reported); without, the next run is checked as before.
    pub fn release_quarantine(&self, id: &str, allow: bool) -> Result<InstalledApp> {
        if allow && !self.users_may_allow_guard_rules() {
            bail!(
                "your organisation does not let users allow what devstribution stopped. {}",
                self.policy
                    .as_ref()
                    .map(Policy::ask_whom)
                    .unwrap_or_default()
            );
        }
        let _ = self.save_unsaved_stops();
        let unsaved = self.unsaved_stops().get(id).cloned();
        self.update_state(|state| {
            let app = state
                .apps
                .get_mut(id)
                .ok_or_else(|| anyhow!("`{id}` is not installed"))?;
            if let Some(q) = app.quarantine.take().or(unsaved)
                && allow
                && !app.guard_allow.contains(&q.hit.rule)
            {
                app.guard_allow.push(q.hit.rule);
            }
            Ok(())
        })?;
        self.unsaved_stops().remove(id);
        self.app(id)
    }

    fn save_run(&self, record: &RunRecord) -> Result<()> {
        let path = self.paths.runs().join(&record.id).join("run.json");
        fs::write(path, serde_json::to_vec_pretty(record)?)
            .context("cannot save the run's record (run.json): check that the disk has free space")
    }

    pub fn run(&self, run_id: &str) -> Result<RunRecord> {
        if run_id.contains(['/', '\\', '.']) {
            bail!("invalid run id");
        }
        let data =
            fs::read(self.paths.runs().join(run_id).join("run.json")).with_context(|| {
                format!("run `{run_id}` is no longer there: it may have been cleaned up")
            })?;
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

    // -- cleanup -----------------------------------------------------------------------------

    fn journal(&self) -> Journal {
        Journal::new(self.paths.journal())
    }

    fn journal_run(&self, run_id: &str) {
        let path = self.paths.runs().join(run_id).join("run.json");
        if let Some(sha) = journal::file_sha256(&path) {
            let _ = self
                .journal()
                .append("run", json!({ "run_id": run_id, "run_sha256": sha }));
        }
    }

    /// The runs whose results a cleanup could remove, with what it would free. Runs still
    /// running are never in it. Without `remove_records` a run keeps its `run.json`, so it
    /// stays in the history and the statistics.
    pub fn cleanup_plan(
        &self,
        older_than_days: Option<u32>,
        app_id: Option<&str>,
        remove_records: bool,
    ) -> Result<CleanupPlan> {
        let cutoff = older_than_days.map(|d| now_secs().saturating_sub(u64::from(d) * 86_400));
        let mut runs = Vec::new();
        let mut bytes = 0;
        for r in self.runs(app_id, usize::MAX)? {
            if r.status == RunStatus::Running
                || cutoff.is_some_and(|c| r.started_at > c)
                || (r.output_removed.is_some() && !remove_records)
            {
                continue;
            }
            let files = run_files(&self.paths.runs().join(&r.id), remove_records);
            let size: u64 = files.iter().map(|(_, n)| n).sum();
            if size == 0 && !remove_records {
                continue;
            }
            bytes += size;
            runs.push(CleanupRun {
                run_id: r.id,
                app_id: r.app_id,
                app_title: r.app_title,
                started_at: r.started_at,
                status: r.status,
                bytes: size,
            });
        }
        Ok(CleanupPlan { runs, bytes })
    }

    /// Removes the results and logs of these runs, and writes what it removed to the journal.
    /// Only ever inside `runs\<id>\`: folders an app was pointed at are not touched. With
    /// `remove_records` the run leaves the history too.
    pub fn cleanup(&self, run_ids: &[String], remove_records: bool) -> Result<CleanupResult> {
        let mut done = Vec::new();
        let mut failed = Vec::new();
        let mut total = 0;
        for id in run_ids {
            let Ok(mut record) = self.run(id) else {
                failed.push(id.clone());
                continue;
            };
            if record.status == RunStatus::Running {
                failed.push(id.clone());
                continue;
            }
            let dir = self.paths.runs().join(id);
            let before = run_files(&dir, remove_records);
            let mut clean = true;
            match fs::read_dir(&dir) {
                Ok(entries) if remove_records => {
                    drop(entries);
                    clean = fs::remove_dir_all(&dir).is_ok();
                }
                Ok(entries) => {
                    for e in entries.flatten() {
                        if e.file_name() == "run.json" {
                            continue;
                        }
                        let path = e.path();
                        let is_dir = path
                            .symlink_metadata()
                            .is_ok_and(|m| m.is_dir() && !is_link(&m));
                        let removed = if is_dir {
                            fs::remove_dir_all(&path)
                        } else {
                            fs::remove_file(&path).or_else(|_| fs::remove_dir(&path))
                        };
                        clean &= removed.is_ok();
                    }
                }
                Err(_) => clean = false,
            }
            let after = run_files(&dir, remove_records);
            let gone: Vec<&(String, u64)> = before.iter().filter(|f| !after.contains(f)).collect();
            let bytes: u64 = gone.iter().map(|(_, n)| n).sum();
            if !clean || !after.is_empty() {
                failed.push(id.clone());
            }
            let mut run_sha256 = None;
            if clean && !remove_records {
                record.output_removed = Some(now_secs());
                if self.save_run(&record).is_err() {
                    failed.push(id.clone());
                }
                run_sha256 = journal::file_sha256(&dir.join("run.json"));
            }
            if gone.is_empty() {
                continue;
            }
            total += bytes;
            done.push(json!({
                "run_id": id,
                "app_id": record.app_id,
                "bytes": bytes,
                "file_count": gone.len(),
                // The longest lists are cut: the journal says what was removed, not everything.
                "files": gone.iter().take(100).collect::<Vec<_>>(),
                "record_removed": clean && remove_records,
                "run_sha256": run_sha256,
            }));
        }
        let journal_error = if done.is_empty() {
            None
        } else {
            self.journal()
                .append(
                    "cleanup",
                    json!({ "runs": done, "bytes": total, "remove_records": remove_records }),
                )
                .err()
                .map(|e| format!("{e:#}"))
        };
        Ok(CleanupResult {
            runs: done.len(),
            bytes: total,
            failed,
            journal_error,
        })
    }

    /// The journal checked against the run folders.
    pub fn journal_report(&self) -> JournalReport {
        let journal = self.journal();
        let reading = journal.read();
        // Per run: the hash of its `run.json` as last recorded, `None` once its record was
        // removed by a cleanup.
        let mut last: BTreeMap<String, Option<String>> = BTreeMap::new();
        for e in &reading.entries {
            match e.kind.as_str() {
                "run" => {
                    if let (Some(id), Some(sha)) =
                        (e.data["run_id"].as_str(), e.data["run_sha256"].as_str())
                    {
                        last.insert(id.to_string(), Some(sha.to_string()));
                    }
                }
                "cleanup" => {
                    for r in e.data["runs"].as_array().into_iter().flatten() {
                        let Some(id) = r["run_id"].as_str() else {
                            continue;
                        };
                        if r["record_removed"].as_bool() == Some(true) {
                            last.insert(id.to_string(), None);
                        } else if let Some(sha) = r["run_sha256"].as_str() {
                            last.insert(id.to_string(), Some(sha.to_string()));
                        }
                    }
                }
                _ => {}
            }
        }
        let mut changed = Vec::new();
        let mut missing = Vec::new();
        for (id, sha) in last {
            let Some(sha) = sha else {
                continue;
            };
            let path = self.paths.runs().join(&id).join("run.json");
            match journal::file_sha256(&path) {
                None => missing.push(id),
                Some(now) if now != sha => changed.push(id),
                Some(_) => {}
            }
        }
        JournalReport {
            file: journal.path().to_path_buf(),
            lines: reading.entries.len(),
            broken_at: reading.broken_at.map(|i| i + 1),
            changed,
            missing,
            cleanups: reading
                .entries
                .into_iter()
                .filter(|e| e.kind == "cleanup")
                .rev()
                .collect(),
        }
    }

    /// A plain-text summary for a bug report or IT: versions, installed apps, the latest runs
    /// and the state of the journal. No secrets, inputs, results or keys; the user's home and
    /// data folders are written as `~` and `<data>`. A failed run's first error line is
    /// included, and that is text the app chose, so the user should read it before sending.
    pub fn diagnostics(&self, launcher_version: &str, uv_version: Option<&str>) -> String {
        let root = self.paths.root.display().to_string();
        let home = dirs::home_dir().map(|h| h.display().to_string());
        let redact = |text: &str| {
            let mut out = text.to_string();
            // The data folder is usually inside the home folder: replace it first.
            for (from, to) in [(Some(root.clone()), "<data>"), (home.clone(), "~")] {
                if let Some(from) = from.filter(|f| !f.is_empty()) {
                    out = out.replace(&from, to).replace(&from.replace('\\', "/"), to);
                }
            }
            out
        };
        let mut out = String::from("devstribution diagnostics\n");
        out += &format!(
            "Launcher {launcher_version} on {}, {} mode\n",
            current_platform(),
            if self.policy.is_some() {
                "managed"
            } else {
                "free"
            }
        );
        out += &format!("{}\n", uv_version.unwrap_or("uv not found"));
        out += &format!("Taken on {} (UTC)\n", date(now_secs()));

        match self.installed() {
            Ok(apps) => {
                out += &format!("\nApps ({})\n", apps.len());
                for a in apps {
                    out += &format!(
                        "  {} {} ({}){}\n",
                        a.record.id,
                        a.record.version,
                        a.manifest.kind.as_str(),
                        if a.record.quarantine.is_some() {
                            ", stopped by the guard"
                        } else {
                            ""
                        }
                    );
                }
            }
            Err(e) => out += &format!("\nApps: could not be read: {}\n", redact(&format!("{e:#}"))),
        }

        out += "\nLatest runs\n";
        match self.runs(None, 10) {
            Ok(runs) if runs.is_empty() => out += "  none\n",
            Ok(runs) => {
                for r in runs {
                    let status = serde_json::to_value(r.status)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_default();
                    out += &format!(
                        "  {} {status} {} {} ms{}\n",
                        date(r.started_at),
                        r.app_id,
                        r.duration_ms,
                        if r.output_removed.is_some() {
                            ", results cleaned up"
                        } else {
                            ""
                        }
                    );
                    if let Some(line) = r.error.as_deref().and_then(|e| e.lines().next()) {
                        let line: String = line.chars().take(160).collect();
                        out += &format!("    {}\n", redact(&line));
                    }
                    for hit in &r.guard {
                        out += &format!("    guard: {}\n", redact(&hit.message));
                    }
                }
            }
            Err(e) => out += &format!("  could not be read: {}\n", redact(&format!("{e:#}"))),
        }

        let j = self.journal_report();
        out += "\nJournal\n";
        out += &match j.broken_at {
            Some(line) => format!("  broken at line {line} of {}\n", j.lines + 1),
            None => format!("  {} lines, adding up\n", j.lines),
        };
        if !j.changed.is_empty() || !j.missing.is_empty() {
            out += &format!(
                "  {} run records changed, {} run folders missing\n",
                j.changed.len(),
                j.missing.len()
            );
        }
        out += &format!("  {} cleanups\n", j.cleanups.len());

        out += "\nLauncher log (latest lines)\n";
        let log = crate::applog::tail(&self.paths.launcher_log(), 20);
        if log.is_empty() {
            out += "  empty\n";
        }
        for line in log {
            out += &format!("  {}\n", redact(&line));
        }
        out
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
        fs::remove_dir_all(env_dir).context(
            "cannot replace the app's old Python environment: close the app if it is still open, then use Repair",
        )?;
    }
    fs::rename(&building, env_dir).context(
        "cannot move the app's new Python environment into place: close the app if it is still open, then use Repair",
    )
}

/// Whether a verified package may be installed given what is installed and pinned: the
/// publisher's key must be the pinned one, and the version must not go back (nor stay the same
/// with different content). Run when the package is inspected and again when it is installed.
///
/// In managed mode the policy, not the pins, says which key a namespace uses: the trust is
/// `Approved` or `NotApproved` (what the policy then does is decided by the caller).
fn admit(
    state: &State,
    policy: Option<&Policy>,
    id: &AppId,
    version: &Version,
    publisher_key: &str,
    content_sha256: &str,
    content_hash: &str,
) -> Result<(InstallAction, Trust)> {
    let trust = match (policy, state.pins.get(&id.namespace)) {
        (Some(p), _) if p.approves(&id.namespace, publisher_key) => Trust::Approved,
        (Some(_), _) => Trust::NotApproved,
        (None, pin) => trust_on_first_use(pin, id, publisher_key)?,
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

/// Free mode: the publisher's key must be the one pinned for the namespace, if any.
fn trust_on_first_use(pin: Option<&Pin>, id: &AppId, publisher_key: &str) -> Result<Trust> {
    match pin {
        Some(pin) if same_key(&pin.key, publisher_key) => Ok(Trust::FirstUse),
        Some(pin) => bail!(
            "this package is signed with key {}, but the apps from `{}` you installed before were \
             signed with key {}: it may not come from the same publisher, so it was refused",
            fingerprint_of(publisher_key).unwrap_or_default(),
            id.namespace,
            fingerprint_of(&pin.key).unwrap_or_else(|| pin.key_id.clone()),
        ),
        None => Ok(Trust::NewPublisher),
    }
}

fn same_key(a: &str, b: &str) -> bool {
    match (parse_public_key(a), parse_public_key(b)) {
        (Ok(a), Ok(b)) => public_key_base64(&a) == public_key_base64(&b),
        _ => false,
    }
}

/// Whether the entry is a link (symbolic link or junction): cleanup neither counts what is
/// behind one nor follows it, so a link an app left cannot lead it out of the run's folder.
fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// The files of a run's folder as (path inside it, size), links counted as themselves. Its
/// `run.json` is left out unless the whole record goes.
fn run_files(dir: &Path, with_record: bool) -> Vec<(String, u64)> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, u64)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let Ok(meta) = e.path().symlink_metadata() else {
                continue;
            };
            let name = format!("{rel}{}", e.file_name().to_string_lossy());
            if meta.is_dir() && !is_link(&meta) {
                walk(&e.path(), &format!("{name}/"), out);
            } else {
                out.push((name, meta.len()));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, "", &mut files);
    if !with_record {
        files.retain(|(name, _)| name != "run.json");
    }
    files.sort();
    files
}

fn fingerprint_of(key: &str) -> Option<String> {
    parse_public_key(key).ok().map(|k| public_key_id(&k))
}

/// What a user sees when `state.json` cannot be read: what it is, and what to do about it.
const STATE_UNREADABLE: &str = "the list of installed apps (state.json) cannot be read. If the file is damaged, close devstribution and move it out of the state folder: it then starts with no apps, and you install them again";

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

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(Paths {
            root: dir.path().to_path_buf(),
        })
        .unwrap();
        (dir, store)
    }

    /// A finished run with `kb` kilobytes of results and a log, journaled as a real one is.
    fn add_run(store: &Store, id: &str, status: RunStatus, started_at: u64, kb: usize) {
        let run_dir = store.paths.runs().join(id);
        let output_dir = run_dir.join("output");
        fs::create_dir_all(output_dir.join("sub")).unwrap();
        fs::write(
            output_dir.join("sub").join("report.xlsx"),
            vec![0; kb * 1024],
        )
        .unwrap();
        fs::write(run_dir.join("app.log"), "log").unwrap();
        let record = RunRecord {
            id: id.to_string(),
            app_id: "alice/hello".into(),
            app_title: "Hello".into(),
            version: "1.0.0".into(),
            kind: "task".into(),
            started_at,
            finished_at: Some(started_at + 1),
            status,
            duration_ms: 1000,
            outputs: Map::new(),
            error: None,
            hint: None,
            guard: Vec::new(),
            output_dir,
            result_folders: Vec::new(),
            output_removed: None,
        };
        store.save_run(&record).unwrap();
        store.journal_run(id);
    }

    #[test]
    fn cleanup_frees_results_but_keeps_the_history() {
        let (_dir, store) = store();
        let now = now_secs();
        add_run(
            &store,
            "20260101-000000-hello-aa",
            RunStatus::Ok,
            now - 90 * 86_400,
            4,
        );
        add_run(
            &store,
            "20260102-000000-hello-bb",
            RunStatus::Ok,
            now - 2 * 86_400,
            4,
        );
        add_run(
            &store,
            "20260103-000000-hello-cc",
            RunStatus::Running,
            now - 100 * 86_400,
            4,
        );

        let plan = store.cleanup_plan(Some(30), None, false).unwrap();
        assert_eq!(plan.runs.len(), 1, "only the old, finished run");
        assert_eq!(plan.runs[0].run_id, "20260101-000000-hello-aa");
        assert_eq!(plan.bytes, 4 * 1024 + 3);

        let ids = vec![plan.runs[0].run_id.clone()];
        let done = store.cleanup(&ids, false).unwrap();
        assert_eq!((done.runs, done.bytes), (1, 4 * 1024 + 3));
        assert!(done.failed.is_empty() && done.journal_error.is_none());

        let dir = store.paths.runs().join(&ids[0]);
        assert!(dir.join("run.json").is_file());
        assert!(!dir.join("output").exists() && !dir.join("app.log").exists());
        assert!(store.run(&ids[0]).unwrap().output_removed.is_some());
        assert_eq!(
            store.runs(None, 10).unwrap().len(),
            3,
            "still in the history"
        );
        assert!(
            store
                .cleanup_plan(Some(30), None, false)
                .unwrap()
                .runs
                .is_empty()
        );

        let report = store.journal_report();
        assert_eq!(report.broken_at, None);
        assert!(report.changed.is_empty() && report.missing.is_empty());
        assert_eq!(report.cleanups.len(), 1);
        assert_eq!(report.cleanups[0].data["runs"][0]["file_count"], 2);
    }

    #[test]
    fn diagnostics_say_what_happened_and_hide_paths() {
        let (dir, store) = store();
        add_run(&store, "20260101-000000-hello-aa", RunStatus::Error, 1, 1);
        let mut record = store.run("20260101-000000-hello-aa").unwrap();
        record.error = Some(format!(
            "FileNotFoundError: {}\\secret.xlsx\nsecond line",
            dir.path().display()
        ));
        store.save_run(&record).unwrap();

        let text = store.diagnostics("9.9.9", Some("uv 1.2.3"));
        assert!(text.contains("Launcher 9.9.9"));
        assert!(text.contains("uv 1.2.3"));
        assert!(text.contains("error alice/hello"), "{text}");
        assert!(text.contains("<data>"), "{text}");
        assert!(!text.contains(&dir.path().display().to_string()), "{text}");
        assert!(
            !text.contains("second line"),
            "only the first line of an error"
        );
        assert!(text.contains("adding up"));
        assert!(text.contains("Launcher log"));
    }

    #[test]
    fn a_running_run_is_never_cleaned() {
        let (_dir, store) = store();
        add_run(&store, "20260103-000000-hello-cc", RunStatus::Running, 1, 1);
        let done = store
            .cleanup(&["20260103-000000-hello-cc".to_string()], true)
            .unwrap();
        assert_eq!(done.runs, 0);
        assert_eq!(done.failed.len(), 1);
        assert!(
            store
                .paths
                .runs()
                .join("20260103-000000-hello-cc")
                .join("output")
                .is_dir()
        );
    }

    #[test]
    fn removing_the_record_too_leaves_no_gap_in_the_journal() {
        let (_dir, store) = store();
        add_run(&store, "20260101-000000-hello-aa", RunStatus::Error, 1, 1);
        let ids = vec!["20260101-000000-hello-aa".to_string()];
        store.cleanup(&ids, true).unwrap();
        assert!(!store.paths.runs().join(&ids[0]).exists());
        let report = store.journal_report();
        assert!(report.missing.is_empty(), "a cleanup line accounts for it");
    }

    #[test]
    fn what_the_launcher_did_not_do_shows_in_the_report() {
        let (_dir, store) = store();
        add_run(&store, "20260101-000000-hello-aa", RunStatus::Ok, 1, 1);
        add_run(&store, "20260102-000000-hello-bb", RunStatus::Ok, 1, 1);
        fs::remove_dir_all(store.paths.runs().join("20260101-000000-hello-aa")).unwrap();
        let edited = store
            .paths
            .runs()
            .join("20260102-000000-hello-bb")
            .join("run.json");
        let text = fs::read_to_string(&edited)
            .unwrap()
            .replace("1.0.0", "9.9.9");
        fs::write(&edited, text).unwrap();

        let report = store.journal_report();
        assert_eq!(report.missing, vec!["20260101-000000-hello-aa"]);
        assert_eq!(report.changed, vec!["20260102-000000-hello-bb"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_in_the_results_does_not_lead_cleanup_out_of_the_run() {
        let (dir, store) = store();
        add_run(&store, "20260101-000000-hello-aa", RunStatus::Ok, 1, 1);
        let outside = dir.path().join("keep-me");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious.txt"), "x").unwrap();
        let link = store
            .paths
            .runs()
            .join("20260101-000000-hello-aa")
            .join("output")
            .join("link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success();
        assert!(made, "mklink /J works without elevation");

        store
            .cleanup(&["20260101-000000-hello-aa".to_string()], true)
            .unwrap();
        assert!(outside.join("precious.txt").is_file());
    }
}

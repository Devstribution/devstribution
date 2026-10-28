// The launcher's backend API. In the app it calls the Rust core over Tauri IPC; opened in a
// plain browser (design work, screenshots) it falls back to a mock with demo data.

export type InputType = "file" | "folder" | "text" | "number" | "bool" | "choice" | "date" | "secret";

export interface Input {
  name: string;
  type: InputType;
  label?: string | null;
  help?: string | null;
  accept: string[];
  choices: string[];
  default?: unknown;
  required: boolean;
}

export interface Output {
  name: string;
  type: "file" | "table" | "text";
  label?: string | null;
}

export interface Permissions {
  network: string[];
  folders: string[];
  sap: boolean;
  secrets: string[];
}

export interface Origin {
  zone_id: number;
  host_url?: string | null;
  referrer_url?: string | null;
}

/** Free mode: `first_use` (key pinned before) or `new_publisher`. Managed mode: whether the
 * organisation's policy approves the author and key. */
export type Trust = "first_use" | "new_publisher" | "approved" | "not_approved";

export interface AppView {
  id: string;
  title: string;
  description?: string | null;
  version: string;
  kind: string;
  namespace: string;
  key_id: string;
  trust: Trust;
  icon?: string | null;
  inputs: Input[];
  outputs: Output[];
  permissions: Permissions;
  requires: string[];
  installed_at: number;
  origin?: Origin | null;
  last_inputs: Record<string, unknown>;
  secrets: { name: string; set: boolean }[];
  /** Set when the runtime guard stopped a run: the app does not run until the user decides. */
  quarantine?: Quarantine | null;
  /** Guard rules the user allowed for this app after a stop. */
  guard_allow: string[];
  /** False when the organisation keeps "Allow and run again" for IT. */
  may_allow_guard_rules: boolean;
}

/** What the runtime guard did: `stop` ended the run, `block` made one call fail, `observe` only reports. */
export interface GuardHit {
  action: "stop" | "block" | "observe";
  rule: string;
  message: string;
  detail: string;
}

export interface Quarantine {
  run_id: string;
  at: number;
  hit: GuardHit;
}

export type RunStatus = "running" | "ok" | "error" | "cancelled";

export interface RunRecord {
  id: string;
  app_id: string;
  app_title: string;
  version: string;
  kind: string;
  started_at: number;
  finished_at?: number | null;
  status: RunStatus;
  duration_ms: number;
  outputs: Record<string, unknown>;
  error?: string | null;
  guard?: GuardHit[];
  output_dir: string;
  /** When a cleanup removed the results and log; the run stays in the history. */
  output_removed?: number | null;
}

export interface CleanupRun {
  run_id: string;
  app_id: string;
  app_title: string;
  started_at: number;
  status: RunStatus;
  bytes: number;
}

export interface CleanupPlan {
  runs: CleanupRun[];
  bytes: number;
}

export interface CleanupResult {
  runs: number;
  bytes: number;
  failed: string[];
  journal_error?: string | null;
}

export interface JournalEntry {
  seq: number;
  at: number;
  kind: string;
  data: { runs?: { run_id: string; app_id: string; bytes: number; file_count: number; record_removed: boolean }[]; bytes?: number; remove_records?: boolean };
  hash: string;
}

/** The journal checked against the run folders. */
export interface JournalReport {
  file: string;
  lines: number;
  broken_at?: number | null;
  changed: string[];
  missing: string[];
  cleanups: JournalEntry[];
}

export interface RunResult {
  status: RunStatus;
  outputs: Record<string, unknown>;
  error?: string | null;
  traceback?: string | null;
  duration_ms: number;
  guard?: GuardHit[];
}

export type RunEvent =
  | { type: "progress"; value: number | null; message: string }
  | { type: "log"; level: string; message: string }
  | { type: "output"; line: string }
  | { type: "cancelling" }
  | ({ type: "guard" } & GuardHit)
  | { type: "finished"; result: RunResult };

export interface Stats {
  apps_installed: number;
  runs: number;
  ok: number;
  failed: number;
  cancelled: number;
  success_rate: number | null;
  total_duration_ms: number;
  per_day: { date: string; ok: number; error: number; cancelled: number }[];
  top_apps: { app_id: string; title: string; runs: number }[];
  /** Every app that ever ran, latest first; `runs` counts the stats window (30 days). */
  usage: { app_id: string; runs: number; last_run_at: number }[];
}

export interface Overview {
  apps: AppView[];
  stats: Stats;
  recent_runs: RunRecord[];
}

export type Severity = "info" | "warning" | "danger";

/** One rule matched in one file by the code scan (`devstribution_core::scan`). */
export interface Finding {
  rule: string;
  category: string;
  severity: Severity;
  message: string;
  path: string;
  line?: number | null;
  evidence: string;
  occurrences: number;
}

export interface ScanReport {
  files_scanned: number;
  findings: Finding[];
  /** The check of the packages in `uv.lock`, when the package has one. */
  dependencies?: { packages: number; list_date: string } | null;
}

/** What changed since the installed version (`devstribution_core::diff`). */
export interface AppDiff {
  old_version: string;
  new_version: string;
  /** What deserves a look first, most important first. */
  highlights: { severity: Severity; message: string }[];
  permissions: {
    network_added: string[]; network_removed: string[];
    folders_added: string[]; folders_removed: string[];
    secrets_added: string[]; secrets_removed: string[];
    sap?: boolean | null;
  };
  declared: string[];
  packages: { name: string; old?: string | null; new?: string | null }[];
  files: { path: string; status: "added" | "removed" | "changed"; binary: boolean; lines_added: number; lines_removed: number }[];
  findings_added: Finding[];
  findings_removed: Finding[];
}

export interface PendingInstall {
  token: string;
  id: string;
  title: string;
  description?: string | null;
  version: string;
  kind: string;
  namespace: string;
  key_id: string;
  trust: Trust;
  icon?: string | null;
  permissions: Permissions;
  requires: string[];
  platforms: string[];
  platform_supported: boolean;
  input_count: number;
  file_count: number;
  size_bytes: number;
  origin?: Origin | null;
  installed_version?: string | null;
  action: "install" | "update" | "reinstall";
  warnings: string[];
  scan: ScanReport;
  /** For an update: what changed since the installed version. */
  changes?: AppDiff | null;
  /** Set when the organisation's policy refuses the package: it cannot be installed. */
  blocked?: { reasons: string[]; contact?: string | null; author_not_approved: boolean } | null;
}

export interface StepEvent {
  key: string;
  step: "files" | "environment" | "ready";
  message: string;
}

/** Managed mode's policy, set by IT through the registry. */
export interface Policy {
  source: string;
  authors: { namespace: string; name?: string | null; fingerprints: string[] }[];
  contact?: string | null;
  dry_run: boolean;
  scan_danger: "refuse" | "ask";
  users_may_allow_guard_rules: boolean;
  guard_report_only: string[];
  min_launcher_version?: string | null;
  problems: string[];
  log_file: string;
}

export interface Settings {
  version: string;
  data_dir: string;
  log_file: string;
  platform: string;
  mode: "free" | "managed";
  policy?: Policy | null;
  uv_version?: string | null;
  publishers: { namespace: string; key_id: string; pinned_at: number; source: string }[];
}

type Unlisten = () => void;

export interface Api {
  preview: boolean;
  overview(): Promise<Overview>;
  getApp(id: string): Promise<AppView>;
  pickPackage(): Promise<string | null>;
  pickInput(input: Input): Promise<string | null>;
  /** Native yes/no dialog; `window.confirm` is not available in the launcher's webview. */
  ask(message: string, title: string, okLabel: string): Promise<boolean>;
  inspectPackage(path: string): Promise<PendingInstall>;
  confirmInstall(token: string): Promise<AppView>;
  discardInstall(token: string): Promise<void>;
  /** Stops the Python environment an install or Repair is preparing. */
  cancelEnvironment(): Promise<void>;
  uninstall(id: string): Promise<void>;
  repair(id: string): Promise<void>;
  /** Ends a quarantine; `allow` lets the rule that stopped the app pass from now on. */
  releaseQuarantine(id: string, allow: boolean): Promise<AppView>;
  startRun(id: string, inputs: Record<string, unknown>): Promise<RunRecord>;
  cancelRun(runId: string): Promise<void>;
  listRuns(appId?: string, limit?: number): Promise<RunRecord[]>;
  /** `null` when the run has no log file; an empty string when the app wrote nothing. */
  runLog(runId: string): Promise<string | null>;
  openOutput(runId: string, path: string | null, reveal: boolean): Promise<void>;
  openDataFolder(): Promise<void>;
  /** Opens the licences of the software inside devstribution. */
  openNotices(): Promise<void>;
  /** What a cleanup would remove. Nothing is deleted by asking. */
  cleanupPreview(olderThanDays: number | null, appId: string | null, removeRecords: boolean): Promise<CleanupPlan>;
  cleanupRuns(runIds: string[], removeRecords: boolean): Promise<CleanupResult>;
  cleanupLog(): Promise<JournalReport>;
  /** Plain text for a bug report: versions, apps, the latest runs. No secrets or results. */
  diagnostics(): Promise<string>;
  setSecret(id: string, name: string, value: string): Promise<void>;
  clearSecret(id: string, name: string): Promise<void>;
  settings(): Promise<Settings>;
  /** Starts devstribution's own uninstaller and closes the launcher. */
  uninstallLauncher(): Promise<void>;
  takePendingFiles(): Promise<string[]>;
  /** Saves a .trust file for a refused package's author, to send to IT. False if cancelled. */
  saveTrustRequest(token: string): Promise<boolean>;
  /** Run events arrive in batches (at most about ten a second), so a chatty app cannot flood the window. */
  onRunEvents(cb: (runId: string, events: RunEvent[]) => void): Promise<Unlisten>;
  onInstallStep(cb: (e: StepEvent) => void): Promise<Unlisten>;
  onRunsChanged(cb: () => void): Promise<Unlisten>;
  /** A finished run that could not be saved, with the reason. */
  onRunNotRecorded(cb: (message: string) => void): Promise<Unlisten>;
  onOpenFiles(cb: (paths: string[]) => void): Promise<Unlisten>;
  onDragDrop(cb: (e: { type: "over" | "leave" | "drop"; paths: string[] }) => void): Promise<Unlisten>;
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriApi(): Promise<Api> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  const dialog = await import("@tauri-apps/plugin-dialog");
  return {
    preview: false,
    overview: () => invoke("overview"),
    getApp: (id) => invoke("get_app", { id }),
    async pickPackage() {
      const path = await dialog.open({
        title: "Install an app",
        multiple: false,
        directory: false,
        // A downloaded .dsapp often arrives renamed to .zip; its content is checked all the same.
        filters: [{ name: "devstribution app (.dsapp, or .zip after a download)", extensions: ["dsapp", "zip"] }],
      });
      return typeof path === "string" ? path : null;
    },
    async pickInput(input) {
      const extensions = input.accept.map((a) => a.replace(/^\./, "")).filter(Boolean);
      const path = await dialog.open({
        title: input.label ?? input.name,
        multiple: false,
        directory: input.type === "folder",
        filters: input.type === "file" && extensions.length ? [{ name: input.label ?? "Files", extensions }] : undefined,
      });
      return typeof path === "string" ? path : null;
    },
    ask: (message, title, okLabel) => dialog.ask(message, { title, kind: "warning", okLabel, cancelLabel: "Cancel" }),
    inspectPackage: (path) => invoke("inspect_package", { path }),
    confirmInstall: (token) => invoke("confirm_install", { token }),
    discardInstall: (token) => invoke("discard_install", { token }),
    cancelEnvironment: () => invoke("cancel_environment"),
    uninstall: (id) => invoke("uninstall_app", { id }),
    repair: (id) => invoke("repair_app", { id }),
    releaseQuarantine: (id, allow) => invoke("release_quarantine", { id, allow }),
    startRun: (id, inputs) => invoke("start_run", { id, inputs }),
    cancelRun: (runId) => invoke("cancel_run", { runId }),
    listRuns: (appId, limit) => invoke("list_runs", { appId: appId ?? null, limit: limit ?? null }),
    runLog: (runId) => invoke("run_log", { runId }),
    openOutput: (runId, path, reveal) => invoke("open_output", { runId, path, reveal }),
    openDataFolder: () => invoke("open_data_folder"),
    openNotices: () => invoke("open_notices"),
    cleanupPreview: (olderThanDays, appId, removeRecords) => invoke("cleanup_preview", { olderThanDays, appId, removeRecords }),
    cleanupRuns: (runIds, removeRecords) => invoke("cleanup_runs", { runIds, removeRecords }),
    cleanupLog: () => invoke("cleanup_log"),
    diagnostics: () => invoke("diagnostics"),
    setSecret: (id, name, value) => invoke("set_secret", { id, name, value }),
    clearSecret: (id, name) => invoke("clear_secret", { id, name }),
    settings: () => invoke("settings"),
    uninstallLauncher: () => invoke("uninstall_launcher"),
    takePendingFiles: () => invoke("take_pending_files"),
    saveTrustRequest: (token) => invoke("save_trust_request", { token }),
    onRunEvents: (cb) =>
      listen<{ run_id: string; events: RunEvent[] }>("run-events", (e) => cb(e.payload.run_id, e.payload.events)),
    onInstallStep: (cb) => listen<StepEvent>("install-step", (e) => cb(e.payload)),
    onRunsChanged: (cb) => listen("runs-changed", () => cb()),
    onRunNotRecorded: (cb) => listen<string>("run-not-recorded", (e) => cb(e.payload)),
    onOpenFiles: (cb) => listen<string[]>("open-files", (e) => cb(e.payload)),
    onDragDrop: (cb) =>
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "over" || p.type === "enter") cb({ type: "over", paths: "paths" in p ? p.paths : [] });
        else if (p.type === "leave") cb({ type: "leave", paths: [] });
        else if (p.type === "drop") cb({ type: "drop", paths: p.paths });
      }),
  };
}

export async function createApi(): Promise<Api> {
  if (inTauri) return tauriApi();
  const { mockApi } = await import("./mock");
  return mockApi();
}

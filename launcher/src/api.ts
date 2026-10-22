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

export interface AppView {
  id: string;
  title: string;
  description?: string | null;
  version: string;
  kind: string;
  namespace: string;
  key_id: string;
  trust: "first_use" | "new_publisher";
  icon?: string | null;
  inputs: Input[];
  outputs: Output[];
  permissions: Permissions;
  requires: string[];
  installed_at: number;
  origin?: Origin | null;
  last_inputs: Record<string, unknown>;
  secrets: { name: string; set: boolean }[];
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
  output_dir: string;
}

export interface RunResult {
  status: RunStatus;
  outputs: Record<string, unknown>;
  error?: string | null;
  traceback?: string | null;
  duration_ms: number;
}

export type RunEvent =
  | { type: "progress"; value: number | null; message: string }
  | { type: "log"; level: string; message: string }
  | { type: "output"; line: string }
  | { type: "cancelling" }
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

export interface PendingInstall {
  token: string;
  id: string;
  title: string;
  description?: string | null;
  version: string;
  kind: string;
  namespace: string;
  key_id: string;
  trust: "first_use" | "new_publisher";
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
}

export interface StepEvent {
  key: string;
  step: "files" | "environment" | "ready";
  message: string;
}

export interface Settings {
  version: string;
  data_dir: string;
  platform: string;
  mode: string;
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
  uninstall(id: string): Promise<void>;
  repair(id: string): Promise<void>;
  startRun(id: string, inputs: Record<string, unknown>): Promise<RunRecord>;
  cancelRun(runId: string): Promise<void>;
  listRuns(appId?: string, limit?: number): Promise<RunRecord[]>;
  runLog(runId: string): Promise<string>;
  openOutput(runId: string, path: string | null, reveal: boolean): Promise<void>;
  setSecret(id: string, name: string, value: string): Promise<void>;
  clearSecret(id: string, name: string): Promise<void>;
  settings(): Promise<Settings>;
  takePendingFiles(): Promise<string[]>;
  /** Run events arrive in batches (at most about ten a second), so a chatty app cannot flood the window. */
  onRunEvents(cb: (runId: string, events: RunEvent[]) => void): Promise<Unlisten>;
  onInstallStep(cb: (e: StepEvent) => void): Promise<Unlisten>;
  onRunsChanged(cb: () => void): Promise<Unlisten>;
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
        filters: [{ name: "devstribution app", extensions: ["dsapp"] }],
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
    uninstall: (id) => invoke("uninstall_app", { id }),
    repair: (id) => invoke("repair_app", { id }),
    startRun: (id, inputs) => invoke("start_run", { id, inputs }),
    cancelRun: (runId) => invoke("cancel_run", { runId }),
    listRuns: (appId, limit) => invoke("list_runs", { appId: appId ?? null, limit: limit ?? null }),
    runLog: (runId) => invoke("run_log", { runId }),
    openOutput: (runId, path, reveal) => invoke("open_output", { runId, path, reveal }),
    setSecret: (id, name, value) => invoke("set_secret", { id, name, value }),
    clearSecret: (id, name) => invoke("clear_secret", { id, name }),
    settings: () => invoke("settings"),
    takePendingFiles: () => invoke("take_pending_files"),
    onRunEvents: (cb) =>
      listen<{ run_id: string; events: RunEvent[] }>("run-events", (e) => cb(e.payload.run_id, e.payload.events)),
    onInstallStep: (cb) => listen<StepEvent>("install-step", (e) => cb(e.payload)),
    onRunsChanged: (cb) => listen("runs-changed", () => cb()),
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

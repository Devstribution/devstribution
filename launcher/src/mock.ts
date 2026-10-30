// Demo backend for previewing the UI in a browser. Nothing here runs in the real launcher.

import type { Api, AppView, JournalEntry, PendingInstall, Policy, RunEvent, RunRecord, Settings, StepEvent, Stats, Trust } from "./api";

const now = () => Math.floor(Date.now() / 1000);
const DAY = 86_400;

/** `?managed` in the address shows the demo in managed mode, with an example policy. */
const MANAGED = typeof location !== "undefined" && new URLSearchParams(location.search).has("managed");
const UPDATE = typeof location !== "undefined" && new URLSearchParams(location.search).has("update");
const updateFolders: string[] = new URLSearchParams(location.search).has("update") ? ["C:\\Users\\edo\\Releases"] : [];

const MANAGED_POLICY: Policy = {
  source: "HKLM\\SOFTWARE\\Policies\\devstribution · \\\\fileserver\\it\\devstribution\\policy.toml",
  authors: [
    { namespace: "devstribution", name: "devstribution examples", fingerprints: ["2972-B9E7-77FF-C794-0A1B-6E33-91D0-5C2F"] },
    { namespace: "finance", name: "Finance automation team", fingerprints: ["8D41-C0A9-F2B3-7E10-44C2-19AB-E07D-3B58"] },
    { namespace: "ops", name: null, fingerprints: ["51E0-AB77-33C9-D2F4-7F02-8C61-A9E4-0D17"] },
  ],
  contact: "the IT service desk (it-help@example.com)",
  dry_run: false,
  scan_danger: "refuse",
  users_may_allow_guard_rules: false,
  guard_report_only: [],
  min_launcher_version: "0.1.0",
  update_folder: "\\\\fileserver\\it\\devstribution\\apps",
  problems: [],
  log_file: "C:\\Users\\edo\\AppData\\Local\\dev.devstribution.launcher\\data\\state\\policy.log",
};

const base = { trust: (MANAGED ? "approved" : "first_use") as Trust, requires: [], origin: null, last_inputs: {}, secrets: [], quarantine: null, guard_allow: [] as string[], may_allow_guard_rules: !MANAGED };
const noPerms = { network: [], folders: [], sap: false, secrets: [] };

const apps: AppView[] = [
  {
    ...base,
    id: "devstribution/excel-report",
    title: "Excel report",
    description: "Groups the rows of an Excel sheet by a column and writes a summary workbook.",
    version: "0.1.0",
    kind: "form",
    namespace: "devstribution",
    key_id: "2972B9E777FFC794",
    icon: null,
    inputs: [
      { name: "excel_file", type: "file", label: "Excel file", accept: [".xlsx"], choices: [], required: true },
      { name: "group_by", type: "text", label: "Column to group by", accept: [], choices: [], default: "Customer", required: true },
      { name: "sum_column", type: "text", label: "Column to add up", help: "Optional: leave empty to only count rows.", accept: [], choices: [], required: false },
    ],
    outputs: [
      { name: "report", type: "file", label: "Summary workbook" },
      { name: "summary", type: "text" },
    ],
    permissions: { ...noPerms, folders: ["user-selected"] },
    installed_at: now() - 9 * DAY,
    last_inputs: { excel_file: "C:\\Users\\edo\\Documents\\orders-2026.xlsx", group_by: "Customer", sum_column: "Amount" },
  },
  {
    ...base,
    id: "finance/invoice-extractor",
    title: "Invoice extractor",
    description: "Reads PDF invoices from a folder, extracts totals and VAT, and posts them to the accounting API.",
    version: "1.4.2",
    kind: "form",
    namespace: "finance",
    key_id: "8D41C0A9F2B37E10",
    icon: null,
    inputs: [
      { name: "folder", type: "folder", label: "Invoices folder", accept: [], choices: [], required: true },
      { name: "since", type: "date", label: "Invoices since", accept: [], choices: [], required: false },
      { name: "currency", type: "choice", label: "Currency", accept: [], choices: ["EUR", "USD", "GBP"], default: "EUR", required: true },
      { name: "post", type: "bool", label: "Post to accounting", help: "Off = dry run, only the report is written.", accept: [], choices: [], default: false, required: true },
    ],
    outputs: [{ name: "report", type: "file", label: "Extraction report" }],
    permissions: { network: ["api.accounting.example.com"], folders: ["user-selected"], sap: false, secrets: ["api_token"] },
    secrets: [{ name: "api_token", set: true }],
    installed_at: now() - 21 * DAY,
  },
  {
    ...base,
    id: "ops/sap-stock-check",
    title: "SAP stock check",
    description: "Checks stock levels for a list of materials in SAP and flags the ones below their reorder point.",
    version: "2.0.1",
    kind: "form",
    namespace: "ops",
    key_id: "51E0AB7733C9D2F4",
    icon: null,
    inputs: [
      { name: "materials", type: "file", label: "Materials list", accept: [".xlsx", ".csv"], choices: [], required: true },
      { name: "plant", type: "number", label: "Plant", accept: [], choices: [], default: 1000, required: true },
    ],
    outputs: [{ name: "report", type: "file", label: "Stock report" }],
    permissions: { ...noPerms, sap: true, folders: ["user-selected"] },
    requires: ["sap-gui-scripting"],
    installed_at: now() - 3 * DAY,
  },
  {
    ...base,
    id: "acme/supplier-reconciliation",
    title: "Supplier reconciliation",
    description: "Matches supplier statements against the ledger and flags the differences.",
    version: "1.2.0",
    kind: "form",
    namespace: "acme",
    key_id: "C3F19A0B66D2E871",
    icon: null,
    inputs: [
      { name: "statement", type: "file", label: "Supplier statement", accept: [".xlsx", ".pdf"], choices: [], required: true },
      { name: "ledger", type: "file", label: "Ledger export", accept: [".xlsx"], choices: [], required: true },
    ],
    outputs: [{ name: "report", type: "file", label: "Reconciliation" }],
    permissions: { ...noPerms, folders: ["user-selected"] },
    requires: ["excel"],
    installed_at: now() - 6 * DAY,
  },
  {
    ...base,
    id: "finance/bank-statements",
    title: "Bank statements import",
    description: "Downloads the month's statements from the bank portal and adds them to the ledger file.",
    version: "0.9.0",
    kind: "form",
    namespace: "finance",
    key_id: "8D41C0A9F2B37E10",
    icon: null,
    inputs: [{ name: "month", type: "date", label: "Month", accept: [], choices: [], required: true }],
    outputs: [{ name: "summary", type: "text", label: "Summary" }],
    permissions: { network: ["bank.example.com"], folders: [], sap: false, secrets: [] },
    installed_at: now() - 12 * DAY,
    // Shows the runtime guard's quarantine in the browser demo.
    quarantine: {
      run_id: "demo-stopped",
      at: now() - 2 * 3600,
      hit: {
        action: "stop", rule: "start-with-windows", message: "tried to make a program start with Windows",
        detail: "created hkcu\\software\\microsoft\\windows\\currentversion\\run",
      },
    },
  },
  {
    ...base,
    id: "devstribution/hello-gui",
    title: "Hello GUI",
    description: "A tkinter window: the smallest app that brings its own UI.",
    version: "0.1.0",
    kind: "gui",
    namespace: "devstribution",
    key_id: "2972B9E777FFC794",
    icon: null,
    inputs: [],
    outputs: [],
    permissions: noPerms,
    installed_at: now() - 1 * DAY,
  },
];

function seeded(seed: number) {
  return () => {
    seed = (seed * 16807) % 2147483647;
    return seed / 2147483647;
  };
}

function demoRuns(): RunRecord[] {
  const rand = seeded(42);
  const runs: RunRecord[] = [];
  // Weighted so "often" and "recent" differ: the reconciliation app ran last, Excel report most.
  const weights = [0.42, 0.16, 0.08, 0.1, 0.24];
  const pick = () => { let x = rand(); for (let i = 0; i < weights.length; i++) { x -= weights[i]; if (x < 0) return apps[i]; } return apps[0]; };
  for (let d = 29; d >= 0; d--) {
    const n = Math.floor(rand() * 4) + (d % 7 < 5 ? 2 : 0);
    for (let i = 0; i < n; i++) {
      const app = d === 0 && i === n - 1 ? apps[3] : pick();
      const started = now() - d * DAY - Math.floor(rand() * 8 * 3600);
      const r = rand();
      const status = r > 0.93 ? "error" : r > 0.88 ? "cancelled" : "ok";
      const dur = Math.floor(2000 + rand() * 90_000);
      runs.push({
        id: `${started}-${app.id.split("/")[1]}`,
        app_id: app.id,
        app_title: app.title,
        version: app.version,
        kind: app.kind,
        started_at: started,
        finished_at: started + Math.floor(dur / 1000),
        status,
        duration_ms: dur,
        outputs: status === "ok" ? { report: `C:\\Users\\edo\\AppData\\Local\\devstribution\\runs\\${started}\\output\\report.xlsx` } : {},
        error: status === "error" ? "ValueError: column 'Amount' not found; the sheet has: Order, Customer, Total" : null,
        hint: status === "error" ? "This is most likely a problem in the app, not something you did. Copy the details and send them to the app’s author." : null,
        output_dir: `C:\\Users\\edo\\AppData\\Local\\devstribution\\runs\\${started}\\output`,
      });
    }
  }
  return runs.sort((a, b) => b.started_at - a.started_at);
}

let runs = demoRuns();
/** Set by cancelEnvironment: the demo install stops at its next step. */
let envCancelled = false;
const cleanups: JournalEntry[] = [];

function stats(): Stats {
  const per_day = [] as Stats["per_day"];
  for (let d = 29; d >= 0; d--) {
    const date = new Date((now() - d * DAY) * 1000).toISOString().slice(0, 10);
    per_day.push({ date, ok: 0, error: 0, cancelled: 0 });
  }
  const top = new Map<string, { app_id: string; title: string; runs: number }>();
  let ok = 0, failed = 0, cancelled = 0, total = 0;
  for (const r of runs) {
    const date = new Date(r.started_at * 1000).toISOString().slice(0, 10);
    const day = per_day.find((p) => p.date === date);
    if (!day || r.status === "running") continue;
    if (r.status === "ok") { day.ok++; ok++; }
    if (r.status === "error") { day.error++; failed++; }
    if (r.status === "cancelled") { day.cancelled++; cancelled++; }
    total += r.duration_ms;
    const t = top.get(r.app_id) ?? { app_id: r.app_id, title: r.app_title, runs: 0 };
    t.runs++;
    top.set(r.app_id, t);
  }
  return {
    apps_installed: apps.length,
    runs: ok + failed + cancelled,
    ok, failed, cancelled,
    success_rate: ok + failed ? ok / (ok + failed) : null,
    total_duration_ms: total,
    per_day,
    top_apps: [...top.values()].sort((a, b) => b.runs - a.runs).slice(0, 5),
    usage: [...new Set(runs.map((r) => r.app_id))].map((app_id) => ({
      app_id,
      runs: top.get(app_id)?.runs ?? 0,
      last_run_at: Math.max(...runs.filter((r) => r.app_id === app_id).map((r) => r.started_at)),
    })).sort((a, b) => b.last_run_at - a.last_run_at),
  };
}

const pending: PendingInstall = {
  token: "demo",
  id: "acme-ops/weekly-kpi-digest",
  title: "Weekly KPI digest",
  description: "Collects the week's KPIs from the shared drive and builds the Monday slide deck.",
  version: "1.0.0",
  kind: "form",
  namespace: "acme-ops",
  key_id: "C3F19A0B66D2E871",
  trust: "new_publisher",
  icon: null,
  permissions: { network: ["sharepoint.acme.example"], folders: ["user-selected"], sap: false, secrets: ["sharepoint_token"] },
  requires: ["excel"],
  platforms: ["windows"],
  platform_supported: true,
  input_count: 2,
  file_count: 7,
  size_bytes: 48_211,
  origin: { zone_id: 3, host_url: "https://github.com/acme-ops/weekly-kpi-digest/releases/download/v1.0.0/weekly-kpi-digest-1.0.0.dsapp" },
  installed_version: null,
  action: "install",
  warnings: [
    "First app from `acme-ops` on this computer. Its name is not verified: install it only if you trust where you got it. Future versions must come from the same key.",
    "This file was downloaded from the internet.",
  ],
  scan: {
    files_scanned: 4,
    findings: [
      { rule: "dynamic-code", category: "dynamic_code", severity: "warning", message: "Runs code that is built while the app runs, which cannot be reviewed in advance", path: "kpi/rules.py", line: 41, evidence: "eval(...)", occurrences: 1 },
      { rule: "network", category: "network", severity: "info", message: "Connects to other computers", path: "kpi/sharepoint.py", line: 3, evidence: "import requests", occurrences: 4 },
      { rule: "processes", category: "processes", severity: "info", message: "Starts other programs", path: "main.py", line: 12, evidence: "subprocess.run(...)", occurrences: 1 },
    ],
    dependencies: { packages: 14, list_date: "2026-10-23" },
  },
};

type Listener<T> = (v: T) => void;

export function mockApi(): Api {
  const runListeners: Listener<[string, RunEvent]>[] = [];
  const stepListeners: Listener<StepEvent>[] = [];
  const updateListeners: Listener<[number, number | null]>[] = [];
  const changed: Listener<void>[] = [];
  const timers = new Map<string, number>();
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const emit = (id: string, e: RunEvent) => { for (const l of runListeners) l([id, e]); };
  const off = <T>(list: T[], item: T) => () => { const i = list.indexOf(item); if (i >= 0) list.splice(i, 1); };

  return {
    preview: true,
    async overview() {
      return { apps: structuredClone(apps), stats: stats(), recent_runs: runs.slice(0, 8) };
    },
    async getApp(id) {
      const a = apps.find((x) => x.id === id);
      if (!a) throw new Error(`\`${id}\` is not installed`);
      return structuredClone(a);
    },
    async pickPackage() { return "C:\\Users\\edo\\Downloads\\weekly-kpi-digest-1.0.0.dsapp"; },
    async pickInput(input) {
      return input.type === "folder" ? "C:\\Users\\edo\\Documents\\Invoices" : "C:\\Users\\edo\\Documents\\orders-2026.xlsx";
    },
    async ask(message) { return window.confirm(message); },
    async inspectPackage() {
      await sleep(500);
      const p = structuredClone(pending);
      if (MANAGED) {
        // The demo's managed mode does not approve this package's author.
        p.trust = "not_approved";
        p.warnings = p.warnings.filter((w) => !w.startsWith("First app"));
        p.blocked = { reasons: ["your organisation has not approved the author `acme-ops`"], contact: MANAGED_POLICY.contact, author_not_approved: true };
      } else if (UPDATE) {
        // `?update`: the same app over an installed 0.9.0 that asked for less.
        p.action = "update";
        p.installed_version = "0.9.0";
        p.trust = "first_use";
        p.warnings = [];
        p.origin = null;
        p.changes = {
          old_version: "0.9.0",
          new_version: "1.0.0",
          highlights: [
            { severity: "warning", message: "Now connects to sharepoint.acme.example" },
            { severity: "warning", message: "New in the code: Runs code that is built while the app runs, which cannot be reviewed in advance (kpi/rules.py)" },
            { severity: "warning", message: "1 new package: msal" },
            { severity: "info", message: "New in the code: Connects to other computers (kpi/sharepoint.py)" },
            { severity: "info", message: "2 packages updated" },
          ],
          permissions: { network_added: ["sharepoint.acme.example"], network_removed: [], folders_added: [], folders_removed: [], secrets_added: [], secrets_removed: [], sap: null },
          declared: ["inputs: workbook → workbook, week"],
          packages: [{ name: "msal", old: null, new: "1.31.0" }, { name: "openpyxl", old: "3.1.2", new: "3.1.5" }, { name: "requests", old: "2.32.2", new: "2.32.3" }],
          files: [
            { path: "kpi/rules.py", status: "changed", binary: false, lines_added: 14, lines_removed: 3 },
            { path: "kpi/sharepoint.py", status: "added", binary: false, lines_added: 52, lines_removed: 0 },
            { path: "main.py", status: "changed", binary: false, lines_added: 6, lines_removed: 2 },
            { path: "pyproject.toml", status: "changed", binary: false, lines_added: 4, lines_removed: 1 },
            { path: "uv.lock", status: "changed", binary: false, lines_added: 40, lines_removed: 6 },
          ],
          findings_added: [],
          findings_removed: [],
        };
      }
      return p;
    },
    async confirmInstall() {
      const steps: [StepEvent["step"], string][] = [
        ["files", "Copying the verified files"],
        ["environment", "Using CPython 3.12.8"],
        ["environment", "Resolved 9 packages in 12ms"],
        ["environment", "Prepared 9 packages in 1.21s"],
        ["environment", "Installed 9 packages in 48ms"],
        ["ready", "Ready"],
      ];
      envCancelled = false;
      for (const [step, message] of steps) {
        await sleep(450);
        if (envCancelled) throw new Error("preparing the Python environment was cancelled");
        for (const l of stepListeners) l({ key: "demo", step, message });
      }
      const app: AppView = {
        ...base,
        id: pending.id, title: pending.title, description: pending.description, version: pending.version,
        kind: pending.kind, namespace: pending.namespace, key_id: pending.key_id, icon: null,
        inputs: [
          { name: "week", type: "date", label: "Week starting", accept: [], choices: [], required: true },
          { name: "team", type: "choice", label: "Team", accept: [], choices: ["Sales", "Ops", "Support"], default: "Ops", required: true },
        ],
        outputs: [{ name: "deck", type: "file", label: "Slide deck" }],
        permissions: pending.permissions, requires: pending.requires,
        installed_at: now(), secrets: [{ name: "sharepoint_token", set: false }],
      };
      apps.push(app);
      return structuredClone(app);
    },
    async discardInstall() {},
    async cancelEnvironment() { envCancelled = true; },
    async releaseQuarantine(id, allow) {
      const app = apps.find((a) => a.id === id)!;
      if (app.quarantine && allow) app.guard_allow = [...app.guard_allow, app.quarantine.hit.rule];
      app.quarantine = null;
      return structuredClone(app);
    },
    async uninstall(id) { const i = apps.findIndex((a) => a.id === id); if (i >= 0) apps.splice(i, 1); },
    async repair(id) {
      envCancelled = false;
      for (const m of ["Rebuilding the Python environment", "Installed 3 packages in 31ms", "Ready"]) {
        await sleep(400);
        if (envCancelled) throw new Error("preparing the Python environment was cancelled");
        for (const l of stepListeners) l({ key: id, step: m === "Ready" ? "ready" : "environment", message: m });
      }
    },
    async startRun(id, inputs) {
      const app = apps.find((a) => a.id === id)!;
      app.last_inputs = { ...inputs };
      const record: RunRecord = {
        id: `${now()}-${id.split("/")[1]}-${Math.random().toString(16).slice(2, 6)}`,
        app_id: id, app_title: app.title, version: app.version, kind: app.kind,
        started_at: now(), finished_at: null, status: "running", duration_ms: 0, outputs: {}, error: null,
        output_dir: "C:\\Users\\edo\\AppData\\Local\\devstribution\\runs\\demo\\output",
      };
      runs.unshift(record);
      const script: RunEvent[] = app.kind === "gui"
        ? [{ type: "log", level: "info", message: "Window opened" }, { type: "progress", value: null, message: "Running: close the app's window to finish" }]
        : [
            { type: "progress", value: 0.05, message: "Opening orders-2026.xlsx" },
            { type: "output", line: "openpyxl: reading sheet 'Orders' (read-only mode)" },
            { type: "progress", value: 0.25, message: "Reading row 500" },
            { type: "progress", value: 0.48, message: "Reading row 1000" },
            { type: "progress", value: 0.71, message: "Reading row 1500" },
            { type: "log", level: "info", message: "1,842 rows, 6 groups" },
            { type: "progress", value: 0.95, message: "Writing the report" },
          ];
      let i = 0;
      const started = Date.now();
      const timer = window.setInterval(() => {
        if (i < script.length) { emit(record.id, script[i++]); return; }
        if (app.kind === "gui" && Date.now() - started < 5000) return;
        window.clearInterval(timer);
        timers.delete(record.id);
        const result = {
          status: "ok" as const,
          outputs: app.outputs.length
            ? { report: `${record.output_dir}\\orders-2026-by-customer.xlsx`, summary: "1842 rows in 6 groups of Customer" }
            : {},
          duration_ms: Date.now() - started,
          // What the runtime guard reported, for the result's "What it did" line.
          guard: [
            { action: "observe" as const, rule: "program", message: "C:\\Windows\\system32\\cmd.exe /c ver", detail: "" },
            { action: "observe" as const, rule: "files", message: "wrote 1 file in its output folder", detail: "" },
          ],
        };
        Object.assign(record, { status: "ok", outputs: result.outputs, duration_ms: result.duration_ms, finished_at: now(), guard: result.guard });
        emit(record.id, { type: "finished", result });
        for (const l of changed) l();
      }, 650);
      timers.set(record.id, timer);
      return structuredClone(record);
    },
    async cancelRun(runId) {
      const t = timers.get(runId);
      if (t === undefined) return;
      emit(runId, { type: "cancelling" });
      await sleep(600);
      window.clearInterval(t);
      timers.delete(runId);
      const r = runs.find((x) => x.id === runId)!;
      Object.assign(r, { status: "cancelled", finished_at: now() });
      emit(runId, { type: "finished", result: { status: "cancelled", outputs: {}, error: "the app was stopped", duration_ms: 1200 } });
      for (const l of changed) l();
    },
    async listRuns(appId, limit) { return runs.filter((r) => !appId || r.app_id === appId).slice(0, limit ?? 200); },
    async runLog() {
      return "openpyxl: reading sheet 'Orders' (read-only mode)\nTraceback (most recent call last):\n  File \"main.py\", line 17, in run\n    sum_index = _column(header, sum_column)\nValueError: column 'Amount' not found; the sheet has: Order, Customer, Total\n";
    },
    async openOutput() {},
    async openDataFolder() {},
    async openNotices() {},
    async cleanupPreview(olderThanDays, appId, removeRecords) {
      const cutoff = olderThanDays === null ? Infinity : now() - olderThanDays * DAY;
      const list = runs
        .filter((r) => r.status !== "running" && r.started_at <= cutoff && (!appId || r.app_id === appId) && (removeRecords || !r.output_removed))
        .map((r) => ({ run_id: r.id, app_id: r.app_id, app_title: r.app_title, started_at: r.started_at, status: r.status, bytes: r.output_removed ? 0 : 40_000 + (r.started_at % 9) * 310_000 }));
      return { runs: list, bytes: list.reduce((n, r) => n + r.bytes, 0) };
    },
    async cleanupRuns(ids, removeRecords) {
      const hit = runs.filter((r) => ids.includes(r.id));
      const bytes = hit.reduce((n, r) => n + (r.output_removed ? 0 : 40_000 + (r.started_at % 9) * 310_000), 0);
      if (removeRecords) runs = runs.filter((r) => !ids.includes(r.id));
      else hit.forEach((r) => { r.output_removed = now(); });
      cleanups.unshift({ seq: cleanups.length, at: now(), kind: "cleanup", hash: "", data: { runs: hit.map((r) => ({ run_id: r.id, app_id: r.app_id, bytes: 0, file_count: 2, record_removed: removeRecords })), bytes, remove_records: removeRecords } });
      return { runs: hit.length, bytes, failed: [] };
    },
    async diagnostics() {
      return ["devstribution diagnostics", "Launcher 0.2.0 on windows, free mode", "uv 0.9.0", "", `Apps (${apps.length})`,
        ...apps.map((a) => `  ${a.id} ${a.version} (${a.kind})`), "", "Latest runs",
        ...runs.slice(0, 10).map((r) => `  ${new Date(r.started_at * 1000).toISOString().slice(0, 10)} ${r.status} ${r.app_id} ${r.duration_ms} ms`), "", "Journal", `  ${runs.length} lines, adding up`, "  0 cleanups", ""].join("\n");
    },
    async cleanupLog() { return { file: "C:\\Users\\edo\\AppData\\Local\\devstribution\\state\\journal.jsonl", lines: runs.length + cleanups.length, changed: [], missing: [], cleanups }; },
    async setSecret(id, name) { const a = apps.find((x) => x.id === id); const s = a?.secrets.find((x) => x.name === name); if (s) s.set = true; },
    async clearSecret(id, name) { const a = apps.find((x) => x.id === id); const s = a?.secrets.find((x) => x.name === name); if (s) s.set = false; },
    async settings(): Promise<Settings> {
      return {
        version: "0.1.0",
        data_dir: "C:\\Users\\edo\\AppData\\Local\\devstribution",
        log_file: "C:\\Users\\edo\\AppData\\Local\\devstribution\\logs\\launcher.log",
        platform: "windows",
        mode: MANAGED ? "managed" : "free",
        policy: MANAGED ? MANAGED_POLICY : null,
        uv_version: "uv 0.13.4",
        updates: true,
        update_folders: updateFolders,
        publishers: [
          { namespace: "devstribution", key_id: "2972B9E777FFC794", pinned_at: now() - 9 * DAY, source: "first-use" },
          { namespace: "finance", key_id: "8D41C0A9F2B37E10", pinned_at: now() - 21 * DAY, source: "first-use" },
          { namespace: "ops", key_id: "51E0AB7733C9D2F4", pinned_at: now() - 3 * DAY, source: "first-use" },
          { namespace: "acme", key_id: "C3F19A0B66D2E871", pinned_at: now() - 6 * DAY, source: "first-use" },
        ],
      };
    },
    async uninstallLauncher() { throw new Error("The browser demo has no uninstaller: in the launcher, this opens devstribution's own uninstaller."); },
    async takePendingFiles() { return []; },
    async saveTrustRequest() { return false; },
    async checkAppUpdates() {
      await sleep(300);
      if (!updateFolders.length && !MANAGED) return { updates: [], problems: [] };
      const a = apps.find((x) => x.id === "ops/sap-stock-check");
      return {
        updates: a && a.version === "2.0.1"
          ? [{ id: a.id, version: "2.1.0", installed_version: a.version, path: "C:\\Users\\edo\\Releases\\sap-stock-check-2.1.0.dsapp" }]
          : [],
        problems: [],
      };
    },
    async addUpdateFolder() {
      if (!updateFolders.includes("C:\\Users\\edo\\Releases")) updateFolders.push("C:\\Users\\edo\\Releases");
      return [...updateFolders];
    },
    async removeUpdateFolder(folder) {
      updateFolders.splice(updateFolders.indexOf(folder), 1);
      return [...updateFolders];
    },
    async checkUpdate() {
      await sleep(600);
      return {
        version: "0.3.0", current: "0.1.0",
        notes: "## Added\n\n- **The launcher updates itself**: it looks for a new version when it starts.\n- Keyboard and screen reader basics.\n\n## Fixed\n\n- Error messages stay until you close them.",
      };
    },
    async installUpdate() {
      const total = 14_200_000;
      for (let done = 0; done <= total; done += 1_420_000) { for (const l of updateListeners) l([done, total]); await sleep(250); }
      throw new Error("The browser demo cannot install updates: in the launcher, devstribution closes here and the new version installs.");
    },
    async onUpdateProgress(cb) { const l: Listener<[number, number | null]> = ([d, t]) => cb(d, t); updateListeners.push(l); return off(updateListeners, l); },
    async onRunEvents(cb) { const l: Listener<[string, RunEvent]> = ([id, e]) => cb(id, [e]); runListeners.push(l); return off(runListeners, l); },
    async onInstallStep(cb) { stepListeners.push(cb); return off(stepListeners, cb); },
    async onRunsChanged(cb) { changed.push(cb); return off(changed, cb); },
    async onRunNotRecorded() { return () => {}; },
    async onOpenFiles() { return () => {}; },
    async onDragDrop() { return () => {}; },
  };
}

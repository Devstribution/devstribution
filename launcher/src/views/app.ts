import {
  ArrowLeft, Play, Square, MoreHorizontal, Wrench, Trash2, FolderOpen, FileCheck2, ExternalLink,
  CircleCheck, CircleX, CircleSlash, Loader, Globe, Folder, KeyRound, Database, Puzzle, Sparkles, ShieldCheck, ShieldAlert,
} from "lucide";
import type { AppView, Input, RunRecord } from "../api";
import type { Ctx, LiveRun, Rendered } from "../main";
import { h, icon, appIcon, clear, duration, fileName, relTime, statusPill, toast, errorText, trustChip, KIND_LABEL } from "../ui";

/** Values the form starts with: last run's values, else the manifest defaults. */
function initialValues(app: AppView): Record<string, unknown> {
  const v: Record<string, unknown> = {};
  for (const i of app.inputs) {
    if (i.type === "secret") continue;
    v[i.name] = app.last_inputs[i.name] ?? i.default ?? (i.type === "bool" ? false : null);
  }
  return v;
}

function missingRequired(app: AppView, values: Record<string, unknown>): Input[] {
  return app.inputs.filter((i) => i.type !== "secret" && i.required && (values[i.name] === null || values[i.name] === undefined || values[i.name] === ""));
}

/** Starts a run (asking for missing secrets first) and remembers its live state. */
export async function startRun(ctx: Ctx, app: AppView, values: Record<string, unknown>): Promise<LiveRun | null> {
  for (const s of app.secrets.filter((x) => !x.set)) {
    const value = await askSecret(app, s.name);
    if (value === null) return null;
    await ctx.api.setSecret(app.id, s.name, value);
    s.set = true;
  }
  try {
    const record = await ctx.api.startRun(app.id, values);
    const run: LiveRun = {
      runId: record.id, appId: app.id, startedAt: Date.now(), progress: null,
      message: app.kind === "form" ? "Starting" : "Opening the app", lines: [], cancelling: false,
    };
    ctx.live.set(record.id, run);
    return run;
  } catch (e) {
    toast(errorText(e), "error");
    return null;
  }
}

/** ▶ on a tile: run right away when nothing needs filling in, else open the app page. */
export async function quickRun(ctx: Ctx, app: AppView) {
  const values = initialValues(app);
  if (missingRequired(app, values).length) {
    ctx.navigate(`#/app/${encodeURIComponent(app.id)}`);
    return;
  }
  if (await startRun(ctx, app, values)) ctx.navigate(`#/app/${encodeURIComponent(app.id)}`);
}

function askSecret(app: AppView, name: string): Promise<string | null> {
  return new Promise((resolve) => {
    const input = h("input", { class: "input", type: "password", autocomplete: "off", placeholder: "Paste the value" });
    const done = (v: string | null) => { overlay.remove(); resolve(v); };
    const overlay = h("div", { class: "overlay", onclick: (e: Event) => { if (e.target === overlay) done(null); } },
      h("div", { class: "modal", style: "width:min(460px,calc(100vw - 48px))" },
        h("div", { class: "modal-head" }, h("div", { class: "kpi-icon violet" }, icon(KeyRound, 20)),
          h("div", null, h("h2", null, `${app.title} needs “${name}”`), h("div", { class: "muted" }, "Asked once. Kept in your system’s credential manager, never in files."))),
        h("div", { class: "modal-body" }, input),
        h("div", { class: "modal-foot" },
          h("button", { class: "btn", onclick: () => done(null) }, "Cancel"),
          h("button", { class: "btn primary", onclick: () => input.value && done(input.value) }, "Save and continue"))));
    input.addEventListener("keydown", (e) => { if (e.key === "Enter" && input.value) done(input.value); if (e.key === "Escape") done(null); });
    document.body.appendChild(overlay);
    input.focus();
  });
}

function field(ctx: Ctx, input: Input, values: Record<string, unknown>): HTMLElement {
  const label = input.label ?? input.name;
  const set = (v: unknown) => { values[input.name] = v; };
  let control: HTMLElement;
  const v = values[input.name];
  switch (input.type) {
    case "file":
    case "folder": {
      const shown = h("input", { class: `input ${v ? "" : "is-empty"}`, readonly: true, value: v ? String(v) : "", placeholder: input.type === "file" ? `Choose a file${input.accept.length ? ` (${input.accept.join(", ")})` : ""}` : "Choose a folder" });
      const pick = async () => {
        const p = await ctx.api.pickInput(input);
        if (p) { set(p); shown.value = p; shown.classList.remove("is-empty"); }
      };
      shown.addEventListener("click", pick);
      control = h("div", { class: "file-field" }, shown, h("button", { class: "btn", onclick: pick }, icon(input.type === "file" ? FileCheck2 : Folder, 16), "Browse"));
      break;
    }
    case "bool": {
      const sw = h("button", { class: `switch ${v ? "on" : ""}`, role: "switch", "aria-checked": String(!!v), "aria-label": label });
      sw.addEventListener("click", () => { const on = !sw.classList.contains("on"); sw.classList.toggle("on", on); sw.setAttribute("aria-checked", String(on)); set(on); });
      control = sw;
      break;
    }
    case "choice": {
      if (input.choices.length <= 4) {
        const seg = h("div", { class: "segmented" });
        const draw = () => seg.replaceChildren(...input.choices.map((c) =>
          h("button", { class: values[input.name] === c ? "on" : "", onclick: () => { set(c); draw(); } }, c)));
        draw();
        control = seg;
      } else {
        const sel = h("select", { class: "input" }, input.choices.map((c) => h("option", { value: c, selected: v === c }, c)));
        sel.addEventListener("change", () => set(sel.value));
        control = sel;
      }
      break;
    }
    default: {
      const type = input.type === "number" ? "number" : input.type === "date" ? "date" : "text";
      const el = h("input", { class: "input", type, value: v === null || v === undefined ? "" : String(v) });
      el.addEventListener("input", () => set(type === "number" ? (el.value === "" ? null : Number(el.value)) : el.value));
      control = el;
    }
  }
  return h("div", { class: "field" },
    h("label", null, label, input.required ? h("span", { class: "req" }, "*") : null),
    control,
    input.help ? h("div", { class: "help" }, input.help) : null);
}

function perms(app: AppView): HTMLElement {
  const items: [typeof Globe, string, string][] = [];
  const p = app.permissions;
  if (p.network.length) items.push([Globe, "Internet", p.network.join(", ")]);
  if (p.folders.length) items.push([Folder, "Files", p.folders.includes("user-selected") ? "Files and folders you choose" : p.folders.join(", ")]);
  if (p.secrets.length) items.push([KeyRound, "Secrets", p.secrets.join(", ")]);
  if (p.sap) items.push([Database, "SAP", "Uses your SAP GUI session"]);
  if (app.requires.length) items.push([Puzzle, "Needs", app.requires.join(", ")]);
  if (!items.length) items.push([Sparkles, "Nothing special", "No internet, files or secrets declared"]);
  return h("div", { class: "perm-list" }, items.map(([ic, t, s]) =>
    h("div", { class: "perm" }, h("div", { class: "pi" }, icon(ic, 17)), h("div", null, h("div", { class: "pt" }, t), h("div", { class: "ps" }, s)))));
}

export async function appView(ctx: Ctx, id: string): Promise<Rendered> {
  const app = await ctx.api.getApp(id);
  const values = initialValues(app);
  const isForm = app.kind === "form";

  // ---------- run panel ----------
  const panel = h("div", { class: "run-panel" });
  const logBox = h("div", { class: "log" });
  const logCard = h("details", { class: "card" }, h("summary", null, "Activity log"), h("div", { style: "margin-top:12px" }, logBox));
  let run = ctx.liveFor(app.id);
  let timer: number | undefined;

  const runButton = h("button", { class: "btn primary big" }, icon(Play, 20), isForm ? "Run" : "Open");
  runButton.addEventListener("click", async () => {
    const missing = missingRequired(app, values);
    if (missing.length) { toast(`Fill in: ${missing.map((i) => i.label ?? i.name).join(", ")}`, "error"); return; }
    runButton.disabled = true;
    const r = await startRun(ctx, app, { ...values });
    runButton.disabled = false;
    if (r) { run = r; draw(); }
  });

  const drawLog = () => {
    if (!run) return;
    clear(logBox, run.lines.slice(-400).map((l) => h("div", { class: `lvl-${l.level}` }, l.level === "output" ? l.text : `[${l.level}] ${l.text}`)));
    logBox.scrollTop = logBox.scrollHeight;
  };

  function draw() {
    window.clearInterval(timer);
    const running = run && !run.result;
    runButton.disabled = !!running;
    if (!run) {
      clear(panel,
        h("div", { class: "run-state" }, h("div", { class: "big-icon idle" }, icon(Play, 24)),
          h("div", null, h("h3", null, "Ready"), h("p", null, isForm
            ? (app.inputs.length ? "Check the settings, then press Run." : "Nothing to set up: just press Run.")
            : "Press Open to start the app in its own window."))));
      return;
    }
    drawLog();
    if (running) {
      const r = run;
      const elapsed = h("span", null, duration(Date.now() - r.startedAt));
      timer = window.setInterval(() => { elapsed.textContent = duration(Date.now() - r.startedAt); }, 1000);
      const pct = r.progress === null ? null : Math.round(r.progress * 100);
      clear(panel,
        h("div", { class: "run-state" }, h("div", { class: "big-icon running" }, icon(Loader, 24, "spin")),
          h("div", { style: "flex:1;min-width:0" }, h("h3", null, r.cancelling ? "Stopping…" : r.message || "Running"),
            h("p", null, elapsed, pct === null ? "" : ` · ${pct}%`)),
          h("button", { class: "btn", disabled: r.cancelling, onclick: () => ctx.api.cancelRun(r.runId) }, icon(Square, 15), isForm ? "Cancel" : "Stop")),
        h("div", { class: `progress ${pct === null ? "indeterminate" : ""}` }, h("div", { class: "fill", style: `width:${pct ?? 0}%` })));
      return;
    }
    const res = run.result!;
    const head = {
      ok: [CircleCheck, "ok", isForm ? "Done" : "Closed", `Finished in ${duration(res.duration_ms)}`],
      error: [CircleX, "error", "It didn’t work", "The app stopped with an error"],
      cancelled: [CircleSlash, "cancelled", "Cancelled", "Stopped before the end; cleanup ran"],
      running: [Loader, "running", "Running", ""],
    }[res.status] as [typeof Play, string, string, string];
    const outputs = app.outputs.map((o) => {
      const value = res.outputs[o.name];
      if (value === undefined || value === null) return null;
      if (o.type === "file") {
        const path = String(value);
        return h("div", { class: "output-file" },
          h("div", { class: "ficon" }, icon(FileCheck2, 20)),
          h("div", { style: "flex:1;min-width:0" }, h("div", { class: "name" }, fileName(path)), h("div", { class: "label" }, o.label ?? o.name)),
          h("button", { class: "btn small", title: "Show in folder", onclick: () => ctx.api.openOutput(run!.runId, path, true).catch((e) => toast(errorText(e), "error")) }, icon(FolderOpen, 15)),
          h("button", { class: "btn primary small", onclick: () => ctx.api.openOutput(run!.runId, path, false).catch((e) => toast(errorText(e), "error")) }, icon(ExternalLink, 15), "Open result"));
      }
      if (o.type === "table" && Array.isArray(value)) {
        const rows = value as unknown[];
        const cols = rows.length && typeof rows[0] === "object" && !Array.isArray(rows[0]) ? Object.keys(rows[0] as object) : [];
        return h("div", { class: "output-text" }, h("div", { class: "label" }, o.label ?? o.name),
          h("div", { class: "table-scroll" }, h("table", { class: "table" },
            cols.length ? h("thead", null, h("tr", null, cols.map((c) => h("th", null, c)))) : null,
            h("tbody", null, rows.slice(0, 50).map((row) => h("tr", null,
              (cols.length ? cols.map((c) => (row as Record<string, unknown>)[c]) : (row as unknown[])).map((cell) => h("td", null, String(cell ?? "")))))))));
      }
      return h("div", { class: "output-text" }, h("div", { class: "label" }, o.label ?? o.name), h("div", null, String(value)));
    });
    clear(panel,
      h("div", { class: "run-state" }, h("div", { class: `big-icon ${head[1]}` }, icon(head[0], 24)),
        h("div", { style: "flex:1" }, h("h3", null, head[2]), h("p", null, head[3]))),
      res.status === "error" && res.error ? h("div", { class: "error-box" }, res.error) : null,
      outputs.some(Boolean) ? h("div", { class: "outputs" }, outputs) : null,
      res.status === "ok" && app.outputs.some((o) => o.type === "file")
        ? h("button", { class: "btn ghost small", style: "align-self:flex-start", onclick: () => ctx.api.openOutput(run!.runId, null, false) }, icon(FolderOpen, 15), "Open result folder")
        : null);
  }
  draw();

  const unsubscribe = ctx.onLive((r) => {
    if (r.appId !== app.id) return;
    if (!run || r.runId === run.runId || r.startedAt > run.startedAt) {
      run = r;
      draw();
    }
  });

  // ---------- form ----------
  const formCard = h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h3", null, "Settings"), h("span", { class: "hint" }, isForm && Object.keys(app.last_inputs).length ? "Filled in from your last run" : "")),
    isForm && app.inputs.length
      ? h("div", { class: "form" }, app.inputs.filter((i) => i.type !== "secret").map((i) => field(ctx, i, values)))
      : h("div", { class: "empty" }, h("div", { class: "art" }, icon(Sparkles, 28)), h("h4", null, "No setup needed"),
          h("p", null, isForm ? "This app has no settings." : "This app brings its own window and settings.")),
    app.secrets.length
      ? h("div", { style: "margin-top:20px" }, h("div", { class: "nav-label", style: "padding-left:0" }, "Secrets"),
          h("div", { class: "list" }, app.secrets.map((s) =>
            h("div", { class: "list-item" }, h("div", { class: "pi kpi-icon violet", style: "width:34px;height:34px;border-radius:10px" }, icon(KeyRound, 16)),
              h("div", { style: "flex:1" }, h("div", { class: "t" }, s.name), h("div", { class: "s" }, s.set ? "Saved in the credential store" : "Asked at the next run")),
              s.set ? h("button", { class: "btn small", onclick: async () => { await ctx.api.clearSecret(app.id, s.name); s.set = false; toast(`“${s.name}” removed`); ctx.navigate(location.hash); } }, "Forget") : null))))
      : null);

  // ---------- history and details ----------
  const historyBox = h("div", { class: "list" });
  async function loadHistory() {
    const runs: RunRecord[] = await ctx.api.listRuns(app.id, 6);
    clear(historyBox, runs.length ? runs.map((r) =>
      h("div", { class: "list-item" },
        h("div", { style: "flex:1" }, h("div", { class: "t" }, relTime(r.started_at)), h("div", { class: "s" }, `${duration(r.duration_ms)} · v${r.version}`)),
        statusPill(r.status),
        r.status === "ok" ? h("button", { class: "btn ghost small", title: "Open result folder", onclick: () => ctx.api.openOutput(r.id, null, false) }, icon(FolderOpen, 15)) : null))
      : h("div", { class: "muted" }, "No runs yet."));
  }
  loadHistory();
  // The backend writes the finished run's record, then says so: reload the list only then.
  const offChanged = ctx.api.onRunsChanged(() => loadHistory());

  const menuWrap = h("div", { class: "menu-wrap" });
  const menu = h("div", { class: "menu", hidden: true },
    h("button", { onclick: async () => {
      menu.hidden = true;
      toast("Rebuilding the environment…");
      try { await ctx.api.repair(app.id); toast("Repaired: the environment was rebuilt", "ok"); } catch (e) { toast(errorText(e), "error"); }
    } }, icon(Wrench, 16), "Repair"),
    h("button", { class: "danger", onclick: async () => {
      menu.hidden = true;
      const ok = await ctx.api.ask(`Uninstall ${app.title}? Its environment and saved secrets are removed; past results stay in the Runs history.`, "Uninstall", "Uninstall");
      if (!ok) return;
      try { await ctx.api.uninstall(app.id); toast(`${app.title} uninstalled`, "ok"); await ctx.refreshNav(); ctx.navigate("#/apps"); } catch (e) { toast(errorText(e), "error"); }
    } }, icon(Trash2, 16), "Uninstall"));
  const closeMenu = (e: Event) => { if (!menuWrap.contains(e.target as Node)) menu.hidden = true; };
  document.addEventListener("click", closeMenu);
  menuWrap.append(h("button", { class: "icon-btn", "aria-label": "More", onclick: () => { menu.hidden = !menu.hidden; } }, icon(MoreHorizontal, 18)), menu);

  const hero = h("section", { class: "card hero" },
    appIcon(app, "lg"),
    h("div", null,
      h("h2", null, app.title),
      h("div", { class: "meta" }, h("span", null, `by ${app.namespace}`), h("span", null, "·"), h("span", null, `v${app.version}`), h("span", { class: "chip blue" }, KIND_LABEL[app.kind] ?? app.kind), trustChip(app.trust)),
      app.description ? h("div", { class: "muted", style: "margin-top:8px;max-width:640px" }, app.description) : null),
    h("div", { class: "actions" }, runButton, menuWrap));

  const about = h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h3", null, "What it can do")), perms(app),
    h("dl", { class: "kv", style: "margin-top:18px" },
      h("dt", null, "Publisher"), h("dd", null, app.namespace, " ", app.trust === "first_use" ? icon(ShieldCheck, 13) : icon(ShieldAlert, 13)),
      h("dt", null, "Signing key"), h("dd", { class: "mono" }, app.key_id),
      h("dt", null, "Installed"), h("dd", null, relTime(app.installed_at)),
      app.origin?.host_url ? [h("dt", null, "Downloaded from"), h("dd", null, app.origin.host_url)] : null));

  const node = h("div", { class: "grid" },
    h("button", { class: "back", onclick: () => ctx.navigate("#/apps") }, icon(ArrowLeft, 16), "Apps"),
    hero,
    h("div", { class: "grid app-page" },
      h("div", { class: "grid" }, formCard, about),
      h("div", { class: "grid" },
        h("section", { class: "card" }, h("div", { class: "card-head" }, h("h3", null, "Run")), panel),
        logCard,
        h("section", { class: "card" }, h("div", { class: "card-head" }, h("h3", null, "Recent runs")), historyBox))));

  return {
    title: "Apps",
    subtitle: app.title,
    node,
    cleanup: () => { unsubscribe(); offChanged.then((off) => off()); window.clearInterval(timer); document.removeEventListener("click", closeMenu); },
  };
}

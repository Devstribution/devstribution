import {
  X, Square, MoreHorizontal, Wrench, Trash2, FolderOpen, ExternalLink, Pin, PinOff,
  Loader, Globe, Folder, KeyRound, Database, Puzzle, Sparkles, ShieldX, Download,
} from "lucide";
import type { AppView, GuardHit, Input, RunRecord } from "../api";
import type { Ctx, LiveRun } from "../main";
import { h, icon, appIcon, clear, duration, fileName, relTime, statusText, toast, progressToast, errorText, runError, trustTag, trapFocus, announce, menuButton, keepFocus, KIND_LABEL } from "../ui";

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
async function startRun(ctx: Ctx, app: AppView, values: Record<string, unknown>): Promise<LiveRun | null> {
  for (const s of app.secrets.filter((x) => !x.set)) {
    const value = await askSecret(app, s.name);
    if (value === null) {
      toast(`${app.title} was not started: it needs “${s.name}”.`);
      return null;
    }
    await ctx.api.setSecret(app.id, s.name, value);
    s.set = true;
  }
  try {
    const record = await ctx.api.startRun(app.id, values);
    const run: LiveRun = {
      runId: record.id, appId: app.id, appTitle: app.title, startedAt: Date.now(), progress: null,
      message: app.kind === "form" ? "Starting" : "Opening the app", lines: [], cancelling: false,
    };
    ctx.live.set(record.id, run);
    return run;
  } catch (e) {
    toast(errorText(e), "error");
    return null;
  }
}

function askSecret(app: AppView, name: string): Promise<string | null> {
  return new Promise((resolve) => {
    const input = h("input", { class: "input", type: "password", autocomplete: "off", placeholder: "Paste the value", "aria-label": name });
    const done = (v: string | null) => { release(); overlay.remove(); resolve(v); };
    const overlay = h("div", { class: "overlay", onclick: (e: Event) => { if (e.target === overlay) done(null); } },
      h("div", { class: "modal narrow", role: "dialog", "aria-label": `${app.title} needs ${name}` },
        h("div", { class: "modal-head" }, h("div", { class: "perm" }, h("div", { class: "pi" }, icon(KeyRound, 18))),
          h("div", null, h("h2", null, `${app.title} needs “${name}”`), h("div", { class: "muted small" }, "Asked once. Kept in your system’s credential manager, never in files."))),
        h("div", { class: "modal-body" }, input),
        h("div", { class: "modal-foot" },
          h("button", { class: "btn", onclick: () => done(null) }, "Cancel"),
          h("button", { class: "btn primary", onclick: () => input.value && done(input.value) }, "Save and continue"))));
    input.addEventListener("keydown", (e) => { if (e.key === "Enter" && input.value) done(input.value); if (e.key === "Escape") done(null); });
    document.body.appendChild(overlay);
    const release = trapFocus(overlay.firstElementChild as HTMLElement, input);
  });
}

function field(ctx: Ctx, input: Input, values: Record<string, unknown>): HTMLElement {
  const label = input.label ?? input.name;
  const id = `in-${input.name}`;
  const set = (v: unknown) => { values[input.name] = v; };
  let control: HTMLElement;
  const v = values[input.name];
  switch (input.type) {
    case "file":
    case "folder": {
      const shown = h("input", { id, class: `input ${v ? "" : "is-empty"}`, readonly: true, value: v ? String(v) : "", title: v ? String(v) : "",
        placeholder: input.type === "file" ? `Choose a file${input.accept.length ? ` (${input.accept.join(", ")})` : ""}` : "Choose a folder" });
      const pick = async () => {
        const p = await ctx.api.pickInput(input);
        if (p) { set(p); shown.value = p; shown.title = p; shown.classList.remove("is-empty"); }
      };
      shown.addEventListener("click", pick);
      control = h("div", { class: "file-field" }, shown, h("button", { class: "btn", onclick: pick }, input.type === "file" ? "Choose…" : "Choose folder…"));
      break;
    }
    case "bool": {
      const sw = h("button", { id, class: `switch ${v ? "on" : ""}`, role: "switch", "aria-checked": String(!!v), "aria-label": label });
      sw.addEventListener("click", () => { const on = !sw.classList.contains("on"); sw.classList.toggle("on", on); sw.setAttribute("aria-checked", String(on)); set(on); });
      control = sw;
      break;
    }
    case "choice": {
      if (input.choices.length <= 4) {
        const seg = h("div", { class: "segmented", role: "radiogroup", "aria-label": label });
        const draw = () => keepFocus([seg], () => seg.replaceChildren(...input.choices.map((c) =>
          h("button", { class: values[input.name] === c ? "on" : "", role: "radio", "aria-checked": String(values[input.name] === c), onclick: () => { set(c); draw(); } }, c))));
        draw();
        control = seg;
      } else {
        const sel = h("select", { id, class: "input" }, input.choices.map((c) => h("option", { value: c, selected: v === c }, c)));
        sel.addEventListener("change", () => set(sel.value));
        control = sel;
      }
      break;
    }
    default: {
      const type = input.type === "number" ? "number" : input.type === "date" ? "date" : "text";
      const el = h("input", { id, class: "input", type, value: v === null || v === undefined ? "" : String(v) });
      el.addEventListener("input", () => set(type === "number" ? (el.value === "" ? null : Number(el.value)) : el.value));
      control = el;
    }
  }
  return h("div", { class: "field" },
    h("label", { for: id }, label, input.required ? h("span", { class: "req", "aria-hidden": "true" }, "*") : null),
    control,
    input.help ? h("div", { class: "help" }, input.help) : null);
}

function perms(app: AppView): HTMLElement {
  const items: [typeof Globe, string, string][] = [];
  const p = app.permissions;
  if (p.network.length) items.push([Globe, "Internet", p.network.join(", ")]);
  if (p.folders.length) items.push([Folder, "Files", p.folders.includes("user-selected") ? "The files and folders you choose" : p.folders.join(", ")]);
  if (p.secrets.length) items.push([KeyRound, "Secrets", p.secrets.join(", ")]);
  if (p.sap) items.push([Database, "SAP", "Uses your SAP GUI session"]);
  if (app.requires.length) items.push([Puzzle, "Needs", app.requires.join(", ")]);
  if (!items.length) items.push([Sparkles, "Nothing special", "No internet, files or secrets declared"]);
  return h("div", { class: "perm-list" }, items.map(([ic, t, s]) =>
    h("div", { class: "perm" }, h("div", { class: "pi" }, icon(ic, 16)), h("div", null, h("div", { class: "pt" }, t), h("div", { class: "ps" }, s)))));
}

/** Opens the app's side panel; returns a function that closes it. */
export function openAppPanel(ctx: Ctx, id: string, onClosed: () => void): () => void {
  const scrim = h("div", { class: "scrim" });
  const panel = h("aside", { class: "panel", role: "dialog", "aria-label": "App" });
  document.body.append(scrim, panel);
  const release = trapFocus(panel);
  const disposers: (() => void)[] = [];
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    for (const d of disposers) d();
    release();
    scrim.remove();
    panel.remove();
    onClosed();
  };
  scrim.addEventListener("click", close);
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && !document.querySelector(".overlay")) close(); };
  document.addEventListener("keydown", onKey);
  disposers.push(() => document.removeEventListener("keydown", onKey));

  clear(panel, h("div", { class: "panel-body loading" }, icon(Loader, 24, "spin")));
  ctx.api.getApp(id).then((app) => { if (!closed) build(app); }).catch((e) => { toast(errorText(e), "error"); close(); });

  function build(app: AppView) {
    panel.setAttribute("aria-label", app.title);
    const values = initialValues(app);
    const isForm = app.kind === "form";
    let run = ctx.liveFor(app.id);
    let timer: number | undefined;
    disposers.push(() => window.clearInterval(timer));

    // ---------- run state ----------
    const stateBox = h("div");
    let live: { title: HTMLElement; elapsed: HTMLElement; pct: HTMLElement; cancel: HTMLElement; bar: HTMLElement } | null = null;
    const logBox = h("div", { class: "log" });
    const runButton = h("button", { class: "btn primary big" });
    runButton.addEventListener("click", async () => {
      const missing = missingRequired(app, values);
      if (missing.length) { toast(`Fill in: ${missing.map((i) => i.label ?? i.name).join(", ")}`, "error"); return; }
      runButton.disabled = true;
      const r = await startRun(ctx, app, { ...values });
      runButton.disabled = false;
      if (r) { run = r; draw(); stateBox.scrollIntoView({ behavior: "smooth", block: "nearest" }); }
    });

    function drawLog() {
      if (!run) return;
      clear(logBox, run.lines.slice(-400).map((l) => h("div", { class: `lvl-${l.level}` }, l.level === "output" ? l.text : `[${l.level}] ${l.text}`)));
      logBox.scrollTop = logBox.scrollHeight;
    }

    // What screen readers last heard of this run.
    let said = "";
    const say = (text: string) => { if (text !== said) { said = text; announce(text); } };

    function draw() {
      window.clearInterval(timer);
      const running = !!run && !run.result;
      runButton.disabled = running || !!app.quarantine;
      runButton.textContent = running ? (isForm ? "Running…" : "Open") : run?.result ? (isForm ? "Run again" : "Open again") : (isForm ? "Run" : "Open");
      if (!run) { clear(stateBox); logCard.hidden = true; return; }
      logCard.hidden = false;
      drawLog();
      if (running) {
        const r = run;
        const pct = r.progress === null ? null : Math.round(r.progress * 100);
        // Built once per run and then only updated: a Cancel button replaced between the
        // press and the release of a click would never receive it.
        if (stateBox.dataset.run !== r.runId) {
          stateBox.dataset.run = r.runId;
          live = {
            title: h("strong"), elapsed: h("span"), pct: h("span"),
            cancel: h("button", { class: "btn small", onclick: () => ctx.api.cancelRun(r.runId) }, icon(Square, 13), isForm ? "Cancel" : "Stop"),
            bar: h("div", { class: "progress", role: "progressbar", "aria-label": `${app.title} progress` }, h("div", { class: "fill" })),
          };
          clear(stateBox, h("div", { class: "state-box" },
            h("div", { class: "row" }, h("div", { class: "grow" }, live.title, h("div", { class: "sub" }, live.elapsed, live.pct)), live.cancel),
            live.bar));
        }
        const v = live!;
        v.title.textContent = r.cancelling ? "Stopping…" : r.message || "Running";
        v.elapsed.textContent = duration(Date.now() - r.startedAt);
        v.pct.textContent = pct === null ? "" : ` · ${pct}%`;
        (v.cancel as HTMLButtonElement).disabled = r.cancelling;
        v.bar.classList.toggle("indeterminate", pct === null);
        (v.bar.firstElementChild as HTMLElement).style.width = `${pct ?? 0}%`;
        if (pct === null) v.bar.removeAttribute("aria-valuenow"); else v.bar.setAttribute("aria-valuenow", String(pct));
        // The step, and every quarter of the way: more would talk over the user.
        say(`${v.title.textContent}${pct === null ? "" : `, ${Math.floor(pct / 25) * 25}%`}`);
        timer = window.setInterval(() => { v.elapsed.textContent = duration(Date.now() - r.startedAt); }, 1000);
        return;
      }
      delete stateBox.dataset.run;
      const res = run.result!;
      const outputs = app.outputs.map((o) => {
        const value = res.outputs[o.name];
        if (value === undefined || value === null) return null;
        if (o.type === "file") {
          const path = String(value);
          return h("div", { class: "output-file" },
            h("div", { class: "grow" }, h("div", { class: "name", title: fileName(path) }, fileName(path)), h("div", { class: "label" }, o.label ?? o.name)),
            h("button", { class: "btn small", title: "Show in folder", "aria-label": "Show in folder", onclick: () => ctx.api.openOutput(run!.runId, path, true).catch((e) => toast(errorText(e), "error")) }, icon(FolderOpen, 15)),
            h("button", { class: "btn primary small", onclick: () => ctx.api.openOutput(run!.runId, path, false).catch((e) => toast(errorText(e), "error")) }, icon(ExternalLink, 14), "Open"));
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
      const stopped = res.guard?.some((g) => g.action === "stop");
      const head = stopped ? ["error", "Stopped by devstribution", "The app did something apps should not do."] : {
        ok: ["ok", isForm ? `Done in ${duration(res.duration_ms)}` : "Closed", isForm ? "Your results are below." : ""],
        error: ["error", "It didn’t work", "The app stopped with an error."],
        cancelled: ["", "Cancelled", "Stopped before the end; cleanup ran."],
        running: ["", "Running", ""],
      }[res.status];
      // Only a run followed here: opening the panel later does not read out an old result.
      if (said) say(`${app.title}: ${head[1]}`);
      clear(stateBox, h("div", { class: `state-box ${head[0]}` },
        h("div", { class: "row" }, h("div", { class: "grow" }, h("strong", null, head[1]), head[2] ? h("div", { class: "sub" }, head[2]) : null),
          res.status === "ok" && app.outputs.some((o) => o.type === "file")
            ? h("button", { class: "btn small", onclick: () => ctx.api.openOutput(run!.runId, null, false) }, icon(FolderOpen, 14), "Folder")
            : null),
        res.status === "error" ? runError(res, app.title) : null,
        outputs.some(Boolean) ? h("div", { class: "outputs" }, outputs) : null,
        whatItDid(res.guard ?? [])));
    }

    // ---------- quarantine ----------
    // The runtime guard stopped a run: nothing runs until the user allows it or uninstalls.
    const q = app.quarantine;
    const quarantine = q
      ? h("div", { class: "banner red quarantine", role: "alert" }, h("div", { class: "bi" }, icon(ShieldX, 18)),
          h("div", { class: "grow" },
            h("strong", null, "Blocked by devstribution"),
            h("div", null, `${relTime(q.at)} the app ${q.hit.message}, so devstribution stopped it. It won’t run again until you decide.`),
            q.hit.detail ? h("div", { class: "mono detail" }, q.hit.detail) : null,
            app.may_allow_guard_rules ? null : h("div", null, "Your organisation decides whether it may run past this: ask your IT department."),
            h("div", { class: "banner-actions" },
              h("button", { class: "btn small", onclick: () => uninstall() }, icon(Trash2, 14), "Uninstall"),
              !app.may_allow_guard_rules ? null : h("button", { class: "btn small", onclick: async () => {
                const ok = await ctx.api.ask(
                  `Let ${app.title} run even though it ${q.hit.message}? Only do this if you trust ${app.namespace} and know why the app needs to. devstribution still reports it every time.`,
                  "Allow and run again", "Allow");
                if (!ok) return;
                try { await ctx.api.releaseQuarantine(app.id, true); toast(`${app.title} can run again`, "ok"); close(); ctx.openApp(app.id); }
                catch (e) { toast(errorText(e), "error"); }
              } }, "Allow and run again"))))
      : null;

    // ---------- a newer version in an update folder ----------
    const newer = ctx.appUpdates.get(app.id);
    const updateBanner = newer
      ? h("div", { class: "banner blue" }, h("div", { class: "bi" }, icon(Download, 18)),
          h("div", { class: "grow" },
            h("strong", null, `Version ${newer.version} is available`),
            h("div", null, `You have ${app.version}. You’ll see what changed before anything is installed.`),
            h("div", { class: "banner-actions" },
              h("button", { class: "btn small", onclick: () => { close(); ctx.install(newer.path); } }, "Review the update"))))
      : null;

    async function uninstall() {
      more.close();
      const ok = await ctx.api.ask(`Uninstall ${app.title}? Its environment and saved secrets are removed; past results stay in Activity.`, "Uninstall", "Uninstall");
      if (!ok) return;
      try { await ctx.api.uninstall(app.id); toast(`${app.title} uninstalled`, "ok"); close(); await ctx.refresh(); } catch (e) { toast(errorText(e), "error"); }
    }

    // ---------- form and details ----------
    const form = isForm && app.inputs.some((i) => i.type !== "secret")
      ? h("div", { class: "form" },
          Object.keys(app.last_inputs).length ? h("div", { class: "note" }, "Filled in from your last run") : null,
          app.inputs.filter((i) => i.type !== "secret").map((i) => field(ctx, i, values)))
      : h("div", { class: "muted" }, isForm ? "Nothing to set up: just press Run." : "This app opens in its own window.");

    const secrets = app.secrets.length
      ? h("div", { class: "settings-group" }, h("div", { class: "section-label" }, "Secrets"),
          app.secrets.map((s) =>
            h("div", { class: "secret-row" }, icon(KeyRound, 16),
              h("div", { class: "grow" }, h("div", { class: "strong" }, s.name), h("div", { class: "s" }, s.set ? "Saved in the credential store" : "Asked at the next run")),
              s.set ? h("button", { class: "btn small", onclick: async () => {
                if (!await ctx.api.ask(`Forget “${s.name}”? ${app.title} will ask for it again at the next run.`, "Forget secret", "Forget")) return;
                await ctx.api.clearSecret(app.id, s.name);
                toast(`“${s.name}” removed`);
                close();
                ctx.openApp(app.id);
              } }, "Forget") : null)))
      : null;

    const logCard = h("details", { class: "disclosure", hidden: true }, h("summary", null, "Activity log"), h("div", null, logBox));
    const historyBox = h("div", { class: "history" });
    async function loadHistory() {
      const runs: RunRecord[] = await ctx.api.listRuns(app.id, 5);
      clear(historyBox, runs.length
        ? runs.map((r) => h("div", { class: "history-item" },
            h("span", { class: "grow" }, relTime(r.started_at), h("span", { class: "muted" }, ` · ${duration(r.duration_ms)}`)),
            statusText(r.status),
            r.status === "ok" && !r.output_removed ? h("button", { class: "btn ghost small", title: "Open result folder", "aria-label": "Open result folder", onclick: () => ctx.api.openOutput(r.id, null, false) }, icon(FolderOpen, 14)) : null))
        : h("div", { class: "muted small" }, "No runs yet."));
    }
    loadHistory();
    ctx.api.onRunsChanged(() => { if (!closed) loadHistory(); }).then((off) => { if (closed) off(); else disposers.push(off); });

    const about = h("details", { class: "disclosure" }, h("summary", null, "What it can do"),
      h("div", null, perms(app),
        h("dl", { class: "kv" },
          h("dt", null, "Publisher"), h("dd", null, app.namespace),
          h("dt", null, "Signing key"), h("dd", { class: "mono" }, app.key_id),
          h("dt", null, "Installed"), h("dd", null, relTime(app.installed_at)),
          app.origin?.host_url ? [h("dt", null, "Downloaded from"), h("dd", null, app.origin.host_url)] : null)));
    const history = h("details", { class: "disclosure" }, h("summary", null, "Recent runs"), h("div", null, historyBox));

    // ---------- more menu ----------
    const menu = h("div", { class: "menu", hidden: true, role: "menu" });
    const drawMenu = () => clear(menu,
      h("button", { role: "menuitem", onclick: () => { ctx.pins.toggle(app.id); more.close(); drawMenu(); } },
        icon(ctx.pins.has(app.id) ? PinOff : Pin, 16), ctx.pins.has(app.id) ? "Unpin from sidebar" : "Pin to sidebar"),
      h("button", { role: "menuitem", onclick: async () => {
        more.close();
        // Cancel stops uv; the old environment stays in place until a new one is complete.
        let stopped = false;
        const progress = progressToast("Rebuilding the environment…", {
          label: "Cancel", busy: "Cancelling…",
          run: () => { stopped = true; ctx.api.cancelEnvironment().catch(() => {}); },
        });
        const off = await ctx.api.onInstallStep((e) => { if (e.key === app.id) progress.update(e.message); });
        try { await ctx.api.repair(app.id); progress.end(); toast("Repaired: the environment was rebuilt", "ok"); }
        catch (e) { progress.end(); toast(stopped ? "Repair cancelled. The app’s environment is as it was." : errorText(e), stopped ? "info" : "error"); }
        finally { off(); }
      } }, icon(Wrench, 16), "Repair"),
      h("button", { role: "menuitem", class: "danger", onclick: () => uninstall() }, icon(Trash2, 16), "Uninstall"));
    drawMenu();
    const moreBtn = h("button", { class: "btn big square", "aria-label": "More" }, icon(MoreHorizontal, 18));
    const more = menuButton(moreBtn, menu);
    const menuWrap = h("div", { class: "menu-wrap" }, moreBtn, menu);
    const closeMenu = (e: Event) => { if (!menuWrap.contains(e.target as Node)) more.close(); };
    document.addEventListener("click", closeMenu);
    disposers.push(() => document.removeEventListener("click", closeMenu));

    clear(panel,
      h("div", { class: "panel-head" },
        h("div", { class: "top" }, appIcon(app, "lg"),
          h("div", { class: "actions" }, h("button", { class: "icon-btn", "aria-label": "Close", onclick: close }, icon(X, 16)))),
        h("div", null, h("h2", null, app.title),
          h("div", { class: "by" }, `by ${app.namespace} · v${app.version} · ${KIND_LABEL[app.kind] ?? app.kind}`, trustTag(app.trust))),
        app.description ? h("div", { class: "desc" }, app.description) : null),
      h("div", { class: "panel-body" }, quarantine, updateBanner, form, secrets, stateBox, logCard, history, about),
      h("div", { class: "panel-foot" }, runButton, menuWrap));

    draw();
    // At most one redraw per frame, however many events arrive.
    let frame = 0;
    disposers.push(() => cancelAnimationFrame(frame));
    disposers.push(ctx.onLive((r) => {
      if (r.appId !== app.id) return;
      if (!run || r.runId === run.runId || r.startedAt > run.startedAt) {
        run = r;
        if (frame) return;
        frame = requestAnimationFrame(() => {
          frame = 0;
          const justFinished = !!run?.result && !stateBox.querySelector(".state-box.ok, .state-box.error");
          // A stop quarantines the app: show the panel again, with the decision on top.
          if (justFinished && run?.result?.guard?.some((g) => g.action === "stop")) {
            close();
            ctx.openApp(app.id);
            return;
          }
          draw();
          if (justFinished) stateBox.scrollIntoView({ behavior: "smooth", block: "nearest" });
        });
      }
    }));
    panel.tabIndex = -1;
    panel.focus({ preventScroll: true });
  }

  return close;
}

/** One line from the runtime guard's reports: the files the run changed, the hosts it contacted
 *  and the programs it started. */
function whatItDid(hits: GuardHit[]): HTMLElement | null {
  const seen = (rule: string) => hits.filter((g) => g.action === "observe" && g.rule === rule).map((g) => g.message);
  const parts: string[] = [];
  const files = seen("files")[0];
  if (files) parts.push(files);
  const hosts = [...new Set(seen("network"))];
  if (hosts.length) parts.push(`contacted ${hosts.join(", ")}`);
  const programs = [...new Set(seen("program").map(programName))];
  if (programs.length) parts.push(`started ${programs.join(", ")}`);
  if (!parts.length) return null;
  const text = parts.join(" · ");
  return h("div", { class: "did-line", title: "Reported by devstribution's runtime guard" },
    h("strong", null, "What it did: "), text.charAt(0).toUpperCase() + text.slice(1));
}

/** `C:\Windows\system32\cmd.exe /c ver` → `cmd.exe`; paths may contain spaces. */
function programName(command: string): string {
  const text = command.replace(/"/g, "");
  const program = text.match(/^(.*?\.(?:exe|com|bat|cmd))(?:\s|$)/i);
  const path = program ? program[1] : text.split(/\s/)[0];
  return path.split(/[\\/]/).pop() || command;
}

import { ShieldAlert, ShieldCheck, ShieldX, ShieldOff, Globe, Folder, KeyRound, Database, Puzzle, Sparkles, Check, Loader, Download,
  SquareTerminal, Cpu, Code, EyeOff, Eye, CalendarClock, PackagePlus, PackageCheck, Package, Binary, ScanSearch, FileDiff } from "lucide";
import type { AppDiff, Finding, PendingInstall, StepEvent } from "../api";
import type { Ctx } from "../main";
import { h, icon, appIcon, bytes, clear, toast, errorText, KIND_LABEL } from "../ui";

function permissionRows(p: PendingInstall): HTMLElement {
  const rows: [typeof Globe, string, string][] = [];
  if (p.permissions.network.length) rows.push([Globe, "Connects to the internet", p.permissions.network.join(", ")]);
  if (p.permissions.folders.length) rows.push([Folder, "Reads and writes files", p.permissions.folders.includes("user-selected") ? "The files and folders you choose" : p.permissions.folders.join(", ")]);
  if (p.permissions.secrets.length) rows.push([KeyRound, "Uses secrets", `${p.permissions.secrets.join(", ")} (asked at first run)`]);
  if (p.permissions.sap) rows.push([Database, "Uses SAP", "Through your SAP GUI session"]);
  if (p.requires.length) rows.push([Puzzle, "Needs installed", p.requires.join(", ")]);
  if (!rows.length) rows.push([Sparkles, "Nothing declared", "No internet, files or secrets"]);
  return h("div", { class: "perm-list" }, rows.map(([ic, t, s]) =>
    h("div", { class: "perm" }, h("div", { class: "pi" }, icon(ic, 16)), h("div", null, h("div", { class: "pt" }, t), h("div", { class: "ps" }, s)))));
}

const CATEGORY_ICON: Record<string, typeof Globe> = {
  network: Globe, processes: SquareTerminal, native_code: Cpu, dynamic_code: Code, obfuscation: EyeOff,
  persistence: CalendarClock, credentials: KeyRound, surveillance: Eye, evasion: ShieldOff,
  runtime_install: PackagePlus, sap: Database, binary: Binary, dependencies: Package,
};

/** What the code scan found, one row per kind of finding, with the files it was found in. */
function scanRows(p: PendingInstall): HTMLElement {
  const groups = new Map<string, Finding[]>();
  for (const f of p.scan.findings) {
    const key = `${f.rule}
${f.message}`;
    groups.set(key, [...(groups.get(key) ?? []), f]);
  }
  const where = (f: Finding) => (f.line ? `${f.path}:${f.line} ${f.evidence}` : f.path) + (f.occurrences > 1 ? ` (+${f.occurrences - 1})` : "");
  const rows = [...groups.values()].map((fs) => {
    const f = fs[0];
    const shown = fs.slice(0, 3).map(where).join(" · ") + (fs.length > 3 ? ` · ${fs.length - 3} more` : "");
    return h("div", { class: "perm" },
      h("div", { class: `pi ${f.severity === "info" ? "" : f.severity}`, title: f.severity }, icon(CATEGORY_ICON[f.category] ?? ScanSearch, 16)),
      h("div", null, h("div", { class: "pt" }, f.message), h("div", { class: "ps mono" }, shown)));
  });
  if (!rows.length) {
    rows.push(h("div", { class: "perm" }, h("div", { class: "pi" }, icon(ScanSearch, 16)),
      h("div", null, h("div", { class: "pt" }, "Nothing unusual found"),
        h("div", { class: "ps" }, `${p.scan.files_scanned} Python file${p.scan.files_scanned === 1 ? "" : "s"} checked`))));
  }
  const deps = p.scan.dependencies;
  if (deps && deps.packages && !p.scan.findings.some((f) => f.rule === "malicious-package")) {
    rows.push(h("div", { class: "perm" }, h("div", { class: "pi" }, icon(PackageCheck, 16)),
      h("div", null, h("div", { class: "pt" }, "No package reported as malicious"),
        h("div", { class: "ps" }, `${deps.packages} package${deps.packages === 1 ? "" : "s"} checked against PyPI's malicious-package reports up to ${new Date(`${deps.list_date}T00:00:00`).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}`))));
  }
  return h("div", { class: "perm-list" }, rows);
}

/** Whether an update may do something the installed version could not. */
function widened(d: AppDiff): boolean {
  const p = d.permissions;
  return p.network_added.length > 0 || p.folders_added.length > 0 || p.secrets_added.length > 0 || p.sap === true;
}

/** An update's changes: what deserves a look first, then the packages and files in a fold. */
function changeRows(d: AppDiff): HTMLElement[] {
  const rows = d.highlights.map((x) =>
    h("div", { class: "perm" },
      h("div", { class: `pi ${x.severity === "info" ? "" : x.severity}`, title: x.severity }, icon(FileDiff, 16)),
      h("div", null, h("div", { class: "pt" }, x.message))));
  const changed = d.files.length;
  if (!rows.length) {
    rows.push(h("div", { class: "perm" }, h("div", { class: "pi" }, icon(FileDiff, 16)),
      h("div", null, h("div", { class: "pt" }, "Nothing that needs a look"),
        h("div", { class: "ps" }, `${changed} file${changed === 1 ? "" : "s"} changed, no new permissions, packages or flagged code`))));
  }
  const out: HTMLElement[] = [h("div", { class: "perm-list" }, rows)];
  if (changed || d.packages.length || d.declared.length) {
    const line = (text: string) => h("div", { class: "ps mono" }, text);
    const mark = { added: "+", removed: "−", changed: "~" } as const;
    const pkgs = d.packages.length ? `, ${d.packages.length} package${d.packages.length === 1 ? "" : "s"}` : "";
    out.push(h("details", { class: "disclosure" },
      h("summary", null, `Every change (${changed} file${changed === 1 ? "" : "s"}${pkgs})`),
      h("div", { class: "change-list" },
        d.declared.map(line),
        d.packages.map((c) => line(c.old && c.new ? `${c.name} ${c.old} → ${c.new}` : c.new ? `+ ${c.name} ${c.new}` : `− ${c.name} ${c.old}`)),
        d.files.map((f) => line(`${mark[f.status]} ${f.path}${f.binary ? "  (can’t be read)" : f.status === "changed" ? `  +${f.lines_added} −${f.lines_removed}` : ""}`)),
        h("div", { class: "muted small" }, "For the changed lines, run “devstribution diff” on the two versions."))));
  }
  return out;
}

/** A package the organisation's policy refuses: why, who to ask, and a ready-to-send request. */
function blockedScreen(ctx: Ctx, p: PendingInstall, modal: HTMLElement, close: () => void) {
  const contact = p.blocked?.contact;
  const save = async () => {
    try {
      if (await ctx.api.saveTrustRequest(p.token)) toast("Request saved: send the .trust file to your IT department", "ok");
    } catch (e) { toast(errorText(e), "error"); }
  };
  const approvable = p.blocked?.author_not_approved ?? false;
  clear(modal,
    h("div", { class: "modal-head center" },
      appIcon(p, "xl"),
      h("div", null, h("h2", null, `${p.title} can’t be installed`),
        h("div", { class: "muted" }, `from “${p.namespace}” · version ${p.version} · ${KIND_LABEL[p.kind] ?? p.kind}`))),
    h("div", { class: "modal-body" },
      h("div", { class: "banner red" }, h("div", { class: "bi" }, icon(ShieldX, 18)),
        h("div", { class: "grow" }, h("strong", null, "Your organisation’s policy doesn’t allow it"),
          (p.blocked?.reasons ?? []).map((r) => h("div", null, `${r[0].toUpperCase()}${r.slice(1)}.`.replace(/`([^`]+)`/g, "“$1”"))))),
      h("div", { class: "soft-text" },
        contact ? `Ask ${contact}. ` : "Ask your IT department. ",
        approvable
          ? "To approve this author, they need its signing key: save it as a request and send it to them. They should check the key "
          : "Nothing was installed.",
        approvable ? h("span", { class: "mono" }, p.key_id) : null,
        approvable ? " with the author before approving it." : null),
      h("div", { class: "section-label" }, "What this app can do"), permissionRows(p),
      h("div", { class: "section-label" }, "What the code does"), scanRows(p)),
    h("div", { class: "modal-foot" },
      h("button", { class: "btn", onclick: close }, "Close"),
      approvable ? h("button", { class: "btn primary", onclick: save }, "Save a request for IT…") : null));
}

/** The confirmation screen. Any .dsapp, however it arrived, goes through here. */
export async function installFlow(ctx: Ctx, path?: string) {
  const chosen = path ?? (await ctx.api.pickPackage());
  if (!chosen) return;

  const modal = h("div", { class: "modal", role: "dialog", "aria-modal": "true", "aria-label": "Install app" });
  const overlay = h("div", { class: "overlay" }, modal);
  // What Escape does right now; null while it cannot be stopped (a Cancel already sent).
  let dismiss: (() => void) | null = null;
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && dismiss) dismiss(); };
  const end = () => { document.removeEventListener("keydown", onKey); overlay.remove(); };
  document.addEventListener("keydown", onKey);
  document.body.appendChild(overlay);

  let cancelled = false;
  dismiss = () => { cancelled = true; end(); };
  clear(modal,
    h("div", { class: "modal-body loading" }, icon(Loader, 26, "spin"), h("div", { class: "muted" }, "Checking the package…")),
    h("div", { class: "modal-foot" }, h("button", { class: "btn", onclick: () => dismiss?.() }, "Cancel")));

  let p: PendingInstall;
  try {
    p = await ctx.api.inspectPackage(chosen);
  } catch (e) {
    if (cancelled) return;
    dismiss = end;
    clear(modal,
      h("div", { class: "modal-head center" }, h("div", { class: "modal-icon red" }, icon(ShieldAlert, 26)),
        h("div", null, h("h2", null, "This app can’t be installed"), h("div", { class: "muted" }, chosen.split(/[\\/]/).pop()))),
      h("div", { class: "modal-body" }, h("div", { class: "error-box" }, errorText(e)),
        h("div", { class: "muted small" }, "Nothing was installed. If you expected this file to work, contact its publisher.")),
      h("div", { class: "modal-foot" }, h("button", { class: "btn primary", onclick: end }, "Close")));
    return;
  }
  if (cancelled) { ctx.api.discardInstall(p.token); return; }

  const cancel = () => { ctx.api.discardInstall(p.token); end(); };
  dismiss = cancel;
  const actionLabel = p.action === "update" ? `Update from v${p.installed_version}` : p.action === "reinstall" ? "Reinstall" : "Install";
  const verb = p.action === "update" ? "Update" : p.action === "reinstall" ? "Reinstall" : "Install";
  const newPublisher = p.trust === "new_publisher";
  const internet = p.origin && p.origin.zone_id >= 3;
  const danger = p.scan.findings.some((f) => f.severity === "danger");
  const asksMore = p.changes ? widened(p.changes) : false;
  if (p.blocked) return blockedScreen(ctx, p, modal, cancel);

  clear(modal,
    h("div", { class: "modal-head center" },
      appIcon(p, "xl"),
      h("div", null, h("h2", null, `${verb} ${p.title}?`),
        h("div", { class: "muted" }, `from “${p.namespace}” · version ${p.version} · ${KIND_LABEL[p.kind] ?? p.kind}`))),
    h("div", { class: "modal-body" },
      p.description ? h("div", { class: "soft-text center" }, p.description) : null,
      danger
        ? h("div", { class: "banner red" }, h("div", { class: "bi" }, icon(ShieldX, 18)),
            h("div", null, h("strong", null, "The code does things malware does"),
              "See “What the code does” below. Install only if the publisher can explain each one."))
        : null,
      p.trust === "approved"
        ? h("div", { class: "banner green" }, h("div", { class: "bi" }, icon(ShieldCheck, 18)),
            h("div", null, h("strong", null, "Approved by your organisation"),
              `Your IT department approves apps from “${p.namespace}” signed with this key (`, h("span", { class: "mono" }, p.key_id), ")."))
        : p.trust === "not_approved"
        ? h("div", { class: "banner amber" }, h("div", { class: "bi" }, icon(ShieldAlert, 18)),
            h("div", null, h("strong", null, `“${p.namespace}” isn’t approved by your organisation`),
              "Your IT department is only trying out its policy, so you can still install it. Key ", h("span", { class: "mono" }, p.key_id), "."))
        : newPublisher
        ? h("div", { class: "banner amber" }, h("div", { class: "bi" }, icon(ShieldAlert, 18)),
            h("div", null, h("strong", null, `New publisher: “${p.namespace}”`),
              "The package is intact and signed, but this name isn’t verified. Install it only if you trust where it came from. Later versions must come from the same key ",
              h("span", { class: "mono" }, p.key_id), "."))
        : h("div", { class: "banner green" }, h("div", { class: "bi" }, icon(ShieldCheck, 18)),
            h("div", null, h("strong", null, "Same publisher as before"),
              `Signed with the key of the “${p.namespace}” apps you already have (`, h("span", { class: "mono" }, p.key_id), ").")),
      internet
        ? h("div", { class: "banner blue" }, h("div", { class: "bi" }, icon(Download, 18)),
            h("div", { class: "grow wrap-anywhere" }, h("strong", null, "Downloaded from the internet"), p.origin?.host_url ?? "Unknown address"))
        : null,
      p.warnings.filter((w) => !w.startsWith("First app") && !w.startsWith("This file was downloaded")).map((w) =>
        h("div", { class: "banner amber" }, h("div", { class: "bi" }, icon(ShieldAlert, 18)), h("div", null, w))),
      asksMore
        ? h("div", { class: "banner amber" }, h("div", { class: "bi" }, icon(ShieldAlert, 18)),
            h("div", null, h("strong", null, "This update asks for more than before"),
              "See “What changed” below. Update only if you expected it."))
        : null,
      p.changes ? [h("div", { class: "section-label" }, `What changed since v${p.changes.old_version}`), ...changeRows(p.changes)] : null,
      h("div", { class: "section-label" }, "What this app can do"), permissionRows(p),
      h("div", { class: "section-label" }, "What the code does"), scanRows(p),
      h("div", { class: "muted small" }, "Read without running it: well-hidden code can be missed, and a flagged line can be harmless."),
      h("div", { class: "facts" },
        h("div", { class: "fact" }, h("div", { class: "k" }, "Size"), h("div", { class: "v" }, bytes(p.size_bytes))),
        h("div", { class: "fact" }, h("div", { class: "k" }, "Files"), h("div", { class: "v" }, String(p.file_count))),
        h("div", { class: "fact" }, h("div", { class: "k" }, "Runs on"), h("div", { class: "v" }, p.platforms.join(", "))))),
    h("div", { class: "modal-foot" },
      h("button", { class: "btn", onclick: cancel }, "Cancel"),
      h("button", { class: "btn primary", onclick: () => confirm() }, danger || asksMore ? `${verb} anyway` : actionLabel)));

  async function confirm() {
    const steps: [StepEvent["step"] | "verify", string][] = [
      ["verify", "Signature and files verified"],
      ["files", "Copy the app"],
      ["environment", "Prepare its Python environment"],
      ["ready", "Ready"],
    ];
    let active: string = "files";
    let detail = "";
    // Cancel stops uv and everything it started; the install then fails and nothing is kept.
    let stopping = false;
    const stop = () => {
      if (stopping) return;
      stopping = true;
      dismiss = null;
      stopBtn.disabled = true;
      stopBtn.textContent = "Cancelling…";
      ctx.api.cancelEnvironment().catch(() => {});
    };
    const stopBtn = h("button", { class: "btn", onclick: stop }, "Cancel") as HTMLButtonElement;
    const list = h("div", { class: "steps" });
    const draw = () => {
      const idx = steps.findIndex(([k]) => k === active);
      clear(list, steps.map(([, label], i) => {
        const done = i < idx || active === "ready";
        const cls = done ? "done" : i === idx ? "active" : "";
        return h("div", { class: `step ${cls}` },
          h("div", { class: "si" }, done ? icon(Check, 14) : i === idx ? icon(Loader, 14, "spin") : String(i + 1)),
          h("div", { class: "grow" }, h("div", { class: "st" }, label), i === idx && detail && active !== "ready" ? h("div", { class: "ss" }, detail) : null));
      }));
    };
    draw();
    clear(modal,
      h("div", { class: "modal-head" }, appIcon(p, "lg"), h("div", null, h("h2", null, `Installing ${p.title}`), h("div", { class: "muted" }, `version ${p.version}`))),
      h("div", { class: "modal-body" }, list),
      h("div", { class: "modal-foot" }, h("span", { class: "muted small grow" }, "This can take a few minutes the first time."), stopBtn));
    dismiss = stop;
    const unlisten = await ctx.api.onInstallStep((e) => {
      if (e.key !== p.token && e.key !== "demo") return;
      active = e.step;
      detail = e.message;
      draw();
    });
    try {
      const app = await ctx.api.confirmInstall(p.token);
      active = "ready";
      draw();
      unlisten();
      await ctx.refresh();
      const foot = modal.querySelector(".modal-foot")!;
      dismiss = end;
      clear(foot,
        h("button", { class: "btn", onclick: end }, "Close"),
        h("button", { class: "btn primary", onclick: () => { end(); ctx.openApp(app.id); } }, "Open app"));
    } catch (e) {
      unlisten();
      end();
      if (stopping) toast(`Installation cancelled. ${p.title} was not installed.`, "info");
      else toast(`Installation failed: ${errorText(e)}`, "error");
    }
  }
}


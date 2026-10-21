import { ShieldAlert, ShieldCheck, Globe, Folder, KeyRound, Database, Puzzle, Sparkles, Check, Loader, Download, X, Play } from "lucide";
import type { PendingInstall, StepEvent } from "../api";
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
    h("div", { class: "perm" }, h("div", { class: "pi" }, icon(ic, 17)), h("div", null, h("div", { class: "pt" }, t), h("div", { class: "ps" }, s)))));
}

/** The confirmation screen. Any .dsapp, however it arrived, goes through here. */
export async function installFlow(ctx: Ctx, path?: string) {
  const chosen = path ?? (await ctx.api.pickPackage());
  if (!chosen) return;

  const modal = h("div", { class: "modal" });
  const overlay = h("div", { class: "overlay" }, modal);
  document.body.appendChild(overlay);
  clear(modal, h("div", { class: "modal-body", style: "padding:40px;align-items:center" }, icon(Loader, 28, "spin"), h("div", { class: "muted" }, "Checking the package…")));

  let p: PendingInstall;
  try {
    p = await ctx.api.inspectPackage(chosen);
  } catch (e) {
    clear(modal,
      h("div", { class: "modal-head" }, h("div", { class: "kpi-icon", style: "background:var(--danger-soft);color:var(--danger)" }, icon(ShieldAlert, 22)),
        h("div", null, h("h2", null, "This app can’t be installed"), h("div", { class: "muted" }, chosen.split(/[\\/]/).pop()))),
      h("div", { class: "modal-body" }, h("div", { class: "error-box" }, errorText(e)),
        h("div", { class: "muted" }, "Nothing was installed. If you expected this file to work, contact its publisher.")),
      h("div", { class: "modal-foot" }, h("button", { class: "btn primary", onclick: () => overlay.remove() }, "Close")));
    return;
  }

  const cancel = () => { ctx.api.discardInstall(p.token); overlay.remove(); };
  const actionLabel = p.action === "update" ? `Update from v${p.installed_version}` : p.action === "reinstall" ? "Reinstall" : "Install";
  const newPublisher = p.trust === "new_publisher";
  const internet = p.origin && p.origin.zone_id >= 3;

  clear(modal,
    h("div", { class: "modal-head" }, appIcon(p, "lg"),
      h("div", { style: "flex:1;min-width:0" },
        h("h2", null, p.title),
        h("div", { class: "muted" }, `by ${p.namespace} · v${p.version} · ${KIND_LABEL[p.kind] ?? p.kind}`)),
      h("button", { class: "icon-btn", "aria-label": "Cancel", onclick: cancel }, icon(X, 18))),
    h("div", { class: "modal-body" },
      p.description ? h("div", { style: "color:var(--text-2)" }, p.description) : null,
      newPublisher
        ? h("div", { class: "banner amber" }, h("div", { class: "bi" }, icon(ShieldAlert, 20)),
            h("div", null, h("strong", null, `First app from “${p.namespace}”`),
              "The files are intact and signed, but this publisher’s name is not verified. Install it only if you trust where you got it. Future versions will have to come from the same key ",
              h("span", { class: "mono" }, p.key_id), "."))
        : h("div", { class: "banner green" }, h("div", { class: "bi" }, icon(ShieldCheck, 20)),
            h("div", null, h("strong", null, `Same publisher as before`),
              `Signed with the key of the “${p.namespace}” apps you already installed (`, h("span", { class: "mono" }, p.key_id), ").")),
      internet
        ? h("div", { class: "banner blue" }, h("div", { class: "bi" }, icon(Download, 20)),
            h("div", null, h("strong", null, "Downloaded from the internet"), p.origin?.host_url ?? "Unknown address"))
        : null,
      p.warnings.filter((w) => !w.startsWith("First app") && !w.startsWith("This file was downloaded")).map((w) =>
        h("div", { class: "banner amber" }, h("div", { class: "bi" }, icon(ShieldAlert, 20)), h("div", null, w))),
      h("div", null, h("div", { class: "nav-label", style: "padding-left:0" }, "What this app can do"), permissionRows(p)),
      h("div", { class: "facts" },
        h("div", { class: "fact" }, h("div", { class: "k" }, "Size"), h("div", { class: "v" }, bytes(p.size_bytes))),
        h("div", { class: "fact" }, h("div", { class: "k" }, "Files"), h("div", { class: "v" }, String(p.file_count))),
        h("div", { class: "fact" }, h("div", { class: "k" }, "Runs on"), h("div", { class: "v" }, p.platforms.join(", "))))),
    h("div", { class: "modal-foot" },
      h("button", { class: "btn", onclick: cancel }, "Cancel"),
      h("button", { class: "btn primary", onclick: () => confirm() }, icon(Check, 18), actionLabel)));

  async function confirm() {
    const steps: [StepEvent["step"] | "verify", string][] = [
      ["verify", "Signature and files verified"],
      ["files", "Copy the app"],
      ["environment", "Prepare its Python environment"],
      ["ready", "Ready"],
    ];
    let active: string = "files";
    let detail = "";
    const list = h("div", { class: "steps" });
    const draw = () => {
      const idx = steps.findIndex(([k]) => k === active);
      clear(list, steps.map(([, label], i) => {
        const done = i < idx || active === "ready";
        const cls = done ? "done" : i === idx ? "active" : "";
        return h("div", { class: `step ${cls}` },
          h("div", { class: "si" }, done ? icon(Check, 15) : i === idx ? icon(Loader, 15, "spin") : String(i + 1)),
          h("div", null, h("div", { class: "st" }, label), i === idx && detail && active !== "ready" ? h("div", { class: "ss" }, detail) : null));
      }));
    };
    draw();
    clear(modal,
      h("div", { class: "modal-head" }, appIcon(p, "lg"), h("div", null, h("h2", null, `Installing ${p.title}`), h("div", { class: "muted" }, `v${p.version}`))),
      h("div", { class: "modal-body" }, list),
      h("div", { class: "modal-foot" }));
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
      await ctx.refreshNav();
      const foot = modal.querySelector(".modal-foot")!;
      clear(foot,
        h("button", { class: "btn", onclick: () => { overlay.remove(); ctx.navigate(location.hash); } }, "Close"),
        h("button", { class: "btn primary", onclick: () => { overlay.remove(); ctx.navigate(`#/app/${encodeURIComponent(app.id)}`); } }, icon(Play, 17), "Open app"));
    } catch (e) {
      unlisten();
      overlay.remove();
      toast(`Installation failed: ${errorText(e)}`, "error");
    }
  }
}

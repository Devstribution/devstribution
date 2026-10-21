import { FolderOpen, X } from "lucide";
import type { RunRecord, RunStatus } from "../api";
import type { Ctx, Rendered } from "../main";
import { h, icon, appIcon, duration, relTime, statusPill, fileName, toast, errorText } from "../ui";

const FILTERS: [RunStatus | "all", string][] = [["all", "All"], ["ok", "Succeeded"], ["error", "Failed"], ["cancelled", "Cancelled"]];
let filter: RunStatus | "all" = "all";

function details(ctx: Ctx, r: RunRecord) {
  const log = h("div", { class: "log" }, "Loading…");
  ctx.api.runLog(r.id).then((t) => { log.textContent = t || "The app wrote nothing to its log."; }).catch(() => { log.textContent = ""; });
  const close = () => overlay.remove();
  const outputs = Object.entries(r.outputs);
  const overlay = h("div", { class: "overlay", onclick: (e: Event) => { if (e.target === overlay) close(); } },
    h("div", { class: "modal" },
      h("div", { class: "modal-head" }, appIcon({ id: r.app_id }),
        h("div", { style: "flex:1" }, h("h2", null, r.app_title), h("div", { class: "muted" }, `${new Date(r.started_at * 1000).toLocaleString()} · ${duration(r.duration_ms)} · v${r.version}`)),
        statusPill(r.status),
        h("button", { class: "icon-btn", "aria-label": "Close", onclick: close }, icon(X, 18))),
      h("div", { class: "modal-body" },
        r.error ? h("div", { class: "error-box" }, r.error) : null,
        outputs.length ? h("div", { class: "outputs" }, outputs.map(([k, v]) =>
          h("div", { class: "output-text" }, h("div", { class: "label" }, k), h("div", { class: "mono" }, typeof v === "string" ? v : JSON.stringify(v))))) : null,
        h("div", null, h("div", { class: "nav-label", style: "padding-left:0" }, "Log"), log)),
      h("div", { class: "modal-foot" },
        h("button", { class: "btn", onclick: () => ctx.api.openOutput(r.id, null, false).catch((e) => toast(errorText(e), "error")) }, icon(FolderOpen, 16), "Open result folder"),
        h("button", { class: "btn primary", onclick: close }, "Close"))));
  document.body.appendChild(overlay);
}

export async function runsView(ctx: Ctx): Promise<Rendered> {
  const [runs, overview] = await Promise.all([ctx.api.listRuns(undefined, 300), ctx.api.overview()]);
  const icons = new Map(overview.apps.map((a) => [a.id, a.icon]));
  const chips = h("div", { class: "filters" });
  const body = h("tbody");
  const summary = (r: RunRecord) => {
    if (r.error) return h("span", { class: "muted" }, r.error.length > 70 ? r.error.slice(0, 70) + "…" : r.error);
    const first = Object.values(r.outputs)[0];
    return first ? h("span", { class: "muted" }, typeof first === "string" && /[\\/]/.test(first) ? fileName(first) : String(first)) : h("span", { class: "muted" }, "–");
  };
  const draw = () => {
    chips.replaceChildren(...FILTERS.map(([k, label]) =>
      h("button", { class: `filter ${filter === k ? "active" : ""}`, onclick: () => { filter = k; draw(); } },
        label, k === "all" ? ` ${runs.length}` : ` ${runs.filter((r) => r.status === k).length}`)));
    const shown = runs.filter((r) => filter === "all" || r.status === filter);
    body.replaceChildren(...shown.map((r) =>
      h("tr", { onclick: () => details(ctx, r) },
        h("td", null, h("div", { class: "row" }, appIcon({ id: r.app_id, icon: icons.get(r.app_id) }, "sm"), h("strong", null, r.app_title))),
        h("td", null, statusPill(r.status)),
        h("td", { title: new Date(r.started_at * 1000).toLocaleString() }, relTime(r.started_at)),
        h("td", null, duration(r.duration_ms)),
        h("td", null, summary(r)))));
  };
  draw();
  return {
    title: "Runs",
    subtitle: "Every run, with its results and log",
    node: h("div", { class: "grid" }, chips,
      h("section", { class: "card" },
        runs.length
          ? h("table", { class: "table" }, h("thead", null, h("tr", null, ["App", "Status", "Started", "Duration", "Result"].map((t) => h("th", null, t)))), body)
          : h("div", { class: "empty" }, h("h4", null, "No runs yet"), h("p", null, "Runs appear here with their results and logs.")))),
  };
}

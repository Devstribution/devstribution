import { FolderOpen, X } from "lucide";
import type { RunRecord, RunStatus } from "../api";
import type { Ctx, Rendered } from "../main";
import { h, icon, appIcon, clear, duration, relTime, statusText, toast, errorText, runError, plural, trapFocus, STATUS_LABEL } from "../ui";
import { runsChart } from "../charts";
import { runOutcome } from "./home";

const FILTERS: (RunStatus | "all")[] = ["all", "ok", "error", "cancelled"];
let filter: RunStatus | "all" = "all";

function details(ctx: Ctx, r: RunRecord) {
  const log = h("div", { class: "log" }, "Loading…");
  ctx.api.runLog(r.id)
    .then((t) => { log.textContent = t === null ? "No log was kept for this run." : t || "The app wrote nothing to its log."; })
    .catch(() => { log.textContent = "The log could not be read."; });
  const close = () => { release(); overlay.remove(); document.removeEventListener("keydown", onKey); };
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
  document.addEventListener("keydown", onKey);
  const outputs = Object.entries(r.outputs);
  const overlay = h("div", { class: "overlay", onclick: (e: Event) => { if (e.target === overlay) close(); } },
    h("div", { class: "modal", role: "dialog", "aria-label": `${r.app_title} run` },
      h("div", { class: "modal-head" }, appIcon({ id: r.app_id }, "sm"),
        h("div", { class: "grow" }, h("h2", null, r.app_title),
          h("div", { class: "muted small" }, `${new Date(r.started_at * 1000).toLocaleString()} · ${duration(r.duration_ms)} · v${r.version}`)),
        statusText(r.status),
        h("button", { class: "icon-btn", "aria-label": "Close", onclick: close }, icon(X, 16))),
      h("div", { class: "modal-body" },
        runError(r, r.app_title, () => log.textContent ?? ""),
        outputs.length ? h("div", { class: "outputs" }, outputs.map(([k, v]) =>
          h("div", { class: "output-text" }, h("div", { class: "label" }, k), h("div", { class: "mono" }, typeof v === "string" ? v : JSON.stringify(v))))) : null,
        h("div", { class: "section-label" }, "Log"), log),
      h("div", { class: "modal-foot" },
        r.output_removed ? h("span", { class: "muted small grow" }, "Results removed by a cleanup") : h("button", { class: "btn", onclick: () => ctx.api.openOutput(r.id, null, false).catch((e) => toast(errorText(e), "error")) }, icon(FolderOpen, 16), "Open result folder"),
        h("button", { class: "btn primary", onclick: () => { close(); ctx.openApp(r.app_id); } }, "Open the app"))));
  document.body.appendChild(overlay);
  const release = trapFocus(overlay.firstElementChild as HTMLElement);
}

export async function activityView(ctx: Ctx): Promise<Rendered> {
  const [{ apps, stats }, runs] = await Promise.all([ctx.api.overview(), ctx.api.listRuns(undefined, 300)]);
  const icons = new Map(apps.map((a) => [a.id, a.icon]));
  const finished = stats.ok + stats.failed;
  const used = stats.usage.filter((u) => u.runs > 0 && icons.has(u.app_id)).length;
  const unused = apps.filter((a) => !stats.usage.some((u) => u.app_id === a.id && u.runs > 0));

  const kpi = (k: string, v: string, s: string) => h("div", { class: "kpi" }, h("div", { class: "k" }, k), h("div", { class: "v" }, v), h("div", { class: "s" }, s));
  const kpis = h("div", { class: "kpis" },
    kpi("Runs this month", String(stats.runs), `${stats.ok} done · ${stats.failed} didn’t work`),
    kpi("Worked", stats.success_rate === null ? "–" : `${Math.round(stats.success_rate * 100)}%`, finished ? `${stats.ok} of ${plural(finished, "finished run")}` : "No finished runs yet"),
    kpi("Time running", duration(stats.total_duration_ms), stats.runs ? `about ${duration(Math.round(stats.total_duration_ms / stats.runs))} per run` : "–"),
    kpi("Apps used", `${used} of ${apps.length}`, unused.length === 1 ? `${unused[0].title}: not this month` : unused.length ? `${unused.length} not this month` : "All of them"));

  const maxRuns = Math.max(1, ...stats.top_apps.map((a) => a.runs));
  const dash = h("div", { class: "dash" },
    h("section", { class: "card" },
      h("div", { class: "card-head" }, h("h2", null, "Runs, last 14 days"),
        h("div", { class: "legend" },
          h("span", null, h("i", { class: "ok" }), "done"),
          h("span", null, h("i", { class: "err" }), "didn’t work"),
          h("span", null, h("i", { class: "cancel" }), "cancelled"))),
      runsChart(stats.per_day.slice(-14))),
    h("section", { class: "card" },
      h("div", { class: "card-head" }, h("h2", null, "Most used this month")),
      stats.top_apps.length
        ? h("div", { class: "bar-list" }, stats.top_apps.slice(0, 4).map((t) =>
            h("button", { class: "bar-row", onclick: () => icons.has(t.app_id) && ctx.openApp(t.app_id) },
              appIcon({ id: t.app_id, icon: icons.get(t.app_id) }, "xs"),
              h("div", { class: "col" }, h("div", { class: "t" }, t.title),
                h("div", { class: "bar-track" }, h("div", { class: "bar-fill", style: `width:${(t.runs / maxRuns) * 100}%` }))),
              h("span", { class: "n" }, String(t.runs)))))
        : h("div", { class: "muted small" }, "Run an app to see it here.")));

  const chips = h("div", { class: "chips", role: "radiogroup", "aria-label": "Show" });
  const list = h("div", { class: "run-list" });
  const draw = () => {
    clear(chips, FILTERS.map((f) => {
      const n = f === "all" ? runs.length : runs.filter((r) => r.status === f).length;
      return h("button", { class: `chip ${filter === f ? "on" : ""}`, role: "radio", "aria-checked": String(filter === f), onclick: () => { filter = f; draw(); } },
        `${f === "all" ? "All" : STATUS_LABEL[f]} ${n}`);
    }));
    const shown = runs.filter((r) => filter === "all" || r.status === filter);
    clear(list, shown.length
      ? shown.map((r) =>
          h("button", { class: "run-row", onclick: () => details(ctx, r) },
            appIcon({ id: r.app_id, icon: icons.get(r.app_id) }, "sm"),
            h("div", { class: "col" }, h("div", { class: "t" }, r.app_title), h("div", { class: "s" }, runOutcome(r).what)),
            h("span", { class: "when", title: new Date(r.started_at * 1000).toLocaleString() }, `${relTime(r.started_at)} · ${duration(r.duration_ms)}`),
            statusText(r.status)))
      : h("div", { class: "empty-box" }, runs.length ? "No runs with this result." : "No runs yet: open an app and press Run."));
  };
  draw();

  return {
    node: h("div", { class: "page" },
      h("div", { class: "page-title" }, h("h1", null, "Activity")),
      kpis, dash, chips, list),
  };
}

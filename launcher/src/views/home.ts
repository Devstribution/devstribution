import { Activity, CircleCheck, LayoutGrid, Play, Plus, Timer, Sparkles, FileSpreadsheet, Bot, Workflow, ArrowRight } from "lucide";
import type { Ctx, Rendered } from "../main";
import { h, icon, appIcon, duration, relTime, statusPill, greeting, KIND_LABEL } from "../ui";
import { runsChart, ring } from "../charts";
import { quickRun } from "./app";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export async function homeView(ctx: Ctx): Promise<Rendered> {
  const { apps, stats, recent_runs } = await ctx.api.overview();

  if (apps.length === 0) {
    return {
      title: greeting(),
      subtitle: "Welcome to devstribution.",
      node: h("div", { class: "grid" },
        h("section", { class: "card welcome" },
          h("div", null,
            h("h2", null, "Run Python apps in one click"),
            h("p", null, "Tools, automations and proofs of concept from publishers you trust: verified, set up and started for you. No installers, no terminal."),
            h("button", { class: "btn big", onclick: () => ctx.install() }, icon(Plus, 20), "Install your first app")),
          h("div", { class: "art" }, [FileSpreadsheet, Bot, Workflow, Sparkles, Play, Activity].map((i) => h("div", null, icon(i, 28))))),
      ),
    };
  }

  const today = stats.per_day[stats.per_day.length - 1];
  const todayRuns = today ? today.ok + today.error + today.cancelled : 0;
  const half = Math.floor(stats.per_day.length / 2);
  const sum = (d: typeof stats.per_day) => d.reduce((a, x) => a + x.ok + x.error + x.cancelled, 0);
  const prev = sum(stats.per_day.slice(0, half));
  const last = sum(stats.per_day.slice(half));
  const trend = prev ? Math.round(((last - prev) / prev) * 100) : null;

  const kpi = (cls: string, ic: typeof Activity, label: string, value: string, foot: (Node | string)[], extra?: Node) =>
    h("div", { class: "card kpi" },
      h("div", { class: "kpi-top" }, h("div", { class: `kpi-icon ${cls}` }, icon(ic, 21)), h("div", { class: "kpi-label" }, label),
        extra ? h("div", { style: "margin-left:auto" }, extra) : null),
      h("div", { class: "kpi-value" }, value),
      h("div", { class: "kpi-foot" }, ...foot));

  const rate = stats.success_rate;
  const kpis = h("div", { class: "grid kpis" },
    kpi("blue", LayoutGrid, "Installed apps", String(stats.apps_installed), [`${plural(apps.filter((a) => a.kind === "form").length, "form")} · ${apps.filter((a) => a.kind !== "form").length} other`]),
    kpi("violet", Activity, "Runs", String(stats.runs), [
      trend === null ? h("span", { class: "trend flat" }, "new") : h("span", { class: `trend ${trend >= 0 ? "up" : "flat"}` }, `${trend >= 0 ? "+" : ""}${trend}%`),
      `vs previous week · ${todayRuns} today`,
    ]),
    kpi("green", CircleCheck, "Success rate", rate === null ? "–" : `${Math.round(rate * 100)}%`,
      [`${stats.ok} succeeded · ${stats.failed} failed`], rate === null ? undefined : ring(rate, 46)),
    kpi("orange", Timer, "Time running apps", duration(stats.total_duration_ms), ["work done for you, last 14 days"]),
  );

  const chartCard = h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h3", null, "Runs per day"), h("span", { class: "hint" }, "last 14 days"),
      h("div", { class: "right legend" },
        h("span", null, h("i", { style: "background:linear-gradient(#7a4dff,#3f6bff)" }), "Succeeded"),
        h("span", null, h("i", { style: "background:var(--danger)" }), "Failed"),
        h("span", null, h("i", { style: "background:var(--warning)" }), "Cancelled"))),
    runsChart(stats.per_day));

  const maxRuns = Math.max(1, ...stats.top_apps.map((a) => a.runs));
  const topCard = h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h3", null, "Most used"), h("span", { class: "hint" }, "runs, 14 days")),
    stats.top_apps.length
      ? h("div", { class: "bar-list" }, stats.top_apps.map((t) =>
          h("div", { class: "bar-row", style: "cursor:pointer", onclick: () => ctx.navigate(`#/app/${encodeURIComponent(t.app_id)}`) },
            appIcon({ id: t.app_id, icon: apps.find((a) => a.id === t.app_id)?.icon }, "sm"),
            h("div", null, h("div", { class: "title" }, t.title),
              h("div", { class: "bar-track" }, h("div", { class: "bar-fill", style: `width:${(t.runs / maxRuns) * 100}%` }))),
            h("strong", null, String(t.runs)))))
      : h("div", { class: "empty" }, h("p", null, "Run an app to see it here.")));

  const quick = h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h3", null, "Your apps"),
      h("div", { class: "right" }, h("button", { class: "btn ghost small", onclick: () => ctx.navigate("#/apps") }, "All apps", icon(ArrowRight, 15)))),
    h("div", { class: "quick-apps" }, apps.slice(0, 6).map((a) =>
      h("div", { class: "quick-app", onclick: () => ctx.navigate(`#/app/${encodeURIComponent(a.id)}`) },
        appIcon(a, "sm"),
        h("div", { style: "min-width:0" }, h("div", { class: "t" }, a.title), h("div", { class: "s" }, KIND_LABEL[a.kind] ?? a.kind)),
        h("button", { class: "play", title: `Run ${a.title}`, onclick: (e: Event) => { e.stopPropagation(); quickRun(ctx, a); } }, icon(Play, 15))))));

  const recent = h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h3", null, "Recent runs"),
      h("div", { class: "right" }, h("button", { class: "btn ghost small", onclick: () => ctx.navigate("#/runs") }, "History", icon(ArrowRight, 15)))),
    recent_runs.length
      ? h("div", { class: "list" }, recent_runs.slice(0, 6).map((r) =>
          h("div", { class: "list-item", style: "cursor:pointer", onclick: () => ctx.navigate(`#/app/${encodeURIComponent(r.app_id)}`) },
            appIcon({ id: r.app_id, icon: apps.find((a) => a.id === r.app_id)?.icon }, "sm"),
            h("div", { style: "min-width:0;flex:1" }, h("div", { class: "t" }, r.app_title), h("div", { class: "s" }, `${relTime(r.started_at)} · ${duration(r.duration_ms)}`)),
            statusPill(r.status))))
      : h("div", { class: "empty" }, h("p", null, "No runs yet.")));

  return {
    title: greeting(),
    subtitle: "Here is what your apps did in the last 14 days.",
    node: h("div", { class: "grid" }, kpis,
      h("div", { class: "grid two-one" }, chartCard, topCard),
      h("div", { class: "grid one-one" }, quick, recent)),
  };
}

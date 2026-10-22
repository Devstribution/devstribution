import { Search, Clock, TrendingUp, FileSpreadsheet, Bot, Workflow } from "lucide";
import type { AppView, RunRecord } from "../api";
import type { Ctx, Rendered } from "../main";
import { h, icon, appIcon, clear, relTime, statusText, fileName, plural } from "../ui";

let query = "";

function greeting(): string {
  const hr = new Date().getHours();
  return hr < 12 ? "Good morning" : hr < 18 ? "Good afternoon" : "Good evening";
}

function tile(ctx: Ctx, app: AppView, sub: string): HTMLElement {
  return h("button", { class: "tile", onclick: () => ctx.openApp(app.id) },
    appIcon(app, "xl"), h("div", { class: "t", title: app.title }, app.title), h("div", { class: "s" }, sub));
}

/** One line on what a run left behind, and the button that goes with it. */
export function runOutcome(r: RunRecord): { what: string; action: string } {
  if (r.status === "error") return { what: r.error ?? "The app stopped with an error", action: "See what happened" };
  if (r.status === "cancelled") return { what: "Stopped before the end", action: "Run again" };
  if (r.status === "running") return { what: "Running now", action: "Open" };
  const first = Object.values(r.outputs)[0];
  const what = typeof first === "string" ? (/[\\/]/.test(first) ? fileName(first) : first) : "Finished";
  return { what, action: Object.keys(r.outputs).length ? "Open result" : "Open" };
}

export async function homeView(ctx: Ctx): Promise<Rendered> {
  const { apps, stats, recent_runs } = await ctx.api.overview();

  if (apps.length === 0) {
    return {
      node: h("div", { class: "page welcome" },
        h("div", { class: "art" }, [FileSpreadsheet, Bot, Workflow].map((ic) => h("div", null, icon(ic, 28)))),
        h("h1", null, "Run Python apps in one click"),
        h("p", null, "Tools and automations from publishers you trust: checked, set up and started for you. No installers, no terminal."),
        h("button", { class: "btn primary big", onclick: () => ctx.install() }, "Install your first app"),
        h("p", { class: "small" }, "Or drop a .dsapp file anywhere on this window.")),
    };
  }

  const byId = new Map(apps.map((a) => [a.id, a]));
  const usage = stats.usage.filter((u) => byId.has(u.app_id));
  // Recent: the two apps run last, topped up with apps installed but not run yet (a new user's
  // first app must be one click away). Often: the two run most this month, not already in Recent.
  const recent = usage.slice(0, 2).map((u) => byId.get(u.app_id)!);
  const fresh = apps.filter((a) => !usage.some((u) => u.app_id === a.id)).sort((a, b) => b.installed_at - a.installed_at);
  recent.push(...fresh.slice(0, 2 - recent.length));
  const often = [...usage]
    .filter((u) => u.runs > 0 && !recent.some((a) => a.id === u.app_id))
    .sort((a, b) => b.runs - a.runs)
    .slice(0, 2);
  const lastRun = new Map(usage.map((u) => [u.app_id, u.last_run_at]));

  const shelf = (title: string, ic: typeof Clock, tiles: HTMLElement[], empty: string) =>
    h("section", { class: "shelf", "aria-label": title },
      h("div", { class: "shelf-title" }, icon(ic, 14), title),
      tiles.length ? h("div", { class: "shelf-tiles" }, tiles) : h("div", { class: "shelf-empty" }, empty));

  const shelves = h("div", { class: "shelves" },
    shelf("Recent", Clock, recent.map((a) => tile(ctx, a, lastRun.has(a.id) ? `Used ${relTime(lastRun.get(a.id)!)}` : `New · installed ${relTime(a.installed_at)}`)), "Apps you run show up here."),
    shelf("You use these often", TrendingUp, often.map((u) => tile(ctx, byId.get(u.app_id)!, `${plural(u.runs, "run")} this month`)),
      "Your most used apps show up here."));

  const results = recent_runs.filter((r) => r.status !== "running").slice(0, 3);
  const latest = results.length
    ? h("section", { class: "stack" },
        h("div", { class: "card-head flush" }, h("h2", { class: "section-title" }, "Latest results"),
          h("button", { class: "link", onclick: () => ctx.navigate("#/activity") }, "All activity")),
        h("div", { class: "results" }, results.map((r) => {
          const { what, action } = runOutcome(r);
          const app = byId.get(r.app_id);
          const act = () => {
            if (action !== "Open result") return ctx.openApp(r.app_id);
            const file = Object.values(r.outputs).find((v): v is string => typeof v === "string" && /[\\/]/.test(v));
            ctx.api.openOutput(r.id, file ?? null, false).catch(() => ctx.openApp(r.app_id));
          };
          return h("div", { class: "result" },
            h("div", { class: "result-head" }, appIcon({ id: r.app_id, icon: app?.icon }, "sm"),
              h("div", { class: "who" }, h("div", { class: "t" }, r.app_title), h("div", { class: "s" }, relTime(r.started_at))),
              statusText(r.status)),
            h("div", { class: "what", title: what }, what),
            app ? h("button", { class: "btn small", onclick: act }, action) : h("span", { class: "muted tiny" }, "Uninstalled"));
        })))
    : null;

  // Typing in the search shows matching apps in place of the shelves.
  const matches = h("div", { class: "search-results" });
  const input = h("input", { type: "search", placeholder: "Search your apps", "aria-label": "Search your apps", value: query });
  const draw = () => {
    const q = query.trim().toLowerCase();
    const found = apps.filter((a) => `${a.title} ${a.description ?? ""} ${a.namespace}`.toLowerCase().includes(q));
    shelves.hidden = !!q;
    if (latest) latest.hidden = !!q;
    matches.hidden = !q;
    clear(matches, found.length
      ? found.map((a) => tile(ctx, a, a.namespace))
      : h("div", { class: "empty-box" }, `No app matches “${query.trim()}”.`));
  };
  input.addEventListener("input", () => { query = input.value; draw(); });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      const first = matches.querySelector("button.tile") as HTMLButtonElement | null;
      first?.click();
    }
    if (e.key === "Escape") { query = ""; input.value = ""; draw(); }
  });
  draw();

  const sub = apps.length > 4 ? `Pick up where you left off, or search all ${apps.length} apps.` : `${plural(apps.length, "app")}, ready to run.`;
  return {
    node: h("div", { class: "page home" },
      h("div", { class: "hello" }, h("h1", null, greeting()), h("p", null, sub)),
      h("label", { class: "search-pill" }, icon(Search, 18), input, h("kbd", null, "Ctrl K")),
      matches, shelves, latest),
  };
}


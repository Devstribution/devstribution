import { Search } from "lucide";
import type { AppView } from "../api";
import type { Ctx, Rendered } from "../main";
import { h, icon, appIcon, clear, relTime, worksWith, plural, keepFocus } from "../ui";

// Kept across visits (and across the refresh after each run) so the page doesn't reset under the user.
const state = { query: "", works: "All", publisher: "Anyone", sort: "Most used" as Sort };
type Sort = "Most used" | "Recent" | "A–Z";
const SORTS: Sort[] = ["Most used", "Recent", "A–Z"];

export async function appsView(ctx: Ctx): Promise<Rendered> {
  const { apps, stats } = await ctx.api.overview();
  const usage = new Map(stats.usage.map((u) => [u.app_id, u]));
  const runs = (a: AppView) => usage.get(a.id)?.runs ?? 0;
  const last = (a: AppView) => usage.get(a.id)?.last_run_at ?? 0;

  // Only filters some app actually matches; a filter that empties the list on its own is noise.
  const worksOptions = ["Excel", "SAP", "Internet", "Own window"].filter((w) => apps.some((a) => worksWith(a).includes(w)));
  const publishers = [...new Set(apps.map((a) => a.namespace))].sort();
  if (state.works !== "All" && !worksOptions.includes(state.works)) state.works = "All";
  if (state.publisher !== "Anyone" && !publishers.includes(state.publisher)) state.publisher = "Anyone";

  const input = h("input", { type: "search", placeholder: "Search apps", "aria-label": "Search apps", value: state.query });
  const sortBox = h("div", { class: "seg", role: "radiogroup", "aria-label": "Sort" });
  const chips = h("div", { class: "filters" });
  const countLine = h("div", { class: "count-line" });
  const list = h("div", { class: "app-list" });

  const chip = (label: string, on: boolean, pick: () => void) =>
    h("button", { class: `chip ${on ? "on" : ""}`, "aria-pressed": String(on), onclick: pick }, label);

  const draw = () => keepFocus([sortBox, chips], () => {
    clear(sortBox, SORTS.map((s) =>
      h("button", { class: state.sort === s ? "on" : "", role: "radio", "aria-checked": String(state.sort === s), onclick: () => { state.sort = s; draw(); } }, s)));
    clear(chips,
      worksOptions.length ? h("div", { class: "chips", role: "group", "aria-label": "Works with" }, h("span", { class: "label" }, "Works with"),
        ["All", ...worksOptions].map((w) => chip(w, state.works === w, () => { state.works = w; draw(); }))) : null,
      publishers.length > 1 ? h("div", { class: "chips", role: "group", "aria-label": "Publisher" }, h("span", { class: "label" }, "Publisher"),
        ["Anyone", ...publishers].map((p) => chip(p, state.publisher === p, () => { state.publisher = p; draw(); }))) : null);

    const q = state.query.trim().toLowerCase();
    const shown = apps
      .filter((a) => state.works === "All" || worksWith(a).includes(state.works))
      .filter((a) => state.publisher === "Anyone" || a.namespace === state.publisher)
      .filter((a) => !q || `${a.title} ${a.description ?? ""} ${a.namespace}`.toLowerCase().includes(q))
      .sort(state.sort === "A–Z" ? (a, b) => a.title.localeCompare(b.title)
        : state.sort === "Recent" ? (a, b) => last(b) - last(a)
        : (a, b) => runs(b) - runs(a) || last(b) - last(a));

    const filtered = state.works !== "All" || state.publisher !== "Anyone" || !!q;
    clear(countLine, apps.length === 0 ? "No apps yet" : shown.length === apps.length ? `All ${plural(apps.length, "app")}` : `${shown.length} of ${plural(apps.length, "app")}`,
      filtered ? [" · ", h("button", { class: "link", onclick: () => {
        Object.assign(state, { query: "", works: "All", publisher: "Anyone" });
        input.value = "";
        draw();
      } }, "Clear filters")] : null);

    clear(list, shown.length
      ? shown.map((a) => {
          const w = worksWith(a);
          return h("div", { class: "app-row" },
            appIcon(a),
            h("div", { class: "main-col" }, h("div", { class: "t" }, a.title), h("div", { class: "d", title: a.description ?? "" }, a.description ?? "No description.")),
            h("div", { class: "meta" },
              h("span", null, `by ${a.namespace}${w.length ? ` · ${w.join(", ")}` : ""}`),
              a.quarantine
                ? h("span", { class: "tag red", title: `Stopped by devstribution: the app ${a.quarantine.hit.message}` }, "Blocked")
                : h("span", null, last(a) ? `Used ${relTime(last(a))} · ${plural(runs(a), "run")} this month` : "Not used yet"),
              ctx.appUpdates.get(a.id) ? h("span", { class: "tag" }, "Update available") : null),
            h("button", { class: "btn primary", onclick: () => ctx.openApp(a.id) }, "Open"));
        })
      : h("div", { class: "empty-box" }, apps.length === 0
          ? "No apps yet. Install one with the button below, or drop a .dsapp file anywhere on this window."
          : "No app matches these filters."));
  });
  input.addEventListener("input", () => { state.query = input.value; draw(); });
  draw();

  return {
    node: h("div", { class: "page" },
      h("div", { class: "page-title" }, h("h1", null, "Apps"),
        h("label", { class: "search-box" }, icon(Search, 16), input),
        sortBox),
      chips, countLine, list,
      h("button", { class: "btn self-start", onclick: () => ctx.install() }, "Install another app…")),
  };
}

import { Play, Plus, ShieldCheck, ShieldAlert } from "lucide";
import type { Ctx, Rendered } from "../main";
import { h, icon, appIcon, KIND_LABEL } from "../ui";
import { quickRun } from "./app";

const FILTERS: [string, string][] = [["all", "All"], ["form", "Forms"], ["gui", "Desktop"], ["cli", "Command line"]];
let filter = "all";

export async function appsView(ctx: Ctx): Promise<Rendered> {
  const { apps } = await ctx.api.overview();
  const grid = h("div", { class: "apps-grid" });
  const chips = h("div", { class: "filters" });

  const draw = () => {
    chips.replaceChildren(...FILTERS.map(([key, label]) =>
      h("button", { class: `filter ${filter === key ? "active" : ""}`, onclick: () => { filter = key; draw(); } }, label)));
    const shown = apps.filter((a) =>
      (filter === "all" || a.kind === filter) &&
      (!ctx.search || `${a.title} ${a.description ?? ""} ${a.namespace}`.toLowerCase().includes(ctx.search)));
    grid.replaceChildren(
      ...shown.map((a) =>
        h("article", { class: "card app-card", onclick: () => ctx.navigate(`#/app/${encodeURIComponent(a.id)}`) },
          h("div", { class: "top" }, appIcon(a),
            h("div", { style: "min-width:0" },
              h("h3", null, a.title),
              h("div", { class: "by" }, a.trust === "first_use" ? icon(ShieldCheck, 13) : icon(ShieldAlert, 13), `by ${a.namespace} · v${a.version}`))),
          h("p", { class: "desc" }, a.description ?? "No description."),
          h("div", { class: "foot" },
            h("span", { class: "chip blue" }, KIND_LABEL[a.kind] ?? a.kind),
            a.permissions.network.length ? h("span", { class: "chip" }, "Internet") : null,
            a.permissions.sap ? h("span", { class: "chip violet" }, "SAP") : null,
            h("button", { class: "play", title: `Run ${a.title}`, onclick: (e: Event) => { e.stopPropagation(); quickRun(ctx, a); } }, icon(Play, 17))))),
      h("button", { class: "install-tile", onclick: () => ctx.install() },
        h("div", { class: "plus" }, icon(Plus, 22)),
        h("strong", null, "Install an app"),
        h("span", null, "Choose a .dsapp file, or drop one on the window")),
    );
  };
  draw();

  return {
    title: "Apps",
    subtitle: ctx.search ? `Matching “${ctx.search}”` : `${apps.length} installed`,
    node: h("div", { class: "grid" }, chips, grid),
  };
}

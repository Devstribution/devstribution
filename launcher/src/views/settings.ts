import { ShieldCheck, HardDrive, Info, Building2 } from "lucide";
import type { Ctx, Rendered } from "../main";
import { h, icon, relTime } from "../ui";

export async function settingsView(ctx: Ctx): Promise<Rendered> {
  const s = await ctx.api.settings();
  const head = (ic: typeof Info, cls: string, title: string, hint?: string) =>
    h("div", { class: "card-head" }, h("div", { class: `kpi-icon ${cls}`, style: "width:36px;height:36px;border-radius:10px" }, icon(ic, 18)),
      h("h3", null, title), hint ? h("span", { class: "hint" }, hint) : null);

  return {
    title: "Settings",
    subtitle: "How this launcher decides what to trust",
    node: h("div", { class: "grid one-one" },
      h("div", { class: "grid" },
        h("section", { class: "card" }, head(Building2, "blue", "Mode"),
          h("div", { class: "banner blue" }, h("div", null,
            h("strong", null, "Free mode"),
            "You decide which apps to install. Every package is checked: files must match the publisher's signature, and later versions must come from the same publisher key. ",
            "When your IT department manages devstribution, its policy (approved publishers, blocked apps) appears here and replaces these choices."))),
        h("section", { class: "card" }, head(ShieldCheck, "green", "Publishers you trust", `${s.publishers.length}`),
          s.publishers.length
            ? h("table", { class: "table" },
                h("thead", null, h("tr", null, ["Publisher", "Key", "Since", "How"].map((t) => h("th", null, t)))),
                h("tbody", null, s.publishers.map((p) => h("tr", { style: "cursor:default" },
                  h("td", null, h("strong", null, p.namespace)),
                  h("td", { class: "mono" }, p.key_id),
                  h("td", null, relTime(p.pinned_at)),
                  h("td", null, h("span", { class: "chip" }, p.source === "first-use" ? "First install" : p.source))))))
            : h("div", { class: "muted" }, "None yet: the first app you install from a publisher pins its key."))),
      h("div", { class: "grid" },
        h("section", { class: "card" }, head(HardDrive, "violet", "Storage"),
          h("dl", { class: "kv" },
            h("dt", null, "Data folder"), h("dd", { class: "mono" }, s.data_dir),
            h("dt", null, "Contents"), h("dd", null, "apps, Python environments, run results and logs"))),
        h("section", { class: "card" }, head(Info, "orange", "About"),
          h("dl", { class: "kv" },
            h("dt", null, "Launcher"), h("dd", null, `devstribution ${s.version}`),
            h("dt", null, "Environments"), h("dd", null, s.uv_version ?? "uv not found"),
            h("dt", null, "Platform"), h("dd", null, s.platform),
            h("dt", null, "License"), h("dd", null, "MIT or Apache-2.0"))))),
  };
}

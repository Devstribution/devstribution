import { X } from "lucide";
import type { Ctx } from "../main";
import { h, icon, clear, relTime, toast, errorText } from "../ui";

export type Theme = "light" | "dark" | "system";
const THEMES: [Theme, string][] = [["light", "Light"], ["dark", "Dark"], ["system", "System"]];

let open: (() => void) | null = null;

/** The settings popover above the round button in the bottom-right corner. */
export function toggleSettings(ctx: Ctx) {
  if (open) { open(); return; }
  const scrim = h("div", { class: "scrim clear" });
  const pop = h("aside", { class: "popover", role: "dialog", "aria-label": "Settings" });
  const close = () => {
    scrim.remove();
    pop.remove();
    document.removeEventListener("keydown", onKey);
    open = null;
  };
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
  scrim.addEventListener("click", close);
  document.addEventListener("keydown", onKey);
  document.body.append(scrim, pop);
  open = close;

  const themeSeg = h("div", { class: "seg full", role: "radiogroup", "aria-label": "Appearance" });
  const drawTheme = () => clear(themeSeg, THEMES.map(([t, label]) =>
    h("button", { class: ctx.theme.get() === t ? "on" : "", role: "radio", "aria-checked": String(ctx.theme.get() === t), onclick: () => { ctx.theme.set(t); drawTheme(); } }, label)));
  drawTheme();

  const head = h("div", { class: "popover-head" }, h("strong", null, "Settings"),
    h("button", { class: "icon-btn", "aria-label": "Close", onclick: close }, icon(X, 16)));
  clear(pop, head, h("div", { class: "settings-group" }, h("span", { class: "section-label" }, "Appearance"), themeSeg));

  ctx.api.settings().then((s) => {
    if (open !== close) return;
    pop.append(
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Publishers you trust"),
        s.publishers.length
          ? s.publishers.map((p) => h("div", { class: "publisher" },
              h("div", { style: "flex:1;min-width:0" }, h("div", { class: "name" }, p.namespace), h("div", { class: "mono muted" }, p.key_id)),
              h("span", { class: "muted", style: "font-size:12px", title: p.source === "first-use" ? "Pinned when you installed its first app" : p.source }, relTime(p.pinned_at))))
          : h("div", { class: "muted", style: "font-size:13px" }, "None yet: the first app you install from a publisher pins its key.")),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Mode"),
        h("div", { style: "font-size:13px;color:var(--text-2)" },
          "Free mode: you decide what to install. Every package is checked, and later versions must come from the same publisher key. If your IT department manages devstribution, its rules show here.")),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Storage"),
        h("div", { class: "mono", style: "overflow-wrap:anywhere;color:var(--text-2)" }, s.data_dir),
        h("div", { class: "muted", style: "font-size:12px" }, "Apps, their Python environments, run results and logs.")),
      h("div", { class: "about" }, `devstribution ${s.version} · ${s.uv_version ?? "uv not found"} · ${s.platform} · MIT or Apache-2.0`));
  }).catch((e) => toast(errorText(e), "error"));
}

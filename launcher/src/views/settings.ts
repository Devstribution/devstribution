import { X, Check, Pipette } from "lucide";
import type { Ctx } from "../main";
import { h, icon, clear, relTime, toast, errorText } from "../ui";
import { ACCENTS, DEFAULT_LOOK, textOn, type Look, type Theme, type StatusColours } from "../look";

const THEMES: [Theme, string][] = [["light", "Light"], ["dark", "Dark"], ["system", "System"]];
const STATUS: [StatusColours, string][] = [["standard", "Standard"], ["colour-blind", "Colour-blind safe"]];

let open: (() => void) | null = null;

/** A segmented control for one of the look's choices. */
function choice<K extends "theme" | "status">(ctx: Ctx, key: K, options: [Look[K], string][], label: string, redraw: () => void) {
  return h("div", { class: "seg full", role: "radiogroup", "aria-label": label }, options.map(([v, text]) => {
    const on = ctx.look.get()[key] === v;
    return h("button", { class: on ? "on" : "", role: "radio", "aria-checked": String(on), onclick: () => { ctx.look.set({ [key]: v }); redraw(); } }, text);
  }));
}

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

  const appearance = h("div", { class: "stack" });
  const drawAppearance = () => {
    const look = ctx.look.get();
    const custom = look.accent.startsWith("#") ? look.accent : null;
    const picker = h("input", { type: "color", value: custom ?? "#2563eb", "aria-label": "Pick your own accent colour" }) as HTMLInputElement;
    picker.addEventListener("input", () => ctx.look.set({ accent: picker.value }));
    picker.addEventListener("change", drawAppearance);
    const changed = JSON.stringify(look) !== JSON.stringify(DEFAULT_LOOK);
    clear(appearance,
      h("div", { class: "settings-group" }, h("span", { class: "section-label" }, "Theme"), choice(ctx, "theme", THEMES, "Theme", drawAppearance)),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Accent colour: buttons, selections, progress"),
        h("div", { class: "swatches", role: "radiogroup", "aria-label": "Accent colour" },
          ACCENTS.map((a) => {
            const on = look.accent === a.id;
            return h("button", {
              class: `swatch ${a.id} ${on ? "on" : ""}`, role: "radio", "aria-checked": String(on), "aria-label": a.label, title: a.label,
              onclick: () => { ctx.look.set({ accent: a.id }); drawAppearance(); },
            }, on ? icon(Check, 16) : null);
          }),
          h("label", { class: `swatch custom ${custom ? "on" : ""}`, title: "Your own colour", style: custom ? `background:${custom};color:${textOn(custom)}` : "" },
            icon(custom ? Check : Pipette, 16), picker))),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Status colours: done, didn’t work, warnings"),
        choice(ctx, "status", STATUS, "Status colours", drawAppearance)),
      changed ? h("button", { class: "link small self-start", onclick: () => { ctx.look.reset(); drawAppearance(); } }, "Back to the default look") : null);
  };
  drawAppearance();

  const head = h("div", { class: "popover-head" }, h("strong", null, "Settings"),
    h("button", { class: "icon-btn", "aria-label": "Close", onclick: close }, icon(X, 16)));
  clear(pop, head, appearance);

  ctx.api.settings().then((s) => {
    if (open !== close) return;
    pop.append(
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Publishers you trust"),
        s.publishers.length
          ? s.publishers.map((p) => h("div", { class: "publisher" },
              h("div", { class: "grow" }, h("div", { class: "name" }, p.namespace), h("div", { class: "mono muted" }, p.key_id)),
              h("span", { class: "muted tiny", title: p.source === "first-use" ? "Pinned when you installed its first app" : p.source }, relTime(p.pinned_at))))
          : h("div", { class: "muted small" }, "None yet: the first app you install from a publisher pins its key.")),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Mode"),
        h("div", { class: "small soft-text" },
          "Free mode: you decide what to install. Every package is checked, and later versions must come from the same publisher key. If your IT department manages devstribution, its rules show here.")),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Storage"),
        h("div", { class: "mono soft-text wrap-anywhere" }, s.data_dir),
        h("div", { class: "muted tiny" }, "Apps, their Python environments, run results and logs.")),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Uninstall"),
        h("button", { class: "btn small danger self-start", onclick: () => uninstallLauncher(ctx) }, "Uninstall devstribution…")),
      h("div", { class: "about" }, `devstribution ${s.version} · ${s.uv_version ?? "uv not found"} · ${s.platform} · MIT or Apache-2.0`));
  }).catch((e) => toast(errorText(e), "error"));
}

/** Hands over to the setup program's uninstaller, which asks whether to delete app data too. */
async function uninstallLauncher(ctx: Ctx) {
  const ok = await ctx.api.ask(
    "devstribution will close and its uninstaller will open. Apps that are running stop. The uninstaller asks whether to delete your apps, their results and saved secrets as well.",
    "Uninstall devstribution", "Continue");
  if (!ok) return;
  try { await ctx.api.uninstallLauncher(); } catch (e) { toast(errorText(e), "error"); }
}

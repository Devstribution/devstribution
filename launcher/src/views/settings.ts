import { X, Check, Pipette, FolderOpen, Trash2, ClipboardCopy } from "lucide";
import type { Ctx } from "../main";
import { openCleanup } from "./cleanup";
import type { Policy, Settings } from "../api";
import { h, icon, clear, relTime, toast, errorText, copyText } from "../ui";
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

  const openCleanupDialog = () => {
    ctx.api.overview().then(({ apps }) => { close(); openCleanup(ctx, apps); }).catch((e) => toast(errorText(e), "error"));
  };

  ctx.api.settings().then((s) => {
    if (open !== close) return;
    const free = [
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
    ];
    pop.append(
      ...(s.policy ? managedGroups(s.policy) : free),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Storage"),
        h("div", { class: "mono soft-text wrap-anywhere" }, s.data_dir),
        h("div", { class: "muted tiny" }, "Apps, their Python environments, run results and logs."),
        h("div", { class: "inline" },
          h("button", { class: "btn small self-start", onclick: () => ctx.api.openDataFolder().catch((e) => toast(errorText(e), "error")) },
            icon(FolderOpen, 14), "Open folder"),
          h("button", { class: "btn small self-start", onclick: openCleanupDialog }, icon(Trash2, 14), "Clean up…"))),
      h("div", { class: "settings-group" },
        h("span", { class: "section-label" }, "Uninstall"),
        h("button", { class: "btn small danger self-start", onclick: () => uninstallLauncher(ctx) }, "Uninstall devstribution…")),
      aboutGroup(ctx, s));
  }).catch((e) => toast(errorText(e), "error"));
}

/** What ships inside devstribution, and under which licence. The Rust libraries are in Cargo.lock. */
const COMPONENTS: [string, string][] = [
  ["uv (Astral): sets up each app's Python", "MIT or Apache-2.0"],
  ["Tauri and the Rust libraries it builds on", "mostly MIT or Apache-2.0; the full list is in the licence texts"],
  ["Microsoft Edge WebView2: draws this window", "Microsoft's licence; installed with Windows"],
  ["Manrope, the typeface", "SIL Open Font Licence 1.1"],
  ["Lucide, the icons", "ISC"],
  ["Python: downloaded by uv when an app is first set up", "PSF licence"],
];

function aboutGroup(ctx: Ctx, s: Settings) {
  const copy = async () => {
    try {
      await copyText(await ctx.api.diagnostics());
      toast("Copied. Read it before you send it: it has no secrets, but it lists your apps and the first line of each failed run’s error.", "ok");
    } catch (e) { toast(errorText(e), "error"); }
  };
  return h("div", { class: "settings-group" },
    h("span", { class: "section-label" }, "About"),
    h("div", { class: "small soft-text" }, `devstribution ${s.version} · ${s.uv_version ?? "uv not found"} · ${s.platform}`),
    h("div", { class: "small soft-text" }, "Free software, MIT or Apache-2.0: github.com/Devstribution/devstribution"),
    h("div", { class: "muted tiny wrap-anywhere" }, `Log: ${s.log_file}`),
    h("details", { class: "disclosure" }, h("summary", null, "Software inside devstribution"),
      h("dl", { class: "kv" }, COMPONENTS.map(([name, licence]) => [h("dt", null, name), h("dd", null, licence)])),
      h("button", { class: "link small", onclick: () => ctx.api.openNotices().catch((e) => toast(errorText(e), "error")) }, "Open all licence texts")),
    h("button", { class: "btn small self-start", onclick: copy }, icon(ClipboardCopy, 14), "Copy diagnostics"),
    h("div", { class: "muted tiny" }, "For a bug report or your IT: versions, your apps and the latest runs. No secrets, inputs or results."));
}

/** Managed mode: the policy IT set, and the authors it approves. */
function managedGroups(p: Policy): HTMLElement[] {
  const rules = [
    p.scan_danger === "refuse" ? "Packages whose code does things malware does are not installed." : "You decide about packages whose code does things malware does.",
    p.users_may_allow_guard_rules ? "You may let an app run past what devstribution stopped." : "Only IT can let an app run past what devstribution stopped.",
    p.guard_report_only.length ? `Only reported, never stopped: ${p.guard_report_only.join(", ")}.` : null,
    p.min_launcher_version ? `devstribution ${p.min_launcher_version} or newer is required.` : null,
  ].filter((r): r is string => !!r);
  return [
    h("div", { class: "settings-group" },
      h("span", { class: "section-label" }, "Mode"),
      h("div", { class: "small soft-text" },
        p.dry_run
          ? "Managed by your organisation, on trial: its policy is being tried out, so nothing is refused yet. What it would refuse is logged."
          : "Managed by your organisation: only apps from the authors it approves install and run."),
      p.contact ? h("div", { class: "small soft-text" }, `Questions: ${p.contact}`) : null,
      p.problems.map((pr) => h("div", { class: "banner amber small" }, h("div", null, pr))),
      h("ul", { class: "small soft-text policy-rules" }, rules.map((r) => h("li", null, r))),
      h("div", { class: "muted tiny wrap-anywhere" }, `Policy: ${p.source}`),
      h("div", { class: "muted tiny wrap-anywhere" }, `Log: ${p.log_file}`)),
    h("div", { class: "settings-group" },
      h("span", { class: "section-label" }, "Approved authors"),
      p.authors.length
        ? p.authors.map((a) => h("div", { class: "publisher" },
            h("div", { class: "grow" }, h("div", { class: "name" }, a.name ? `${a.namespace} · ${a.name}` : a.namespace),
              a.fingerprints.map((f) => h("div", { class: "mono muted" }, f)))))
        : h("div", { class: "muted small" }, "None: no app can be installed until IT approves an author.")),
  ];
}

/** Hands over to the setup program's uninstaller, which asks whether to delete app data too. */
async function uninstallLauncher(ctx: Ctx) {
  const ok = await ctx.api.ask(
    "devstribution will close and its uninstaller will open. Apps that are running stop. The uninstaller asks whether to delete your apps, their results and saved secrets as well.",
    "Uninstall devstribution", "Continue");
  if (!ok) return;
  try { await ctx.api.uninstallLauncher(); } catch (e) { toast(errorText(e), "error"); }
}

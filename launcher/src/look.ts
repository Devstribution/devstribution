// The user's appearance choices: theme, accent colour, status colours. The colours themselves live in
// style.css as tokens; this only switches between them (see docs/design.md).

export type Theme = "light" | "dark" | "system";
export type StatusColours = "standard" | "colour-blind";

export interface Look {
  theme: Theme;
  /** An ACCENTS id, or a "#rrggbb" the user picked. */
  accent: string;
  status: StatusColours;
}

export const DEFAULT_LOOK: Look = { theme: "system", accent: "ink", status: "standard" };

/** Preset accents. "ink" is the default: black on light, white on dark. The rest carry white text in both themes. */
export const ACCENTS: { id: string; label: string }[] = [
  { id: "ink", label: "Ink" },
  { id: "blue", label: "Blue" },
  { id: "violet", label: "Violet" },
  { id: "green", label: "Green" },
  { id: "orange", label: "Orange" },
  { id: "pink", label: "Pink" },
  { id: "teal", label: "Teal" },
];

const HEX = /^#[0-9a-f]{6}$/i;

/** Black or white, whichever reads better on the colour (WCAG relative luminance). */
export function textOn(hex: string): string {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  const l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return (l + 0.05) / 0.05 > 1.05 / (l + 0.05) ? "#18181b" : "#ffffff";
}

/** Keeps only values this version understands, so a bad or old stored value falls back to the default. */
export function cleanLook(v: Partial<Look> | null | undefined): Look {
  const theme = v?.theme && ["light", "dark", "system"].includes(v.theme) ? v.theme : DEFAULT_LOOK.theme;
  const accent = v?.accent && (ACCENTS.some((a) => a.id === v.accent) || HEX.test(v.accent)) ? v.accent : DEFAULT_LOOK.accent;
  const status = v?.status === "colour-blind" ? "colour-blind" : "standard";
  return { theme, accent, status };
}

const media = window.matchMedia?.("(prefers-color-scheme: dark)");

export function applyLook(look: Look) {
  const root = document.documentElement;
  const dark = look.theme === "dark" || (look.theme === "system" && !!media?.matches);
  root.dataset.theme = dark ? "dark" : "light";
  root.dataset.status = look.status;
  if (HEX.test(look.accent)) {
    delete root.dataset.accent;
    root.style.setProperty("--accent", look.accent);
    root.style.setProperty("--on-accent", textOn(look.accent));
  } else {
    root.dataset.accent = look.accent;
    root.style.removeProperty("--accent");
    root.style.removeProperty("--on-accent");
  }
}

export function onSystemThemeChange(cb: () => void) {
  media?.addEventListener?.("change", cb);
}

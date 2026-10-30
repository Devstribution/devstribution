// Small DOM helpers: element builder, icons, formatting, toasts and recurring bits of UI.

import { createElement, type IconNode } from "lucide";
import { CircleAlert, CircleCheck, Info, Loader, ShieldCheck, ShieldAlert, ShieldX, X } from "lucide";
import type { RunStatus } from "./api";

type Child = Node | string | number | null | undefined | false | Child[];
type Attrs = Record<string, unknown> & { class?: string; style?: string };

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs?: Attrs | null,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs ?? {})) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on") && typeof value === "function") {
      el.addEventListener(key.slice(2).toLowerCase(), value as EventListener);
    } else if (key === "class") {
      el.className = String(value);
    } else if (key in el && key !== "style" && key !== "list") {
      (el as unknown as Record<string, unknown>)[key] = value;
    } else {
      el.setAttribute(key, value === true ? "" : String(value));
    }
  }
  append(el, children);
  return el;
}

function append(el: Node, children: Child[]) {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    if (Array.isArray(c)) append(el, c);
    else el.appendChild(c instanceof Node ? c : document.createTextNode(String(c)));
  }
}

export function icon(node: IconNode, size = 18, cls = ""): SVGElement {
  const svg = createElement(node);
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("stroke-width", "2");
  if (cls) svg.classList.add(...cls.split(" "));
  return svg;
}

export function clear(el: Element, ...children: Child[]) {
  el.replaceChildren();
  append(el, children);
}

// ---------- app icon tiles ----------

/** Solid tile colours (--app-1 to --app-8 in style.css): the apps carry the colour, the launcher's chrome stays neutral. */
const PALETTE = Array.from({ length: 8 }, (_, i) => `var(--app-${i + 1})`);

function hash(s: string) {
  let x = 0;
  for (const c of s) x = (x * 31 + c.charCodeAt(0)) >>> 0;
  return x;
}

export function appColor(id: string): string {
  return PALETTE[hash(id) % PALETTE.length];
}

const PYTHON_SVG = `<svg viewBox="0 0 24 24" fill="#fff"><path d="M11.9 2C7.3 2 7.6 4 7.6 4v2.1H12v.6H5.8S2.8 6.4 2.8 11.1s2.6 4.5 2.6 4.5H7v-2.2s-.1-2.6 2.6-2.6h4.3s2.5 0 2.5-2.4V4.4S16.8 2 11.9 2zM9.5 3.4a.8.8 0 1 1 0 1.6.8.8 0 0 1 0-1.6z"/><path opacity=".78" d="M12.1 22c4.6 0 4.3-2 4.3-2v-2.1H12v-.6h6.2s3 .3 3-4.4-2.6-4.5-2.6-4.5H17v2.2s.1 2.6-2.6 2.6h-4.3s-2.5 0-2.5 2.4v4S7.2 22 12.1 22zm2.4-1.4a.8.8 0 1 1 0-1.6.8.8 0 0 1 0 1.6z"/></svg>`;

/** The app's own icon, or the Python logo on a colour picked from its id. */
export function appIcon(app: { id: string; icon?: string | null }, size: "" | "xs" | "sm" | "lg" | "xl" = ""): HTMLElement {
  const tile = h("div", { class: `app-icon ${size}` });
  if (app.icon) {
    tile.appendChild(h("img", { src: app.icon, alt: "" }));
  } else {
    tile.style.background = appColor(app.id);
    tile.innerHTML = PYTHON_SVG;
  }
  return tile;
}

// ---------- formatting ----------

export function relTime(secs: number): string {
  const d = Math.floor(Date.now() / 1000) - secs;
  if (d < 45) return "just now";
  if (d < 3600) return `${Math.round(d / 60)} min ago`;
  if (d < 86_400) return `${Math.round(d / 3600)} h ago`;
  if (d < 7 * 86_400) return `${Math.round(d / 86_400)} d ago`;
  return new Date(secs * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

export function duration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(1)} GB`;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

export const KIND_LABEL: Record<string, string> = {
  form: "Form app",
  gui: "Desktop app",
  web: "Web app",
  cli: "Command line",
  background: "Automation",
};

// ---------- recurring bits ----------

export const STATUS_LABEL: Record<RunStatus, string> = { ok: "Done", error: "Didn’t work", cancelled: "Cancelled", running: "Running" };

export function statusText(status: RunStatus): HTMLElement {
  return h("span", { class: `status-text ${status}` }, STATUS_LABEL[status]);
}

export function trustTag(trust: string): HTMLElement {
  if (trust === "approved") {
    return h("span", { class: "tag green", title: "Your organisation approves this author and its signing key" }, icon(ShieldCheck, 12), "Approved");
  }
  if (trust === "not_approved") {
    return h("span", { class: "tag red", title: "Your organisation's policy does not approve this author and key" }, icon(ShieldX, 12), "Not approved");
  }
  return trust === "first_use" || trust === "rotated"
    ? h("span", { class: "tag green", title: "Signed with the key pinned when you installed this publisher's first app" },
        icon(ShieldCheck, 12), "Signature verified")
    : h("span", { class: "tag amber", title: "First app seen from this publisher" }, icon(ShieldAlert, 12), "New publisher");
}

/** What an app works with, from what its manifest declares: the Apps page filters on these. */
export function worksWith(app: { kind: string; requires: string[]; permissions: { network: string[]; sap: boolean } }): string[] {
  const out: string[] = [];
  if (app.requires.some((r) => r.toLowerCase().includes("excel"))) out.push("Excel");
  if (app.permissions.sap || app.requires.some((r) => r.toLowerCase().startsWith("sap"))) out.push("SAP");
  if (app.permissions.network.length) out.push("Internet");
  if (app.kind !== "form") out.push("Own window");
  return out;
}

export function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

// ---------- keyboard and screen readers ----------

const FOCUSABLE = "button, [href], input, select, textarea, summary, [tabindex]:not([tabindex='-1'])";

function focusables(box: HTMLElement): HTMLElement[] {
  return [...box.querySelectorAll<HTMLElement>(FOCUSABLE)]
    .filter((el) => !(el as HTMLButtonElement).disabled && el.getClientRects().length > 0);
}

// The open dialogs, the topmost last: only it keeps Tab inside.
const traps: HTMLElement[] = [];

document.addEventListener("keydown", (e) => {
  const box = traps[traps.length - 1];
  if (e.key !== "Tab" || !box) return;
  const items = focusables(box);
  if (!items.length) { e.preventDefault(); box.focus(); return; }
  const first = items[0], last = items[items.length - 1];
  const at = document.activeElement;
  if (e.shiftKey && (at === first || at === box || !box.contains(at))) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && (at === last || !box.contains(at))) { e.preventDefault(); first.focus(); }
});

/**
 * Makes `box` a modal dialog: marked `aria-modal`, focus moves into it (to `initial`, or the
 * dialog itself so its label is read) and Tab stays inside. When its content is replaced, focus
 * goes back to the dialog. The returned function gives focus back to where it was before.
 */
export function trapFocus(box: HTMLElement, initial?: HTMLElement): () => void {
  const before = document.activeElement as HTMLElement | null;
  box.setAttribute("aria-modal", "true");
  if (!box.hasAttribute("tabindex")) box.tabIndex = -1;
  traps.push(box);
  const watch = new MutationObserver(() => {
    // Only the topmost: a dialog under another one must not take the focus back.
    if (box.isConnected && traps[traps.length - 1] === box && !box.contains(document.activeElement)) box.focus();
  });
  watch.observe(box, { childList: true, subtree: true });
  (initial ?? box).focus();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    watch.disconnect();
    traps.splice(traps.indexOf(box), 1);
    if (before?.isConnected) before.focus();
  };
}

/** Redraws with `draw`, then puts focus back on the control at the same place inside `boxes`,
 * so clicking a segmented control or a chip that redraws its group does not drop the focus. */
export function keepFocus(boxes: HTMLElement[], draw: () => void) {
  const at = document.activeElement;
  const box = boxes.find((b) => b.contains(at));
  const i = box ? focusables(box).indexOf(at as HTMLElement) : -1;
  draw();
  if (box && i >= 0 && !box.contains(document.activeElement)) focusables(box)[i]?.focus();
}

// Arrow keys move between the options of a radio group and pick, as screen reader users expect.
document.addEventListener("keydown", (e) => {
  const at = e.target as HTMLElement;
  const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
  const group = at.closest?.("[role=radiogroup]");
  if (!step || !group || at.getAttribute("role") !== "radio") return;
  const radios = [...group.querySelectorAll<HTMLElement>("[role=radio]")];
  const next = radios[(radios.indexOf(at) + step + radios.length) % radios.length];
  e.preventDefault();
  next.focus();
  next.click();
});

let liveBox: HTMLElement | null = null;

/** Says `message` to screen readers without moving focus. */
export function announce(message: string) {
  if (!liveBox) {
    liveBox = h("div", { class: "sr-only", "aria-live": "polite" });
    document.body.appendChild(liveBox);
  }
  // Replaced, not appended: a reader announces the new text.
  liveBox.textContent = "";
  const box = liveBox;
  window.setTimeout(() => { box.textContent = message; }, 50);
}

/** A `⋯` menu: the button opens it on the first item, arrow keys move, Escape and Tab close it
 * and Escape gives focus back to the button. */
export function menuButton(button: HTMLElement, menu: HTMLElement): { close(): void } {
  const items = () => [...menu.querySelectorAll<HTMLElement>("[role=menuitem]")];
  const open = (on: boolean, focusButton = false) => {
    menu.hidden = !on;
    button.setAttribute("aria-expanded", String(on));
    if (on) items()[0]?.focus();
    else if (focusButton) button.focus();
  };
  button.setAttribute("aria-haspopup", "menu");
  button.setAttribute("aria-expanded", "false");
  button.addEventListener("click", () => open(!!menu.hidden));
  button.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); open(true); }
  });
  menu.addEventListener("keydown", (e) => {
    const list = items();
    const i = list.indexOf(document.activeElement as HTMLElement);
    const go = (j: number) => { e.preventDefault(); list[(j + list.length) % list.length]?.focus(); };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(list.length - 1);
    else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); open(false, true); }
    else if (e.key === "Tab") open(false);
  });
  return { close: () => open(false) };
}

let toastBox: HTMLElement | null = null;

function toastsBox(): HTMLElement {
  if (!toastBox) {
    toastBox = h("div", { class: "toasts", role: "status" });
    document.body.appendChild(toastBox);
  }
  return toastBox;
}

/** A toast that stays until `end()`. `stop` adds a button that runs once, then waits disabled
 * ("Stopping…") until the work ends. */
export function progressToast(message: string, stop?: { label: string; busy: string; run: () => void }): { update(message: string): void; end(): void } {
  const text = h("div", null, message);
  const btn = stop ? h("button", { class: "link", onclick: () => { btn!.disabled = true; btn!.textContent = stop.busy; stop.run(); } }, stop.label) as HTMLButtonElement : null;
  const el = h("div", { class: "toast info" }, icon(Loader, 17, "spin"), text, btn);
  toastsBox().appendChild(el);
  return { update: (m) => { text.textContent = m; }, end: () => el.remove() };
}

/** A short message at the bottom. Errors stay until closed, so there is time to read them. */
export function toast(message: string, kind: "info" | "error" | "ok" = "info", action?: { label: string; run: () => void }) {
  const toastBox = toastsBox();
  const ic = kind === "error" ? CircleAlert : kind === "ok" ? CircleCheck : Info;
  const el = h("div", { class: `toast ${kind}`, role: kind === "error" ? "alert" : null }, icon(ic, 17), h("div", null, message),
    action ? h("button", { class: "link", onclick: () => { el.remove(); action.run(); } }, action.label) : null,
    kind === "error" ? h("button", { class: "toast-close", "aria-label": "Close", title: "Close", onclick: () => el.remove() }, icon(X, 15)) : null);
  toastBox.appendChild(el);
  if (kind !== "error") setTimeout(() => el.remove(), action ? 9000 : 4000);
}

/** Copies text; the webview allows it from a click. */
export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const box = h("textarea", { value: text, style: "position:fixed;opacity:0" }) as HTMLTextAreaElement;
    document.body.appendChild(box);
    box.select();
    const ok = document.execCommand("copy");
    box.remove();
    if (!ok) throw new Error("the clipboard is not available");
  }
}

/** A failed run's error: the message, what to try, and for a crash the details for the author.
 * `log` gives the details where the traceback is not kept (a past run: it is in the run's log). */
export function runError(r: { error?: string | null; hint?: string | null; traceback?: string | null }, app: string, log?: () => string): HTMLElement | null {
  if (!r.error) return null;
  const copy = async () => {
    try {
      await copyText(`${app}
${r.error}

${r.traceback ?? log?.() ?? ""}`.trim());
      toast("Copied. Read it before you send it: it may show file names from your PC.", "ok");
    } catch (e) { toast(errorText(e), "error"); }
  };
  return h("div", { class: "run-error" },
    h("div", { class: "error-box" }, r.error),
    r.hint ? h("div", { class: "hint" }, r.hint) : null,
    r.traceback
      ? h("details", { class: "disclosure" }, h("summary", null, "Details for the author"),
          h("div", null, h("div", { class: "log" }, r.traceback),
            h("button", { class: "btn small", onclick: copy }, "Copy details")))
      : log && r.hint ? h("div", null, h("button", { class: "btn small", onclick: copy }, "Copy details")) : null);
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}

export function greeting(): string {
  const hr = new Date().getHours();
  return hr < 12 ? "Good morning" : hr < 18 ? "Good afternoon" : "Good evening";
}

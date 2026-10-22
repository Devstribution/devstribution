// Small DOM helpers: element builder, icons, formatting, toasts and recurring bits of UI.

import { createElement, type IconNode } from "lucide";
import { CircleAlert, CircleCheck, Info, ShieldCheck, ShieldAlert } from "lucide";
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

/** Solid tile colours: the apps carry the colour, the launcher's chrome stays neutral. */
const PALETTE = ["#2563eb", "#7c3aed", "#0e9f6e", "#ea580c", "#db2777", "#0891b2", "#4f46e5", "#ca8a04"];

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
  return `${(n / 1024 ** 2).toFixed(1)} MB`;
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
  return trust === "first_use"
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

let toastBox: HTMLElement | null = null;

export function toast(message: string, kind: "info" | "error" | "ok" = "info", action?: { label: string; run: () => void }) {
  if (!toastBox) {
    toastBox = h("div", { class: "toasts", role: "status" });
    document.body.appendChild(toastBox);
  }
  const ic = kind === "error" ? CircleAlert : kind === "ok" ? CircleCheck : Info;
  const el = h("div", { class: `toast ${kind}` }, icon(ic, 17), h("div", null, message),
    action ? h("button", { class: "link", onclick: () => { el.remove(); action.run(); } }, action.label) : null);
  toastBox.appendChild(el);
  setTimeout(() => el.remove(), kind === "error" || action ? 9000 : 4000);
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

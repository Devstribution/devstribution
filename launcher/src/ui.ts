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

const PALETTE = [
  ["#3f6bff", "#6d8cff"],
  ["#7a4dff", "#a17bff"],
  ["#14b8b0", "#38d6c4"],
  ["#ff8a3d", "#ffb067"],
  ["#f04494", "#ff7ab8"],
  ["#17b26a", "#4fd492"],
  ["#2b8cff", "#5ec1ff"],
];

function hash(s: string) {
  let x = 0;
  for (const c of s) x = (x * 31 + c.charCodeAt(0)) >>> 0;
  return x;
}

const PYTHON_SVG = `<svg viewBox="0 0 24 24" fill="#fff"><path d="M11.9 2C7.3 2 7.6 4 7.6 4v2.1H12v.6H5.8S2.8 6.4 2.8 11.1s2.6 4.5 2.6 4.5H7v-2.2s-.1-2.6 2.6-2.6h4.3s2.5 0 2.5-2.4V4.4S16.8 2 11.9 2zM9.5 3.4a.8.8 0 1 1 0 1.6.8.8 0 0 1 0-1.6z"/><path opacity=".78" d="M12.1 22c4.6 0 4.3-2 4.3-2v-2.1H12v-.6h6.2s3 .3 3-4.4-2.6-4.5-2.6-4.5H17v2.2s.1 2.6-2.6 2.6h-4.3s-2.5 0-2.5 2.4v4S7.2 22 12.1 22zm2.4-1.4a.8.8 0 1 1 0-1.6.8.8 0 0 1 0 1.6z"/></svg>`;

/** The app's own icon, or the Python logo on a colour picked from its id. */
export function appIcon(app: { id: string; icon?: string | null }, size: "" | "sm" | "lg" = ""): HTMLElement {
  const tile = h("div", { class: `app-icon ${size}` });
  if (app.icon) {
    tile.appendChild(h("img", { src: app.icon, alt: "" }));
  } else {
    const [a, b] = PALETTE[hash(app.id) % PALETTE.length];
    tile.style.background = `linear-gradient(135deg, ${a}, ${b})`;
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

export function statusPill(status: RunStatus): HTMLElement {
  const label = { ok: "Succeeded", error: "Failed", cancelled: "Cancelled", running: "Running" }[status];
  return h("span", { class: `status ${status}` }, label);
}

export function trustChip(trust: string): HTMLElement {
  return trust === "first_use"
    ? h("span", { class: "chip green", title: "Signed with the same key as the first app you installed from this publisher" },
        icon(ShieldCheck, 13), "Same key as first install")
    : h("span", { class: "chip amber", title: "First app seen from this publisher" }, icon(ShieldAlert, 13), "New publisher");
}

let toastBox: HTMLElement | null = null;

export function toast(message: string, kind: "info" | "error" | "ok" = "info") {
  if (!toastBox) {
    toastBox = h("div", { class: "toasts" });
    document.body.appendChild(toastBox);
  }
  const ic = kind === "error" ? CircleAlert : kind === "ok" ? CircleCheck : Info;
  const el = h("div", { class: `toast ${kind}` }, icon(ic, 18), h("div", null, message));
  toastBox.appendChild(el);
  setTimeout(() => el.remove(), kind === "error" ? 9000 : 4000);
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

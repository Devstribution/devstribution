// Hand-drawn SVG charts (no chart library): stacked daily bars and a progress ring.

import type { Stats } from "./api";

const NS = "http://www.w3.org/2000/svg";

function s<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number> = {}, text?: string) {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  if (text !== undefined) el.textContent = text;
  return el;
}

/** A maximum with 4 whole-number gridline steps: 4, 8, 12, 20, 40, 100... */
function niceMax(v: number): number {
  const raw = Math.max(1, Math.ceil(v / 4));
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 3, 5, 10].map((m) => m * mag).find((x) => x >= raw) ?? raw;
  return step * 4;
}

/** Runs per day, stacked: succeeded (gradient), failed (red), cancelled (amber). */
export function runsChart(days: Stats["per_day"]): SVGSVGElement {
  const W = 720, H = 240, L = 34, R = 8, T = 12, B = 30;
  const svg = s("svg", { viewBox: `0 0 ${W} ${H}`, class: "chart", preserveAspectRatio: "none" });
  const defs = s("defs");
  const grad = s("linearGradient", { id: "barGradient", x1: 0, y1: 0, x2: 0, y2: 1 });
  grad.append(s("stop", { offset: 0, "stop-color": "#7a4dff" }), s("stop", { offset: 1, "stop-color": "#3f6bff" }));
  defs.append(grad);
  svg.append(defs);

  const max = niceMax(Math.max(1, ...days.map((d) => d.ok + d.error + d.cancelled)));
  const plotH = H - T - B;
  for (let i = 0; i <= 4; i++) {
    const y = T + plotH - (plotH * i) / 4;
    svg.append(s("line", { x1: L, x2: W - R, y1: y, y2: y, class: "grid-line" }));
    svg.append(s("text", { x: L - 8, y: y + 4, "text-anchor": "end", class: "axis" }, String(Math.round((max * i) / 4))));
  }
  const slot = (W - L - R) / days.length;
  const bw = Math.min(26, slot * 0.56);
  days.forEach((d, i) => {
    const x = L + slot * i + (slot - bw) / 2;
    let y = T + plotH;
    const parts: [number, string][] = [[d.ok, "bar-ok"], [d.error, "bar-err"], [d.cancelled, "bar-cancel"]];
    const total = d.ok + d.error + d.cancelled;
    parts.forEach(([n, cls], idx) => {
      if (!n) return;
      const bh = (plotH * n) / max;
      y -= bh;
      const isTop = parts.slice(idx + 1).every(([m]) => !m);
      const r = isTop ? Math.min(6, bw / 2) : 0;
      // Rounded top only on the topmost segment.
      const path = r
        ? `M${x},${y + bh} V${y + r} Q${x},${y} ${x + r},${y} H${x + bw - r} Q${x + bw},${y} ${x + bw},${y + r} V${y + bh} Z`
        : `M${x},${y + bh} V${y} H${x + bw} V${y + bh} Z`;
      svg.append(s("path", { d: path, class: cls }));
    });
    const date = new Date(d.date + "T12:00:00");
    const hit = s("rect", { x: L + slot * i, y: T, width: slot, height: plotH, class: "bar-hit" });
    hit.append(s("title", {}, `${date.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })}: ${total} runs (${d.ok} succeeded, ${d.error} failed, ${d.cancelled} cancelled)`));
    svg.append(hit);
    if (i % 2 === days.length % 2 || days.length <= 7) {
      const label = date.toLocaleDateString(undefined, { day: "numeric", month: "short" });
      svg.append(s("text", { x: L + slot * i + slot / 2, y: H - 8, "text-anchor": "middle", class: "axis" }, label));
    }
  });
  return svg;
}

/** A ring filled to `value` (0..1). */
export function ring(value: number, size = 56): HTMLElement {
  const r = size / 2 - 5;
  const c = 2 * Math.PI * r;
  const wrap = document.createElement("div");
  wrap.className = "ring";
  wrap.style.width = wrap.style.height = `${size}px`;
  const svg = s("svg", { width: size, height: size, viewBox: `0 0 ${size} ${size}` });
  svg.append(
    s("circle", { cx: size / 2, cy: size / 2, r, fill: "none", "stroke-width": 7, class: "track" }),
    s("circle", {
      cx: size / 2, cy: size / 2, r, fill: "none", "stroke-width": 7, class: "value",
      "stroke-dasharray": c, "stroke-dashoffset": c * (1 - Math.max(0, Math.min(1, value))),
    }),
  );
  wrap.append(svg);
  return wrap;
}

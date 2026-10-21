import "@fontsource-variable/inter";
import "./styles.css";
import logo from "./logo.svg";

import { House, LayoutGrid, History, Settings as SettingsIcon, Plus, Search, Moon, Sun, Upload } from "lucide";
import { createApi, type Api, type RunEvent, type RunResult } from "./api";
import { h, icon, clear, toast, errorText } from "./ui";
import { homeView } from "./views/home";
import { appsView } from "./views/apps";
import { appView } from "./views/app";
import { runsView } from "./views/runs";
import { settingsView } from "./views/settings";
import { installFlow } from "./views/install";

/** Live state of runs started in this session, kept across page changes. */
export interface LiveRun {
  runId: string;
  appId: string;
  startedAt: number;
  progress: number | null;
  message: string;
  lines: { level: string; text: string }[];
  cancelling: boolean;
  result?: RunResult;
}

export interface Ctx {
  api: Api;
  navigate(hash: string): void;
  install(path?: string): Promise<void>;
  search: string;
  live: Map<string, LiveRun>;
  /** Latest live run per app id. */
  liveFor(appId: string): LiveRun | undefined;
  onLive(cb: (run: LiveRun) => void): () => void;
  refreshNav(): Promise<void>;
}

export interface Rendered {
  title: string;
  subtitle?: string;
  node: HTMLElement;
  cleanup?: () => void;
}

async function boot() {
  const api = await createApi();
  const root = document.getElementById("app")!;

  // ---------- theme ----------
  const stored = (() => { try { return localStorage.getItem("theme"); } catch { return null; } })();
  const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
  let theme = stored ?? (prefersDark ? "dark" : "light");
  const applyTheme = () => document.documentElement.setAttribute("data-theme", theme);
  applyTheme();

  // ---------- live runs ----------
  const live = new Map<string, LiveRun>();
  const liveListeners = new Set<(r: LiveRun) => void>();
  const pushLive = (r: LiveRun) => liveListeners.forEach((l) => l(r));
  await api.onRunEvent((runId, e: RunEvent) => {
    const r = live.get(runId);
    if (!r) return;
    if (e.type === "progress") { r.progress = e.value; r.message = e.message; }
    else if (e.type === "log") r.lines.push({ level: e.level, text: e.message });
    else if (e.type === "output") r.lines.push({ level: "output", text: e.line });
    else if (e.type === "cancelling") r.cancelling = true;
    else if (e.type === "finished") r.result = e.result;
    if (r.lines.length > 2000) r.lines.splice(0, r.lines.length - 2000);
    pushLive(r);
  });

  // ---------- shell ----------
  const navItems: [string, string, typeof House][] = [
    ["#/home", "Home", House],
    ["#/apps", "Apps", LayoutGrid],
    ["#/runs", "Runs", History],
    ["#/settings", "Settings", SettingsIcon],
  ];
  const counts = { apps: h("span", { class: "count" }), runs: h("span", { class: "count" }) };
  const nav = navItems.map(([hash, label, ic]) =>
    h("button", { class: "nav-item", "data-hash": hash, onclick: () => ctx.navigate(hash) },
      icon(ic, 19), label,
      label === "Apps" ? counts.apps : label === "Runs" ? counts.runs : null),
  );
  const sidebar = h("aside", { class: "sidebar" },
    h("div", { class: "brand" }, h("img", { src: logo, alt: "" }), h("div", { class: "brand-name" }, "dev", h("span", null, "stribution"))),
    h("div", { class: "nav-label" }, "Workspace"),
    nav,
    h("div", { class: "sidebar-spacer" }),
    h("div", { class: "sidebar-card" },
      h("strong", null, "Got an app?"),
      h("p", null, "Open a .dsapp file or drop it anywhere on this window."),
      h("button", { class: "btn small", onclick: () => ctx.install() }, icon(Plus, 16), "Install app")),
    h("div", { class: "mode-pill" }, h("span", { class: "dot" }), "Free mode",
      api.preview ? h("span", { class: "preview-badge", style: "margin-left:auto" }, "Preview data") : null),
  );

  const title = h("h1");
  const subtitle = h("div", { class: "sub" });
  const searchInput = h("input", { placeholder: "Search apps", "aria-label": "Search apps" });
  const themeBtn = h("button", { class: "icon-btn", title: "Light or dark", "aria-label": "Toggle dark mode" });
  const renderThemeIcon = () => clear(themeBtn, icon(theme === "dark" ? Sun : Moon, 18));
  renderThemeIcon();
  themeBtn.addEventListener("click", () => {
    theme = theme === "dark" ? "light" : "dark";
    try { localStorage.setItem("theme", theme); } catch { /* private mode */ }
    applyTheme();
    renderThemeIcon();
  });
  const topbar = h("header", { class: "topbar" },
    h("div", null, title, subtitle),
    h("div", { class: "topbar-actions" },
      h("label", { class: "search" }, icon(Search, 17), searchInput),
      themeBtn,
      h("button", { class: "btn primary", onclick: () => ctx.install() }, icon(Plus, 18), "Install app")),
  );
  const content = h("div", { class: "content" });
  const main = h("main", { class: "main" }, topbar, content);
  root.append(sidebar, main);

  // ---------- context and router ----------
  let current: Rendered | null = null;
  let renderSeq = 0;
  const ctx: Ctx = {
    api,
    search: "",
    live,
    navigate(hash) {
      if (location.hash === hash) render();
      else location.hash = hash;
    },
    async install(path) { await installFlow(ctx, path); },
    liveFor(appId) {
      let best: LiveRun | undefined;
      for (const r of live.values()) if (r.appId === appId && (!best || r.startedAt > best.startedAt)) best = r;
      return best;
    },
    onLive(cb) { liveListeners.add(cb); return () => liveListeners.delete(cb); },
    async refreshNav() {
      try {
        const o = await api.overview();
        counts.apps.textContent = String(o.apps.length);
        counts.runs.textContent = String(o.stats.runs);
      } catch { /* shown by the page */ }
    },
  };

  searchInput.addEventListener("input", () => {
    ctx.search = searchInput.value.trim().toLowerCase();
    if (!location.hash.startsWith("#/apps")) ctx.navigate("#/apps");
    else render();
  });

  async function render() {
    const seq = ++renderSeq;
    const [, page, ...rest] = (location.hash || "#/home").split("/");
    nav.forEach((b) => b.classList.toggle("active", b.dataset.hash === `#/${page === "app" ? "apps" : page}`));
    try {
      let view: Rendered;
      if (page === "apps") view = await appsView(ctx);
      else if (page === "app") view = await appView(ctx, decodeURIComponent(rest.join("/")));
      else if (page === "runs") view = await runsView(ctx);
      else if (page === "settings") view = await settingsView(ctx);
      else view = await homeView(ctx);
      if (seq !== renderSeq) { view.cleanup?.(); return; }
      current?.cleanup?.();
      current = view;
      title.textContent = view.title;
      subtitle.textContent = view.subtitle ?? "";
      clear(content, view.node);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }
  window.addEventListener("hashchange", render);
  await api.onRunsChanged(() => { ctx.refreshNav(); });

  // ---------- opening .dsapp files: association, second launch, drag and drop ----------
  const dropOverlay = h("div", { class: "drop-overlay", hidden: true },
    h("div", null, icon(Upload, 40), "Drop to install"));
  document.body.appendChild(dropOverlay);
  await api.onDragDrop((e) => {
    const apps = e.paths.filter((p) => p.toLowerCase().endsWith(".dsapp"));
    dropOverlay.hidden = !(e.type === "over" && (apps.length > 0 || e.paths.length === 0));
    if (e.type === "drop") {
      if (apps.length) ctx.install(apps[0]);
      else if (e.paths.length) toast("Only .dsapp files can be installed", "error");
    }
  });
  await api.onOpenFiles((paths) => { if (paths[0]) ctx.install(paths[0]); });

  await ctx.refreshNav();
  await render();
  const pending = await api.takePendingFiles();
  if (pending[0]) ctx.install(pending[0]);
}

boot();

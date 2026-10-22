import "@fontsource-variable/manrope";
import "./style.css";

import { House, LayoutGrid, History, Settings as SettingsIcon, Upload, Play } from "lucide";
import { createApi, type Api, type AppView, type RunEvent, type RunResult } from "./api";
import { h, icon, appIcon, clear, toast, errorText } from "./ui";
import { homeView } from "./views/home";
import { appsView } from "./views/apps";
import { activityView } from "./views/activity";
import { openAppPanel } from "./views/panel";
import { toggleSettings } from "./views/settings";
import { applyLook, cleanLook, onSystemThemeChange, type Look } from "./look";
import { installFlow } from "./views/install";

/** Live state of runs started in this session, kept across page changes. */
export interface LiveRun {
  runId: string;
  appId: string;
  appTitle: string;
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
  /** Opens an app's side panel over the current page. */
  openApp(id: string): void;
  live: Map<string, LiveRun>;
  /** Latest live run per app id. */
  liveFor(appId: string): LiveRun | undefined;
  onLive(cb: (run: LiveRun) => void): () => void;
  /** Reloads the sidebar and the current page (after installs, uninstalls, finished runs). */
  refresh(): Promise<void>;
  pins: { has(id: string): boolean; toggle(id: string): void };
  /** Theme, accent and status colours, from Settings. */
  look: { get(): Look; set(change: Partial<Look>): void; reset(): void };
}

export interface Rendered {
  node: HTMLElement;
  cleanup?: () => void;
}

function stored<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function store(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* private mode: not remembered */ }
}

async function boot() {
  const api = await createApi();
  const root = document.getElementById("app")!;

  // ---------- look: theme, accent and status colours (a per-user preference) ----------
  let look = cleanLook(stored<Partial<Look>>("look", {}));
  applyLook(look);
  onSystemThemeChange(() => applyLook(look));

  // ---------- pinned apps (a per-user preference, kept in the webview's storage) ----------
  let pinned = stored<string[]>("pinned", []);

  // ---------- live runs ----------
  const live = new Map<string, LiveRun>();
  const liveListeners = new Set<(r: LiveRun) => void>();
  let openPanel: { appId: string; close: () => void } | null = null;
  await api.onRunEvents((runId, events: RunEvent[]) => {
    const r = live.get(runId);
    if (!r) return;
    for (const e of events) {
      if (e.type === "progress") { r.progress = e.value; r.message = e.message; }
      else if (e.type === "log") r.lines.push({ level: e.level, text: e.message });
      else if (e.type === "output") r.lines.push({ level: "output", text: e.line });
      else if (e.type === "cancelling") r.cancelling = true;
      else if (e.type === "finished") {
        r.result = e.result;
        // Its panel shows the result; otherwise say so, wherever the user is.
        if (openPanel?.appId !== r.appId) {
          const msg = { ok: `${r.appTitle} finished`, error: `${r.appTitle} didn’t work`, cancelled: `${r.appTitle} was cancelled`, running: "" }[e.result.status];
          toast(msg, e.result.status === "ok" ? "ok" : e.result.status === "error" ? "error" : "info", { label: "Open", run: () => ctx.openApp(r.appId) });
        }
      }
    }
    if (r.lines.length > 2000) r.lines.splice(0, r.lines.length - 2000);
    liveListeners.forEach((l) => l(r));
    drawLiveDot();
  });

  // ---------- shell ----------
  const navItems: [string, string, typeof House][] = [["#/home", "Home", House], ["#/apps", "Apps", LayoutGrid], ["#/activity", "Activity", History]];
  const appCount = h("span", { class: "count" });
  const liveDot = h("span", { class: "live-dot", hidden: true, title: "An app is running" });
  const nav = navItems.map(([hash, label, ic]) =>
    h("button", { class: "nav-item", "data-hash": hash, onclick: () => ctx.navigate(hash) },
      icon(ic, 18), label, label === "Apps" ? appCount : label === "Activity" ? liveDot : null));
  const pinLabel = h("div", { class: "nav-label" }, "Pinned");
  const pinList = h("div");
  const sidebar = h("nav", { class: "sidebar", "aria-label": "Main" },
    h("div", { class: "brand" }, h("div", { class: "brand-mark" }, icon(Play, 12)), "devstribution",
      api.preview ? h("span", { class: "preview-badge", title: "Demo data: this is the browser preview, not the launcher" }, "Preview") : null),
    nav,
    pinLabel, pinList,
    h("div", { class: "sidebar-spacer" }),
    h("div", { class: "sidebar-card" },
      h("strong", null, "Got an app?"),
      h("p", null, "Open a .dsapp file, or drop it anywhere on this window."),
      h("button", { class: "btn primary small", onclick: () => ctx.install() }, "Install app")),
    h("div", { class: "mode-line" }, h("span", { class: "dot" }), "Free mode · signatures checked"),
  );
  const main = h("main", { class: "main" });
  const fab = h("button", { class: "fab", "aria-label": "Settings", title: "Settings", onclick: () => toggleSettings(ctx) }, icon(SettingsIcon, 22));
  root.append(sidebar, main, fab);

  let apps: AppView[] = [];
  function drawLiveDot() {
    liveDot.hidden = ![...live.values()].some((r) => !r.result);
  }
  function drawPins() {
    const shown = pinned.map((id) => apps.find((a) => a.id === id)).filter((a): a is AppView => !!a);
    pinLabel.hidden = shown.length === 0;
    clear(pinList, shown.map((a) =>
      h("button", { class: "pin-item", onclick: () => ctx.openApp(a.id) }, appIcon(a, "xs"), h("span", null, a.title))));
  }

  // ---------- context and router ----------
  let current: Rendered | null = null;
  let renderSeq = 0;
  const ctx: Ctx = {
    api,
    live,
    navigate(hash) {
      if (location.hash === hash) render();
      else location.hash = hash;
    },
    async install(path) { await installFlow(ctx, path); },
    openApp(id) {
      openPanel?.close();
      const close = openAppPanel(ctx, id, () => { if (openPanel?.close === close) openPanel = null; });
      openPanel = { appId: id, close };
    },
    liveFor(appId) {
      let best: LiveRun | undefined;
      for (const r of live.values()) if (r.appId === appId && (!best || r.startedAt > best.startedAt)) best = r;
      return best;
    },
    onLive(cb) { liveListeners.add(cb); return () => liveListeners.delete(cb); },
    async refresh() {
      try {
        apps = (await api.overview()).apps;
        appCount.textContent = String(apps.length);
        pinned = pinned.filter((id) => apps.some((a) => a.id === id));
        drawPins();
      } catch { /* the page shows the error */ }
      await render();
    },
    pins: {
      has: (id) => pinned.includes(id),
      toggle(id) {
        pinned = pinned.includes(id) ? pinned.filter((p) => p !== id) : [...pinned, id];
        store("pinned", pinned);
        drawPins();
      },
    },
    look: {
      get: () => look,
      set(change) { look = cleanLook({ ...look, ...change }); store("look", look); applyLook(look); },
      reset() { look = cleanLook(null); store("look", look); applyLook(look); },
    },
  };

  async function render() {
    const seq = ++renderSeq;
    let page = (location.hash || "#/home").split("/")[1] || "home";
    if (page === "runs") page = "activity";
    if (page === "app") {
      // Old links to an app page: open its panel over Home.
      const id = decodeURIComponent(location.hash.split("/").slice(2).join("/"));
      history.replaceState(null, "", "#/home");
      page = "home";
      if (id) ctx.openApp(id);
    }
    nav.forEach((b) => b.classList.toggle("active", b.dataset.hash === `#/${page}`));
    try {
      const view = page === "apps" ? await appsView(ctx) : page === "activity" ? await activityView(ctx) : await homeView(ctx);
      if (seq !== renderSeq) { view.cleanup?.(); return; }
      current?.cleanup?.();
      current = view;
      clear(main, view.node);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }
  window.addEventListener("hashchange", render);
  await api.onRunsChanged(() => { ctx.refresh(); });

  // ---------- opening .dsapp files: association, second launch, drag and drop ----------
  const dropOverlay = h("div", { class: "drop-overlay", hidden: true },
    h("div", null, icon(Upload, 40), "Drop to install"));
  document.body.appendChild(dropOverlay);
  await api.onDragDrop((e) => {
    const dsapps = e.paths.filter((p) => p.toLowerCase().endsWith(".dsapp"));
    dropOverlay.hidden = !(e.type === "over" && (dsapps.length > 0 || e.paths.length === 0));
    if (e.type === "drop") {
      if (dsapps.length) ctx.install(dsapps[0]);
      else if (e.paths.length) toast("Only .dsapp files can be installed", "error");
    }
  });
  await api.onOpenFiles((paths) => { if (paths[0]) ctx.install(paths[0]); });

  // Ctrl+K: the search on Home.
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      if (location.hash && location.hash !== "#/home") ctx.navigate("#/home");
      setTimeout(() => (document.querySelector(".search-pill input") as HTMLInputElement | null)?.focus(), 50);
    }
  });

  await ctx.refresh();
  const pending = await api.takePendingFiles();
  if (pending[0]) ctx.install(pending[0]);
}

boot();

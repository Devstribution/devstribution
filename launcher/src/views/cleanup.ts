import { X, Trash2, ScrollText } from "lucide";
import type { Ctx } from "../main";
import type { AppView, CleanupPlan, JournalReport } from "../api";
import { h, icon, clear, bytes, plural, toast, errorText } from "../ui";

const AGES: [string, number | null][] = [["Older than 7 days", 7], ["Older than 30 days", 30], ["Older than 90 days", 90], ["Older than a year", 365], ["All finished runs", null]];

/**
 * Settings → Storage → Clean up. Shows what would go, and how much room it frees, before
 * anything is deleted. The results and logs go; the runs stay in Activity unless asked.
 */
export function openCleanup(ctx: Ctx, apps: AppView[]) {
  let age: number | null = 30;
  let appId: string | null = null;
  let records = false;
  let plan: CleanupPlan | null = null;
  let busy = false;
  let showLog = false;

  const close = () => { overlay.remove(); document.removeEventListener("keydown", onKey); };
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
  document.addEventListener("keydown", onKey);

  const body = h("div", { class: "modal-body" });
  const foot = h("div", { class: "modal-foot" });
  const overlay = h("div", { class: "overlay", onclick: (e: Event) => { if (e.target === overlay) close(); } },
    h("div", { class: "modal", role: "dialog", "aria-label": "Clean up results" },
      h("div", { class: "modal-head" },
        h("div", { class: "grow" }, h("h2", null, "Clean up results"),
          h("div", { class: "muted small" }, "Remove the files apps made and their logs to free room.")),
        h("button", { class: "icon-btn", "aria-label": "Close", onclick: close }, icon(X, 16))),
      body, foot));
  document.body.appendChild(overlay);

  async function load() {
    plan = null;
    draw();
    try {
      plan = await ctx.api.cleanupPreview(age, appId, records);
    } catch (e) {
      toast(errorText(e), "error");
      plan = { runs: [], bytes: 0 };
    }
    draw();
  }

  async function remove() {
    if (!plan || !plan.runs.length || busy) return;
    const n = plan.runs.length;
    const ok = await ctx.api.ask(
      records
        ? `Delete the results, logs and Activity entries of ${plural(n, "run")} (${bytes(plan.bytes)})? This cannot be undone.`
        : `Delete the results and logs of ${plural(n, "run")} (${bytes(plan.bytes)})? The runs stay in Activity. This cannot be undone.`,
      "Clean up results", "Delete");
    if (!ok) return;
    busy = true;
    draw();
    try {
      const r = await ctx.api.cleanupRuns(plan.runs.map((x) => x.run_id), records);
      toast(`Removed ${bytes(r.bytes)} from ${plural(r.runs, "run")}.`, "ok");
      if (r.failed.length) toast(`${plural(r.failed.length, "run")} could not be cleaned completely: something may still have its files open.`, "error");
      if (r.journal_error) toast(`The cleanup log could not be written: ${r.journal_error}`, "error");
      await ctx.refresh();
    } catch (e) {
      toast(errorText(e), "error");
    }
    busy = false;
    await load();
  }

  function draw() {
    if (showLog) return drawLog();
    const select = (options: [string, string][], value: string, label: string, on: (v: string) => void) => {
      const el = h("select", { class: "input", "aria-label": label, onchange: () => on(el.value) },
        options.map(([text, v]) => h("option", { value: v, selected: v === value }, text)));
      return el;
    };
    const summary = plan === null
      ? "Counting…"
      : plan.runs.length
        ? `${plural(plan.runs.length, "run")} · ${bytes(plan.bytes)} to free`
        : "Nothing to clean up with these choices.";
    clear(body,
      h("div", { class: "inline" },
        select(AGES.map(([t, d]) => [t, String(d)]), String(age), "Age", (v) => { age = v === "null" ? null : Number(v); load(); }),
        select([["All apps", ""], ...apps.map((a): [string, string] => [a.title, a.id])], appId ?? "", "App", (v) => { appId = v || null; load(); })),
      h("label", { class: "inline small" },
        h("input", { type: "checkbox", checked: records, onchange: (e: Event) => { records = (e.target as HTMLInputElement).checked; load(); } }),
        h("span", null, "Also remove these runs from Activity")),
      h("div", { class: "strong" }, summary),
      plan && plan.runs.length ? h("div", { class: "muted tiny" },
        `${plural(new Set(plan.runs.map((r) => r.app_id)).size, "app")}. Oldest run: ${new Date(Math.min(...plan.runs.map((r) => r.started_at)) * 1000).toLocaleDateString()}.`) : null,
      h("div", { class: "muted tiny" }, "Runs in progress are skipped. Files in folders you chose for an app are never touched: only what devstribution kept for the run."));
    clear(foot,
      h("button", { class: "btn", onclick: () => { showLog = true; draw(); } }, icon(ScrollText, 16), "Cleanup log"),
      h("button", { class: "btn danger", disabled: busy || !plan || !plan.runs.length, onclick: remove },
        icon(Trash2, 16), plan?.runs.length ? `Delete ${bytes(plan.bytes)}` : "Delete"));
  }

  async function drawLog() {
    clear(body, h("div", { class: "muted" }, "Reading the log…"));
    clear(foot, h("button", { class: "btn", onclick: () => { showLog = false; draw(); } }, "Back"));
    let report: JournalReport;
    try {
      report = await ctx.api.cleanupLog();
    } catch (e) {
      clear(body, h("div", { class: "error-box" }, errorText(e)));
      return;
    }
    if (!showLog) return;
    const problems = [
      report.broken_at ? `The log was changed outside devstribution: it stops adding up at line ${report.broken_at}.` : null,
      report.changed.length ? `${plural(report.changed.length, "run record")} changed since the run finished.` : null,
      report.missing.length ? `${plural(report.missing.length, "run folder")} gone without a cleanup removing ${report.missing.length === 1 ? "it" : "them"}.` : null,
    ].filter((p): p is string => !!p);
    clear(body,
      problems.length
        ? h("div", { class: "banner amber small" }, h("div", null, problems.map((p) => h("div", null, p))))
        : h("div", { class: "small soft-text" }, `${plural(report.lines, "line")}, all adding up. No run was changed or removed except by a cleanup below.`),
      report.cleanups.length
        ? h("div", { class: "stack" }, report.cleanups.map((c) => {
            const runs = c.data.runs ?? [];
            return h("div", { class: "publisher" },
              h("div", { class: "grow" },
                h("div", { class: "name" }, `${bytes(c.data.bytes ?? 0)} from ${plural(runs.length, "run")}${c.data.remove_records ? ", records removed" : ""}`),
                h("div", { class: "muted tiny" }, new Date(c.at * 1000).toLocaleString())));
          }))
        : h("div", { class: "muted small" }, "No cleanup yet."),
      h("div", { class: "muted tiny wrap-anywhere" }, `File: ${report.file}`));
  }

  load();
}

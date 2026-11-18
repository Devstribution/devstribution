import { ShieldCheck, ShieldAlert, FileDown, FileText, ClipboardCopy } from "lucide";
import type { AppView, GuardEvent, SecurityView } from "../api";
import type { Ctx, Rendered } from "../main";
import { h, icon, appIcon, clear, relTime, toast, errorText, copyText, plural, trustTag } from "../ui";
import { managedGroups, revocationsGroup } from "./settings";

type Filter = "all" | GuardEvent["action"];
const FILTERS: [Filter, string][] = [["all", "All"], ["stop", "Stopped"], ["block", "Blocked"], ["observe", "Reported"]];
let filter: Filter = "all";

const ACTION: Record<GuardEvent["action"], [string, string]> = {
  stop: ["red", "Stopped the run"],
  block: ["amber", "Blocked a call"],
  observe: ["", "Reported"],
};

/** One line of the overview: a check, whether it is fine, and what it found. */
function check(ok: boolean, title: string, text: string) {
  return h("div", { class: `sec-check ${ok ? "" : "warn"}` },
    icon(ok ? ShieldCheck : ShieldAlert, 20, "bi"),
    h("div", { class: "grow" }, h("div", { class: "t" }, title), h("div", { class: "s" }, text)));
}

function overview(s: SecurityView, apps: AppView[]) {
  const stopped = apps.filter((a) => a.quarantine);
  const revoked = apps.filter((a) => a.revoked);
  const stops = s.events.filter((e) => e.action === "stop").length;
  const blocks = s.events.filter((e) => e.action === "block").length;
  const j = s.journal;
  const journalOk = j.broken_at == null && !j.changed.length && !j.missing.length;
  const r = s.revocations;
  const p = s.policy;
  return h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h2", null, "At a glance")),
    h("div", { class: "sec-checks" },
      check(!p?.problems.length,
        p ? (p.dry_run ? "Managed by your organisation, on trial" : "Managed by your organisation") : "You decide what to install",
        p ? `${plural(p.authors.length, "approved author")}. ${p.problems.length ? p.problems[0] : `Policy from ${p.source}.`}`
          : `${plural(s.publishers.length, "publisher")} trusted. Updates must be signed by the same publisher.`),
      check(true, "Signatures and code scan",
        "Every package is checked against its publisher’s signature, and its code is scanned, before it installs."),
      check(!stopped.length,
        "Runtime guard",
        stopped.length
          ? `${stopped.map((a) => a.title).join(", ")} ${stopped.length === 1 ? "is" : "are"} held after a stop, until you decide.`
          : s.events.length
            ? `In the latest ${s.runs_read} runs: ${plural(stops, "stop")}, ${plural(blocks, "blocked call")}, ${plural(s.events.length - stops - blocks, "report")}.`
            : `Nothing to report in the latest ${s.runs_read} runs.`),
      check(!revoked.length && !r.expired,
        "Revoked keys",
        revoked.length
          ? `Revoked: ${revoked.map((a) => a.title).join(", ")}.`
          : r.serial != null
            ? `List ${r.serial}${r.expired ? ", expired" : ""}: ${`${r.entries} ${r.entries === 1 ? "entry" : "entries"}`}, ${plural(r.statements, "publisher statement")}.`
            : r.statements
              ? `${plural(r.statements, "publisher statement")} imported. Revocation lists come with a later version.`
              : "None known. Revocation lists come with a later version."),
      check(journalOk,
        "Run journal",
        j.broken_at != null
          ? `The chain breaks at line ${j.broken_at}: the journal was edited.`
          : j.changed.length || j.missing.length
            ? `${plural(j.changed.length, "run record")} changed, ${plural(j.missing.length, "run folder")} gone without a cleanup.`
            : `${plural(j.lines, "line")}, each linked to the one before: nothing was changed or removed unrecorded.`),
      check(!!s.launcher_bom,
        "devstribution itself",
        s.launcher_bom
          ? `Version ${s.version}, ${plural(s.launcher_bom.components, "component")} listed in its bill of materials.`
          : `Version ${s.version}. This build has no bill of materials (a development build).`)));
}

function appsCard(ctx: Ctx, s: SecurityView, apps: AppView[]) {
  const save = async (a: AppView) => {
    try { if (await ctx.api.saveAppSbom(a.id)) toast(`Saved the bill of materials of ${a.title}`, "ok"); }
    catch (e) { toast(errorText(e), "error"); }
  };
  return h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h2", null, "Installed apps")),
    h("div", { class: "muted tiny sec-note" }, "SBOM saves the app’s bill of materials: every package it installs, with its hash, as CycloneDX for your IT’s tools."),
    apps.length
      ? h("div", { class: "sec-apps" }, apps.map((a) => {
          const n = s.packages[a.id] ?? 0;
          const net = a.permissions.network;
          return h("div", { class: "sec-app" },
            appIcon(a, "sm"),
            h("div", { class: "grow" },
              h("button", { class: "link t", onclick: () => ctx.openApp(a.id) }, a.title),
              h("div", { class: "muted tiny wrap-anywhere" }, `${a.id} ${a.version} · key `, h("span", { class: "mono tiny" }, a.key_id)),
              h("div", { class: "muted tiny" },
                [n ? plural(n, "package") : "No packages", net.length ? `network: ${net.join(", ")}` : "no network declared",
                  a.guard_allow.length ? `allowed after a stop: ${a.guard_allow.join(", ")}` : null].filter(Boolean).join(" · "))),
            h("div", { class: "sec-tags" },
              a.quarantine ? h("span", { class: "tag red" }, "Held") : null,
              a.revoked ? h("span", { class: "tag red" }, "Revoked") : null,
              trustTag(a.trust)),
            h("button", { class: "btn small", onclick: () => save(a), title: `Save the bill of materials of ${a.title}` }, icon(FileDown, 14), "SBOM"));
        }))
      : h("div", { class: "muted small" }, "No apps installed yet."));
}

function eventsCard(ctx: Ctx, s: SecurityView) {
  const chips = h("div", { class: "chips", role: "radiogroup", "aria-label": "Show" });
  const list = h("div", { class: "sec-events" });
  const draw = () => {
    clear(chips, FILTERS.map(([f, label]) => {
      const n = f === "all" ? s.events.length : s.events.filter((e) => e.action === f).length;
      return h("button", { class: `chip ${filter === f ? "on" : ""}`, role: "radio", "aria-checked": String(filter === f), onclick: () => { filter = f; draw(); } }, `${label} ${n}`);
    }));
    const shown = s.events.filter((e) => filter === "all" || e.action === filter);
    clear(list, shown.length
      ? shown.slice(0, 100).map((e) => {
          const [cls, what] = ACTION[e.action];
          return h("div", { class: "sec-event" },
            h("span", { class: `tag ${cls}` }, what),
            h("div", { class: "grow" },
              h("div", { class: "small" }, h("button", { class: "link", onclick: () => ctx.openApp(e.app_id) }, e.app_title), ` ${e.message}`),
              h("div", { class: "muted tiny mono wrap-anywhere" }, [e.rule, e.detail].filter(Boolean).join(" · "))),
            h("span", { class: "muted tiny nowrap", title: new Date(e.at * 1000).toLocaleString() }, relTime(e.at)));
        })
      : h("div", { class: "muted small" }, s.events.length ? "Nothing of this kind." : "The runtime guard has nothing to report."));
  };
  draw();
  return h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h2", null, "Runtime guard"),
      h("span", { class: "hint" }, `What it did in the latest ${s.runs_read} runs, newest first`)),
    h("div", { class: "stack tight" }, chips, list,
      h("div", { class: "muted tiny wrap-anywhere" }, `Everything is also in the log: ${s.log_file}`)));
}

function trustCard(ctx: Ctx, s: SecurityView) {
  const free = h("div", { class: "settings-group" },
    h("span", { class: "section-label" }, "Publishers you trust"),
    s.publishers.length
      ? s.publishers.map((p) => h("div", { class: "publisher" },
          h("div", { class: "grow" }, h("div", { class: "name" }, p.namespace), h("div", { class: "mono muted" }, p.key_id)),
          h("span", { class: "muted tiny", title: p.source === "first-use" ? "Pinned when you installed its first app" : p.source }, relTime(p.pinned_at))))
      : h("div", { class: "muted small" }, "None yet: a publisher shows here when you install their first app."));
  return h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h2", null, "Who is trusted")),
    h("div", { class: "stack" }, ...(s.policy ? managedGroups(s.policy) : [free]), revocationsGroup(ctx, s.revocations)));
}

function launcherCard(ctx: Ctx, s: SecurityView) {
  const verify = "gh attestation verify SHA256SUMS.txt --repo Devstribution/devstribution";
  const b = s.launcher_bom;
  const save = async () => {
    try { if (await ctx.api.saveLauncherSbom()) toast("Saved devstribution’s bill of materials", "ok"); }
    catch (e) { toast(errorText(e), "error"); }
  };
  return h("section", { class: "card" },
    h("div", { class: "card-head" }, h("h2", null, `devstribution ${s.version}`)),
    h("div", { class: "stack tight" },
      h("div", { class: "small soft-text" }, b
        ? `Built from ${`${b.rust} Rust ${b.rust === 1 ? "library" : "libraries"}`}, ${plural(b.npm, "npm package")} and ${plural(b.programs, "program")} (uv), each with its licence and hash.`
        : "This build has no bill of materials: release builds carry one."),
      h("div", { class: "inline" },
        b ? h("button", { class: "btn small", onclick: save }, icon(FileDown, 14), "Save bill of materials") : null,
        h("button", { class: "btn small", onclick: () => ctx.api.openNotices().catch((e) => toast(errorText(e), "error")) }, icon(FileText, 14), "Licence texts")),
      h("span", { class: "section-label" }, "Check a download came from this project"),
      h("div", { class: "muted tiny" }, "Each release is built on GitHub and attested: its SHA256SUMS.txt names every file by its hash. With the GitHub CLI, in the folder you downloaded to:"),
      h("div", { class: "inline" },
        h("code", { class: "mono wrap-anywhere grow" }, verify),
        h("button", { class: "icon-btn", "aria-label": "Copy the command", title: "Copy", onclick: () => copyText(verify).then(() => toast("Copied", "ok"), (e) => toast(errorText(e), "error")) }, icon(ClipboardCopy, 14)))));
}

export async function securityView(ctx: Ctx): Promise<Rendered> {
  const [s, { apps }] = await Promise.all([ctx.api.security(), ctx.api.overview()]);
  return {
    node: h("div", { class: "page" },
      h("div", { class: "page-title" }, h("h1", null, "Security")),
      h("div", { class: "muted small" }, "What devstribution checks, what it found, and what each app is made of. Nothing here leaves this computer."),
      overview(s, apps),
      h("div", { class: "sec-grid" },
        h("div", { class: "stack" }, appsCard(ctx, s, apps), eventsCard(ctx, s)),
        h("div", { class: "stack" }, trustCard(ctx, s), launcherCard(ctx, s)))),
  };
}

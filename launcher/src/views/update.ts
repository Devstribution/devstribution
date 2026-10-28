import { Download, X } from "lucide";
import type { Ctx } from "../main";
import type { LauncherUpdate } from "../api";
import { h, icon, clear, bytes, toast, errorText, trapFocus, announce } from "../ui";

/**
 * Looks for a newer devstribution. At start (`manual` false) it only says something when there
 * is one, and failures go to the log: no connection is not worth a message. From Settings it
 * always answers.
 */
export async function checkForUpdate(ctx: Ctx, manual: boolean) {
  let found: LauncherUpdate | null;
  try {
    found = await ctx.api.checkUpdate();
  } catch (e) {
    if (manual) toast(errorText(e), "error");
    return;
  }
  if (!found) {
    if (manual) toast("devstribution is up to date", "ok");
    return;
  }
  const u = found;
  if (manual) openUpdate(ctx, u);
  else toast(`devstribution ${u.version} is available`, "info", { label: "See what’s new", run: () => openUpdate(ctx, u) });
}

/** What the new version brings, and "Update and restart". */
function openUpdate(ctx: Ctx, u: LauncherUpdate) {
  const close = () => { release(); overlay.remove(); document.removeEventListener("keydown", onKey); };
  // Escape and "Later" do nothing once the download started: the installer takes over from it.
  let busy = false;
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && !busy) close(); };
  document.addEventListener("keydown", onKey);

  const status = h("div", { class: "stack tight" });
  const later = h("button", { class: "btn", onclick: () => { if (!busy) close(); } }, "Later") as HTMLButtonElement;
  const go = h("button", { class: "btn primary", onclick: () => install() }, icon(Download, 16), "Update and restart") as HTMLButtonElement;
  const overlay = h("div", { class: "overlay", onclick: (e: Event) => { if (e.target === overlay && !busy) close(); } },
    h("div", { class: "modal", role: "dialog", "aria-label": `devstribution ${u.version}` },
      h("div", { class: "modal-head" },
        h("div", { class: "grow" }, h("h2", null, `devstribution ${u.version} is available`),
          h("div", { class: "muted small" }, `You have ${u.current}. The update is checked against devstribution’s own key before it runs.`)),
        h("button", { class: "icon-btn", "aria-label": "Close", onclick: () => { if (!busy) close(); } }, icon(X, 16))),
      h("div", { class: "modal-body" },
        u.notes ? [h("div", { class: "section-label" }, "What’s new"), h("div", { class: "release-notes" }, plainNotes(u.notes))] : null,
        h("div", { class: "muted small" }, "devstribution closes, the new version installs (no administrator rights needed), then opens again. Your apps, results and secrets stay as they are."),
        status),
      h("div", { class: "modal-foot" }, later, go)));
  document.body.appendChild(overlay);
  const release = trapFocus(overlay.firstElementChild as HTMLElement);

  async function install() {
    busy = true;
    later.disabled = go.disabled = true;
    const fill = h("div", { class: "fill" });
    const bar = h("div", { class: "progress indeterminate", role: "progressbar", "aria-label": "Download" }, fill);
    const line = h("div", { class: "muted small" }, "Downloading…");
    clear(status, bar, line);
    announce("Downloading the update");
    const off = await ctx.api.onUpdateProgress((done, total) => {
      bar.classList.toggle("indeterminate", !total);
      if (total) {
        const pct = Math.min(100, Math.round((done / total) * 100));
        fill.style.width = `${pct}%`;
        bar.setAttribute("aria-valuenow", String(pct));
      }
      line.textContent = total ? `Downloading… ${bytes(done)} of ${bytes(total)}` : `Downloading… ${bytes(done)}`;
    });
    try {
      // On success the launcher closes here and the installer takes over.
      await ctx.api.installUpdate();
    } catch (e) {
      busy = false;
      later.disabled = go.disabled = false;
      clear(status, h("div", { class: "error-box", role: "alert" }, errorText(e)));
    } finally {
      off();
    }
  }
}

/** Release notes are Markdown: shown as text, without the marks that only format it. */
function plainNotes(md: string): string {
  return md
    .replace(/\r\n/g, "\n")
    .replace(/^#+\s*/gm, "")
    .replace(/^>\s?/gm, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .trim();
}

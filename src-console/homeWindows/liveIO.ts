/** Tauri side of live Home windows: the home_window_* commands and their events. No page content, no URLs in logs. */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { inTauri } from "../lib/localStores";
import type { LiveIO, LiveOpenResult } from "./live";
import type { SignedSite } from "./adapters";

export const REFUSED_FALLBACK = "Web windows are off while Local only or outbound-disabled is on. Nothing was opened.";

type Call = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
const tauriCall: Call = (cmd, args) => invoke(cmd, args);

const errText = (e: unknown): string => (typeof e === "string" ? e : e instanceof Error ? e.message : "");

/** Backend errors carry a stable prefix: refused: / unsupported: / failed:. */
export function classifyOpenError(e: unknown): Extract<LiveOpenResult, { ok: false }> {
  const t = errText(e);
  if (t.startsWith("refused:")) return { ok: false, reason: "refused", copy: t.slice(8).trim() || REFUSED_FALLBACK };
  if (t.startsWith("unsupported:")) return { ok: false, reason: "unsupported", copy: t.slice(12).trim() };
  return { ok: false, reason: "failed", copy: t.replace(/^failed:\s*/, "") || "This page could not be opened." };
}

export function createLiveIO(call: Call = tauriCall): LiveIO & {
  preflight(): Promise<{ ok: true } | { ok: false; copy: string }>;
  sites(): Promise<SignedSite[]>;
  signOut(site: string): Promise<void>;
  undo(): Promise<string | null>;
} {
  return {
    async open({ id, kind, url }) {
      try {
        const r = await call<{ replaced: string | null }>("home_window_open", { id, kind, target: { url } });
        return { ok: true, replaced: r.replaced ?? null };
      } catch (e) { return classifyOpenError(e); }
    },
    async close(id) { await call("home_window_close", { id }); },
    async focus(id) { await call("home_window_focus", { id }); },
    async popOut(id) { try { await call("home_window_pop_out", { id }); return true; } catch { return false; } },
    async layout(rects) { await call("home_window_layout", { rects }); },
    async preflight() {
      try { await call("home_window_preflight"); return { ok: true }; }
      catch (e) { return { ok: false, copy: classifyOpenError(e).copy || REFUSED_FALLBACK }; }
    },
    async sites() {
      const rows = await call<{ id: string; host: string; label: string; signed: "in" | "out" | "none" }[]>("home_window_sites");
      return rows.map((r) => ({ id: r.id, host: r.host, label: r.label, signed: r.signed }));
    },
    async signOut(site) { await call("home_window_sign_out", { site }); },
    async undo() { try { return await call<string>("home_window_undo"); } catch { return null; } },
  };
}

export const liveAvailable = (): boolean => inTauri();

export async function listenBackend(on: { state(p: { id: string; state: string; signedIn?: boolean | null }): void; closed(id: string): void }): Promise<() => void> {
  const offs = await Promise.all([
    listen<{ id: string; state: string; signedIn?: boolean | null }>("home-windows://state", (e) => on.state(e.payload)),
    listen<{ id: string }>("home-windows://closed", (e) => on.closed(e.payload.id)),
  ]);
  return () => offs.forEach((o) => o());
}

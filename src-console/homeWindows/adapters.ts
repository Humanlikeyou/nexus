/**
 * design-1111w adapters. The live app is unwired until Claude adds webview embedding,
 * a session store, on-device comments and TTS (HANDOFF-1111w). Sample mode is opt-in.
 *
 * Do not invoke unregistered Tauri commands from here — commands.registered.test.ts
 * must stay green.
 */
import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import { createLiveIO, liveAvailable } from "./liveIO";
import type { Signed } from "./types";

export type SignedSite = {
  id: string;
  host: string;
  label: string;
  signed: Exclude<Signed, null> | "none";
};

export type HomeWindowsAdapter = {
  /** Whether a real webview can be embedded. */
  embedding(): Wired<true>;
  /** Sites with a persistent profile. Status only — never cookies or tokens. */
  signedSites(): Wired<SignedSite[]>;
  /** Sign out deletes that site's profile. */
  signOut(siteId: string): Promise<Wired<{ ok: true }>>;
  /** Local light-tier comment from on-screen text. */
  comment(input: { siteId: string; title: string }): Promise<Wired<{ text: string }>>;
  /** Speak a comment. Live TTS is unwired. */
  speak(text: string): Promise<Wired<{ ok: true }>>;
  /** Live adapter only: re-read the sites list from the backend. */
  refreshSites?(): Promise<void>;
};

const INITIAL_SITES: SignedSite[] = [
  { id: "instagram", host: "instagram.com", label: "Instagram", signed: "in" },
  { id: "tradingview", host: "tradingview.com", label: "TradingView", signed: "in" },
  { id: "youtube", host: "youtube.com", label: "YouTube", signed: "none" },
];
let SAMPLE_SITES: SignedSite[] = INITIAL_SITES.map((s) => ({ ...s }));
export const resetSampleSites = (): void => { SAMPLE_SITES = INITIAL_SITES.map((s) => ({ ...s })); };

const sampleAdapter: HomeWindowsAdapter = {
  embedding: () => sample(true),
  signedSites: () => sample(SAMPLE_SITES.map((s) => ({ ...s }))),
  async signOut(siteId) {
    const list = SAMPLE_SITES.find((s) => s.id === siteId);
    if (list) list.signed = "none";
    return sample({ ok: true as const });
  },
  async comment() {
    return UNWIRED;
  },
  async speak() {
    return UNWIRED;
  },
};

const unwiredAdapter: HomeWindowsAdapter = {
  embedding: () => UNWIRED,
  signedSites: () => UNWIRED,
  async signOut() { return UNWIRED; },
  async comment() { return UNWIRED; },
  async speak() { return UNWIRED; },
};

/** In the app (not sample mode): webviews, profiles, sign out are real; comments and TTS stay unwired. */
let liveSites: SignedSite[] = [];
let liveIo: ReturnType<typeof createLiveIO> | null = null;
export const liveIO = () => (liveIo ??= createLiveIO());
export const resetLiveSites = (io?: ReturnType<typeof createLiveIO>): void => { liveSites = []; liveIo = io ?? null; };

const liveAdapter: HomeWindowsAdapter = {
  embedding: () => ({ state: "live", value: true }),
  signedSites: () => ({ state: "live", value: liveSites.map((s) => ({ ...s })) }),
  async refreshSites() { liveSites = await liveIO().sites(); },
  async signOut(siteId) {
    await liveIO().signOut(siteId); // throws if he declines the native dialog
    liveSites = await liveIO().sites();
    return { state: "live", value: { ok: true as const } };
  },
  async comment() { return UNWIRED; },
  async speak() { return UNWIRED; },
};

export const homeWindowsAdapter = (): HomeWindowsAdapter =>
  sampleAdaptersEnabled() ? sampleAdapter : liveAvailable() ? liveAdapter : unwiredAdapter;
export const liveEmbedding = (): boolean => !sampleAdaptersEnabled() && liveAvailable();

/** Secrets must never appear in this UI. Used by tests. */
export const SECRET_PATTERN = /cookie\s*=|token\s*=|password\s*=|sessionid\s*=|authorization\s*:|xox[bap]-|sk_live|sk_test|ghp_|-----BEGIN/i;

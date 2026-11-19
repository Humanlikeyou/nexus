import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import { APP_VERSION } from "../app/version";
import { makeLiveStandalone } from "./liveAdapter";

/**
 * design-1111s: Amara OS as a standalone app (first-run, Services, Backup, Updates, Notifications, WHOOP 4).
 * Every surface reads from this adapter. In the app it is unwired ("Needs wiring") until Claude builds
 * the supervisor, commands and hooks in docs/design/HANDOFF-1111s.md. The sample adapter is preview/test
 * only, and every value it returns is shown with a "Sample data" tag.
 * Secrets contract: nothing here ever carries a secret value. Key fields report only `set: boolean`.
 */

// ---- Services (Settings › Services) ----
export type ServiceState = "healthy" | "degraded" | "restarting" | "stopped" | "waiting" | "failed";
export type ServiceGroup = "core" | "automation" | "memory" | "health" | "side";
export type Service = {
  id: string; group: ServiceGroup; name: string; what: string; state: ServiceState; meta: string;
  /** Built into Amara (health sync is part of Amara, not a separate program). */
  builtIn?: boolean;
  reasonOwner?: "user" | "supervisor" | "transition";
  restartAttempt?: number; restartMax?: number;
};
export const SERVICE_GROUPS: Array<{ id: ServiceGroup; title: string }> = [
  { id: "core", title: "Core" },
  { id: "automation", title: "Automations and crews" },
  { id: "memory", title: "Memory and knowledge" },
  { id: "health", title: "Health, home and money" },
  { id: "side", title: "Side tool" },
];
export type ServiceEvent = { at: string; level: "ok" | "warn" | "fail"; text: string; hint?: string };
/** Plain-words events plus a redacted raw tail (the backend redacts; the UI never sees a secret). */
export type ServiceLogs = { events: ServiceEvent[]; tail: string[] };
export type ServicesOverview = { services: Service[]; checked: string };
export type ServicesSummary = { attention: number; updateReady: boolean };

// ---- Keys (Set / Not set only) ----
export type SecretName = "telegram_bot_token" | "telegram_chat_id" | "n8n_encryption_key" | "ha_token" | "freqtrade_api" | "coach_keys" | "letta_server_password" | "weaviate_api_key" | "n8n_api_key" | "crew_service_token";
export type SecretStatus = { name: SecretName; label: string; hint?: string; set: boolean };

// ---- Backup and Move to a new Mac ----
export type BackupSettings = { nightly: boolean; at: string; dir: string; keep: number; beforeUpdate: boolean; skipOnBatteryBelow: number };
/** Taisun, 14:59 JST Nov 11: nightly at 03:00 to ~/Amara Backups, keep 14, plus one before every update. */
export const BACKUP_DEFAULTS: BackupSettings = { nightly: true, at: "03:00", dir: "~/Amara Backups", keep: 14, beforeUpdate: true, skipOnBatteryBelow: 20 };
export type BackupRun = { pct: number; done: string; total: string; left: string; phase: string };
export type BackupStatus = {
  /** `checksumOk` is null until `backup_verify_last` has answered for this file. `checks` says what ran: per-file sha256 against the manifest, or a structure read (the packager's manifest has no hashes). */
  last: { at: string; size: string; checksumOk: boolean | null; checks?: "sha256" | "structure" } | null;
  running: BackupRun | null;
  settings: BackupSettings;
  backups: Array<{ id: string; label: string; madeAt: string; version: string; daysOld: number }>;
};
/** The n8n encryption key travels in its own passphrase-locked key file, never inside a backup. */
export type KeyFileStatus = { made: boolean; madeAt?: string; where: string };
export const BACKUP_CONTENTS = ["Chats and Today", "Goals, check-ins, life admin", "Memory (Letta)", "Database (Postgres)", "Knowledge base", "Crews", "n8n workflows", "Muse sessions and replays", "Health store", "Freqtrade settings", "Home Assistant config", "Amara settings"];

// ---- Updates (Settings › General) ----
export type UpdateStatus = {
  current: string;
  /** Version text comes from the signed manifest only; null when the manifest has none. */
  available: { version: string | null; notes: string[]; scheduledFor: string } | null;
  /** Live only: the backend's own state. `not_configured` and `outbound_disabled` mean no check can run, and the UI says so. */
  backend?: "not_configured" | "outbound_disabled" | "idle" | "available";
  lastCheck: string;
  autoUpdate: boolean; checkDaily: boolean;
  history: Array<{ version: string; when: string; how: string }>;
};

// ---- Notifications (Telegram relay) ----
export type AlertKind = "sleep" | "recovery" | "effort" | "battery" | "sync" | "lucid" | "illness";
export const ALERT_KINDS: Array<{ id: AlertKind; label: string }> = [
  { id: "sleep", label: "Sleep" }, { id: "recovery", label: "Recovery" }, { id: "effort", label: "Effort" },
  { id: "battery", label: "Strap battery" }, { id: "sync", label: "Sync problems" }, { id: "lucid", label: "Lucid cues" }, { id: "illness", label: "Illness signs" },
];
/** No quiet hours and no rate limit (Taisun, 14:59 JST Nov 11): only on/off, the keys, the kinds and the preview. */
export type RelaySettings = { enabled: boolean; kinds: Record<AlertKind, boolean>; lastSent: string | null;
  /** Kinds the app can't see yet (shown as "not connected yet"). Absent in sample data. */
  notConnected?: AlertKind[] };
export type RelayPreview = { text: string; at: string; from: string };

// ---- WHOOP 4 (Settings › Devices) ----
export type WhoopStatus = {
  /** "legacy" only during the move: a legacy helper still holds the strap and Amara will take over. */
  owner: "amara" | "legacy" | "none";
  connected: boolean;
  batteryPct: number | null; batteryNote: string;
  lastSync: string; lastSyncOk: boolean;
  liveHr: "live" | "delayed" | "off"; liveNote: string;
  gaps14d: number;
  scores: string;
  store: { where: string; verified: boolean; oldCopyUntil: string | null };
  lucid: boolean;
  muse: string;
};
export type PairStep = { id: "bluetooth" | "free" | "wake" | "found" | "backfill"; title: string; sub: string; state: "done" | "active" | "next" };

// ---- First-run setup ----
export type SystemProfile = { ramGb: number; chip: string; fileVault: boolean };
export type ModelTier = { id: string; name: string; minRamGb: number; models: string[]; download: string; note?: string; recommended: boolean; available: boolean };
export type SetupDownload = { models: Array<{ name: string; state: "done" | "downloading" | "waiting"; detail: string; pct: number }>; services: Array<{ name: string; sub?: string; state: "running" | "starting" | "waiting" }> };
export type RestorePreview = { file: string; madeOn: string; version: string; size: string; checksumOk: boolean; contents: Array<{ title: string; sub: string }>; keyFileFound: boolean };

export type Action = Promise<Wired<{ ok: true }>>;

/**
 * An action that must say HOW it ended (Settings workflows, decision 0042). `done` carries what happened;
 * every other state is a distinct, plain-words reason. `message` is fixed copy from the adapter, never raw backend text.
 */
export type Fail = "declined" | "unsupported" | "failed" | "off" | "missing" | "telegram" | "cancelled" | "busy" | "expired" | "gave_up" | "unwired";
export type Outcome<T extends object = object> = ({ state: "done" } & T) | { state: Fail; message?: string };

export interface StandaloneAdapter {
  services(): Wired<ServicesOverview>;
  serviceLogs(id: string): Wired<ServiceLogs>;
  /** `done` carries `healthy`: whether the service reported healthy within the wait (live), else it only restarted. */
  serviceAction(id: string, action: "restart" | "start"): Promise<Outcome<{ healthy: boolean }>>;
  summary(): Wired<ServicesSummary>;
  secrets(): Wired<SecretStatus[]>;
  /** Write-only. The value goes straight to the Keychain; nothing is returned but ok. */
  setSecret(name: SecretName, value: string): Action;
  backup(): Wired<BackupStatus>;
  backupNow(): Action;
  setBackupSettings(s: Partial<BackupSettings>): Action;
  /** Read-only preview of one backup (`backup_list` id): its real contents and checksum. */
  previewBackup(id: string): Promise<Outcome<{ preview: RestorePreview }>>;
  /** Restores one backup (`backup_list` id) through the native approval gate; `lines` is what the backend reports. */
  restore(id: string): Promise<Outcome<{ lines: string[] }>>;
  /** Native folder picker, then saves it as the backup folder. `cancelled` when the picker is closed. */
  pickBackupDir(): Promise<Outcome<{ dir: string }>>;
  keyFile(): Wired<KeyFileStatus>;
  /** Write-only: the passphrase locks the key file and is never stored or returned. */
  makeKeyFile(passphrase: string): Action;
  updates(): Wired<UpdateStatus>;
  setUpdatePrefs(p: { autoUpdate?: boolean; checkDaily?: boolean }): Action;
  checkUpdate(): Promise<Outcome>;
  installUpdate(): Promise<Outcome>;
  relay(): Wired<RelaySettings>;
  setRelay(p: Partial<RelaySettings>): Action;
  relayPreview(): Wired<RelayPreview>;
  /** Asks in a native dialog first; never sends under the outbound switch or navy. */
  relaySendTest(): Promise<Outcome>;
  /** Asks the backend for a fresh preview text (`notify_preview`); the new text shows through `relayPreview()`. */
  writeAnotherPreview(): Promise<Outcome>;
  whoop(): Wired<WhoopStatus>;
  whoopAction(a: "sync" | "buzz" | "forget" | "take_over" | "lucid_on" | "lucid_off"): Action;
  pairSteps(): Wired<PairStep[]>;
  profile(): Wired<SystemProfile>;
  tiers(): Wired<ModelTier[]>;
  setupDownload(): Wired<SetupDownload>;
  restorePreview(): Wired<RestorePreview>;
  /** Setup state: live and incomplete opens the first-run window on launch. */
  setupDone(): Wired<boolean>;
  /** Logic-only setup actions (no UI of their own). Unwired in the base and in the sample adapter. */
  pickTier(tier: string): Action;
  startDownload(tier: string): Action;
  finishSetup(tier: string): Action;
  startServices(): Action;
  resetSetup(): Action;
}

const never = async () => UNWIRED;
const nope = async (): Promise<Outcome<never>> => ({ state: "unwired" });
export const unwiredStandalone: StandaloneAdapter = {
  services: () => UNWIRED, serviceLogs: () => UNWIRED, serviceAction: nope, summary: () => UNWIRED,
  secrets: () => UNWIRED, setSecret: never, backup: () => UNWIRED, backupNow: never, setBackupSettings: never, restore: nope, previewBackup: nope, pickBackupDir: nope,
  keyFile: () => UNWIRED, makeKeyFile: never, updates: () => UNWIRED, setUpdatePrefs: never, checkUpdate: nope, installUpdate: nope,
  relay: () => UNWIRED, setRelay: never, relayPreview: () => UNWIRED, relaySendTest: nope, writeAnotherPreview: nope,
  whoop: () => UNWIRED, whoopAction: never, pairSteps: () => UNWIRED,
  profile: () => UNWIRED, tiers: () => UNWIRED, setupDownload: () => UNWIRED, restorePreview: () => UNWIRED, setupDone: () => UNWIRED,
  pickTier: never, startDownload: never, finishSetup: never, startServices: never, resetSetup: never,
};

const OK = { state: "sample", value: { ok: true } } as const;
const DONE = { state: "done" } as const;
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Preview/test only. Every value is "◇ Sample data"; nothing here is a real reading, size, time or version. */
export function sampleStandalone(opts: { whoopOwner?: WhoopStatus["owner"]; relayKeys?: boolean; backupRunning?: boolean; updateReady?: boolean; downloadsDone?: boolean } = {}): StandaloneAdapter {
  const svc: Service[] = [
    { id: "docker", group: "core", name: "Background engine", what: "Docker. Runs the containers below, hidden. You never need to open it.", state: "healthy", meta: "up 6 days" },
    { id: "model-runtime", group: "core", name: "Model runtime", what: "Local AI on this Mac · fast and deep models loaded", state: "healthy", meta: "18.4 GB in memory" },
    { id: "amara-router", group: "core", name: "Router", what: "Picks the fast or deep model for each request", state: "healthy", meta: "answers in 40 ms" },
    { id: "hermes-gateway", group: "core", name: "Assistant (Hermes)", what: "Amara's chat, tools and Telegram gateway", state: "healthy", meta: "up 6 days" },
    { id: "amara-voice", group: "core", name: "Voice", what: "Listening and speaking", state: "healthy", meta: "up 2 h" },
    { id: "n8n", group: "automation", name: "Automations (n8n)", what: "Slow to answer: 3 of the last 10 checks took over 5 s. Workflows still run.", state: "degraded", meta: "since 1:48 PM" },
    { id: "crew-service", group: "automation", name: "Crews", what: "Stopped answering at 1:58 PM. Amara is restarting it.", state: "restarting", meta: "try 2 of 3", reasonOwner: "supervisor", restartAttempt: 2, restartMax: 3 },
    { id: "postgres", group: "memory", name: "Database (Postgres)", what: "Memory, automations, Terrance", state: "healthy", meta: "up 6 days" },
    { id: "letta", group: "memory", name: "Memory (Letta)", what: "What Amara remembers", state: "healthy", meta: "47 memories" },
    { id: "weaviate", group: "memory", name: "Knowledge search", what: "Weaviate and Verba", state: "healthy", meta: "312 documents" },
    { id: "health-reader", group: "health", name: "Health sync · reader", what: "Built into Amara · reads your WHOOP history on this Mac", state: "healthy", meta: "synced 2 min ago", builtIn: true },
    { id: "health-sync", group: "health", name: "Health sync", what: "Built into Amara · talks to your WHOOP 4 strap", state: "waiting", meta: "another app has the strap", builtIn: true, reasonOwner: "transition" },
    { id: "homeassistant", group: "health", name: "Home Assistant", what: "Smart home · you stopped it Oct 26", state: "stopped", meta: "by you", reasonOwner: "user" },
    { id: "freqtrade", group: "health", name: "Freqtrade", what: "Money / ARM trading bot", state: "healthy", meta: "dry status OK" },
    { id: "open-webui", group: "side", name: "Open WebUI", what: "Separate chat window. Not part of Amara; Amara just watches it.", state: "healthy", meta: "this Mac only" },
  ];
  const secrets: SecretStatus[] = [
    { name: "telegram_bot_token", label: "Telegram bot token", hint: "Notifications relay", set: opts.relayKeys !== false },
    { name: "telegram_chat_id", label: "Telegram chat", hint: "Only this chat receives messages", set: opts.relayKeys !== false },
    { name: "n8n_encryption_key", label: "n8n encryption key", hint: "Opens saved n8n logins · travels in your key file", set: true },
    { name: "ha_token", label: "Home Assistant token", set: false },
    { name: "freqtrade_api", label: "Freqtrade API login", set: true },
    { name: "coach_keys", label: "Oura, Hevy, AI coach", set: false },
  ];
  const owner = opts.whoopOwner ?? "amara";
  const legacy = owner === "legacy";
  let relay: RelaySettings = { enabled: opts.relayKeys !== false, kinds: { sleep: true, recovery: true, effort: true, battery: true, sync: true, lucid: false, illness: true }, lastSent: opts.relayKeys === false ? null : "7:12 AM" };
  let lucid = true;
  let made = true;
  return {
    services: () => sample({ services: svc.map((s) => (s.id === "health-sync" && !legacy ? { ...s, state: "healthy" as const, meta: "connected · synced 2 min ago", reasonOwner: undefined } : s)), checked: "12 s ago" }),
    serviceLogs: (id) => sample(id === "n8n" ? {
      events: [
        { at: "2:01 PM", level: "warn", text: "A check took 6.2 s to answer (usual is under 1 s).", hint: "Workflows kept running. No runs failed." },
        { at: "1:56 PM", level: "ok", text: "Workflow \u201cSample morning digest\u201d ran in 3.4 s." },
        { at: "1:52 PM", level: "warn", text: "Memory use climbed to 1.9 GB, near its 2 GB limit.", hint: "Likely cause: a large webhook payload at 1:47 PM." },
        { at: "1:48 PM", level: "warn", text: "First slow check. Amara started watching closely." },
        { at: "9:02 AM", level: "ok", text: "Started with Amara. Healthy." },
      ],
      tail: ["14:01:07 WARN  health  check took 6214 ms", "13:56:40 INFO  run     sample-digest ok 3.4s", "13:52:11 WARN  memory  1.9 GB of 2 GB", "13:47:58 INFO  webhook received ([redacted] path)", "13:48:02 WARN  health  check took 5120 ms", "09:02:15 INFO  start   ready"],
    } : { events: [{ at: "9:02 AM", level: "ok", text: "Started with Amara. Healthy." }], tail: ["09:02:15 INFO  start   ready"] }),
    serviceAction: async () => { await wait(300); return { state: "done", healthy: true }; },
    summary: () => sample({ attention: 1, updateReady: opts.updateReady ?? false }),
    secrets: () => sample(secrets),
    setSecret: async (name) => { await wait(200); const s = secrets.find((x) => x.name === name); if (s) s.set = true; return OK; },
    backup: () => sample({
      last: { at: "Oct 28, 3:00 AM", size: "8.4 GB", checksumOk: true },
      running: opts.backupRunning ? { pct: 62, done: "5.2", total: "8.4 GB", left: "about 2 min left", phase: "Memory and knowledge · copying" } : null,
      settings: BACKUP_DEFAULTS,
      backups: [{ id: "b-oct-28", label: "amara-backup-oct-28", madeAt: "Oct 28, 3:00 AM", version: APP_VERSION, daysOld: 0 }, { id: "b-oct-21", label: "amara-backup-oct-21", madeAt: "Oct 21, 3:00 AM", version: APP_VERSION, daysOld: 7 }],
    }),
    backupNow: async () => { await wait(300); return OK; },
    setBackupSettings: async () => OK,
    previewBackup: async () => ({ state: "done", preview: { file: "amara-backup-oct-21.tar.gz", madeOn: "Oct 21, 3:00 AM", version: APP_VERSION, size: "8.1 GB", checksumOk: true, keyFileFound: true,
      contents: [{ title: "Chats and Today", sub: "1,120 messages" }, { title: "Memory and knowledge", sub: "Letta, Postgres, Weaviate" }, { title: "Settings", sub: "Theme, voice, model size" }] } }),
    restore: async () => { await wait(300); return { state: "done", lines: ["Safety backup of current data saved (sample)", "Restored goals.json (sample)"] }; },
    pickBackupDir: async () => ({ state: "done", dir: "~/Amara Backups" }),
    keyFile: () => sample({ made, madeAt: made ? "Oct 28, 3:00 AM" : undefined, where: "~/Amara Backups · beside the backups, never inside one" }),
    makeKeyFile: async () => { await wait(300); made = true; return OK; },
    updates: () => sample({
      current: APP_VERSION,
      available: opts.updateReady ? { version: null, notes: ["**Services** page: see what's running and restart it in one click.", "**Telegram relay**: Amara sends health alerts in her own words.", "WHOOP 4 pairs directly with Amara.", "Fixes: crews no longer stall after the Mac wakes."], scheduledFor: "tonight at 3:30 AM" } : null,
      lastCheck: "today at 1:00 PM",
      autoUpdate: true, checkDaily: true,
      history: [{ version: APP_VERSION, when: "Oct 14, 3:30 AM", how: "automatic" }, { version: "Earlier version", when: "Sep 20", how: "from the DMG, by you" }],
    }),
    setUpdatePrefs: async () => OK,
    checkUpdate: async () => DONE,
    installUpdate: async () => DONE,
    relay: () => sample(relay),
    setRelay: async (p) => { relay = { ...relay, ...p, kinds: { ...relay.kinds, ...(p.kinds ?? {}) } }; return OK; },
    relaySendTest: async () => DONE,
    writeAnotherPreview: async () => DONE,
    relayPreview: () => sample({ from: "From this morning\u2019s \u201cRecovery ready\u201d alert. Amara writes a new one each time, never a template.", at: "7:12 AM", text: "Morning, Taisun. You slept 7h 32m, about 20 minutes more than usual, and recovery is 74%. HRV is up to 66 ms, so today can take a hard session. Strap is at 18%: charge it before bed so tonight\u2019s sleep is recorded." }),
    whoop: () => sample({
      owner, connected: !legacy,
      batteryPct: 62, batteryNote: legacy ? "as last reported" : "about 2 days left · charging off",
      lastSync: legacy ? "Last write 4 min ago" : "2 min ago · history up to 1:58 PM", lastSyncOk: true,
      liveHr: legacy ? "delayed" : "live", liveNote: legacy ? "From the health store, up to 2 min behind" : "Every second while connected",
      gaps14d: 0, scores: "Worked out on this Mac by Amara\u2019s health engine",
      store: legacy ? { where: "Current store · Amara reads it, read-only", verified: false, oldCopyUntil: null } : { where: "Amara\u2019s folder on this Mac", verified: true, oldCopyUntil: "Nov 27" },
      lucid: legacy ? false : lucid, muse: "In-app Bluetooth · last session Oct 27",
    }),
    whoopAction: async (a) => { await wait(300); if (a === "lucid_on") lucid = true; if (a === "lucid_off") lucid = false; return OK; },
    pairSteps: () => sample([
      { id: "bluetooth", title: "Bluetooth allowed", sub: "macOS permission for Amara", state: "done" },
      { id: "free", title: "Strap is free", sub: "No other app is connected to it", state: "done" },
      { id: "wake", title: "Double-tap the strap to wake it", sub: "Keep it within a metre of the Mac", state: "done" },
      { id: "found", title: "Looking for your strap…", sub: "Found \u201cWHOOP 4.0\u201d · signal good", state: "active" },
      { id: "backfill", title: "Catch up on history", sub: "Pulls the days the strap stored while unpaired", state: "next" },
    ]),
    profile: () => sample({ ramGb: 36, chip: "Apple M3 Pro", fileVault: true }),
    tiers: () => sample([
      { id: "compact", name: "Compact", minRamGb: 16, models: ["Fast: small light model", "Deep: medium model"], download: "About 9 GB download", recommended: false, available: true },
      { id: "standard", name: "Standard", minRamGb: 32, models: ["Fast: light model (same as today)", "Deep: today\u2019s deep model"], download: "About 21 GB download", note: "fits with crews and memory running", recommended: true, available: true },
      { id: "large", name: "Large", minRamGb: 64, models: ["Fast: light model", "Deep: large model"], download: "About 48 GB download", note: "offered when you move to a bigger Mac", recommended: false, available: false },
    ]),
    setupDownload: () => sample({
      models: opts.downloadsDone ? [{ name: "Fast model", state: "done", detail: "5.2 GB", pct: 100 }, { name: "Deep model", state: "done", detail: "15.6 GB", pct: 100 }, { name: "Speech (voice)", state: "done", detail: "0.6 GB", pct: 100 }]
        : [{ name: "Fast model", state: "done", detail: "5.2 GB", pct: 100 }, { name: "Deep model", state: "downloading", detail: "9.8 of 15.6 GB · about 6 min left", pct: 63 }, { name: "Speech (voice)", state: "waiting", detail: "0.6 GB", pct: 0 }],
      services: [{ name: "Background engine (Docker)", sub: "Starts hidden; you never open it", state: "running" }, { name: "Database and memory", sub: "Postgres, Letta, Weaviate", state: "running" }, { name: "Assistant (Hermes) and router", state: "running" }, { name: "Health sync", sub: "Built into Amara · WHOOP 4", state: "running" }, { name: "Automations (n8n) and crews", state: "starting" }, { name: "Model runtime", sub: "Waits for the deep model", state: "waiting" }, { name: "Home Assistant, Freqtrade", state: "waiting" }],
    }),
    restorePreview: () => sample({
      file: "amara-backup-oct-28.amarabackup", madeOn: "Oct 28, 3:00 AM · MacBook Pro", version: APP_VERSION, size: "8.4 GB", checksumOk: true, keyFileFound: true,
      contents: [{ title: "Chats and Today", sub: "1,284 messages · goals, lists, check-ins" }, { title: "Memory and knowledge", sub: "Letta, Postgres, Weaviate · 47 memories · 312 documents" }, { title: "Crews and automations", sub: "4 crews · 23 n8n workflows" }, { title: "Muse sessions and replays", sub: "10 replays · 41 sessions" }, { title: "Health store", sub: "WHOOP 4 history since Feb 17" }, { title: "Settings", sub: "Theme, voice, model size" }],
    }),
    setupDone: () => sample(true),
    pickTier: async () => OK, startDownload: async () => OK, finishSetup: async () => OK, startServices: async () => OK, resetSetup: async () => OK,
  };
}

/** Preview flags (localStorage "nexus.standalonePreview"), read only in sample mode (captures and e2e). Never affects the live app. */
function previewOpts(): Parameters<typeof sampleStandalone>[0] {
  try {
    const raw = globalThis.localStorage?.getItem("nexus.standalonePreview");
    return raw ? JSON.parse(raw) : {};
  } catch { return {}; }
}

let cached: StandaloneAdapter | null = null;
let live: StandaloneAdapter | null = null;
export function standaloneAdapter(): StandaloneAdapter {
  if (!sampleAdaptersEnabled()) {
    // Live reads exist only inside the Tauri app; elsewhere (tests, browser) everything stays unwired.
    const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    return inTauri ? (live ??= makeLiveStandalone(unwiredStandalone)) : unwiredStandalone;
  }
  return (cached ??= sampleStandalone(previewOpts()));
}
/** Tests only. */
export function resetStandaloneAdapter() { cached = null; }

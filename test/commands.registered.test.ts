// @vitest-environment node
/**
 * Every command the frontend invokes must be registered in lib.rs
 * generate_handler! (an unregistered name fails at runtime with "unknown
 * command"). Uses scripts/command-inventory.mjs (docs/terrance-merge-inventory.md).
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error untyped .mjs script
import { commandCalls, inventory, registeredCommands } from "../scripts/command-inventory.mjs";

type Inventory = {
  commands: { name: string; status: string }[];
  unregistered: { name: string; files: string[] }[];
};

describe("Tauri command registry", () => {
  it("the call parser handles generics with parens and line breaks", () => {
    const src = `invoke<import("./types").X[]>("a_one", {}); read<Foo>("b_two", []);\n invoke<{ s: string }>(\n  "c_three");`;
    expect(commandCalls(src).sort()).toEqual(["a_one", "b_two", "c_three"]);
  });

  it("sees invokeBound(\"name\") calls (the bridge dispatcher used by the moved legacy-core modules)", () => {
    expect(commandCalls(`await invokeBound<void>("send_telegram_message", { text });\n invokeBound("get_config")`).sort()).toEqual(["get_config", "send_telegram_message"]);
  });

  it("sees generated-binding calls (__TAURI_INVOKE) so bound commands stay checked", async () => {
    const src = `load: (key: string, options?: InvokeOptions) => __TAURI_INVOKE<string | null>("load_kv", { key }, options),\n n: (o?: X) => __TAURI_INVOKE<N>("n8n_status", undefined, o)`;
    expect(commandCalls(src).sort()).toEqual(["load_kv", "n8n_status"]);
    // Fail-before proof: a bound name missing from generate_handler! is reported as unregistered.
    const registered = new Set(registeredCommands(`generate_handler![a::load_kv,]`).map((c: { name: string }) => c.name));
    expect(commandCalls(src).filter((n: string) => !registered.has(n))).toEqual(["n8n_status"]);
    // The real generated file is scanned and its commands are all registered.
    const { readFileSync } = await import("node:fs");
    const gen = commandCalls(readFileSync(new URL("../src-console/bindings.gen.ts", import.meta.url), "utf8"));
    expect(gen.length).toBeGreaterThanOrEqual(6);
    const lib = registeredCommands(readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8")).map((c: { name: string }) => c.name);
    for (const n of gen) expect(lib).toContain(n);
  });

  it("the Keychain migrate commands are registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["keychain_import_review", "keychain_migrate_one"]) expect(names).toContain(name);
  });

  it("the Phase 9 first-run commands are registered and bound", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    const table = readFileSync(new URL("../src-console/boundTable.ts", import.meta.url), "utf8");
    for (const name of ["model_tiers", "model_download", "setup_done", "setup_complete", "setup_reset", "services_start_all"]) {
      expect(names).toContain(name);
      expect(table).toContain(`${name}:`);
    }
  });

  it("parses generate_handler! without comments", () => {
    const names = registeredCommands(`tauri::generate_handler![\n // x::gone,\n a::one, b::two,\n]`).map((c: { name: string }) => c.name);
    expect(names).toEqual(["one", "two"]);
  });

  it("the Letta memory commands are registered, and there is no memory delete-block command", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["memory_list", "memory_edit", "memory_delete", "memory_restore_last"]) expect(names).toContain(name);
    // "memory_delete" clears a block's value (after approval); it never calls Letta's DELETE (src-tauri memory_write.rs).
    expect(names.filter((n: string) => /^memory_/.test(n)).sort()).toEqual(["memory_delete", "memory_edit", "memory_list", "memory_restore_last"]);
  });

  it("the turn pipeline commands are registered and the registry is managed", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["amara_turn", "amara_turn_cancel"]) expect(names).toContain(name);
    expect(lib).toContain(".manage(commands::amara_turn::TurnRegistry::default())");
  });

  it("the Muse recording readers are registered and no recording or session delete command exists", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    expect(names).toContain("read_business_ledger");
    for (const name of ["list_recordings", "read_recording_window", "read_body_window", "recording_series", "get_session_samples", "mark_recording_aux"]) {
      expect(names).toContain(name);
    }
    // Taisun removed delete_session (docs/decisions/0003); nothing is allow-listed.
    // delete_chat_session only deletes rows of the chat SQLite db (assistant.rs), not Muse files.
    expect(names.filter((n: string) => n !== "delete_chat_session").filter((n: string) => /(delete|remove|purge).*(session|recording|replay|muse)|(session|recording|replay|muse).*(delete|remove|purge)/.test(n))).toEqual([]);
  });

  it("the 12a leftovers (workouts, hypnogram, stop speaking) are registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["noop_recent_workouts", "noop_hypnogram", "voice_stop_speaking", "voice_speak_ready", "voice_speak_local", "voice_dictate_ready", "dictate_once"]) expect(names).toContain(name);
  });

  it("the backup prune plan and restore preview are registered, and no restore apply or prune delete command exists", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["backup_prune_plan", "restore_preview"]) expect(names).toContain(name);
    // Taisun's gates: restore() is not built and nothing deletes a backup (hard rule 7).
    for (const name of ["restore", "backup_restore", "backup_prune", "backup_delete"]) expect(names).not.toContain(name);
  });

  it("the Settings ops commands are registered; nothing takes over the strap or restarts a LaunchAgent", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["service_restart", "service_logs", "whoop_status", "system_profile", "restore_preview", "backup_prune_plan"]) expect(names).toContain(name);
    // Takeover and strap actions are a later stage (and a gate): they must not be registered yet.
    for (const name of ["whoop_take_over", "whoop_sync_now", "whoop_buzz", "whoop_pair_start", "whoop_forget", "service_start", "service_stop"]) expect(names).not.toContain(name);
  });

  it("the supervisor commands are registered (observe, log tail, act setting); start/stop commands are not built", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["services_status", "service_events", "services_summary", "supervisor_get", "service_log_tail", "supervisor_set_enabled"]) expect(names).toContain(name);
    for (const name of ["service_start", "service_stop"]) expect(names).not.toContain(name);
  });

  it("the outbound Settings default commands are registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["outbound_get", "outbound_set"]) expect(names).toContain(name);
  });

  it("the Hemi-Sync commands are registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["hemisync_import", "hemisync_library", "hemisync_session_link", "hemisync_session_finish", "hemisync_session_rate", "hemisync_sessions", "hemisync_stats", "hemisync_ranking"]) expect(names).toContain(name);
  });

  it("the money commands are registered and none can move money", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    const money = ["money_accounts_list", "money_account_add", "money_watch_add", "money_balance_set_manual", "money_import_pick", "fx_rate_get", "fx_rate_set_manual", "fx_rate_refresh", "money_summary_read", "money_facts_read"];
    for (const name of money) expect(names).toContain(name);
    // The renderer-supplied-path import is gone: the only import command opens its own native dialog.
    expect(names).not.toContain("money_import_statement");
    expect(names.filter((n: string) => /^(money|fx|treasury)_/.test(n) && /transfer|pay|trade|convert|withdraw|send|buy|sell|move|wire|remit|swap|sign|broadcast|order|deposit|exchange/.test(n))).toEqual([]);
  });

  it("the treasury commands are registered, with no way to move money and no repeat of the personal money names", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    const treasury = ["treasury_stream_add", "treasury_stream_update", "treasury_streams_list", "treasury_import_pick", "treasury_import_preview", "treasury_attention_set", "treasury_attention_list", "treasury_income_month", "treasury_income_trend", "treasury_per_hour", "treasury_fx_status"];
    for (const name of treasury) expect(names).toContain(name);
    expect(names.filter((n: string) => n.startsWith("treasury_")).sort()).toEqual([...treasury].sort());
  });

  it("patterns_read and weekly_review_read are registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    expect(names).toContain("patterns_read");
    expect(names).toContain("weekly_review_read");
  });

  it("goal_progress_week is registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    expect(names).toContain("goal_progress_week");
  });

  it("system_map_status is registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    expect(names).toContain("system_map_status");
  });

  it("every frontend invoke/read names a registered command", async () => {
    const inv = (await inventory()) as Inventory;
    expect(inv.commands.length).toBeGreaterThan(100);
    expect(inv.unregistered.map((u) => `${u.name} (${u.files.join(", ")})`)).toEqual([]);
  }, 30_000);
});

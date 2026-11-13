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
    for (const name of ["noop_recent_workouts", "noop_hypnogram", "voice_stop_speaking"]) expect(names).toContain(name);
  });

  it("the backup prune plan and restore preview are registered, and no restore apply or prune delete command exists", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["backup_prune_plan", "restore_preview"]) expect(names).toContain(name);
    // Taisun's gates: restore() is not built and nothing deletes a backup (hard rule 7).
    for (const name of ["restore", "backup_restore", "backup_prune", "backup_delete"]) expect(names).not.toContain(name);
  });

  it("the Hemi-Sync commands are registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    for (const name of ["hemisync_import", "hemisync_library", "hemisync_session_link", "hemisync_session_finish", "hemisync_session_rate", "hemisync_sessions", "hemisync_stats"]) expect(names).toContain(name);
  });

  it("the money commands are registered and none can move money", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    const money = ["money_accounts_list", "money_account_add", "money_watch_add", "money_balance_set_manual", "money_import_pick", "fx_rate_get", "fx_rate_set_manual", "fx_rate_refresh"];
    for (const name of money) expect(names).toContain(name);
    // The renderer-supplied-path import is gone: the only import command opens its own native dialog.
    expect(names).not.toContain("money_import_statement");
    expect(names.filter((n: string) => /^(money|fx)_/.test(n) && /transfer|pay|trade|convert|withdraw|send|buy|sell/.test(n))).toEqual([]);
  });

  it("patterns_read and weekly_review_read are registered", async () => {
    const { readFileSync } = await import("node:fs");
    const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    const names = registeredCommands(lib).map((c: { name: string }) => c.name);
    expect(names).toContain("patterns_read");
    expect(names).toContain("weekly_review_read");
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

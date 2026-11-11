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

  it("every frontend invoke/read names a registered command", async () => {
    const inv = (await inventory()) as Inventory;
    expect(inv.commands.length).toBeGreaterThan(100);
    expect(inv.unregistered.map((u) => `${u.name} (${u.files.join(", ")})`)).toEqual([]);
  }, 30_000);
});

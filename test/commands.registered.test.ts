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

  it("every frontend invoke/read names a registered command", async () => {
    const inv = (await inventory()) as Inventory;
    expect(inv.commands.length).toBeGreaterThan(100);
    expect(inv.unregistered.map((u) => `${u.name} (${u.files.join(", ")})`)).toEqual([]);
  }, 30_000);
});

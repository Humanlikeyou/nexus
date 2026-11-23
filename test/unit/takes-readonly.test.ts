import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// HANDOFF-1111t §10/§11: Home Assistant is read only for suggestions, and nothing here moves money or calls an HA action.
describe("suggestion layer is read-only", () => {
  const files = ["detectors.ts", "suggestions.ts", "liveSuggest.ts", "selection.ts", "events.ts"].map((f) => readFileSync(`src-console/takes/${f}`, "utf8"));
  it("never names a Home Assistant action or conversation command", () => {
    for (const src of files) expect(src).not.toMatch(/ha_call_service|ha_conversation|ha_container/);
  });
  it("reads Home Assistant only through ha_states", () => {
    expect(files.join("\n").match(/invokeBound<[^>]*>\("ha_[a-z_]+"/g)).toEqual(['invokeBound<HaState[]>("ha_states"']);
  });
  it("invokes no action-like command at all (call_service, turn_on/off, toggle, set_*)", () => {
    const names = [...files.join("\n").matchAll(/invokeBound<[^>]*>\("([a-z_]+)"/g)].map((m) => m[1]);
    expect(names.length).toBeGreaterThan(0);
    for (const n of names) expect(n).not.toMatch(/call_service|turn_on|turn_off|toggle|^set_|conversation|container_(start|stop)|transfer|pay|trade|withdraw/);
  });
});

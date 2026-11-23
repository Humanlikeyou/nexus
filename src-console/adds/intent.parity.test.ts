import { describe, expect, it } from "vitest";
import fixture from "../../test/fixtures/add-intents.json";
import { detectAddIntent, isCancel, isConfirm } from "./intent";

/** The Rust port (src-tauri adds.rs detect_add_intent / is_add_confirm / is_add_cancel) reads the same fixture. */
describe("add intents match the shared fixture", () => {
  it.each(fixture.intents.map((c) => [c.text.slice(0, 60), c] as const))("intent %j", (_n, c) => {
    expect(detectAddIntent(c.text)).toEqual(c.intent);
  });
  it.each(fixture.turns.map((c) => [c.text, c] as const))("confirm/cancel %j", (_n, c) => {
    expect(isConfirm(c.text)).toBe(c.confirm);
    expect(isCancel(c.text)).toBe(c.cancel);
  });
});

import { describe, expect, it } from "vitest";
import { parseNavyPrompt } from "./parse";

describe("parseNavyPrompt", () => {
  it("turns on for the approved phrase, with or without a comma or period", () => {
    for (const q of ["Amara, go dark.", "amara go dark", "Amara, go dark", "  AMARA, GO DARK!  "]) {
      expect(parseNavyPrompt(q)).toBe("on");
    }
  });
  it("accepts a bare 'go dark' and 'back to normal'", () => {
    expect(parseNavyPrompt("go dark")).toBe("on");
    expect(parseNavyPrompt("Amara, back to normal.")).toBe("off");
    expect(parseNavyPrompt("back to normal")).toBe("off");
  });
  it("does not match nearby phrases", () => {
    for (const q of ["don't go dark", "go darker", "go dark later", "back to normal tomorrow", "open TradingView", ""]) {
      expect(parseNavyPrompt(q)).toBeNull();
    }
  });
});

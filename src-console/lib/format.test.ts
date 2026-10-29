import { describe, expect, it } from "vitest";
import { date, dateTime, humanize, time } from "./format";

// test:unit runs with TZ=Asia/Tokyo. 2026-10-28T04:38Z = Oct 28, 1:38 PM in Tokyo.
const at = (iso: string) => Date.parse(iso);

describe("time / date / dateTime (Asia/Tokyo)", () => {
  it("runs in Tokyo", () => expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe("Asia/Tokyo"));

  it("time(): numeric hour, no leading zero, 2-digit minute", () => {
    expect(time(at("2026-10-28T12:00:00Z"))).toBe("9:00 PM");
    expect(time(at("2026-10-28T04:38:00Z"))).toBe("1:38 PM");
    expect(time(at("2026-10-28T00:05:00Z"))).toBe("9:05 AM");
    expect(time("2026-10-28T13:40:00+09:00")).toBe("1:40 PM");
    expect(time(1_793_000_000)).toBe("4:33 PM"); // seconds are accepted
    expect(time(null)).toBe("--");
    expect(time("nope")).toBe("--");
  });

  it("date(): short month, numeric day; the Tokyo calendar day, not UTC", () => {
    expect(date(at("2026-10-28T04:38:00Z"))).toBe("Oct 28");
    expect(date(at("2026-10-28T15:30:00Z"))).toBe("Oct 29"); // 00:30 JST on the 29th
    expect(date(new Date(2026, 6, 6))).toBe("Jul 6");
    expect(date(undefined)).toBe("No record");
    expect(date("not a date")).toBe("not a date");
  });

  it("dateTime(): 'Oct 28, 1:38 PM', never ' at ' or a leading zero", () => {
    const s = dateTime(at("2026-10-28T04:38:00Z"));
    expect(s).toBe("Oct 28, 1:38 PM");
    expect(s).not.toContain(" at ");
    expect(dateTime("2026-10-28T13:40:00+09:00")).toBe("Oct 28, 1:40 PM");
    expect(dateTime(at("2026-10-28T23:05:00Z"))).toBe("Oct 29, 8:05 AM");
    expect(dateTime(0)).toBe("No record");
  });
});

describe("humanize", () => {
  it.each([
    ["qa", "QA"],
    ["api_service", "API service"],
    ["e_commerce", "E-commerce"],
    ["daily_brief", "Daily brief"],
    ["ui_review", "UI review"],
    ["seo-audit", "SEO audit"],
    ["local_llm", "Local LLM"],
    ["ai", "AI"],
    ["open_app", "Open app"],
    ["mac_action", "Mac action"],
    ["aid_station", "Aid station"], // "ai" only as a whole word
    ["quality", "Quality"],
    ["skipped", "Skipped"],
    [null, "Unknown"],
  ])("%s → %s", (input, expected) => expect(humanize(input)).toBe(expected));
});

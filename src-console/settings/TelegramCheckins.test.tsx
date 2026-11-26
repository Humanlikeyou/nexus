import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { CHECKIN_COPY, TelegramCheckins, lastAppliedLine, sentTodayLine, type InboxStatus, type ProactiveSettings } from "./TelegramCheckins";

const settings = (): ProactiveSettings => ({ checkin: { enabled: true, at: "22:00" }, habits: { enabled: true }, money: { enabled: true, weekday: "Sun", at: "19:00" } });
const status = (p: Partial<InboxStatus> = {}): InboxStatus => ({ done: 2, rejected: 0, lastApplied: { kind: "checkin", at: "2026-11-25T09:00:00+09:00" }, lastRejectedReason: null, sentToday: [{ kind: "checkin", at: "2026-11-25T22:00:00+09:00", result: "sent" }], ...p });

function fake(over: Partial<Record<string, unknown>> = {}) {
  const call = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd in over) { const v = over[cmd]; if (v instanceof Error) throw v; return v; }
    if (cmd === "proactive_settings_get") return settings();
    if (cmd === "inbox_status") return status();
    if (cmd === "proactive_settings_set") return (args as { settings: ProactiveSettings }).settings;
    throw new Error(`unexpected ${cmd}`);
  });
  return call as unknown as <T>(c: string, a?: Record<string, unknown>) => Promise<T> & typeof call;
}

describe("TelegramCheckins", () => {
  it("shows the saved settings and a one-line status", async () => {
    render(<TelegramCheckins call={fake() as never} />);
    await waitFor(() => expect(screen.getByRole("switch", { name: CHECKIN_COPY.checkin.title })).not.toBeDisabled());
    expect(screen.getByRole("switch", { name: CHECKIN_COPY.checkin.title })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByLabelText("Check-in time")).toHaveValue("22:00");
    expect(screen.getByLabelText("Money day")).toHaveValue("Sun");
    expect(screen.getByText(/Last reply applied: check-in, Nov 25/)).toBeInTheDocument();
    expect(screen.getByText(/Sent today: check-in/)).toBeInTheDocument();
  });

  it("is disabled until the settings load and when they can't be read", async () => {
    render(<TelegramCheckins call={fake({ proactive_settings_get: new Error("x"), inbox_status: new Error("x") }) as never} />);
    await Promise.resolve();
    expect(screen.getByRole("switch", { name: CHECKIN_COPY.habits.title })).toBeDisabled();
    expect(screen.getByText(CHECKIN_COPY.footer)).toBeInTheDocument();
  });

  it("saves a toggle with the whole settings object", async () => {
    const call = fake();
    render(<TelegramCheckins call={call as never} />);
    await waitFor(() => expect(screen.getByRole("switch", { name: CHECKIN_COPY.habits.title })).not.toBeDisabled());
    fireEvent.click(screen.getByRole("switch", { name: CHECKIN_COPY.habits.title }));
    await waitFor(() => expect(screen.getByRole("switch", { name: CHECKIN_COPY.habits.title })).toHaveAttribute("aria-checked", "false"));
    const mock = call as unknown as { mock: { calls: unknown[][] } };
    const set = mock.mock.calls.find((c) => c[0] === "proactive_settings_set");
    expect(set?.[1]).toEqual({ settings: { ...settings(), habits: { enabled: false } } });
  });

  it("saves a new check-in time and the money weekday", async () => {
    const call = fake();
    render(<TelegramCheckins call={call as never} />);
    await waitFor(() => expect(screen.getByLabelText("Check-in time")).not.toBeDisabled());
    fireEvent.change(screen.getByLabelText("Check-in time"), { target: { value: "21:15" } });
    fireEvent.change(screen.getByLabelText("Money day"), { target: { value: "Fri" } });
    const mock = call as unknown as { mock: { calls: unknown[][] } };
    await waitFor(() => expect(mock.mock.calls.filter((c) => c[0] === "proactive_settings_set")).toHaveLength(2));
    const sets = mock.mock.calls.filter((c) => c[0] === "proactive_settings_set").map((c) => (c[1] as { settings: ProactiveSettings }).settings);
    expect(sets[0].checkin.at).toBe("21:15");
    expect(sets[1].money.weekday).toBe("Fri");
  });

  it("says so when a save fails and keeps what the backend has", async () => {
    render(<TelegramCheckins call={fake({ proactive_settings_set: new Error("invalid input") }) as never} />);
    await waitFor(() => expect(screen.getByRole("switch", { name: CHECKIN_COPY.money.title })).not.toBeDisabled());
    fireEvent.click(screen.getByRole("switch", { name: CHECKIN_COPY.money.title }));
    await waitFor(() => expect(screen.getByText(CHECKIN_COPY.error)).toBeInTheDocument());
    expect(screen.getByRole("switch", { name: CHECKIN_COPY.money.title })).toHaveAttribute("aria-checked", "true");
  });
});

describe("status lines", () => {
  it("covers none, held back and failed", () => {
    expect(lastAppliedLine(status({ lastApplied: null }))).toBe("No Telegram reply applied yet");
    expect(sentTodayLine(status({ sentToday: [] }))).toBe("Nothing sent today");
    expect(sentTodayLine(status({ sentToday: [{ kind: "money", at: "x", result: "suppressed" }] }))).toBe("Nothing sent today, held back while outbound is off");
    expect(sentTodayLine(status({ sentToday: [{ kind: "habit", at: "x", result: "sent" }, { kind: "checkin", at: "x", result: "failed" }] }))).toBe("Sent today: habits, one didn't send");
    expect(lastAppliedLine(null)).toBe("");
  });
});

import { useCallback, useEffect, useState } from "react";
import { invokeBound } from "../bridge";
import { InsetGroup, ListRow, Select, TextField, Toggle } from "../ui";

/** Mirrors proactive.rs (ProactiveSettings) and inbox.rs (InboxStatus). */
export type ProactiveSettings = {
  checkin: { enabled: boolean; at: string };
  habits: { enabled: boolean };
  money: { enabled: boolean; weekday: string; at: string };
};
export type InboxStatus = {
  done: number;
  rejected: number;
  lastApplied: { kind: string; at: string } | null;
  lastRejectedReason: string | null;
  sentToday: Array<{ kind: string; at: string; result: string }>;
};
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export const CHECKIN_COPY = {
  title: "Check-ins on Telegram",
  footer: "Amara asks on Telegram and saves your reply in the app. Questions are fixed text and never include your numbers.",
  checkin: { title: "Daily check-in", subtitle: "Asks how you are if you haven't checked in by this time (JST)." },
  habits: { title: "Habit reminders", subtitle: "One message a day for habits not marked after their reminder time." },
  money: { title: "Weekly money check-in", subtitle: "Asks for your balance and anything new, once a week (JST)." },
  error: "Couldn't save that. Try again.",
};
const KIND_LABEL: Record<string, string> = { checkin: "check-in", habit: "habits", money: "money" };
const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map((d) => ({ value: d, label: d }));
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;

/** "Last reply applied: check-in, Nov 25" or a plain none. */
export function lastAppliedLine(s: InboxStatus | null): string {
  if (!s) return "";
  if (!s.lastApplied) return "No Telegram reply applied yet";
  const d = new Date(s.lastApplied.at);
  const when = Number.isNaN(d.getTime()) ? "" : `, ${d.toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "Asia/Tokyo" })}`;
  return `Last reply applied: ${KIND_LABEL[s.lastApplied.kind] ?? s.lastApplied.kind}${when}`;
}

/** "Sent today: check-in" (a suppressed one says so: outbound is off, nothing was sent). */
export function sentTodayLine(s: InboxStatus | null): string {
  if (!s) return "";
  if (s.sentToday.length === 0) return "Nothing sent today";
  const sent = s.sentToday.filter((x) => x.result === "sent").map((x) => KIND_LABEL[x.kind] ?? x.kind);
  const held = s.sentToday.filter((x) => x.result === "suppressed").length;
  const failed = s.sentToday.filter((x) => x.result === "failed").length;
  const parts = [sent.length ? `Sent today: ${sent.join(", ")}` : "Nothing sent today"];
  if (held) parts.push("held back while outbound is off");
  if (failed) parts.push("one didn't send");
  return parts.join(", ");
}

/** Settings › Check-ins on Telegram. Saves each change at once through proactive_settings_set. */
export function TelegramCheckins({ call = invokeBound as Call }: { call?: Call }) {
  const [s, setS] = useState<ProactiveSettings | null>(null);
  const [status, setStatus] = useState<InboxStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [times, setTimes] = useState<{ checkin: string; money: string } | null>(null);

  const load = useCallback(async () => {
    const [a, b] = await Promise.allSettled([call<ProactiveSettings>("proactive_settings_get"), call<InboxStatus>("inbox_status")]);
    if (a.status === "fulfilled" && a.value?.checkin) { setS(a.value); setTimes({ checkin: a.value.checkin.at, money: a.value.money.at }); }
    if (b.status === "fulfilled" && b.value && Array.isArray(b.value.sentToday)) setStatus(b.value);
  }, [call]);
  // eslint-disable-next-line react-hooks/set-state-in-effect -- one read of an external source on mount
  useEffect(() => { void load(); }, [load]);

  const save = async (next: ProactiveSettings) => {
    setError(null);
    try {
      const saved = await call<ProactiveSettings>("proactive_settings_set", { settings: next });
      setS(saved);
      setTimes({ checkin: saved.checkin.at, money: saved.money.at });
    } catch { setError(CHECKIN_COPY.error); await load(); }
  };
  const ready = s != null;
  const footer = error ?? ([lastAppliedLine(status), sentTodayLine(status)].filter(Boolean).join(" · ") || CHECKIN_COPY.footer);
  const commitTime = (key: "checkin" | "money", v: string) => {
    if (!s || !TIME.test(v) || v === s[key].at) return;
    void save({ ...s, [key]: { ...s[key], at: v } });
  };
  return (
    <InsetGroup title={CHECKIN_COPY.title} footer={footer}>
      <ListRow title={CHECKIN_COPY.checkin.title} subtitle={CHECKIN_COPY.checkin.subtitle}
        accessory={<Toggle aria-label={CHECKIN_COPY.checkin.title} checked={s?.checkin.enabled ?? false} disabled={!ready}
          onCheckedChange={(v) => { if (s) void save({ ...s, checkin: { ...s.checkin, enabled: v } }); }} />} />
      <ListRow title="Check-in time" subtitle="Japan time"
        accessory={<TextField label="Check-in time" hideLabel type="time" value={times?.checkin ?? "22:00"} disabled={!ready}
          onChange={(e) => { const v = e.target.value; setTimes((t) => (t ? { ...t, checkin: v } : t)); commitTime("checkin", v); }} />} />
      <ListRow title={CHECKIN_COPY.habits.title} subtitle={CHECKIN_COPY.habits.subtitle}
        accessory={<Toggle aria-label={CHECKIN_COPY.habits.title} checked={s?.habits.enabled ?? false} disabled={!ready}
          onCheckedChange={(v) => { if (s) void save({ ...s, habits: { enabled: v } }); }} />} />
      <ListRow title={CHECKIN_COPY.money.title} subtitle={CHECKIN_COPY.money.subtitle}
        accessory={<Toggle aria-label={CHECKIN_COPY.money.title} checked={s?.money.enabled ?? false} disabled={!ready}
          onCheckedChange={(v) => { if (s) void save({ ...s, money: { ...s.money, enabled: v } }); }} />} />
      <ListRow title="Money day" subtitle="Japan time"
        accessory={<Select label="Money day" hideLabel options={WEEKDAYS} value={s?.money.weekday ?? "Sun"} disabled={!ready}
          onChange={(e) => { if (s) void save({ ...s, money: { ...s.money, weekday: e.target.value } }); }} />} />
      <ListRow title="Money time" subtitle="Japan time"
        accessory={<TextField label="Money time" hideLabel type="time" value={times?.money ?? "19:00"} disabled={!ready}
          onChange={(e) => { const v = e.target.value; setTimes((t) => (t ? { ...t, money: v } : t)); commitTime("money", v); }} />} />
    </InsetGroup>
  );
}

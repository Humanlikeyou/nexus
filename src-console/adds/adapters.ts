import { UNWIRED, sample, sampleAdaptersEnabled } from "../lib/sampleAdapters";
import { announceAdds, announceStores, inTauri, live, realInvoke, type Invoke } from "../lib/localStores";
import type { AddAdapter, AddPreview, HistoryItem, ParseResult, Saved } from "./flow";

/**
 * design-1111v §5 adds. Every one is a pure UNWIRED adapter until the backend command exists (HANDOFF-1111v §7),
 * so commands.registered.test.ts stays green and nothing is saved anywhere. Sample adapters are mock mode only,
 * and everything they return is shown with "Sample data".
 */
export type AddKind = "goal" | "date" | "task" | "idea";

/** The command each add will call. "life_date_upsert" is the name from HANDOFF-1111a; the rest are the Lead's call. */
export const ADD_COMMANDS: Record<AddKind, string> = { goal: "goal add command", date: "life_date_upsert", task: "task add command", idea: "content idea command" };

const unwired = (kind: AddKind): AddAdapter => ({
  command: ADD_COMMANDS[kind], wired: false,
  parse: async () => UNWIRED, save: async () => UNWIRED, undo: async () => UNWIRED, history: () => UNWIRED,
});

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const MONTH_NAME = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/** "Sept 14" / "September 14" / "14 Sep" → month index and day; null when there is no date in the text. */
export function findDay(text: string): { month: number; day: number } | null {
  const t = text.toLowerCase();
  const a = /\b(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?\s+(\d{1,2})\b/.exec(t);
  if (a) return { month: MONTHS.indexOf(a[1]), day: Number(a[2]) };
  const b = /\b(\d{1,2})\s+(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\b/.exec(t);
  return b ? { month: MONTHS.indexOf(b[2]), day: Number(b[1]) } : null;
}
const yearIn = (text: string, now: Date) => { const m = /\b(20\d\d)\b/.exec(text); if (m) return Number(m[1]); return /next year/i.test(text) ? now.getFullYear() + 1 : null; };
const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
const firstClause = (text: string) => capital(text.split(/,| remind me| for | by /i)[0].trim().replace(/\s*\(\d{4}\)$/, ""));

/** Sample reading for a date: asks for the year rather than guessing; business dates point to Enterprise. */
export function parseDateSample(text: string, now: Date): ParseResult {
  if (/taiyo|launch|venture|enterprise|tax filing/i.test(text)) {
    return { kind: "elsewhere", message: "Business dates live in Enterprise › Ventures › Taiyo Skin. Change it there and it updates here.", link: { id: "enterprise", view: "ventures/taiyo-skin" }, linkLabel: "Open Taiyo Skin ›" };
  }
  const day = findDay(text);
  if (!day) return { kind: "ask", question: "Which day is it?", options: [], note: "Type the date, e.g. Sept 14. Amara doesn't guess legal dates." };
  const year = yearIn(text, now);
  if (year == null) return { kind: "ask", question: "Which year is it?", options: [String(now.getFullYear() + 1), String(now.getFullYear() + 2)], note: "Amara doesn't guess legal dates." };
  const lane = /visa|residence|immigration|zairyu/i.test(text) ? "Japan · residence" : "Personal";
  const months = /(\d+)\s*months?\s*before/i.exec(text);
  const when = new Date(Date.UTC(year, day.month, day.day));
  const weekday = when.toLocaleDateString("en-US", { weekday: "short", timeZone: "UTC" });
  const remind = months ? new Date(Date.UTC(year, day.month - Number(months[1]), day.day)) : null;
  return { kind: "preview", preview: {
    title: firstClause(text),
    fields: [["Date", `${weekday}, ${MONTH_NAME[day.month]} ${day.day}, ${year}`], ["Lane", lane],
      ["Reminders", remind ? `${remind.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric", timeZone: "UTC" })} (${months![1]} months) · then 30 and 7 days before` : "90, 30 and 7 days before"], ["From", "You typed it"]],
    note: "Reminders show in Needs you today and as a macOS notification.",
  } };
}

export function parseTaskSample(text: string): ParseResult {
  const venture = /taiyo/i.test(text) ? "Taiyo Skin" : /amara/i.test(text) ? "Amara OS" : "No venture yet";
  const due = /\bby (mon|tue|wed|thu|fri|sat|sun)[a-z]*/i.exec(text);
  return { kind: "preview", preview: { title: firstClause(text), fields: [["Venture", venture], ["Due", due ? capital(due[1].toLowerCase()) : "No date"], ["Status", "Backlog"], ["Also shows", due ? "Needs you today on the due date" : "Only here"]] } };
}

export function parseIdeaSample(text: string): ParseResult {
  const venture = /taiyo/i.test(text) ? "Taiyo Skin" : /amara/i.test(text) ? "Amara OS" : "No venture yet";
  const type = /reel|video/i.test(text) ? "Short video" : /thread/i.test(text) ? "Thread" : /blog|post/i.test(text) ? "Blog post" : "Idea";
  return { kind: "preview", preview: { title: firstClause(text.replace(/^(reel|thread|post|blog):\s*/i, "")), fields: [["Type", type], ["Venture", venture], ["Lands in", "Idea"]], note: "Amara drafts; she never posts for you." } };
}

export function parseGoalSample(text: string): ParseResult {
  const per = /(\d+)\s*(nights|days|times)\s*a\s*week/i.exec(text);
  const dur = /(\d+)\s*(min|minutes|h|hours)/i.exec(text);
  return { kind: "preview", preview: { title: firstClause(text.replace(/\s*\d+\s*(min|minutes)\b/i, "")).replace(/\s+before/i, " before"),
    fields: [["Target", [dur ? `${dur[1]} min` : null, per ? `${per[1]} ${per[2]} a week` : "daily"].filter(Boolean).join(" · ")], ["Counts from", "You, one tap a day (no data source on this Mac)"], ["Starts", "Next Monday"], ["Shows in", "This week's habits · Weekly review"]] } };
}

const HISTORY: Record<AddKind, HistoryItem[]> = {
  goal: [{ id: "h1", when: "Oct 26", text: "Made Asleep by 23:30 a habit", via: "Weekly review" }, { id: "h2", when: "Oct 12", text: "Paused Cold shower", via: "voice" }],
  date: [{ id: "h1", when: "Oct 20", text: "Added Residence card expires · Sep 14", via: "typed" }, { id: "h2", when: "Oct 3", text: "Moved Lease renewal to Apr 1", via: "chat" }],
  task: [{ id: "h1", when: "Oct 26", text: "Done: Send supplier the label proof", via: "typed" }, { id: "h2", when: "Oct 24", text: "Added Plan next quarter", via: "chat" }],
  idea: [{ id: "h1", when: "Oct 19", text: "Published Formula teaser", via: "typed" }, { id: "h2", when: "Oct 15", text: "Added Routine reel", via: "voice" }],
};
const ALSO: Record<AddKind, Saved["also"]> = {
  goal: { label: "Weekly review ›", link: { id: "life", view: "weekly-review" } },
  date: { label: "Needs you today ›", link: { id: "terrance", view: "needs-you" } },
  task: { label: "Needs you today ›", link: { id: "terrance", view: "needs-you" } },
  idea: undefined,
};

function sampleAdd(kind: AddKind, parse: (text: string) => ParseResult): AddAdapter {
  const history = [...HISTORY[kind]];
  let n = 0;
  return {
    command: ADD_COMMANDS[kind], wired: true,
    parse: async (text) => sample(parse(text)),
    save: async (p: AddPreview) => { const id = `${kind}-new-${++n}`; history.unshift({ id, when: "Just now", text: `Added ${p.title}`, via: "typed" }); return sample({ id, title: p.title, also: ALSO[kind] }); },
    undo: async (id) => { const i = history.findIndex((h) => h.id === id); if (i >= 0) history.splice(i, 1); return sample({ ok: true as const }); },
    history: () => sample([...history]),
  };
}

/**
 * The real write path (src-tauri adds.rs): add_preview → add_save → add_undo (≤ 8 s) and add_history. Typed, chat
 * and voice all call this one adapter. wired flips true after the first History read succeeds, so a missing backend
 * stays "Needs wiring". Failed calls reject; the Composer keeps the text and shows Try again.
 */
export function makeLiveAdd(kind: AddKind | "habit-done", call: Invoke = realInvoke): AddAdapter & { probe(): Promise<void> } {
  let ok = false;
  let items: HistoryItem[] = [];
  /** "I did my stretch" files a habit day: History and Undo live under kind "habit" (habits.rs `mark_in`). */
  const logKind = kind === "habit-done" ? "habit" : kind;
  const probe = async () => {
    try { items = await call<HistoryItem[]>("add_history", { kind: logKind }); ok = true; } catch { ok = false; }
    announceStores();
  };
  return {
    command: kind === "habit-done" ? "habit_mark" : ADD_COMMANDS[kind],
    get wired() { return ok; },
    probe,
    parse: async (text) => live(await call<ParseResult>("add_preview", { kind, text })),
    save: async (preview: AddPreview, via = "typed") => {
      const saved = await call<Saved>("add_save", { kind, preview, via });
      await probe();
      announceAdds();
      return live(saved);
    },
    undo: async (id) => {
      const done = await call<boolean>("add_undo", { kind: logKind, id });
      if (!done) throw new Error("The 8 second undo window has passed.");
      await probe();
      announceAdds();
      return live({ ok: true as const });
    },
    history: () => (ok ? live([...items]) : UNWIRED),
  };
}

const samples = new Map<AddKind, AddAdapter>();
const lives = new Map<AddKind | "habit-done", AddAdapter>();
/** Chat and voice habit marking. No sample mode: a sample can't mark a real habit, so it says it needs wiring. */
export function habitDoneAdapter(): AddAdapter {
  if (sampleAdaptersEnabled() || !inTauri()) return { command: "habit_mark", wired: false, parse: async () => UNWIRED, save: async () => UNWIRED, undo: async () => UNWIRED, history: () => UNWIRED };
  let l = lives.get("habit-done");
  if (!l) { const made = makeLiveAdd("habit-done"); l = made; lives.set("habit-done", l); void made.probe(); }
  return l;
}
/** The one write path for an add (HANDOFF-1111v §5): the page composer, chat and voice all call this. */
export function addAdapter(kind: AddKind, now: () => Date = () => new Date()): AddAdapter {
  if (!sampleAdaptersEnabled()) {
    if (!inTauri()) return unwired(kind);
    let l = lives.get(kind);
    if (!l) { const made = makeLiveAdd(kind); l = made; lives.set(kind, l); void made.probe(); }
    return l;
  }
  let a = samples.get(kind);
  if (!a) {
    const parse = kind === "date" ? (t: string) => parseDateSample(t, now()) : kind === "task" ? parseTaskSample : kind === "idea" ? parseIdeaSample : parseGoalSample;
    a = sampleAdd(kind, parse);
    samples.set(kind, a);
  }
  return a;
}

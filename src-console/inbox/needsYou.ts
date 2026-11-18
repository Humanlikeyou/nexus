import type { Route } from "../app/routes";
import type { FundingRequest, TaxFlag } from "../business/adapters";
import { dayLabel, dueState } from "../lib/followups";
import type { LifeAdmin } from "../personal/adapters";
import type { FollowupLists, TerrenceAction } from "../types";

/**
 * design-1111v §3: "Needs you today" is the single inbox and it only lists. Every row is a pointer
 * (source, id, title, due, reason, deepLink) to an item that lives in exactly one source; no inbox-only items
 * and no actions except "Open in … ›". Pure merge here; the hook and rows are in NeedsYouToday.tsx.
 */
/** ListsScreen announces every fresh read so the inbox follows To-dos without a second poll. */
export const TODOS_EVENT = "amara:todos";

export type InboxSource = "todos" | "dates" | "checkin" | "decisions" | "approvals" | "crew" | "strap";
export const SOURCE_LABEL: Record<InboxSource, string> = { todos: "To-dos", dates: "Dates & deadlines", checkin: "Check-in", decisions: "Decisions", approvals: "Amara approvals", crew: "Crew approvals", strap: "Strap" };
export const OPEN_LABEL: Record<InboxSource, string> = { todos: "Open in To-dos ›", dates: "Open in Dates & deadlines ›", checkin: "Go to Check-in ↓", decisions: "Open in Decisions ›", approvals: "Open on Home ›", crew: "Open in Agents › Crews ›", strap: "Open in Mind–Body ›" };
/** What each source needs before it can list (HANDOFF-1111v §7). */
export const UNWIRED_REASON: Record<InboxSource, string> = { todos: "Needs wiring · follow-ups", dates: "Needs wiring · life dates store", checkin: "Needs wiring · check-in store", decisions: "Needs wiring · business ledger and decisions log", approvals: "Needs wiring", crew: "Needs wiring · crew approvals (R13-D)", strap: "Needs wiring · NOOP store" };

export type InboxPointer = {
  source: InboxSource;
  id: string;
  title: string;
  /** ISO day or a label like "Oct 31"; null when it has no date. */
  due: string | null;
  reason: string;
  deepLink: Route;
  badge?: { tone: "warning" | "accent" | "neutral"; label: string };
  /** Shown all day but quietly (the evening check-in, Taisun 19:23 JST); not counted in "N things need you". */
  quiet?: boolean;
};
export type SourceState = { source: InboxSource; state: "loading" | "ok" | "failed" | "unwired"; rows: InboxPointer[]; sample?: boolean };

const ORDER: InboxSource[] = ["todos", "decisions", "approvals", "crew", "strap", "dates", "checkin"];
const urgency = (p: InboxPointer) => (p.quiet ? 3 : p.badge?.tone === "warning" ? 0 : p.badge?.tone === "accent" ? 1 : 2);

export type Inbox = { rows: InboxPointer[]; count: number; loading: boolean; failed: InboxSource[]; unwired: InboxSource[]; sample: boolean; sources: number };

export function mergeInbox(sources: SourceState[]): Inbox {
  const ready = sources.filter((s) => s.state === "ok");
  const rows = ready.flatMap((s) => s.rows).sort((a, b) => urgency(a) - urgency(b) || ORDER.indexOf(a.source) - ORDER.indexOf(b.source));
  return {
    rows,
    count: rows.filter((r) => !r.quiet).length,
    loading: sources.some((s) => s.state === "loading"),
    failed: sources.filter((s) => s.state === "failed").map((s) => s.source),
    unwired: sources.filter((s) => s.state === "unwired").map((s) => s.source),
    sample: sources.some((s) => s.state === "ok" && s.sample && s.rows.length > 0),
    sources: new Set(rows.map((r) => r.source)).size,
  };
}

/** To-dos: overdue or due today. */
export function todoPointers(lists: FollowupLists, today: string): InboxPointer[] {
  return lists.open.flatMap((item) => {
    const due = dueState(item.due, today);
    if (due !== "overdue" && due !== "today") return [];
    return [{ source: "todos", id: `todo-${item.id}`, title: item.text, due: item.due, reason: due === "overdue" ? `Overdue since ${dayLabel(item.due as string)}` : "Due today",
      deepLink: { id: "terrance", view: "lists" }, badge: due === "overdue" ? { tone: "warning", label: "Overdue" } : { tone: "accent", label: "Due today" } } satisfies InboxPointer];
  });
}

/** Dates & deadlines: inside a reminder window. Enterprise dates are listed by Enterprise, not here twice. */
export function datePointers(admin: LifeAdmin): InboxPointer[] {
  return admin.dates.filter((d) => d.due && d.lane !== "enterprise").map((d) => ({
    source: "dates", id: `date-${d.id}`, title: d.title, due: d.date, reason: [d.date, d.inDays != null ? `in ${d.inDays} days` : null, `${d.reminder.split(" · ")[0]} reminder`].filter(Boolean).join(" · "),
    deepLink: { id: "life", view: "dates" },
  }));
}

/** Pending decisions: open votes and tax flags that have no record in the decisions log yet. */
export function decisionPointers(votes: FundingRequest[], flags: TaxFlag[], decided: Set<string>): InboxPointer[] {
  return [
    ...votes.filter((v) => v.status === "open" && !decided.has(v.id)).map((v) => ({ source: "decisions" as const, id: `vote-${v.id}`, title: `${v.requester} ${v.purpose.replace(/ \(.*\)$/, "").toLowerCase()} · funding vote`, due: null, reason: `Proposed by the ${v.requester} crew`, deepLink: { id: "enterprise" as const, view: "decisions" } })),
    ...flags.filter((t) => !decided.has(t.id)).map((t) => ({ source: "decisions" as const, id: `flag-${t.id}`, title: `Tax flag · ${t.label.charAt(0).toLowerCase()}${t.label.slice(1)}`, due: null, reason: `${t.area} · ${t.date} · from the business ledger`, deepLink: { id: "enterprise" as const, view: "decisions" } })),
  ];
}

/** Amara's own actions waiting for an OK; they are approved on Home, where they were proposed. */
export function approvalPointers(actions: TerrenceAction[]): InboxPointer[] {
  return actions.filter((a) => a.status === "awaiting_approval").map((a) => ({ source: "approvals", id: `action-${a.id}`, title: a.title, due: null, reason: "Waiting for your OK · nothing runs until you approve", deepLink: { id: "terrance", view: "main" } }));
}

/** The evening check-in: a quiet row all day until it is done (Taisun's default, 19:23 JST). */
export function checkinPointer(doneToday: boolean, reminder: string): InboxPointer[] {
  return doneToday ? [] : [{ source: "checkin", id: "checkin-today", title: "Evening check-in", due: null, reason: `Not done yet today · reminder at ${reminder}`, deepLink: { id: "terrance", view: "checkin" }, quiet: true }];
}

/** To-do rows that were open last time and are now in recently done: they leave with "Done in To-dos". */
export function doneAtSource(previous: InboxPointer[], lists: FollowupLists): InboxPointer[] {
  const done = new Set(lists.recentlyDone.map((i) => `todo-${i.id}`));
  return previous.filter((p) => p.source === "todos" && done.has(p.id));
}

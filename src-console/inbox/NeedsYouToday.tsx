import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Route } from "../app/routes";
import { LinkButton, OffButton, ResultLine, WorkingLine } from "../adds/Composer";
import { listFollowups } from "../bridge";
import { businessAdapter } from "../business/adapters";
import { DECISIONS_EVENT, decisionsLog } from "../decisions/log";
import { todayIso } from "../lib/followups";
import { inTauri, needsYouList, STORES_EVENT, type NeedsYouReport } from "../lib/localStores";
import { UNWIRED, sample, sampleAdaptersEnabled, valueOf, type Wired } from "../lib/sampleAdapters";
import { CHECKIN_EVENT, PERSONAL_DEFAULTS, personalAdapter } from "../personal/adapters";
import type { FollowupLists, SystemSnapshot } from "../types";
import { Badge, Button, Card, DataTag } from "../ui";
import { FOCUS_ACTION_EVENT, OPEN_LABEL, SOURCE_LABEL, TODOS_EVENT, UNWIRED_REASON, approvalPointers, checkinPointer, datePointers, decisionPointers, doneAtSource, mergeInbox, todoPointers, type Inbox, type InboxPointer, type InboxSource, type SourceState } from "./needsYou";
import "../adds/v1111.css";


/** Crew approvals (R13-D) as list pointers: the Lead's ask §7.4. Sample in mock mode only. */
export function crewApprovals(): Wired<InboxPointer[]> {
  if (!sampleAdaptersEnabled()) return UNWIRED;
  return sample([{ source: "crew", id: "crew-r1", title: "Launch post draft from the Taiyo Skin crew", due: null, reason: "Waiting for your OK since 11:20 · nothing is posted until you approve", deepLink: { id: "chat", view: "crews" } }]);
}

/** The evening check-in is done today when a check-in saved today is known (sample: "Just now"). */
function checkinDone(): boolean {
  const c = valueOf(personalAdapter().checkins());
  return Boolean(c?.some((x) => x.when === "Just now"));
}

export type NeedsYou = Inbox & { states: SourceState[]; done: InboxPointer[]; retry: () => void };

/** The five places Needs you today reads (plus Amara's own approvals), merged in the UI (HANDOFF-1111v §7.2). */
export function useNeedsYou(data: SystemSnapshot, now: number): NeedsYou {
  const [todos, setTodos] = useState<{ state: "loading" | "ok" | "failed"; lists: FollowupLists | null }>({ state: "loading", lists: null });
  const [done, setDone] = useState<InboxPointer[]>([]);
  const [tick, setTick] = useState(0);
  // Live backend (needs_you_list): dates, check-in, decisions and crew approvals come from one read-only aggregator.
  // null while it loads or when the backend has no such command (then the UI merge below stays in charge).
  const [agg, setAgg] = useState<NeedsYouReport[] | null>(null);
  const prev = useRef<InboxPointer[]>([]);
  const today = todayIso(new Date(now));

  const take = useCallback((lists: FollowupLists) => {
    const gone = doneAtSource(prev.current, lists);
    if (gone.length) setDone(gone);
    prev.current = todoPointers(lists, today);
    setTodos({ state: "ok", lists });
  }, [today]);
  const load = useCallback(() => { listFollowups().then(take, () => setTodos((t) => ({ state: "failed", lists: t.lists }))); }, [take]);
  useEffect(() => { load(); }, [load, data.refreshedAt]);
  const useAgg = inTauri() && !sampleAdaptersEnabled();
  useEffect(() => {
    if (!useAgg) return;
    let live = true;
    needsYouList().then((r) => { if (live) setAgg(r); }, () => { if (live) setAgg(null); });
    return () => { live = false; };
  }, [useAgg, data.refreshedAt, tick]);
  useEffect(() => {
    const onTodos = (e: Event) => { const lists = (e as CustomEvent<FollowupLists>).detail; if (lists) take(lists); };
    const onDecisions = () => setTick((n) => n + 1);
    window.addEventListener(TODOS_EVENT, onTodos);
    window.addEventListener(DECISIONS_EVENT, onDecisions);
    window.addEventListener(CHECKIN_EVENT, onDecisions);
    window.addEventListener(STORES_EVENT, onDecisions);
    return () => { window.removeEventListener(TODOS_EVENT, onTodos); window.removeEventListener(DECISIONS_EVENT, onDecisions); window.removeEventListener(CHECKIN_EVENT, onDecisions); window.removeEventListener(STORES_EVENT, onDecisions); };
  }, [take]);

  const states = useMemo<SourceState[]>(() => {
    void tick;
    const personal = personalAdapter();
    const admin = personal.lifeAdmin();
    const biz = businessAdapter(data);
    const votes = biz.fundingRequests(), flags = biz.taxFlags();
    const log = valueOf(decisionsLog().list()) ?? [];
    const decided = new Set(log.map((r) => r.itemId));
    const crew = crewApprovals();
    const fromAgg = (source: NeedsYouReport["source"]): SourceState | null => {
      const r = agg?.find((x) => x.source === source);
      return r ? { source: r.source as InboxSource, state: r.state, rows: r.rows as InboxPointer[] } : null;
    };
    return [
      { source: "todos", state: todos.state === "ok" ? "ok" : todos.state, rows: todos.lists ? todoPointers(todos.lists, today) : [] },
      fromAgg("dates") ?? (admin.state === "unwired" ? { source: "dates", state: "unwired", rows: [] } : { source: "dates", state: "ok", rows: datePointers(admin.value), sample: admin.state === "sample" }),
      fromAgg("checkin") ?? { source: "checkin", state: "ok", rows: checkinPointer(checkinDone(), PERSONAL_DEFAULTS.checkInReminder) },
      fromAgg("decisions") ?? (votes.state === "unwired" && flags.state === "unwired" ? { source: "decisions", state: "unwired", rows: [] }
        : { source: "decisions", state: "ok", rows: decisionPointers(valueOf(votes) ?? [], valueOf(flags) ?? [], decided), sample: votes.state === "sample" || flags.state === "sample" }),
      { source: "approvals", state: data.unavailableSources.some((s) => s.command === "list_terrence_actions") ? "failed" : "ok", rows: approvalPointers(data.actions) },
      fromAgg("crew") ?? (crew.state === "unwired" ? { source: "crew", state: "unwired", rows: [] } : { source: "crew", state: "ok", rows: crew.value, sample: crew.state === "sample" }),
      ...(fromAgg("strap") ? [fromAgg("strap") as SourceState] : []),
      ...(fromAgg("watchlist") ? [fromAgg("watchlist") as SourceState] : []),
      ...(fromAgg("habits") ? [fromAgg("habits") as SourceState] : []),
    ];
  }, [data, todos, today, tick, agg]);
  const inbox = mergeInbox(states);
  // Try again re-reads every source: the follow-ups directly, and the aggregator (dates, check-in, decisions, crew, strap, watchlist, habits) through the tick.
  return { ...inbox, states, done, retry: () => { setDone([]); load(); setTick((n) => n + 1); } };
}

function InboxRow({ p, onOpen }: { p: InboxPointer; onOpen?: (route: Route) => void }) {
  return (
    <li data-source={p.source} data-quiet={p.quiet || undefined}>
      <span className="v-src">{SOURCE_LABEL[p.source]}</span>
      <span className="v-inbox__t"><b>{p.title} {p.badge && <Badge tone={p.badge.tone}>{p.badge.label}</Badge>}</b><span>{p.reason}</span></span>
      <LinkButton onClick={onOpen ? () => { if (p.source === "approvals") window.dispatchEvent(new CustomEvent(FOCUS_ACTION_EVENT, { detail: { id: p.id.replace(/^action-/, "") } })); onOpen(p.deepLink); } : undefined} label={`${OPEN_LABEL[p.source].replace(/ [›↓]$/, "")}: ${p.title}`}>{OPEN_LABEL[p.source]}</LinkButton>
    </li>
  );
}

const SKELETON = <li aria-hidden="true"><span className="v-skel" style={{ width: 89 }} /><span className="v-inbox__t"><span className="v-skel" style={{ width: "61.8%" }} /><span className="v-skel" style={{ width: "38.2%", height: 8 }} /></span><span className="v-skel" style={{ width: 89 }} /></li>;

/** Today › Needs you today. Rows only point; finishing an item happens at its source. */
export function NeedsYouToday({ inbox, onOpen }: { inbox: NeedsYou; onOpen?: (route: Route) => void }) {
  const { rows, loading, failed, unwired, done } = inbox;
  const admin = valueOf(personalAdapter().lifeAdmin());
  const next = admin?.dates.find((d) => !d.due && d.lane !== "enterprise");
  const firstLoad = loading && rows.filter((r) => !r.quiet).length === 0;
  const accessory = <span className="pk-acc">{inbox.sample && <DataTag kind="sample" />}<span className="v-note">{firstLoad ? "Loading" : `${inbox.count} from ${inbox.sources} place${inbox.sources === 1 ? "" : "s"}`}</span></span>;
  return (
    <section id="needs-you" className="page-section" data-section="needs-you">
      <Card title="Needs you today" accessory={accessory}>
        <div className="v-inbox" aria-busy={firstLoad || undefined}>
          <ul aria-label="Needs you today">
            {firstLoad && <>{SKELETON}{SKELETON}</>}
            {rows.map((p) => <InboxRow key={p.id} p={p} onOpen={onOpen} />)}
            {unwired.map((s) => (
              <li key={s} data-source={s} data-quiet>
                <span className="v-src">{SOURCE_LABEL[s]}</span>
                <span className="v-inbox__t"><b>{SOURCE_LABEL[s]} will show here</b><span>{UNWIRED_REASON[s]} <DataTag kind="wire" /></span></span>
                <OffButton reason="Needs wiring" label={`Open ${SOURCE_LABEL[s]}`}>Open</OffButton>
              </li>
            ))}
          </ul>
        </div>
        {firstLoad && <WorkingLine>Checking To-dos, Dates, Check-in, Decisions and approvals…</WorkingLine>}
        {!loading && inbox.count === 0 && failed.length === 0 && (
          <p className="v-note"><b>Nothing needs you today.</b>{next ? <> Next: {next.title} on {next.date} · </> : " "}{onOpen && <LinkButton small onClick={() => onOpen({ id: "life", view: "dates" })}>Dates &amp; deadlines ›</LinkButton>}</p>
        )}
        {done.map((p) => <ResultLine key={p.id} extra={onOpen && <LinkButton small onClick={() => onOpen(p.deepLink)}>{OPEN_LABEL[p.source]}</LinkButton>}>Done in {SOURCE_LABEL[p.source]}: &ldquo;{p.title}&rdquo;. It leaves this list now.</ResultLine>)}
        {failed.length > 0 && <ResultLine error extra={<Button size="small" onClick={inbox.retry}>Try again</Button>}>{failed.map((s) => SOURCE_LABEL[s]).join(" and ")} didn&apos;t load. The rest is current.</ResultLine>}
        <p className="v-note">Each item lives in one place; this list only points to it. Finish it there and it leaves here.</p>
      </Card>
    </section>
  );
}

/** weekly_review.rs gives at most one suggestion; weeklyReviewFrom maps none to this title (liveInsights.ts). */
const NO_CHANGE_TITLE = "No change to suggest this week";

/** Sundays: "Your weekly review is ready ›" under card C. Opens Life › Weekly review. */
export function WeeklyReadyCard({ onOpen }: { onOpen?: (route: Route) => void }) {
  const wr = personalAdapter().weeklyReview();
  const v = valueOf(wr);
  const changesToTry = v && v.change.title !== NO_CHANGE_TITLE ? 1 : 0;
  return (
    <button type="button" className="v-ready" disabled={!onOpen} onClick={() => onOpen?.({ id: "life", view: "weekly-review" })} aria-label="Your weekly review is ready. Open Life, Weekly review">
      <span style={{ flex: 1 }}>
        <span className="v-eyebrow">Sunday{v ? ` · ${v.week.split(" · ")[1] ?? ""}` : ""}</span>
        <span className="v-ready__t">Your weekly review is ready ›</span>
        <span className="v-ready__s">{v ? <>{v.title}{changesToTry > 0 && <> · {changesToTry} change to try</>} {wr.state === "sample" && <DataTag kind="sample" />}</> : <>Health, work and money from your week <DataTag kind="wire" /></>}</span>
      </span>
    </button>
  );
}

import { useEffect, useId, useState } from "react";
import type { Route } from "../app/routes";
import { addAdapter } from "../adds/adapters";
import { inTauri } from "../lib/localStores";
import { ContentItemTools } from "../design1118/EnterpriseWiring";
import type { ContentStage } from "../design1118/enterpriseLive";
import { Composer, LinkButton, OffButton, ResultLine } from "../adds/Composer";
import type { Saved } from "../adds/flow";
import { businessAdapter, type BusinessAdapter, type FundingRequest, type TaxFlag } from "../business/adapters";
import { DECISIONS_EVENT, announceDecisions, decisionsLog, jst, type DecisionChoice, type DecisionDraft, type DecisionRecord } from "../decisions/log";
import { humanize, money, plainText } from "../lib/format";
import { valueOf } from "../lib/sampleAdapters";
import type { SystemSnapshot } from "../types";
import { Badge, Button, Card, DataTag, InsetGroup, ListRow, PageHero, StatusText, wiredTag, type StatusKind } from "../ui";
import { waitingDecisions } from "./BusinessPanels";
import { PageTake } from "../design1118/PageTake";
import "./CommandCenter.css";
import "../adds/v1111.css";

const acc = (node: React.ReactNode) => <span className="cc-card__acc">{node}</span>;
type Task = SystemSnapshot["tasks"][number];

/** "1 in progress, 1 waiting": the headline state, and nothing else. */
export function workStatement(tasks: Task[]): string {
  const count = (status: string) => tasks.filter((task) => task.status === status).length;
  const active = count("active"), waiting = count("waiting");
  if (active === 0 && waiting === 0) return tasks.length ? "Nothing in progress" : "No tasks yet";
  return [active ? `${active} in progress` : "", waiting ? `${waiting} waiting` : ""].filter(Boolean).join(", ");
}

/** Re-reads the decisions log whenever a record is appended or undone. */
function useDecisionsTick() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const on = () => setTick((n) => n + 1);
    window.addEventListener(DECISIONS_EVENT, on);
    return () => window.removeEventListener(DECISIONS_EVENT, on);
  }, []);
}

type Pending = { item: FundingRequest | TaxFlag; kind: "vote" | "tax_flag"; choice: DecisionChoice };
const ventureOfRequester = (name: string) => (/taiyo/i.test(name) ? "taiyo-skin" : name.toLowerCase().replace(/\s+/g, "-"));

/** design-1111v §4: Enterprise › Decisions. Waiting votes and tax flags, and the separate decisions log.
 *  Recording a decision writes one log record. It never moves money and never contacts anyone. */
export function DecisionsView({ data, adapter = businessAdapter(data), onOpen }: { data: SystemSnapshot; adapter?: BusinessAdapter; onOpen?: (route: Route) => void }) {
  useDecisionsTick();
  const log = decisionsLog();
  const records = log.list();
  const w = waitingDecisions(adapter);
  const rules = adapter.voteRules();
  const [pending, setPending] = useState<Pending | null>(null);
  const [why, setWhy] = useState("");
  const [recorded, setRecorded] = useState<{ record: DecisionRecord; at: number } | null>(null);
  const [outcomeFor, setOutcomeFor] = useState<string | null>(null);
  const [outcome, setOutcome] = useState("");
  const whyId = useId();
  const now = new Date();
  const unwiredReason = "Needs wiring · decisions log";

  const draftOf = (p: Pending): DecisionDraft => {
    if (p.kind === "vote") {
      const v = p.item as FundingRequest;
      const venture = ventureOfRequester(v.requester);
      return { itemId: v.id, subject: "vote", title: `${v.requester} · ${v.purpose.charAt(0).toLowerCase()}${v.purpose.slice(1)}`, choice: p.choice, note: why.trim() || undefined, proposer: `${v.requester} crew`, sourceLink: { id: "enterprise", view: `ventures/${venture}` }, sourceLabel: `Ventures › ${v.requester} ›` };
    }
    const t = p.item as TaxFlag;
    return { itemId: t.id, subject: "tax_flag", title: `Tax flag · ${t.label.charAt(0).toLowerCase()}${t.label.slice(1)}`, choice: "handled", note: why.trim() || undefined, proposer: "Business ledger", sourceLink: { id: "enterprise", view: "finances" }, sourceLabel: "Finances ›" };
  };
  const record = async () => {
    if (!pending) return;
    const r = await log.append(draftOf(pending));
    if (r.state === "unwired") return;
    setRecorded({ record: r.value, at: Date.parse(r.value.decidedAt) });
    setPending(null); setWhy("");
    announceDecisions();
  };
  const undo = async () => {
    if (!recorded) return;
    await log.undo(recorded.record.id);
    setRecorded(null);
    announceDecisions();
  };
  useEffect(() => {
    if (!recorded) return;
    const t = window.setTimeout(() => setRecorded(null), Math.max(0, 8_000 - (Date.now() - recorded.at)));
    return () => window.clearTimeout(t);
  }, [recorded]);
  const saveOutcome = async (id: string) => {
    if (!outcome.trim()) return;
    await log.addOutcome(id, outcome.trim());
    setOutcomeFor(null); setOutcome("");
    announceDecisions();
  };

  const tag = w.unwired ? <DataTag kind="wire" /> : w.sample ? <DataTag kind="sample" /> : null;
  const act = (item: FundingRequest | TaxFlag, kind: Pending["kind"], choice: DecisionChoice, label: string, primary = false) => log.wired
    ? <Button size="small" variant={primary ? "primary" : "secondary"} aria-pressed={pending?.item.id === item.id ? pending.choice === choice : undefined} onClick={() => { setPending({ item, kind, choice }); setRecorded(null); }}>{label}</Button>
    : <OffButton reason={unwiredReason} variant={primary ? "primary" : "secondary"}>{label}</OffButton>;
  const preview = (p: Pending) => {
    const d = draftOf(p);
    return (
      <section className="v-preview" aria-label={`Preview: ${p.kind === "vote" ? "vote" : "tax flag"}`}>
        <div className="v-preview__top"><span className="v-eyebrow">Check before saving · {p.kind === "vote" ? "Vote" : "Tax flag"}</span><span className="v-grow" />{records.state === "sample" && <DataTag kind="sample" />}</div>
        <h3>{p.kind === "vote" ? `${p.choice === "yes" ? "Yes" : "No"} on ${d.title.replace(/^[^·]+· /, "")}` : `Mark handled: ${d.title.replace(/^Tax flag · /, "")}`}</h3>
        <dl className="v-kv"><dt>Your choice</dt><dd>{p.choice === "handled" ? "Handled" : p.choice === "yes" ? "Yes" : "No"}</dd><dt>When</dt><dd>{now.toLocaleDateString("en-US", { timeZone: "Asia/Tokyo", weekday: "short", month: "short", day: "numeric" })} · {now.toLocaleTimeString("en-GB", { timeZone: "Asia/Tokyo", hour: "2-digit", minute: "2-digit" })} JST</dd><dt>Proposed by</dt><dd>{d.proposer}</dd><dt>Source</dt><dd>{d.sourceLabel}</dd></dl>
        <label className="v-composer__label" htmlFor={whyId}>Why (optional)</label>
        <input id={whyId} className="v-composer__input" value={why} maxLength={500} onChange={(e) => setWhy(e.target.value)} placeholder="A line for future you" />
        <p className="v-note"><b>Nothing moves money. Nothing is sent.</b> This goes in your decisions log.</p>
        <div className="v-actions"><Button size="small" variant="primary" onClick={() => { void record(); }}>{p.kind === "vote" ? "Record vote" : "Record"}</Button><Button size="small" variant="plain" onClick={() => { setPending(null); setWhy(""); }}>Cancel</Button></div>
      </section>
    );
  };
  const list = valueOf(records) ?? [];
  return (
    <div className="biz page-inset cc-page">
      <PageHero title="Decisions" statement={w.unwired ? "Votes and tax flags" : `${w.n} waiting`} detail={<>Votes and tax flags from your ventures. Recording a vote never moves money and never contacts anyone. {tag}</>} />
      {recorded && <ResultLine onUndo={() => { void undo(); }} startedAt={recorded.at} extra={onOpen && <LinkButton small onClick={() => onOpen(recorded.record.sourceLink)}>Also shows on {recorded.record.sourceLabel.replace(/^Ventures › /, "").replace(/ ›$/, "")} › Milestones ›</LinkButton>}>
        Recorded: {recorded.record.choice === "handled" ? "Handled" : recorded.record.choice === "yes" ? "Yes" : "No"} on {recorded.record.title.replace(/^[^·]+· /, "")}, {jst(recorded.record.decidedAt).split(" · ")[1]} JST.</ResultLine>}
      <Card title={w.unwired ? "Waiting" : `Waiting · ${w.n}`} accessory={acc(tag)}>
        {w.unwired ? <p className="cc-note">Funding votes and tax flags from the business ledger appear here.</p> : w.n === 0 ? <p className="cc-note">Nothing is waiting for a decision.</p> : (
          <>
            <InsetGroup>
              {w.open.map((v) => <ListRow key={v.id} title={<>Funding vote · {v.requester} {v.purpose.charAt(0).toLowerCase()}{v.purpose.slice(1)} <Badge tone="warning">Waiting</Badge></>}
                subtitle={<>Proposed by the {v.requester} crew · amount {v.amountUsd.state === "unwired" ? wiredTag(v.amountUsd) : money(v.amountUsd.value)} · vote rules {rules.state === "unwired" ? wiredTag(rules) : `${rules.value.needed} yes votes`}</>}
                accessory={<span className="v-actions">{act(v, "vote", "no", "Vote no")}{act(v, "vote", "yes", "Vote yes", true)}</span>} />)}
              {w.tax.map((t) => <ListRow key={t.id} title={`Tax flag · ${t.label}`} subtitle={`${t.area} · ${t.date} · from the business ledger`}
                accessory={<span className="v-actions">{act(t, "tax_flag", "handled", "Mark handled")}<OffButton reason="Amara doesn't contact anyone outside">Ask the accountant</OffButton></span>} />)}
            </InsetGroup>
            {pending && preview(pending)}
          </>
        )}
      </Card>
      <Card title="Decisions log" accessory={acc(<>{wiredTag(records)}{list.length > 0 && <span className="v-note">{list.length} decisions</span>}</>)}>
        {records.state === "unwired" ? <p className="cc-note">Your choice, the time, who proposed it and a link to the source will collect here once the decisions log exists. It is separate from the business ledger.</p> : (
          <table className="pk-table" aria-label="Decisions log">
            <thead><tr><th scope="col">When (JST)</th><th scope="col">Decision</th><th scope="col">Your choice</th><th scope="col">Proposed by</th><th scope="col">Source</th><th scope="col">Outcome</th></tr></thead>
            <tbody>{list.map((r) => (
              <tr key={r.id} className={recorded?.record.id === r.id ? "v-new" : undefined}>
                <td>{jst(r.decidedAt)}</td><td>{r.title}{r.note && <small>{r.note}</small>}</td><td>{r.choice === "handled" ? "Handled" : r.choice === "yes" ? "Yes" : "No"}</td><td>{r.proposer}</td>
                <td><LinkButton small onClick={onOpen ? () => onOpen(r.sourceLink) : undefined}>{r.sourceLabel}</LinkButton></td>
                <td>{r.outcome ?? (outcomeFor === r.id
                  ? <form className="v-actions" onSubmit={(e) => { e.preventDefault(); void saveOutcome(r.id); }}><label className="ui-visually-hidden" htmlFor={`out-${r.id}`}>Outcome</label><input id={`out-${r.id}`} className="v-composer__input" value={outcome} onChange={(e) => setOutcome(e.target.value)} /><Button size="small" type="submit">Save</Button></form>
                  : <LinkButton small onClick={() => { setOutcomeFor(r.id); setOutcome(""); }} label={`Add outcome: ${r.title}`}>Add outcome</LinkButton>)}</td>
              </tr>
            ))}</tbody>
          </table>
        )}
        <p className="cc-note">Append-only. Undo lasts 8 s after recording; after that a correction is a new record.</p>
      </Card>
    </div>
  );
}

const TASK_KIND: Record<string, StatusKind> = { active: "run", waiting: "wait", backlog: "off", done: "ok" };
const GROUPS: Array<{ status: string; title: string }> = [{ status: "active", title: "In progress" }, { status: "waiting", title: "Waiting" }, { status: "backlog", title: "Backlog" }, { status: "done", title: "Done" }];

/** Enterprise › Tasks (was Work): add a task, then In progress · Waiting · Backlog · Done. Personal to-dos stay on Today. */
export function TasksView({ data, onRetry, onOpen }: { data: SystemSnapshot; onRetry?: () => void; onOpen?: (route: Route) => void }) {
  const [added, setAdded] = useState<Saved[]>([]);
  const tasksDown = data.unavailableSources.some((s) => s.command === "read_tasks") && data.tasks.length === 0;
  const tasks = data.tasks;
  const other = tasks.filter((t) => !GROUPS.some((g) => g.status === t.status));
  return (
    <div className="biz page-inset cc-page">
      <PageHero title="Tasks" statement={tasksDown ? "Tasks are out of reach" : workStatement(tasks)} detail="Enterprise work across your ventures. Personal to-dos are on Today › To-dos." />
      <Composer title="Add a task" kind="Task" adapter={addAdapter("task")} placeholder="e.g. Order 50 sample jars for Taiyo Skin by Friday" addLabel="Add task" saveLabel="Add task"
        hint="Enterprise work only. Personal to-dos live on Today." working="Reading your task…" onOpen={onOpen}
        onSaved={(s) => setAdded((a) => [s, ...a])} onUndone={(id) => setAdded((a) => a.filter((x) => x.id !== id))}
        savedText={(s) => `Added “${s.title}” to Backlog.`} />
      {tasksDown ? <div className="biz__unavailable"><p>Task source unavailable</p>{onRetry && <Button variant="secondary" size="small" onClick={onRetry}>Retry</Button>}</div> : tasks.length === 0 && added.length === 0 ? <p className="cc-note">No tasks yet. Tracked work collects here as it&apos;s created.</p> : (
        <div className="cc-grid2">
          {GROUPS.map((g) => {
            const items = tasks.filter((t) => t.status === g.status);
            const fresh = g.status === "backlog" ? added : [];
            if (items.length === 0 && fresh.length === 0) return null;
            return (
              <Card key={g.status} title={`${g.title} · ${items.length + fresh.length}`}>
                <InsetGroup>
                  {fresh.map((s) => <ListRow key={s.id} className="v-new" title={s.title} subtitle="Just added · sample data" accessory={<Badge tone="accent">New</Badge>} />)}
                  {items.map((t, i) => <ListRow key={`${t.title}-${i}`} title={plainText(t.title)} subtitle={[...(t.tags ?? []).map(humanize), t.priority ? humanize(t.priority) : null, "venture link needs wiring"].filter(Boolean).join(" · ")} accessory={<StatusText kind={TASK_KIND[t.status] ?? "off"}>{humanize(t.status)}</StatusText>} />)}
                </InsetGroup>
              </Card>
            );
          })}
          {other.length > 0 && <Card title={`Other · ${other.length}`}><InsetGroup>{other.map((t, i) => <ListRow key={`${t.title}-${i}`} title={plainText(t.title)} accessory={<StatusText kind="off">{humanize(t.status)}</StatusText>} />)}</InsetGroup></Card>}
        </div>
      )}
      <p className="cc-note">Marking a task done or editing it from here needs wiring; Amara updates tasks from chat today.</p>
    </div>
  );
}

const STAGES = [["ideation", "Idea"], ["drafting", "Drafting"], ["review", "Review"], ["scheduled", "Scheduled"], ["published", "Published"]] as const;

/** design-1118a: Content stays a list (one pipeline rail + a next step) until there are this many items; then the board. */
export const BOARD_AT = 3;

/** Enterprise › Content: the pipeline Idea → Drafting → Review → Scheduled → Published. Amara drafts; she never posts. */
export function ContentView({ data, onRetry, onOpen }: { data: SystemSnapshot; onRetry?: () => void; onOpen?: (route: Route) => void }) {
  const [added, setAdded] = useState<Saved[]>([]);
  const pipeline = data.pipeline;
  const down = data.unavailableSources.some((s) => s.command === "read_content_pipeline") && pipeline.length === 0;
  const count = (stage: string) => pipeline.filter((p) => p.stage === stage).length + (stage === "ideation" ? added.length : 0);
  const total = pipeline.length + added.length;
  const board = total >= BOARD_AT;
  const statement = down ? "The pipeline is out of reach" : total === 0 ? "Pipeline is clear" : [count("drafting") ? `${count("drafting")} drafting` : null, count("review") ? `${count("review")} in review` : null, count("ideation") ? `${count("ideation")} idea${count("ideation") === 1 ? "" : "s"}` : null].filter(Boolean).join(", ") || `${total} items`;
  const items = [...added.map((s) => ({ key: s.id, title: s.title, stage: "ideation" as string, type: "Just added · sample data" })), ...pipeline.map((p, i) => ({ key: `${p.title}-${i}`, title: plainText(p.title), stage: p.stage as string, type: humanize(p.content_type) }))];
  const hereIdx = Math.max(0, STAGES.findIndex(([st]) => count(st) > 0));
  const first = items[0];
  const composer = (
    <Composer title="New idea" kind="Idea" adapter={addAdapter("idea")} placeholder="e.g. Reel: how the label approval works, for Taiyo Skin" addLabel="Add idea" saveLabel="Add idea"
      hint="Shows as a preview first, then lands in Idea." working="Reading your idea…" onOpen={onOpen}
      onSaved={(s) => setAdded((a) => [s, ...a])} onUndone={(id) => setAdded((a) => a.filter((x) => x.id !== id))}
      savedText={(s) => `Added “${s.title}” to Idea.`} />
  );
  return (
    <div className="biz page-inset cc-page d18-page">
      <PageHero title="Content" statement={statement} detail="Ideas travel this path. Amara drafts; she never posts for you." />
      <PageTake surface="content" className="biz__take" />
      {board && composer}
      <Card title="Pipeline" accessory={<span className="cc-card__acc">{board ? "Board · 3 or more items" : `List until there are ${BOARD_AT} or more items`}</span>}>
        {down ? <div className="biz__unavailable"><p>Content pipeline unavailable</p>{onRetry && <Button variant="secondary" size="small" onClick={onRetry}>Retry</Button>}</div> : board ? (
          <div className="v-kanban" data-testid="content-board">
            {STAGES.map(([stage, label]) => {
              const its = items.filter((p) => p.stage === stage);
              return (
                <section key={stage} className="v-col" aria-label={`${label} · ${its.length}`}>
                  <h3>{label} · {its.length}</h3>
                  {its.map((p) => (
                    <div className="v-kc" key={p.key}>
                      <span>{p.title}</span><small>{p.type}</small>
                      {stage === "review" && <div className="v-actions"><OffButton reason="Amara doesn't post for you">Publish</OffButton></div>}
                    </div>
                  ))}
                  {its.length === 0 && <p className="v-note">{stage === "scheduled" ? "Nothing scheduled. You schedule posts in the platform itself." : "Nothing here."}</p>}
                </section>
              );
            })}
          </div>
        ) : (
          <div data-testid="content-list">
            <ol className="d18-rail" aria-label="Pipeline stages">
              {STAGES.map(([stage, label], i) => (
                <li key={stage} data-here={total > 0 && i === hereIdx ? "" : undefined} data-filled={count(stage) > 0 ? "" : undefined}>
                  {total > 0 && i === hereIdx && <span className="d18-rail__here">You are here</span>}
                  <i aria-hidden="true" /><span>{label} · {count(stage)}</span>
                </li>
              ))}
            </ol>
            {count("scheduled") === 0 && <p className="d18-note">Nothing scheduled. You schedule posts in the platform itself.</p>}
            {items.length === 0 ? <p className="d18-note">No ideas yet. Add one below; it starts in Idea.</p> : (
              <ul className="d18-clist">
                {items.map((p) => <li key={p.key}><span className="d18-eyebrow">{STAGES.find(([st]) => st === p.stage)?.[1] ?? humanize(p.stage)}</span><div><b>{p.title}</b><small>{p.type}</small></div></li>)}
              </ul>
            )}
            {first && (
              <section className="d18-next" aria-label="Next step">
                <p className="d18-eyebrow">Next step</p>
                <h3 className="d18-next__t">{first.stage === "ideation" ? "Draft this idea with Amara" : first.stage === "review" ? "Review the draft" : "Keep it moving"}</h3>
                <p className="d18-next__why">Amara writes a draft. She never posts it. The board comes back once there are {BOARD_AT} or more items.</p>
                {first.type.startsWith("Just added") || !inTauri() ? <div className="d18-actions"><OffButton variant="primary" reason="Needs wiring · Amara drafts from an idea">Draft with Amara</OffButton></div> : <ContentItemTools title={first.title} stage={first.stage as ContentStage} />}
              </section>
            )}
          </div>
        )}
        {board && <p className="cc-note">Moving a card between stages needs wiring. Venture links for content need wiring.</p>}
      </Card>
      {!board && composer}
    </div>
  );
}

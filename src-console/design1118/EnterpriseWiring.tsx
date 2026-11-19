import { useEffect, useState } from "react";
import { invokeBound } from "../bridge";
import { useStoresTick } from "../lib/localStores";
import { Button } from "../ui";
import { enterpriseAdapter, type ContentStage, type EnterpriseLive } from "./enterpriseLive";
import "./design1118.css";

/**
 * Enterprise wiring (decision 0039): small live panels over `enterpriseAdapter()`. Nothing here is invented:
 * a missing rate stays "needs a rate" (never a yen figure), a missing percent stays "N of M steps".
 * Every write shows the backend's toast and an 8 s Undo.
 */
type Ent = EnterpriseLive;

/** One 8 s Undo slot: the backend toast plus an Undo button that calls `undo`. */
function useUndoToast() {
  const [t, setT] = useState<{ text: string; undo: (() => Promise<boolean>) | null } | null>(null);
  useEffect(() => {
    if (!t?.undo) return;
    const id = window.setTimeout(() => setT((x) => (x?.undo === t.undo ? { ...x, undo: null } : x)), 8000);
    return () => window.clearTimeout(id);
  }, [t]);
  const run = () => {
    if (!t?.undo) return;
    const u = t.undo;
    setT({ text: t.text, undo: null });
    void u().then((ok) => setT({ text: ok ? "Undone." : "That couldn't be undone. Check History.", undo: null }));
  };
  const node = t && (
    <p className="d18-msg" role="status">{t.text}{t.undo && <> <Button size="small" variant="plain" onClick={run}>Undo</Button></>}</p>
  );
  return [setT, node] as const;
}

const asOfLine = (fx: { asOf: string; stale: boolean }) => `Rate as of ${fx.asOf}${fx.stale ? ". This rate is out of date; update it before you rely on a dollar figure." : "."}`;

/** Finances: the yen to dollar rate (as of, stale note), funds and sources. */
export function LiveFinancesCard({ adapter }: { adapter?: Ent }) {
  useStoresTick();
  const a = adapter ?? enterpriseAdapter();
  const fx = a.fx(), funds = a.funds(), sources = a.sources();
  if (fx.state === "unwired" && funds.state === "unwired" && sources.state === "unwired") return null;
  return (
    <section className="d18-card" aria-labelledby="d18-fin-live" data-testid="finances-live">
      <div className="d18-map__head"><h2 id="d18-fin-live" className="d18-h">Ledger</h2></div>
      {fx.state === "unwired"
        ? <p className="d18-note">No yen rate is set, so yen balances stay in yen without a dollar figure.</p>
        : <p className="d18-note" data-testid="fx-line">1 yen = ${fx.value.usdPerJpy} · {asOfLine(fx.value)}</p>}
      {funds.state !== "unwired" && (
        <ul className="d18-clist" aria-label="Funds">
          {funds.value.map((f) => <li key={f.name}><div><b>{f.name}</b><small>{f.balanceUsd == null ? "Dollar figure needs a rate" : `$${Math.round(f.balanceUsd).toLocaleString("en-US")}`}</small></div></li>)}
        </ul>
      )}
      {sources.state !== "unwired" && (
        <ul className="d18-clist" aria-label="Income sources">
          {sources.value.map((s) => <li key={s.streamId}><div><b>{s.name}</b><small>{s.status}{s.lastImportAt ? ` · last import ${s.lastImportAt.slice(0, 10)}` : " · no import yet"}{s.stale ? " · data is stale" : ""}</small></div></li>)}
        </ul>
      )}
    </section>
  );
}

type GoalOpt = { id: string; title: string; venture_id?: string | null };

/** Venture page: linked goals (N of M, server percent) and a goal-venture picker. */
export function VentureGoalsCard({ ventureId, adapter }: { ventureId: string; adapter?: Ent }) {
  useStoresTick();
  const a = adapter ?? enterpriseAdapter();
  const [setToast, toast] = useUndoToast();
  const [all, setAll] = useState<GoalOpt[]>([]);
  const [pick, setPick] = useState("");
  const ventures = a.ventures();
  useEffect(() => { void invokeBound<GoalOpt[]>("read_goals").then((g) => setAll(Array.isArray(g) ? g : [])).catch(() => setAll([])); }, [ventures]);
  if (ventures.state === "unwired") return null;
  const v = ventures.value.find((x) => x.id === ventureId);
  if (!v) return null;
  const linkedIds = new Set(v.goals.map((g) => g.id));
  const free = all.filter((g) => !linkedIds.has(g.id) && !g.venture_id);
  const link = async (goalId: string, to: string | null) => {
    const r = await a.linkGoal(goalId, to);
    if (r.state === "unwired") { setToast({ text: "That didn't save. Try again in a moment.", undo: null }); return; }
    setPick("");
    setToast({ text: r.value.toast, undo: () => a.undoGoalLink(r.value.id) });
  };
  return (
    <section className="d18-card" aria-labelledby="d18-v-goals" data-testid="venture-goals">
      <div className="d18-map__head"><h2 id="d18-v-goals" className="d18-h">Linked goals</h2></div>
      {v.goals.length === 0 ? <p className="d18-note">No goals are linked to this venture yet.</p> : (
        <ul className="d18-clist">
          {v.goals.map((g) => (
            <li key={g.id}><div><b>{g.title}</b>
              <small>{g.stepsTotal == null ? "Route not ready yet" : `${g.stepsDone ?? 0} of ${g.stepsTotal} steps`}{g.percent != null ? ` · ${Math.round(g.percent)}%` : ""}</small></div>
              <Button size="small" variant="plain" aria-label={`Unlink ${g.title}`} onClick={() => { void link(g.id, null); }}>Unlink</Button></li>
          ))}
        </ul>
      )}
      {free.length > 0 && (
        <div className="d18-actions">
          <label className="d18-note" htmlFor="d18-goal-pick">Link a goal</label>
          <select id="d18-goal-pick" value={pick} onChange={(e) => setPick(e.target.value)}>
            <option value="">Choose a goal</option>
            {free.map((g) => <option key={g.id} value={g.id}>{g.title}</option>)}
          </select>
          <Button size="small" variant="secondary" disabled={!pick} onClick={() => { void link(pick, ventureId); }}>Link</Button>
        </div>
      )}
      {toast}
    </section>
  );
}

const STAGE_LABEL: Record<ContentStage, string> = { ideation: "Idea", drafting: "Drafting", review: "Review", scheduled: "Scheduled", published: "Published" };
const ORDER: ContentStage[] = ["ideation", "drafting", "review", "scheduled", "published"];

/** Content: move a piece to another stage (Undo), Published asks "Did you post it?", and Draft with Amara. */
export function ContentItemTools({ title, stage, adapter }: { title: string; stage: ContentStage; adapter?: Ent }) {
  const a = adapter ?? enterpriseAdapter();
  const [setToast, toast] = useUndoToast();
  const [asking, setAsking] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const move = async (to: ContentStage, posted = false) => {
    setAsking(false);
    const r = await a.setStage(title, stage, to, posted);
    setToast(r.state === "unwired" ? { text: "That didn't move. Try again in a moment.", undo: null } : { text: r.value.toast, undo: () => a.undoStage(r.value.id) });
  };
  const makeDraft = async () => {
    setBusy(true);
    const r = await a.draft(title);
    setBusy(false);
    if (r.state === "unwired") setToast({ text: "Amara couldn't draft this just now. Try again in a moment.", undo: null });
    else setDraft(r.value.text);
  };
  const save = async () => {
    if (draft == null) return;
    const r = await a.saveDraft(title, draft);
    setToast(r.state === "unwired" ? { text: "The draft didn't save. Try again in a moment.", undo: null } : { text: r.value.toast, undo: () => a.undoDraft(r.value.id) });
  };
  const options = ORDER.filter((s) => s !== stage);
  return (
    <div className="d18-actions" data-testid="content-tools">
      <label className="d18-note" htmlFor={`mv-${title}`}>Move “{title}” to</label>
      <select id={`mv-${title}`} value="" onChange={(e) => { const to = e.target.value as ContentStage; if (!to) return; if (to === "published") setAsking(true); else void move(to); }}>
        <option value="">Choose a stage</option>
        {options.map((s) => <option key={s} value={s}>{STAGE_LABEL[s]}</option>)}
      </select>
      {asking && (
        <p className="d18-msg" role="group" aria-label="Did you post it?">Did you post it?{" "}
          <Button size="small" variant="secondary" onClick={() => { void move("published", true); }}>Yes, I posted it</Button>{" "}
          <Button size="small" variant="plain" onClick={() => { void move("published", false); }}>No, not yet</Button></p>
      )}
      <Button size="small" variant="primary" disabled={busy} onClick={() => { void makeDraft(); }}>Draft with Amara</Button>
      {draft != null && (
        <div data-testid="model-draft">
          <p className="d18-eyebrow">Model draft · not posted</p>
          <pre className="d18-note" style={{ whiteSpace: "pre-wrap" }}>{draft}</pre>
          <Button size="small" variant="secondary" onClick={() => { void save(); }}>Save draft</Button>
        </div>
      )}
      {toast}
    </div>
  );
}

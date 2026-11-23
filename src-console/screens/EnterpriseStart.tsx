import { useState } from "react";
import { Badge, Button, Card, InsetGroup, ListRow } from "../ui";
import { LinkButton } from "../adds/Composer";
import { amountText, useVentureMonth, type LedgerWrites, type VentureMonth } from "../business/ledgerWrite";
import type { Venture } from "../business/adapters";
import { useStoresTick } from "../lib/localStores";
import { MoneyForm, VentureForm, type FormVenture } from "./EnterpriseForms";
import "../adds/v1111.css";

/**
 * Enterprise › Overview as the start page (decision 0045 item 8): what this page is, three things you can do,
 * each venture with this month's money in and out, and what needs your call. Numbers come only from the
 * ledger and Treasury; with none, a row says what to do next instead of showing a dash.
 */
export interface Waiting { id: string; text: string; ventureName?: string; view: string }

export const toFormVenture = (v: Venture): FormVenture => ({ id: v.id, name: v.name, stage: v.stage, note: v.note, nextStep: v.nextStep, currency: v.currency, targetMinor: v.targetMinor });

/** "In 12,000 JPY · out 2,500 JPY", one clause per currency, or the next step when nothing is logged. */
export function monthLine(rows: VentureMonth[] | null | undefined, ventureId: string): string {
  if (!rows) return "This month: not available yet";
  const mine = rows.filter((r) => r.ventureId === ventureId);
  if (mine.length === 0) return "This month: nothing logged yet";
  return `This month: ${mine.map((r) => `in ${amountText(r.incomeMinor, r.currency)} · out ${amountText(r.costMinor, r.currency)}`).join(" · ")}`;
}

export function StartActions({ onAdd, onLog, onDecide, active }: { onAdd: () => void; onLog: () => void; onDecide: () => void; active: "venture" | "money" | null }) {
  return (
    <div className="v-actions" role="group" aria-label="What you can do here">
      <Button variant={active === "venture" ? "secondary" : "primary"} size="small" onClick={onAdd} aria-expanded={active === "venture"}>Add a venture</Button>
      <Button variant="secondary" size="small" onClick={onLog} aria-expanded={active === "money"}>Log income or a cost</Button>
      <Button variant="secondary" size="small" onClick={onDecide}>Record a decision</Button>
    </div>
  );
}

export function StartBody({ ventures, waiting, writes, onOpenVenture, onView, tradingLine }: {
  ventures: Venture[] | null; waiting: Waiting[]; writes: LedgerWrites | null;
  onOpenVenture: (id: string) => void; onView?: (view: string) => void; tradingLine?: React.ReactNode;
}) {
  const [panel, setPanel] = useState<"venture" | "money" | null>(null);
  const [logFor, setLogFor] = useState<string | undefined>();
  const [tick, setTick] = useState(0);
  const storeTick = useStoresTick();
  const month = useVentureMonth(writes, undefined, tick + storeTick);
  const list = ventures ?? [];
  const forms = list.map(toFormVenture);
  const close = () => { setPanel(null); setLogFor(undefined); setTick((n) => n + 1); };
  const waitingFor = (name: string) => waiting.filter((w) => w.ventureName === name).length;
  return (
    <>
      <StartActions active={panel} onAdd={() => setPanel(panel === "venture" ? null : "venture")} onLog={() => { setLogFor(undefined); setPanel(panel === "money" ? null : "money"); }} onDecide={() => onView?.("decisions")} />
      {panel === "venture" && <Card title="Add a venture"><VentureForm writes={writes} onClose={close} onSaved={() => setTick((n) => n + 1)} /></Card>}
      {panel === "money" && <Card title="Log income or a cost"><MoneyForm writes={writes} ventures={forms} defaultVentureId={logFor} onClose={close} onAddVenture={() => setPanel("venture")} /></Card>}
      <Card title="Your ventures">
        {list.length === 0 ? (
          <div className="v-composer">
            <p className="cc-note">No ventures yet. Add the first one: a name, where it stands and what comes next. Then you can log what it earns and costs.</p>
            <div className="v-actions"><Button size="small" variant="primary" onClick={() => setPanel("venture")}>Add a venture</Button></div>
          </div>
        ) : (
          <InsetGroup aria-label="Ventures">
            {list.map((v) => {
              const w = waitingFor(v.name);
              return (
                <ListRow key={v.id} data-testid={`venture-row-${v.id}`} title={<>{v.name} <Badge tone="neutral">{v.stage}</Badge></>}
                  subtitle={<>{v.note ? <>{v.note}<br /></> : null}{monthLine(month, v.id)}<br />{v.nextStep ? `Next: ${v.nextStep}` : "Next step: not set. Open it to add one."}{w ? <><br />{w} decision{w === 1 ? "" : "s"} waiting</> : null}</>}
                  accessory={<span className="v-actions"><Button size="small" variant="plain" onClick={() => { setLogFor(v.id); setPanel("money"); }}>Log money</Button><Button size="small" variant="secondary" onClick={() => onOpenVenture(v.id)} aria-label={`Open ${v.name}`}>Open</Button></span>} />
              );
            })}
          </InsetGroup>
        )}
        {tradingLine}
      </Card>
      <Card title="Needs your call">
        {waiting.length === 0 ? <p className="cc-note">Nothing needs your call right now. When a funding vote or tax flag appears, it shows up here.</p> : (
          <InsetGroup aria-label="Waiting for you">
            {waiting.map((w) => <ListRow key={w.id} title={w.text} subtitle={w.ventureName} accessory={<LinkButton small onClick={onView ? () => onView(w.view) : undefined}>Decide ›</LinkButton>} />)}
          </InsetGroup>
        )}
      </Card>
    </>
  );
}

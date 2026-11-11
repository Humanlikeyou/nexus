import { useState } from "react";
import { Button, DataTag, InsetGroup, ListRow, StatusText, Toggle } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { APP_NAME, APP_VERSION } from "../app/version";
import type { StandaloneAdapter } from "./adapters";
import { Rich, Row, SaCard } from "./parts";

/** Settings › General › Updates (design-1111s s4a/s4b). Auto-update on by default. */
export function UpdatesSection({ adapter }: { adapter: StandaloneAdapter }) {
  const [, bump] = useState(0);
  const [note, setNote] = useState<string | null>(null);
  const uw = adapter.updates();
  const u = valueOf(uw);
  const ready = u?.available ?? null;
  const pref = async (p: { autoUpdate?: boolean; checkDaily?: boolean }) => { const r = await adapter.setUpdatePrefs(p); setNote(r.state === "unwired" ? "Update settings need wiring." : null); bump((n) => n + 1); };
  const statement = !u ? "Updates" : ready ? `Update ready${ready.version ? `: ${APP_NAME} ${ready.version}` : ""}` : `${APP_NAME} is up to date`;
  const detail = !u ? <>Version {APP_VERSION} · signed updates install when Amara is idle <DataTag kind="wire" /></> : ready ? `Downloaded and verified · installs ${ready.scheduledFor} when Amara is idle` : `Version ${u.current} · checked ${u.lastCheck}`;
  return (
    <section className="sa-section" id="updates" aria-labelledby="sa-updates-title">
      <header className="sa-section__head"><h2 id="sa-updates-title" className="sa-section__title">Updates</h2><p className="sa-section__statement">{statement}</p><p className="sa-section__detail">{detail}</p></header>
      <div className="pk-g2">
        <div className="pk-stack">
          {ready ? (
            <SaCard title={ready.version ? `What's new in ${ready.version}` : "What's new"} w={uw}>
              <ul className="sa-notes">{ready.notes.map((n) => <li key={n}><Rich text={n} /></li>)}</ul>
              <p className="pk-note">Signed by your Developer ID and checked before install. <DataTag kind="wire" /></p>
              <div className="pk-actions"><Button size="small" variant="primary" onClick={() => setNote("Restart and update needs wiring.")}>Restart and Update Now</Button><Button size="small" variant="secondary" onClick={() => setNote("Scheduling needs wiring.")}>Tonight at 3:30 AM</Button><Button size="small" variant="secondary" disabled>Full Notes ›</Button></div>
            </SaCard>
          ) : (
            <SaCard title="Version" w={uw}>
              <Row title={APP_NAME} sub={`${u?.current ?? APP_VERSION} · this Mac`}>{u ? <StatusText kind="ok">Up to date</StatusText> : <DataTag kind="wire" />}</Row>
              <Row title="Last check" sub={u ? u.lastCheck : "Update checks need the signed update location"}><Button size="small" variant="secondary" disabled={!u} onClick={() => setNote("Checking needs wiring.")}>Check Now</Button></Row>
            </SaCard>
          )}
          <SaCard title="Before an update installs">
            <Row title="Back up first" sub="A backup runs, so you can go back"><StatusText kind="ok">Always</StatusText></Row>
            <Row title="Wait for crews" sub="Running crews and Muse recordings finish first"><StatusText kind="ok">Always</StatusText></Row>
            <Row title="Services" sub="Stopped, updated and started again in order"><StatusText kind="ok">Automatic</StatusText></Row>
          </SaCard>
        </div>
        <div className="pk-stack">
          <InsetGroup title="Update settings" footer={<>Updates come only from your own signed location <DataTag kind="wire" />. Service versions you pin (Letta 0.16.8) are never changed by an app update.</>}>
            <ListRow title="Install updates automatically" subtitle="When Amara is idle, overnight" accessory={<Toggle aria-label="Auto-update" checked={u?.autoUpdate ?? true} disabled={!u} onCheckedChange={(v) => { void pref({ autoUpdate: v }); }} />} />
            <ListRow title="Check daily" subtitle="From your private update location" accessory={<Toggle aria-label="Check daily" checked={u?.checkDaily ?? true} disabled={!u} onCheckedChange={(v) => { void pref({ checkDaily: v }); }} />} />
          </InsetGroup>
          {u && <SaCard title="History" w={uw}>{u.history.map((h) => <Row key={h.version + h.when} title={h.version} sub={`${h.when} · ${h.how}`}><StatusText kind="ok">Installed</StatusText></Row>)}</SaCard>}
          {note && <p className="pk-note" role="status">{note}</p>}
        </div>
      </div>
    </section>
  );
}

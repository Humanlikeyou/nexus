import { useState } from "react";
import { Button, DataTag, InsetGroup, ListRow, StatusText, Toggle } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { APP_NAME, APP_VERSION } from "../app/version";
import type { Outcome, StandaloneAdapter } from "./adapters";
import { Rich, Row, SaCard } from "./parts";

/** Plain-words result of Check Now / Restart and Update (decision 0042): what happened, and what is next. */
export function updateNote(kind: "check" | "install", r: Outcome): string {
  if (r.state === "done") return kind === "check" ? "Checked just now." : "Update started. Amara OS restarts to finish it.";
  switch (r.state) {
    case "unsupported": return kind === "check" ? "Update checking is switched off in this build. Nothing was checked." : "Installing from here isn't available in this build. Install the new version from the DMG. Nothing changed.";
    case "off": return "Outbound is disabled for this session, so Amara can't look for updates. Nothing was checked.";
    case "declined": return "Cancelled. Nothing changed.";
    case "busy": return "Another approval is still open. Finish it, then try again. Nothing changed.";
    case "expired": return "The confirmation timed out. Nothing changed; try again.";
    case "unwired": return kind === "check" ? "Checking isn't connected in this view. Nothing changed." : "Updating isn't connected in this view. Nothing changed.";
    default: return kind === "check" ? "Couldn't reach the update location. Nothing changed." : "The update didn't install. Nothing changed.";
  }
}

/** Settings › General › Updates (design-1111s s4a/s4b). Auto-update on by default. */
export function UpdatesSection({ adapter }: { adapter: StandaloneAdapter }) {
  const [, bump] = useState(0);
  const [note, setNote] = useState<string | null>(null);
  const uw = adapter.updates();
  const u = valueOf(uw);
  const ready = u?.available ?? null;
  const [busy, setBusy] = useState<"check" | "install" | null>(null);
  const pref = async (p: { autoUpdate?: boolean; checkDaily?: boolean }) => { const r = await adapter.setUpdatePrefs(p); setNote(r.state === "unwired" ? "Update settings couldn't be saved. Nothing changed." : "Saved."); bump((n) => n + 1); };
  const run = async (kind: "check" | "install") => { setBusy(kind); const r = await (kind === "check" ? adapter.checkUpdate() : adapter.installUpdate()); setBusy(null); setNote(updateNote(kind, r)); bump((n) => n + 1); };
  // Live data carries the backend's own state; sample data has none. An off updater is shown as off, with its reason, never as "up to date".
  const off = u?.backend === "not_configured" ? "off" : u?.backend === "outbound_disabled" ? "paused" : null;
  const offWhy = off === "off" ? "Update checking is switched off in this build. Install new versions from the DMG." : "Outbound is disabled for this session, so Amara can't look for updates.";
  const statement = !u ? "Updates" : off ? (off === "off" ? "Updates are off" : "Update checks are paused") : ready ? (u.backend ? `Update available${ready.version ? `: ${APP_NAME} ${ready.version}` : ""}` : `Update ready${ready.version ? `: ${APP_NAME} ${ready.version}` : ""}`) : `${APP_NAME} is up to date`;
  const detail = !u ? <>Version {APP_VERSION} · signed updates install when Amara is idle <DataTag kind="wire" /></> : off ? `Version ${u.current} · ${offWhy}` : ready ? (u.backend ? "The update location announced it. It isn't downloaded or verified yet." : `Downloaded and verified · installs ${ready.scheduledFor} when Amara is idle`) : `Version ${u.current} · checked ${u.lastCheck}`;
  return (
    <section className="sa-section" id="updates" aria-labelledby="sa-updates-title">
      <header className="sa-section__head"><h2 id="sa-updates-title" className="sa-section__title">Updates</h2><p className="sa-section__statement">{statement}</p><p className="sa-section__detail">{detail}</p></header>
      <div className="pk-g2">
        <div className="pk-stack">
          {ready ? (
            <SaCard title={ready.version ? `What's new in ${ready.version}` : "What's new"} w={uw}>
              {ready.notes.length > 0 ? <ul className="sa-notes">{ready.notes.map((n) => <li key={n}><Rich text={n} /></li>)}</ul> : <p className="pk-note">The update location sent no release notes.</p>}
              {u?.backend ? <p className="pk-note">The version is the update location&apos;s claim. It is checked against its signature before anything installs.</p> : <p className="pk-note">Signed by your Developer ID and checked before install. <DataTag kind="wire" /></p>}
              <div className="pk-actions"><Button size="small" variant="primary" loading={busy === "install"} onClick={() => { void run("install"); }}>Restart and Update Now</Button>{!u?.backend && <><Button size="small" variant="secondary" onClick={() => setNote("Scheduling needs wiring.")}>Tonight at 3:30 AM</Button><Button size="small" variant="secondary" disabled>Full Notes ›</Button></>}</div>
            </SaCard>
          ) : (
            <SaCard title="Version" w={uw}>
              <Row title={APP_NAME} sub={`${u?.current ?? APP_VERSION} · this Mac`}>{u ? (off ? <StatusText kind="off">{off === "off" ? "Updates off" : "Paused"}</StatusText> : <StatusText kind="ok">Up to date</StatusText>) : <DataTag kind="wire" />}</Row>
              <Row title="Last check" sub={off ? offWhy : u ? u.lastCheck : "Update checks need the signed update location"}><Button size="small" variant="secondary" loading={busy === "check"} disabled={!u || !!off} onClick={() => { void run("check"); }}>Check Now</Button></Row>
            </SaCard>
          )}
          <SaCard title="Before an update installs">
            <Row title="Back up first" sub="A backup runs, so you can go back"><StatusText kind="ok">Always</StatusText></Row>
            <Row title="Wait for crews" sub="Running crews and Muse recordings finish first"><StatusText kind="ok">Always</StatusText></Row>
            <Row title="Services" sub="Stopped, updated and started again in order"><StatusText kind="ok">Automatic</StatusText></Row>
          </SaCard>
        </div>
        <div className="pk-stack">
          <InsetGroup title="Update settings" footer={u?.backend === "not_configured" ? "Saved, but these have no effect while updates are off. Service versions you pin (Letta 0.16.8) are never changed by an app update." : <>Updates come only from your own signed location <DataTag kind="wire" />. Service versions you pin (Letta 0.16.8) are never changed by an app update.</>}>
            <ListRow title="Install updates automatically" subtitle="When Amara is idle, overnight" accessory={<Toggle aria-label="Auto-update" checked={u?.autoUpdate ?? true} disabled={!u} onCheckedChange={(v) => { void pref({ autoUpdate: v }); }} />} />
            <ListRow title="Check daily" subtitle="From your private update location" accessory={<Toggle aria-label="Check daily" checked={u?.checkDaily ?? true} disabled={!u} onCheckedChange={(v) => { void pref({ checkDaily: v }); }} />} />
          </InsetGroup>
          {u && u.history.length > 0 && <SaCard title="History" w={uw}>{u.history.map((h) => <Row key={h.version + h.when} title={h.version} sub={`${h.when} · ${h.how}`}><StatusText kind="ok">Installed</StatusText></Row>)}</SaCard>}
          {note && <p className="pk-note" role="status">{note}</p>}
        </div>
      </div>
    </section>
  );
}

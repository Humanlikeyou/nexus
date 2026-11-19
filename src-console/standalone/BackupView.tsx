import { useId, useRef, useState } from "react";
import { Button, DataTag, Dialog, PageHero, StatusText, Toggle } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { BACKUP_CONTENTS, BACKUP_DEFAULTS, type RestorePreview, type StandaloneAdapter } from "./adapters";
import { Bar, KeyState, Row, SaCard, WireNote } from "./parts";
import { BackupSection, RESTORE_SAFETY_NOTE, RESTORE_WARNING } from "../settings/BackupSection";

/** Wording for the verify result: only a per-file sha256 match is called a checksum. */
const checkWord = (l: { checksumOk: boolean | null; checks?: string }) => (l.checksumOk === null ? "not checked yet" : l.checksumOk ? (l.checks === "sha256" ? "checksum OK" : "readable, no key files found") : "check failed");
const at12 = (hhmm: string) => { const [h, m] = hhmm.split(":").map(Number); return `${((h + 11) % 12) + 1}:${String(m).padStart(2, "0")} ${h < 12 ? "AM" : "PM"}`; };

/** The n8n key file (Taisun, 14:59 JST): passphrase-locked, kept beside the backups and never inside one. */
export function KeyFileSheet({ open, adapter, onClose }: { open: boolean; adapter: StandaloneAdapter; onClose: (msg: string | null) => void }) {
  const a = useRef<HTMLInputElement>(null), b = useRef<HTMLInputElement>(null);
  const id = useId();
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const clear = () => { if (a.current) a.current.value = ""; if (b.current) b.current.value = ""; setErr(null); };
  const save = async () => {
    const p = a.current?.value ?? "", q = b.current?.value ?? "";
    if (p.length < 8) { setErr("Use at least 8 characters."); return; }
    if (p !== q) { setErr("The two passphrases don't match."); return; }
    clear(); setBusy(true);
    const r = await adapter.makeKeyFile(p);
    setBusy(false);
    onClose(r.state === "unwired" ? "Making the key file needs wiring. Nothing was written." : "Key file made. Keep the passphrase in your password manager.");
  };
  return (
    <Dialog open={open} title="Make the n8n key file" onCancel={() => { clear(); onClose(null); }} className="sa-sheet">
      <p>The n8n encryption key opens your saved n8n logins and has to be identical on a new Mac. Amara locks it in its own file with a passphrase you choose. The file is never part of a backup.</p>
      <div className="pk-field"><label htmlFor={`${id}-a`}>Passphrase</label><input ref={a} id={`${id}-a`} className="pk-input" type="password" autoComplete="new-password" spellCheck={false} onChange={() => setErr(null)} /></div>
      <div className="pk-field"><label htmlFor={`${id}-b`}>Type it again</label><input ref={b} id={`${id}-b`} className="pk-input" type="password" autoComplete="new-password" spellCheck={false} onChange={() => setErr(null)} /></div>
      {err && <p role="alert" className="sa-error">{err}</p>}
      <div className="pk-info">Amara never stores the passphrase. Without it the key file can&apos;t be opened, so keep it in your password manager.</div>
      <div className="pk-sheet__actions"><Button size="small" variant="secondary" onClick={() => { clear(); onClose(null); }} disabled={busy}>Cancel</Button><Button size="small" variant="primary" onClick={() => { void save(); }} loading={busy}>Make Key File</Button></div>
    </Dialog>
  );
}

/** Settings › Backup (design-1111s s3a–s3c): back up, restore, Move to a new Mac, keys on this Mac. */
export function BackupView({ adapter }: { adapter: StandaloneAdapter }) {
  const [, bump] = useState(0);
  const [restoring, setRestoring] = useState(false);
  const [keySheet, setKeySheet] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  // Restore: which backup is picked, its real preview, and the result once it has run.
  const [pickId, setPickId] = useState<string | null>(null);
  const [prev, setPrev] = useState<{ id: string; state: "loading" } | { id: string; state: "error"; message: string } | { id: string; state: "ready"; preview: RestorePreview } | null>(null);
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restored, setRestored] = useState<{ label: string; lines: string[] } | null>(null);
  const bw = adapter.backup();
  const b = valueOf(bw);
  const kw = adapter.keyFile();
  const kf = valueOf(kw);
  const sw = adapter.secrets();
  const keys = valueOf(sw);
  const s = b?.settings ?? BACKUP_DEFAULTS;
  const run = b?.running ?? null;
  const picked = b?.backups.find((x) => x.id === pickId) ?? null;
  const ready = prev && prev.id === pickId && prev.state === "ready" ? prev.preview : null;
  const choose = (id: string) => {
    setPickId(id);
    setPrev({ id, state: "loading" });
    void adapter.previewBackup(id).then((r) => setPrev(r.state === "done" ? { id, state: "ready", preview: r.preview } : { id, state: "error", message: r.message ?? "Couldn't read this backup. Nothing was changed." }));
  };
  const closeRestore = () => { setRestoring(false); setPickId(null); setPrev(null); };
  const runRestore = () => {
    if (!picked || !ready) return;
    const label = picked.madeAt;
    setRestoring(false); setRestoreBusy(true); setRestored(null);
    setNote("Waiting for your confirmation. Nothing is replaced until you allow it.");
    void adapter.restore(picked.id).then((r) => {
      setRestoreBusy(false); setPickId(null); setPrev(null);
      if (r.state === "done") { setNote(null); setRestored({ label, lines: r.lines }); }
      else setNote(r.state === "declined" ? "Restore cancelled. Nothing was changed." : r.message ?? "Restore didn't finish. Nothing was changed.");
      bump((n) => n + 1);
    });
  };
  const changeDir = () => {
    void adapter.pickBackupDir().then((r) => {
      if (r.state === "done") setNote(`Backups will go to ${r.dir} from now on. Existing backups stay where they are.`);
      else if (r.state !== "cancelled") setNote(r.message ?? "Couldn't change the backup folder. Nothing changed.");
      bump((n) => n + 1);
    });
  };
  const say = (r: { state: string }, ok: string) => { setNote(r.state === "unwired" ? "That needs wiring. Nothing changed." : ok); bump((n) => n + 1); };
  return (
    <div className="pk-host sa-page">
      <PageHero title="Backup" statement={run ? `Backing up… ${run.pct}%` : b?.last ? `Backed up at ${b.last.at.replace(/^.*, /, "")}` : "Backups"}
        detail={run ? `${run.done} of ${run.total} · ${run.left} · Amara keeps working` : b?.last ? `Every night at ${at12(s.at)} · ${s.keep} kept · ${b.last.size} · ${checkWord(b.last)}` : <>Every night at {at12(s.at)} to {s.dir}, {s.keep} kept, plus one before every update <DataTag kind="wire" /></>} />
      {run && <div className="pk-ok"><strong>Backing up</strong><span className="sa-grow"><Bar pct={run.pct} label="Backup progress" /></span><span>{run.phase}</span><Button size="small" variant="secondary" onClick={() => setNote("Stop needs wiring.")}>Stop</Button></div>}
      <div className="pk-g2">
        <div className="pk-stack">
          <SaCard title="Back up and restore" w={bw}>
            <div className="pk-actions sa-actions-pad">
              <Button size="small" variant="primary" disabled={!!run || !b} onClick={() => { void adapter.backupNow().then((r) => say(r, "Backup finished.")); }}>Back Up Now</Button>
              <Button size="small" variant="secondary" disabled={!b || restoreBusy} onClick={() => setRestoring(true)}>Restore from a Backup…</Button>
            </div>
            {b?.last ? <Row title="Last backup" sub={`${b.last.at} · ${b.last.size} · ${checkWord(b.last)}`}><StatusText kind={b.last.checksumOk === false ? "fail" : "ok"}>{b.last.checksumOk === false ? "Check failed" : "Done"}</StatusText></Row>
              : bw.state === "live" ? <Row title="Last backup" sub="No backup yet in the backup folder"><StatusText kind="off">None</StatusText></Row>
              : <Row title="Last backup" sub="Shows here once nightly backups are wired"><DataTag kind="wire" /></Row>}
            <Row title={`Every night at ${at12(s.at)}`} sub={`Skipped on battery under ${s.skipOnBatteryBelow}%`}><Toggle aria-label="Nightly backup" checked={s.nightly} disabled={!b} onCheckedChange={(v) => { void adapter.setBackupSettings({ nightly: v }).then((r) => say(r, v ? "Nightly backup on." : "Nightly backup off.")); }} /></Row>
            <Row title="Where" sub={`${s.dir} · keeps the last ${s.keep}`}><Button size="small" variant="secondary" disabled={!b} onClick={changeDir}>Change…</Button></Row>
            <Row title="Before each update" sub="A backup runs first, so an update can be undone"><StatusText kind={s.beforeUpdate ? "ok" : "off"}>{s.beforeUpdate ? "On" : "Off"}</StatusText></Row>
          </SaCard>
          <SaCard title="What's in a backup">
            <div className="pk-chips sa-chips-pad">{BACKUP_CONTENTS.map((c) => <span key={c} className="pk-chip" data-on>{c}</span>)}</div>
            <p className="pk-note">One file, readable only by you on this Mac (0600). Paths are stored relative to Amara&apos;s folder, so it restores on any Mac.</p>
          </SaCard>
        </div>
        <div className="pk-stack">
          <div className="pk-warn" role="note"><strong>🔑 Keys are never backed up</strong><span>Your Telegram token, Home Assistant and Freqtrade logins, and Oura, Hevy and AI-coach keys stay in this Mac&apos;s Keychain. After a restore you type them in again. The n8n encryption key travels separately in your passphrase-locked key file.</span></div>
          <SaCard title="n8n key file" w={kw}>
            {kf ? <Row title={kf.made ? "Key file made" : "No key file yet"} sub={kf.made ? `${kf.madeAt} · ${kf.where}` : "Make one before you move to a new Mac"}><StatusText kind={kf.made ? "ok" : "off"}>{kf.made ? "Ready" : "Not made"}</StatusText></Row>
              : <Row title="Key file" sub="Locked with your passphrase, kept outside the backup"><DataTag kind="wire" /></Row>}
            <div className="pk-actions"><Button size="small" variant={kf?.made ? "secondary" : "primary"} onClick={() => setKeySheet(true)}>{kf?.made ? "Make a New Key File…" : "Make Key File…"}</Button></div>
            <p className="pk-note">n8n&apos;s saved logins only open with the same key, so it can&apos;t be regenerated. On the new Mac, setup asks for this file and its passphrase.</p>
          </SaCard>
          <SaCard title="Move to a new Mac">
            <ol className="pk-steps">
              <li><span>Back up this Mac<small>Back Up Now, then copy the backup and your n8n key file to the new Mac or a drive</small></span>{b?.last ? <StatusText kind="ok">Done</StatusText> : <span />}</li>
              <li><span>Install Amara from the DMG<small>Drag Amara OS into Applications and open it</small></span><span /></li>
              <li><span>Choose &ldquo;Restore from a backup&rdquo;<small>Setup checks the file before it changes anything</small></span><span /></li>
              <li><span>Unlock the n8n key file<small>Type its passphrase; the key goes into the new Mac&apos;s Keychain</small></span><span /></li>
              <li><span>Re-enter your other keys<small>Setup lists each missing key</small></span><span /></li>
              <li><span>Re-pair your WHOOP 4<small>Settings › Devices</small></span><span /></li>
              <li><span>Pick the model size for that Mac<small>More memory unlocks a bigger model</small></span><span /></li>
            </ol>
            <p className="pk-note">Your old Mac keeps everything until you erase it yourself.</p>
          </SaCard>
          <SaCard title="Keys on this Mac" w={sw}>
            {keys ? keys.map((k) => <div key={k.name} className="pk-keyrow"><div>{k.label}</div><KeyState set={k.set} /><span /></div>) : <WireNote>Needs wiring: each key shows Set or Not set once Claude builds secrets_status. Values are never shown.</WireNote>}
            {keys && <p className="pk-note">Shows only whether each key is set. Values are never shown.</p>}
          </SaCard>
        </div>
      </div>
      {note && <p className="pk-note" role="status">{note}</p>}
      <section className="sa-legacy" aria-label="Back up to a file now"><BackupSection /></section>
      {restored && (
        <div className="backup-restore__result" role="status">
          <p>Restored from the backup of {restored.label}. Restart Amara OS to load the restored data.</p>
          {restored.lines.length > 0 && <ul>{restored.lines.map((line, i) => <li key={`${i}-${line}`}>{line}</li>)}</ul>}
        </div>
      )}
      <Dialog open={restoring} title={picked ? `Restore from ${picked.label}?` : "Restore from a backup?"} onCancel={closeRestore} className="sa-sheet">
        <p>Pick the backup to restore{bw.state === "sample" && <> <DataTag kind="sample" /></>}. {RESTORE_WARNING}</p>
        {(b?.backups.length ?? 0) === 0 ? <p className="pk-note">There are no backups in the backup folder yet. Back up first.</p> : (
          <div className="pk-chips" role="group" aria-label="Backups">
            {b!.backups.map((x) => <button key={x.id} type="button" className="pk-chip sa-chip" data-on={x.id === pickId || undefined} aria-pressed={x.id === pickId} onClick={() => choose(x.id)}>{x.madeAt}</button>)}
          </div>
        )}
        {prev && prev.id === pickId && prev.state === "loading" && <p className="pk-note" role="status">Checking this backup…</p>}
        {prev && prev.id === pickId && prev.state === "error" && <p role="alert" className="sa-error">{prev.message}</p>}
        {ready && <>
          <dl className="pk-kv"><dt>Backup check</dt><dd><StatusText kind={ready.checksumOk ? "ok" : "fail"}>{ready.checksumOk ? "Passed" : "Failed: this backup can't be restored"}</StatusText></dd><dt>Made</dt><dd>{ready.madeOn}{ready.version ? ` · version ${ready.version}` : ""}</dd><dt>Size</dt><dd>{ready.size}</dd><dt>Keys</dt><dd>Unchanged. They stay in the Keychain.</dd></dl>
          {ready.contents.length > 0 && <div className="pk-warn"><strong>⚠ This backup holds</strong><ul>{ready.contents.map((c) => <li key={c.title}>{c.title} · {c.sub}</li>)}</ul></div>}
          <p className="pk-note">{RESTORE_SAFETY_NOTE} Amara asks you to confirm once more in a native dialog before anything is replaced.</p>
        </>}
        <div className="pk-sheet__actions"><Button size="small" variant="secondary" onClick={closeRestore}>Cancel</Button><Button size="small" variant="destructive" disabled={!ready || !ready.checksumOk} onClick={runRestore}>Save Today, Then Restore</Button></div>
      </Dialog>
      <KeyFileSheet open={keySheet} adapter={adapter} onClose={(m) => { setKeySheet(false); if (m) setNote(m); bump((n) => n + 1); }} />
    </div>
  );
}

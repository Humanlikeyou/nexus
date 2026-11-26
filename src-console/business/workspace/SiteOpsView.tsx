import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, Select, StatusText, TextField, Toggle } from "../../ui";
import { MATERIAL, SITE_FLAG, SITE_STATUS, SITE_TYPES, errText, whenText, type BizDocument, type ContextItem, type SiteChange, type SiteEvent, type SiteMatch, type SiteSnapshot, type WorkspaceCalls } from "./workspaceApi";

/**
 * Decision 0065 (Phase 13): Enterprise › Workspaces › Website changes. The live site is the source of truth.
 * Amara drafts, compares, checks and packages; Taisun approves in the system dialog and uploads in Hostinger's
 * File Manager; Amara verifies the live file and keeps a rollback package. Nothing here writes to the website.
 */
function Preview({ c }: { c: SiteChange }) {
  if (c.contentType === "seo_title" || c.contentType === "seo_description") {
    const m = c.after.match(c.contentType === "seo_title" ? /<title>([^<]*)<\/title>/ : /name="description" content="([^"]*)"/);
    const title = c.after.match(/<title>([^<]*)<\/title>/)?.[1] ?? "";
    const desc = c.after.match(/name="description" content="([^"]*)"/)?.[1] ?? "";
    return (
      <div aria-label="Search result preview" className="pk-note" style={{ border: "1px solid var(--separator)", borderRadius: 8, padding: 8 }}>
        <div>taiyoskin.com</div><b>{title}</b><div>{desc}</div>
        {!m && <StatusText kind="risk">The changed tag wasn't found in the new page.</StatusText>}
      </div>
    );
  }
  return <p className="pk-note">Preview: {c.after.length > 400 ? `${c.after.slice(0, 400)}…` : c.after}</p>;
}

function ChangeCard({ id, c, calls, onChange }: { id: string; c: SiteChange; calls: WorkspaceCalls; onChange: () => void }) {
  const [msg, setMsg] = useState(""); const [busy, setBusy] = useState(false);
  const [events, setEvents] = useState<SiteEvent[] | null>(null);
  const act = (p: Promise<unknown>) => { setBusy(true); setMsg(""); p.then(() => onChange(), (e: unknown) => setMsg(errText(e))).finally(() => setBusy(false)); };
  const errors = c.checks.some((x) => x.level === "error");
  return (
    <Card title={c.title} headingLevel={3} accessory={<span className="pk-acc">{SITE_STATUS[c.status] ?? c.status}</span>}>
      <p className="pk-note">File: public_html{c.target} · drafted {whenText(c.createdAt)}{c.flags.map((f) => <span key={f}> <Badge tone="warning">{SITE_FLAG[f] ?? f}: needs explicit approval</Badge></span>)}</p>
      <pre aria-label={`Changes in ${c.title}`} className="pk-note" style={{ whiteSpace: "pre-wrap", maxHeight: 200, overflow: "auto" }}>{c.diff.map((l) => `${l.op} ${l.text}`).join("\n")}</pre>
      <ul className="pk-list" aria-label={`Checks for ${c.title}`}>{c.checks.map((x, i) => <li key={i}><StatusText kind={x.level === "ok" ? "ok" : x.level === "warn" ? "risk" : "fail"}>{x.text}</StatusText></li>)}</ul>
      <Preview c={c} />
      {c.verifyDetail && <p className="pk-note" role="status">{c.verifyDetail}</p>}
      {c.packageDir && <p className="pk-note">Packages: {c.packageDir} (upload.tar.gz, rollback.tar.gz, README.txt)</p>}
      <p className="pk-note">
        {c.status === "draft" && <><Button size="small" variant="primary" disabled={busy || errors} onClick={() => act(calls.siteApprove(id, c.changeId))}>Approve…</Button> </>}
        {(c.status === "draft" || c.status === "approved") && <><Toggle checked={c.hpanelBackupConfirmed} disabled={busy || c.hpanelBackupConfirmed} onCheckedChange={() => act(calls.siteConfirmBackup(id, c.changeId))}>I took an hPanel backup</Toggle> </>}
        {c.status === "approved" && <><Button size="small" variant="primary" disabled={busy || !c.hpanelBackupConfirmed} onClick={() => act(calls.sitePackage(id, c.changeId))}>Back up and package</Button> </>}
        {(c.status === "packaged" || c.status === "verify_failed") && <><Button size="small" variant="primary" disabled={busy} onClick={() => act(calls.siteVerify(id, c.changeId, false))}>Verify the live site</Button> </>}
        {(c.status === "verify_failed" || c.status === "verified" || c.status === "rollback_failed") && <><Button size="small" variant="secondary" disabled={busy} onClick={() => act(calls.siteVerify(id, c.changeId, true))}>Verify rollback</Button> </>}
        {(c.status === "draft" || c.status === "approved") && <Button size="small" variant="plain" disabled={busy} onClick={() => act(calls.siteReject(id, c.changeId))}>Reject</Button>}
        {" "}<Button size="small" variant="plain" aria-expanded={events !== null} onClick={() => (events ? setEvents(null) : void calls.siteEvents(id, c.changeId).then(setEvents, (e: unknown) => setMsg(errText(e))))}>History</Button>
      </p>
      {errors && c.status === "draft" && <p className="pk-note">Checks marked as errors must be fixed (draft again) before approval.</p>}
      {events && <ol className="pk-list" aria-label={`History of ${c.title}`}>{events.map((e) => <li key={e.seq}>{whenText(e.at)} · {SITE_STATUS[e.toStatus] ?? e.toStatus} · {e.actor === "taisun" ? "you" : e.actor} · {e.detail}</li>)}</ol>}
      {msg && <p className="pk-note" role="alert">{msg}</p>}
    </Card>
  );
}

export function SiteOpsTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const [snap, setSnap] = useState<SiteSnapshot | null>(null);
  const [changes, setChanges] = useState<SiteChange[] | null>(null);
  const [q, setQ] = useState(""); const [hits, setHits] = useState<SiteMatch[] | null>(null);
  const [type, setType] = useState("seo_description");
  const [value, setValue] = useState(""); const [path, setPath] = useState(""); const [doc, setDoc] = useState("");
  const [sources, setSources] = useState<string[]>([]);
  const [approved, setApproved] = useState<ContextItem[] | null>(null);
  const [docs, setDocs] = useState<BizDocument[]>([]);
  const [msg, setMsg] = useState(""); const [busy, setBusy] = useState(false);
  const reload = useCallback(() => void calls.siteChanges(id).then(setChanges, (e: unknown) => setMsg(errText(e))), [calls, id]);
  useEffect(() => {
    reload();
    void calls.context(id, null).then((l) => setApproved(l.filter((i) => i.status === "approved")), () => setApproved([]));
    void calls.documents(id).then((l) => setDocs(l.filter((d) => d.status === "approved")), () => setDocs([]));
  }, [calls, id, reload]);
  const kind = SITE_TYPES.find((t) => t.value === type);
  const isAsset = type.startsWith("asset_");
  const draft = () => {
    setBusy(true); setMsg("");
    calls.siteDraft(id, { contentType: type, value, path: isAsset ? path : null, documentId: type === "asset_replace" ? doc || null : null, sourceRefs: sources })
      .then(() => { setValue(""); reload(); }, (e: unknown) => setMsg(errText(e))).finally(() => setBusy(false));
  };
  return (
    <>
      <p className="pk-note">The live site is the source of truth. Amara drafts and checks a change, you approve it in a system dialog, Amara backs up the live file and packages the change, you upload it in Hostinger's File Manager, and Amara verifies the result. A rollback package is kept for every change. Nothing here writes to the website.</p>
      <Card title="1. Live site" headingLevel={3}>
        <p className="pk-note">{snap ? `Snapshot of ${snap.files.length} files taken ${whenText(snap.createdAt)}. ${snap.limits}` : "Take a snapshot before drafting: changes are computed from the live files."}</p>
        <p className="pk-note"><Button size="small" variant="secondary" disabled={busy} onClick={() => { setBusy(true); calls.siteSnapshot(id).then(setSnap, (e: unknown) => setMsg(errText(e))).finally(() => setBusy(false)); }}>Take a snapshot</Button></p>
        <TextField label="Find content on the site and in approved material" value={q} onChange={(e) => setQ(e.target.value)} />
        <p className="pk-note"><Button size="small" variant="plain" disabled={q.trim().length < 2} onClick={() => void calls.siteLocate(id, q).then(setHits, (e: unknown) => setMsg(errText(e)))}>Find</Button></p>
        {hits && (hits.length === 0 ? <p className="pk-note" role="status">No matches.</p> : <ul className="pk-list" aria-label="Matches">{hits.map((h, i) => <li key={i}><b>{h.kind === "site" ? h.reference : "Approved material"}</b>{h.status && <> <Badge tone="neutral">{h.status}</Badge></>}<br /><span className="pk-note">{h.excerpt}</span></li>)}</ul>)}
      </Card>
      <Card title="2. Draft a change" headingLevel={3}>
        <Select label="What to change" options={SITE_TYPES.map((t) => ({ value: t.value, label: t.label }))} value={type} onChange={(e) => setType(e.target.value)} />
        {kind && !kind.available && <p className="pk-note">This is compiled into the site's JavaScript bundle. It can't be changed safely until a source that reproduces the live build is confirmed.</p>}
        {isAsset && <TextField label="File on the site" placeholder="/logo.png" value={path} onChange={(e) => setPath(e.target.value)} />}
        {type === "asset_replace" && <Select label="Approved document to use" options={[{ value: "", label: docs.length ? "Choose…" : "No approved documents" }, ...docs.map((d) => ({ value: d.docId, label: d.title }))]} value={doc} onChange={(e) => setDoc(e.target.value)} />}
        {!isAsset && <label className="pk-note" style={{ display: "grid", gap: 4 }}>New text<textarea aria-label="New text" rows={type === "seo_title" || type === "seo_description" ? 3 : 8} value={value} onChange={(e) => setValue(e.target.value)} /></label>}
        <p className="pk-note">Based on approved material (required for prices, ingredients and formulations):</p>
        {approved === null ? <p className="pk-note" role="status">Loading approved material…</p> : approved.length === 0 ? <p className="pk-note">No approved material yet. Approve items in Context first.</p> : (
          <ul className="pk-list" aria-label="Approved sources">{approved.map((i) => <li key={i.itemId}><Toggle checked={sources.includes(i.itemId)} onCheckedChange={(v) => setSources((s) => (v ? [...s, i.itemId] : s.filter((x) => x !== i.itemId)))}>{i.title}</Toggle> <span className="pk-dim">{MATERIAL[i.status]?.text} v{i.version}</span></li>)}</ul>
        )}
        <p className="pk-note"><Button size="small" variant="primary" disabled={busy || !kind?.available} onClick={draft}>Draft and check</Button></p>
        {msg && <p className="pk-note" role="alert">{msg}</p>}
      </Card>
      {changes === null ? <p className="pk-note" role="status">Loading website changes…</p> : changes.length === 0 ? <p className="pk-note">No website changes yet.</p> : changes.map((c) => <ChangeCard key={c.changeId} id={id} c={c} calls={calls} onChange={reload} />)}
    </>
  );
}

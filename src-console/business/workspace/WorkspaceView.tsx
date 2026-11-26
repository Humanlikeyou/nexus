import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Badge, Button, Card, PageHero, SegmentedControl, Select, StatusText, TextField, Toggle } from "../../ui";
import {
  ACTOR_LABEL, CONNECTOR_STATE, INSIGHT_KIND, INSIGHT_STATUS, MATERIAL, SECTIONS, SYNC_STATE, errText, liveWorkspaceCalls, sectionLabel, sourceText, whenText,
  type Activity, type BizDocument, type Business, type Connector, type ContextItem, type ContextVersion, type KnowledgeHit, type MemoryNote,
  type Insight, type Overview, type Permission, type Proposal, type Recommendation, type Report, type WorkspaceCalls,
} from "./workspaceApi";
import { SiteOpsTab } from "./SiteOpsView";
import { BusinessBriefingTab } from "./BusinessBriefingView";
import "../../ui/PanelKit.css";

/**
 * Decision 0063 (Phase 11): Enterprise › Workspaces, one workspace per business (design boundary lifted for
 * the Enterprise host). Everything shown comes from the Rust workspace store for the selected business only.
 * No revenue is shown here: business income stays in Finances and Treasury, and ARM trading stays apart.
 */
type Tab = "overview" | "briefing" | "context" | "documents" | "integrations" | "intelligence" | "website" | "permissions" | "approvals" | "activity";
const TABS: Array<{ value: Tab; label: string }> = [
  { value: "overview", label: "Summary" }, { value: "briefing", label: "Briefing" }, { value: "context", label: "Context & memory" }, { value: "documents", label: "Documents" },
  { value: "integrations", label: "Integrations" }, { value: "intelligence", label: "Intelligence" }, { value: "website", label: "Website changes" }, { value: "permissions", label: "Permissions" }, { value: "approvals", label: "Approvals" }, { value: "activity", label: "Activity" },
];

function Material({ status }: { status: string }) {
  const m = MATERIAL[status] ?? { text: status, tone: "neutral" as const };
  return <Badge tone={m.tone}>{m.text}</Badge>;
}

/** Loads `load` for one business; shows loading, error with Retry, or the children. */
function useLoad<T>(load: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [n, setN] = useState(0);
  useEffect(() => {
    let live = true;
    load().then((d) => { if (live) { setData(d); setErr(null); } }, (e: unknown) => { if (live) setErr(errText(e)); });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, n]);
  return { data, err, reload: () => setN((x) => x + 1), setData };
}

function Loading({ err, retry, what }: { err: string | null; retry: () => void; what: string }) {
  if (err) return <p className="pk-note" role="alert"><StatusText kind="fail">{`Couldn't load ${what}: ${err}`}</StatusText> <Button size="small" variant="plain" onClick={retry}>Retry</Button></p>;
  return <p className="pk-note" role="status">Loading {what}…</p>;
}

// ─── Overview ────────────────────────────────────────────────────────────────

function OverviewTab({ id, calls, onChanged }: { id: string; calls: WorkspaceCalls; onChanged: () => void }) {
  const { data, err, reload } = useLoad<Overview>(() => calls.overview(id), [id]);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(""); const [desc, setDesc] = useState(""); const [msg, setMsg] = useState("");
  if (!data) return <Loading err={err} retry={reload} what="the workspace" />;
  const b = data.business;
  const save = () => calls.update(id, name, desc.trim() || null, b.ventureId, b.status).then(() => { setEditing(false); setMsg("Saved."); reload(); onChanged(); }, (e: unknown) => setMsg(errText(e)));
  const archive = () => calls.update(id, b.name, b.description, b.ventureId, b.status === "active" ? "archived" : "active").then(() => { reload(); onChanged(); }, (e: unknown) => setMsg(errText(e)));
  return (
    <>
      <Card title={b.name} headingLevel={3} accessory={<span className="pk-acc">{b.status === "active" ? "Active" : "Archived"}</span>}>
        {editing ? (
          <>
            <TextField label="Business name" value={name} onChange={(e) => setName(e.target.value)} />
            <TextField label="Description" value={desc} onChange={(e) => setDesc(e.target.value)} />
            <p className="pk-note"><Button size="small" variant="primary" onClick={() => void save()}>Save</Button> <Button size="small" variant="plain" onClick={() => setEditing(false)}>Cancel</Button></p>
          </>
        ) : (
          <>
            <p className="pk-note">{b.description || "No description yet."}</p>
            <p className="pk-note">Workspace id {b.businessId} · {b.ventureId ? `linked to the ledger venture "${b.ventureId}"` : "not linked to a ledger venture"} · created {whenText(b.createdAt)}</p>
            <p className="pk-note"><Button size="small" variant="plain" onClick={() => { setName(b.name); setDesc(b.description ?? ""); setEditing(true); }}>Edit details</Button> <Button size="small" variant="plain" onClick={() => void archive()}>{b.status === "active" ? "Archive workspace" : "Restore workspace"}</Button></p>
          </>
        )}
        {msg && <p className="pk-note" role="status">{msg}</p>}
      </Card>
      <Card title="Business context" headingLevel={3}>
        <ul className="pk-list" aria-label="Context by section">
          {data.sections.map((s) => (
            <li key={s.section}><b>{s.label}</b> · {s.approved + s.draft + s.unverified === 0 ? "nothing yet" : [s.approved && `${s.approved} approved`, s.draft && `${s.draft} draft`, s.unverified && `${s.unverified} unverified`].filter(Boolean).join(", ")}</li>
          ))}
        </ul>
        <p className="pk-note">Documents: {data.documents} ({data.approvedDocuments} approved) · Memory notes kept: {data.memoryNotes} · Changes waiting for your approval: {data.pendingProposals}</p>
      </Card>
      <Card title="Integrations" headingLevel={3}>
        <ul className="pk-list" aria-label="Integration status">
          {data.connectors.map((c) => { const s = CONNECTOR_STATE[c.state] ?? { text: c.state, kind: "off" as const }; return <li key={c.connectorId}>{c.label}: <StatusText kind={s.kind}>{s.text}</StatusText></li>; })}
        </ul>
        <p className="pk-note">Nothing here writes outside the app. Business income stays in Finances and Treasury; ARM trading is kept separate.</p>
      </Card>
    </>
  );
}

// ─── Context and memory ──────────────────────────────────────────────────────

function ItemEditor({ start, section, onSave, onCancel }: { start: ContextItem | null; section: string; onSave: (title: string, body: string, status: "draft" | "unverified") => Promise<void>; onCancel: () => void }) {
  const [title, setTitle] = useState(start?.title ?? ""); const [body, setBody] = useState(start?.body ?? ""); const [err, setErr] = useState("");
  return (
    <Card title={start ? `Edit: ${start.title}` : `New item in ${sectionLabel(section)}`} headingLevel={4}>
      <TextField label="Title" value={title} onChange={(e) => setTitle(e.target.value)} />
      <label className="pk-note" style={{ display: "grid", gap: 4 }}>Text<textarea aria-label="Text" rows={8} value={body} onChange={(e) => setBody(e.target.value)} /></label>
      {start?.status === "approved" && <p className="pk-note">This item is approved. Saving an edit makes it a draft until you approve it again.</p>}
      {err && <p className="pk-note" role="alert">{err}</p>}
      <p className="pk-note"><Button size="small" variant="primary" onClick={() => void onSave(title, body, "draft").catch((e: unknown) => setErr(errText(e)))}>Save as draft</Button> <Button size="small" variant="plain" onClick={onCancel}>Cancel</Button></p>
    </Card>
  );
}

function History({ id, item, calls }: { id: string; item: ContextItem; calls: WorkspaceCalls }) {
  const { data, err, reload } = useLoad<ContextVersion[]>(() => calls.contextHistory(id, item.itemId), [id, item.itemId, item.version]);
  if (!data) return <Loading err={err} retry={reload} what="the history" />;
  return <ol className="pk-list" aria-label={`History of ${item.title}`}>{data.map((v) => <li key={v.version}>v{v.version} · {MATERIAL[v.status]?.text ?? v.status} · {v.actor === "taisun" ? "you" : v.actor} · {whenText(v.at)}{v.note ? ` · ${v.note}` : ""}</li>)}</ol>;
}

function ContextTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const [section, setSection] = useState("identity");
  const { data, err, reload } = useLoad<ContextItem[]>(() => calls.context(id, section), [id, section]);
  const [edit, setEdit] = useState<ContextItem | "new" | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [msg, setMsg] = useState("");
  const act = (p: Promise<unknown>, done: string) => p.then(() => { setMsg(done); reload(); }, (e: unknown) => setMsg(errText(e)));
  const save = async (title: string, body: string, status: "draft" | "unverified") => {
    const cur = edit === "new" ? null : edit;
    await calls.saveContext(id, { itemId: cur?.itemId ?? null, section, title, body, status, expectedVersion: cur?.version ?? null });
    setEdit(null); setMsg("Saved as a draft."); reload();
  };
  return (
    <>
      <Select label="Section" options={SECTIONS} value={section} onChange={(e) => { setSection(e.target.value); setEdit(null); setOpen(null); }} />
      {msg && <p className="pk-note" role="status">{msg}</p>}
      {edit && <ItemEditor key={edit === "new" ? "new" : edit.itemId} start={edit === "new" ? null : edit} section={section} onSave={save} onCancel={() => setEdit(null)} />}
      {!data ? <Loading err={err} retry={reload} what="this section" /> : (
        <>
          {data.length === 0 ? <p className="pk-note">Nothing in {sectionLabel(section)} yet.{section === "formulations" ? " Formulations are never taken from the website source; add the approved ones yourself." : ""}</p> : (
            <ul className="pk-list" aria-label={sectionLabel(section)}>
              {data.map((i) => (
                <li key={i.itemId}>
                  <b>{i.title}</b> <Material status={i.status} /> <span className="pk-dim">· {sourceText(i.source, i.sourceRef)} · v{i.version} · {whenText(i.updatedAt)}</span>
                  {i.status === "unverified" && <p className="pk-note">Not checked by you. Amara is not given unverified material unless you allow it in Permissions.</p>}
                  <pre className="pk-note" style={{ whiteSpace: "pre-wrap", margin: "4px 0" }}>{i.body}</pre>
                  <span className="pk-acc" style={{ marginLeft: 0 }}>
                    {i.status !== "approved" && i.status !== "archived" && <Button size="small" variant="primary" aria-label={`Approve ${i.title}`} onClick={() => void act(calls.setContextStatus(id, i.itemId, "approved", i.version), `Approved: ${i.title}`)}>Approve</Button>}
                    {i.status === "approved" && <Button size="small" variant="plain" aria-label={`Return ${i.title} to draft`} onClick={() => void act(calls.setContextStatus(id, i.itemId, "draft", i.version), `Back to draft: ${i.title}`)}>Back to draft</Button>}
                    {i.status !== "archived" ? <Button size="small" variant="plain" aria-label={`Archive ${i.title}`} onClick={() => void act(calls.setContextStatus(id, i.itemId, "archived", i.version), `Archived: ${i.title}`)}>Archive</Button>
                      : <Button size="small" variant="plain" aria-label={`Restore ${i.title}`} onClick={() => void act(calls.setContextStatus(id, i.itemId, "draft", i.version), `Restored as a draft: ${i.title}`)}>Restore</Button>}
                    <Button size="small" variant="plain" aria-label={`Edit ${i.title}`} onClick={() => setEdit(i)}>Edit</Button>
                    <Button size="small" variant="plain" aria-expanded={open === i.itemId} aria-label={`History of ${i.title}`} onClick={() => setOpen(open === i.itemId ? null : i.itemId)}>History</Button>
                  </span>
                  {open === i.itemId && <History id={id} item={i} calls={calls} />}
                </li>
              ))}
            </ul>
          )}
          {!edit && <p className="pk-note"><Button size="small" variant="secondary" onClick={() => setEdit("new")}>Add an item</Button></p>}
        </>
      )}
      <MemoryCard id={id} calls={calls} />
    </>
  );
}

function MemoryCard({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const { data, err, reload } = useLoad<MemoryNote[]>(() => calls.memory(id), [id]);
  const [text, setText] = useState(""); const [msg, setMsg] = useState("");
  const add = () => calls.addMemory(id, text).then(() => { setText(""); setMsg("Kept."); reload(); }, (e: unknown) => setMsg(errText(e)));
  return (
    <Card title="Business memory" headingLevel={3} accessory={<span className="pk-acc">This business only</span>}>
      <p className="pk-note">Notes Amara keeps about this business. They never mix with your personal memory. Notes Amara suggests wait for you.</p>
      {!data ? <Loading err={err} retry={reload} what="memory" /> : data.length === 0 ? <p className="pk-note">No notes yet.</p> : (
        <ul className="pk-list" aria-label="Memory notes">
          {data.map((m) => (
            <li key={m.memoryId}>{m.text} <span className="pk-dim">· {m.status === "proposed" ? "suggested by Amara" : m.status === "kept" ? "kept" : "dismissed"} · {whenText(m.createdAt)}</span>
              {m.status === "proposed" && <> <Button size="small" variant="primary" onClick={() => void calls.decideMemory(id, m.memoryId, "kept").then(reload, (e: unknown) => setMsg(errText(e)))}>Keep</Button> <Button size="small" variant="plain" onClick={() => void calls.decideMemory(id, m.memoryId, "dismissed").then(reload, (e: unknown) => setMsg(errText(e)))}>Dismiss</Button></>}
              {m.status === "kept" && <> <Button size="small" variant="plain" aria-label={`Forget note: ${m.text}`} onClick={() => void calls.decideMemory(id, m.memoryId, "dismissed").then(reload, (e: unknown) => setMsg(errText(e)))}>Forget</Button></>}
            </li>
          ))}
        </ul>
      )}
      <TextField label="New memory note" value={text} onChange={(e) => setText(e.target.value)} />
      <p className="pk-note"><Button size="small" variant="secondary" disabled={!text.trim()} onClick={() => void add()}>Keep this note</Button></p>
      {msg && <p className="pk-note" role="status">{msg}</p>}
    </Card>
  );
}

// ─── Documents and knowledge ─────────────────────────────────────────────────

function DocumentsTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const { data, err, reload } = useLoad<BizDocument[]>(() => calls.documents(id), [id]);
  const [msg, setMsg] = useState("");
  const [q, setQ] = useState(""); const [hits, setHits] = useState<KnowledgeHit[] | null>(null);
  const act = (p: Promise<unknown>, done: string) => p.then(() => { setMsg(done); reload(); }, (e: unknown) => setMsg(errText(e)));
  const add = () => calls.addDocument(id).then((d) => { setMsg(d ? `Added as a draft: ${d.title}` : "No file chosen."); reload(); }, (e: unknown) => setMsg(errText(e)));
  const search = () => calls.search(id, q).then(setHits, (e: unknown) => { setHits(null); setMsg(errText(e)); });
  return (
    <>
      <Card title="Search this business's knowledge" headingLevel={3}>
        <TextField label="Search context and documents" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") void search(); }} />
        <p className="pk-note"><Button size="small" variant="secondary" disabled={q.trim().length < 2} onClick={() => void search()}>Search</Button></p>
        {hits && (hits.length === 0 ? <p className="pk-note" role="status">No matches.</p> : (
          <ul className="pk-list" aria-label="Search results">{hits.map((h) => <li key={`${h.kind}-${h.id}`}><b>{h.title}</b> <Material status={h.status} /> <span className="pk-dim">· {h.kind === "context" ? sectionLabel(h.section ?? "") : "Document"}</span><br /><span className="pk-note">{h.excerpt}</span></li>)}</ul>
        ))}
      </Card>
      <Card title="Documents" headingLevel={3} accessory={<span className="pk-acc"><Button size="small" variant="secondary" onClick={() => void add()}>Add a document…</Button></span>}>
        {msg && <p className="pk-note" role="status">{msg}</p>}
        {!data ? <Loading err={err} retry={reload} what="documents" /> : data.length === 0 ? <p className="pk-note">No documents yet. Add one, or sync the website source in Integrations.</p> : (
          <ul className="pk-list" aria-label="Business documents">
            {data.map((d) => (
              <li key={d.docId}><b>{d.title}</b> <Material status={d.status} /> <span className="pk-dim">· {d.kind} · {(d.bytes / 1024).toFixed(1)} KB · {sourceText(d.source, d.sourceRef)} · {d.searchable ? "searchable" : "not searchable (not text)"}</span>
                <span className="pk-acc" style={{ marginLeft: 0 }}>
                  {d.status !== "approved" && d.status !== "archived" && <Button size="small" variant="primary" aria-label={`Approve ${d.title}`} onClick={() => void act(calls.setDocumentStatus(id, d.docId, "approved", d.version), `Approved: ${d.title}`)}>Approve</Button>}
                  {d.status !== "archived" ? <Button size="small" variant="plain" aria-label={`Archive ${d.title}`} onClick={() => void act(calls.setDocumentStatus(id, d.docId, "archived", d.version), `Archived: ${d.title}`)}>Archive</Button>
                    : <Button size="small" variant="plain" aria-label={`Restore ${d.title}`} onClick={() => void act(calls.setDocumentStatus(id, d.docId, "draft", d.version), `Restored: ${d.title}`)}>Restore</Button>}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </>
  );
}

// ─── Integrations ────────────────────────────────────────────────────────────

function SetupSql({ calls }: { calls: WorkspaceCalls }) {
  const [sql, setSql] = useState<string | null>(null);
  const [msg, setMsg] = useState("");
  if (sql === null) return <p className="pk-note"><Button size="small" variant="plain" onClick={() => void calls.setupSql().then(setSql, (e: unknown) => setMsg(errText(e)))}>Show the setup SQL</Button>{msg && ` ${msg}`}</p>;
  return (
    <>
      <p className="pk-note">Run this once in the Supabase SQL editor, after replacing the password placeholder. It creates a role that can only read the listed, non-identifying columns.</p>
      <pre className="pk-note" style={{ whiteSpace: "pre-wrap", maxHeight: 220, overflow: "auto" }} aria-label="Setup SQL">{sql}</pre>
      <p className="pk-note"><Button size="small" variant="plain" onClick={() => void navigator.clipboard?.writeText(sql).then(() => setMsg("Copied."), () => setMsg("Couldn't copy; select the text instead."))}>Copy</Button> <Button size="small" variant="plain" onClick={() => setSql(null)}>Hide</Button>{msg && ` ${msg}`}</p>
    </>
  );
}

function ConnectorCard({ id, c, calls, onChange }: { id: string; c: Connector; calls: WorkspaceCalls; onChange: (c: Connector) => void }) {
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(c.config.map((f) => [f.name, f.value ?? ""])));
  const [secret, setSecret] = useState<Record<string, string>>({});
  const [msg, setMsg] = useState(""); const [busy, setBusy] = useState(false);
  const s = CONNECTOR_STATE[c.state] ?? { text: c.state, kind: "off" as const };
  const run = (p: Promise<Connector | null>, done: string) => { setBusy(true); return p.then((v) => { if (v) onChange(v); setMsg(v ? done : "Nothing chosen."); }, (e: unknown) => setMsg(errText(e))).finally(() => setBusy(false)); };
  const local = c.check === "local", deferred = c.check === "deferred";
  const typed = c.config.filter((f) => f.name !== "ca_cert");
  const complete = c.config.every((f) => f.value) && c.secrets.every((f) => f.stored);
  const sync = () => { setBusy(true); setMsg("Synchronizing…"); calls.sync(id, c.connectorId).then((r) => { setMsg(`${SYNC_STATE[r.status] ?? r.status}: ${r.detail ?? ""}`); return calls.connectors(id).then((all) => { const me = all.find((x) => x.connectorId === c.connectorId); if (me) onChange(me); }); }, (e: unknown) => setMsg(errText(e))).finally(() => setBusy(false)); };
  return (
    <Card title={c.label} headingLevel={3} accessory={<span className="pk-acc"><StatusText kind={s.kind}>{s.text}</StatusText></span>}>
      <p className="pk-note">{c.description}</p>
      <p className="pk-note" role="status">{c.detail}{!deferred && <span className="pk-dim"> ({c.checkedAt ? `checked ${whenText(c.checkedAt)}` : "not checked yet"})</span>}</p>
      {!deferred && (local ? (
        <p className="pk-note">Folder: {c.config[0]?.value ?? "not chosen"} <Button size="small" variant="secondary" disabled={busy} onClick={() => void run(calls.pickFolder(id, c.connectorId), "Folder saved.")}>Choose folder…</Button></p>
      ) : (
        <>
          {typed.map((f) => <TextField key={f.name} label={f.label} placeholder={f.hint} value={values[f.name] ?? ""} onChange={(e) => setValues((v) => ({ ...v, [f.name]: e.target.value }))} />)}
          {typed.length > 0 && <p className="pk-note"><Button size="small" variant="secondary" disabled={busy} onClick={() => void run(calls.configure(id, c.connectorId, Object.fromEntries(typed.map((f) => [f.name, values[f.name] ?? ""]))), "Settings saved.")}>Save settings</Button></p>}
          {c.config.filter((f) => f.name === "ca_cert").map((f) => (
            <p key={f.name} className="pk-note">{f.label}: {f.value ?? "not chosen"} <span className="pk-dim">({f.hint})</span> <Button size="small" variant="secondary" disabled={busy} onClick={() => void run(calls.pickFile(id, c.connectorId, f.name), "Certificate saved.")}>Choose certificate…</Button></p>
          ))}
          {c.secrets.map((f) => (
            <div key={f.name}>
              <TextField label={f.label} type="password" autoComplete="off" description={f.stored ? "Stored in the Keychain. Enter a new value to replace it." : f.hint} value={secret[f.name] ?? ""} onChange={(e) => setSecret((v) => ({ ...v, [f.name]: e.target.value }))} />
              <p className="pk-note"><Button size="small" variant="secondary" disabled={busy || !(secret[f.name] ?? "").trim()} onClick={() => { const value = secret[f.name] ?? ""; setSecret((v) => ({ ...v, [f.name]: "" })); void run(calls.setSecret(id, c.connectorId, f.name, value), "Stored in the Keychain."); }}>Store in Keychain</Button> {f.stored ? <StatusText kind="ok">Stored</StatusText> : <StatusText kind="off">Not stored</StatusText>}</p>
            </div>
          ))}
          {c.kind === "supabase" && <SetupSql calls={calls} />}
        </>
      ))}
      {!deferred && <p className="pk-note">{SYNC_STATE[c.sync.state] ?? c.sync.state}{c.sync.last ? ` · ${whenText(c.sync.last.finishedAt ?? c.sync.last.startedAt)}` : ""}{c.sync.detail ? ` · ${c.sync.detail}` : ""}</p>}
      {!deferred && (
        <p className="pk-note">
          {!local && <Button size="small" variant="secondary" disabled={busy || !complete} onClick={() => { setBusy(true); setMsg("Checking…"); calls.check(id, c.connectorId).then((v) => { onChange(v); setMsg(""); }, (e: unknown) => setMsg(errText(e))).finally(() => setBusy(false)); }}>Check connection</Button>}{" "}
          <Button size="small" variant="primary" disabled={busy || c.state !== "connected"} onClick={sync}>Sync now</Button>{" "}
          {c.state !== "disconnected" && <Button size="small" variant="plain" disabled={busy} onClick={() => void run(calls.disconnect(id, c.connectorId), "Disconnected.")}>Disconnect…</Button>}
        </p>
      )}
      <p className="pk-note">Writes outside the app: none.</p>
      {msg && <p className="pk-note" role="status">{msg}</p>}
    </Card>
  );
}

function IntegrationsTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const { data, err, reload, setData } = useLoad<Connector[]>(() => calls.connectors(id), [id]);
  if (!data) return <Loading err={err} retry={reload} what="integrations" />;
  return (
    <>
      <p className="pk-note">Everything here is read-only and runs only when you choose Check or Sync. A connection shows as connected only after a check passed. Keys go to the macOS Keychain and are never shown again. Network use follows the outbound switch: while it is off, nothing is sent.</p>
      {data.map((c) => <ConnectorCard key={c.connectorId} id={id} c={c} calls={calls} onChange={(n) => setData((all) => (all ?? []).map((x) => (x.connectorId === n.connectorId ? n : x)))} />)}
    </>
  );
}

// ─── Intelligence ────────────────────────────────────────────────────────────

function InsightView({ i }: { i: Insight }) {
  const k = INSIGHT_KIND[i.kind] ?? { text: i.kind, tone: "neutral" as const };
  const pct = (x: number) => `${(x * 100).toFixed(x < 0.1 ? 1 : 0)}%`;
  const value = i.value == null ? null : i.unit === "share" ? pct(i.value) : `${Number.isInteger(i.value) ? i.value : i.value.toFixed(1)}${i.unit && i.unit !== "share" ? ` ${i.unit}` : ""}`;
  return (
    <li>
      <b>{i.title}</b> <Badge tone={k.tone}>{k.text}</Badge>{i.status !== "available" && <> <Badge tone="neutral">{INSIGHT_STATUS[i.status] ?? i.status}</Badge></>}
      {value && <span className="pk-num"> · {value}</span>}
      <br /><span className="pk-note">{i.statement}{i.n != null ? ` (n = ${i.n})` : ""}{i.window ? ` · ${i.window}` : ""}</span>
      {i.rows.length > 0 && (
        <ul className="pk-list" aria-label={i.title}>
          {i.rows.map((r) => <li key={r.label} className="pk-num">{r.label}: {r.value == null ? "not available" : Number.isInteger(r.value) ? r.value : r.value.toFixed(1)}{r.share != null ? ` (${pct(r.share)})` : ""}</li>)}
        </ul>
      )}
      {i.note && <p className="pk-note">{i.note}</p>}
      {i.sources.length > 0 && <p className="pk-note pk-dim">Source: {i.sources.join("; ")}</p>}
    </li>
  );
}

function ReportView({ id, calls, which }: { id: string; calls: WorkspaceCalls; which: "website" | "customers" }) {
  const { data, err, reload, setData } = useLoad<Report>(() => calls.report(id, which), [id, which]);
  const [busy, setBusy] = useState(false); const [msg, setMsg] = useState("");
  if (!data) return <Loading err={err} retry={reload} what="the report" />;
  const groups = [...new Set(data.insights.map((i) => i.group))];
  const ask = () => { setBusy(true); setMsg("Amara is reading the report on this Mac…"); calls.interpret(id, which).then((r) => { setData(r); setMsg(""); }, (e: unknown) => setMsg(errText(e))).finally(() => setBusy(false)); };
  return (
    <>
      <p className="pk-note">Generated {whenText(data.generatedAt)}. Every line says whether it is measured, calculated, an AI interpretation or an unverified hypothesis. Missing figures are marked as not available and never estimated.{which === "customers" ? " Customer figures are aggregates; groups under 5 people are hidden." : ""}</p>
      {groups.map((g) => (
        <Card key={g} title={g} headingLevel={3}>
          <ul className="pk-list" aria-label={g}>{data.insights.filter((i) => i.group === g).map((i) => <InsightView key={i.id} i={i} />)}</ul>
        </Card>
      ))}
      <Card title="Amara's reading" headingLevel={3} accessory={<span className="pk-acc">Local model only</span>}>
        {data.notes.length === 0 ? <p className="pk-note">No interpretation yet.</p> : (
          <ul className="pk-list" aria-label="Amara's notes">
            {data.notes.map((n) => <li key={n.noteId}><Badge tone={INSIGHT_KIND[n.kind]?.tone ?? "neutral"}>{INSIGHT_KIND[n.kind]?.text ?? n.kind}</Badge> {n.statement} <span className="pk-dim">· based on {n.basedOn.map((b) => data.insights.find((i) => i.id === b)?.title ?? b).join(", ")}</span></li>)}
          </ul>
        )}
        <p className="pk-note"><Button size="small" variant="secondary" disabled={busy} onClick={ask}>Ask Amara to interpret</Button> <span className="pk-dim">Only the aggregated figures above are given to Amara, on this Mac, never the cloud.</span></p>
        {msg && <p className="pk-note" role="status">{msg}</p>}
      </Card>
    </>
  );
}

function IntelligenceTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const [which, setWhich] = useState<"website" | "customers">("website");
  return (
    <>
      <SegmentedControl label="Report" options={[{ value: "website", label: "Website" }, { value: "customers", label: "Customers" }]} value={which} onChange={setWhich} />
      <ReportView key={which} id={id} calls={calls} which={which} />
    </>
  );
}

// ─── Permissions ─────────────────────────────────────────────────────────────

function PermissionsTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const { data, err, reload, setData } = useLoad<Permission[]>(() => calls.permissions(id), [id]);
  const seen = useLoad<ContextItem[]>(() => calls.contextForAmara(id), [id, data]);
  const [msg, setMsg] = useState("");
  if (!data) return <Loading err={err} retry={reload} what="permissions" />;
  const actors = [...new Set(data.map((p) => p.actor))];
  return (
    <>
      <p className="pk-note">You can always do everything. These switches decide what Amara, background jobs and integrations may do for this business only. Approving material, publishing and writing outside the app are never on this list.</p>
      {actors.map((a) => (
        <Card key={a} title={ACTOR_LABEL[a] ?? a} headingLevel={3}>
          <ul className="pk-list" aria-label={`${ACTOR_LABEL[a] ?? a} permissions`}>
            {data.filter((p) => p.actor === a).map((p) => (
              <li key={p.capability}><Toggle checked={p.allowed} onCheckedChange={(v) => void calls.setPermission(id, a, p.capability, v).then((n) => { setData(n); setMsg(""); }, (e: unknown) => setMsg(errText(e)))}>{p.label}</Toggle> <span className="pk-dim">{p.allowed ? "Allowed" : "Not allowed"}</span></li>
            ))}
          </ul>
        </Card>
      ))}
      <Card title="What Amara can see now" headingLevel={3}>
        {seen.err ? <p className="pk-note"><StatusText kind="off">{seen.err.startsWith("not permitted") ? "Nothing: reading this business's context is off." : seen.err}</StatusText></p>
          : !seen.data ? <p className="pk-note" role="status">Checking…</p>
            : <p className="pk-note">{seen.data.length === 0 ? "No items yet (only approved items are given by default)." : `${seen.data.length} item${seen.data.length === 1 ? "" : "s"}: ${seen.data.map((i) => `${i.title} (${MATERIAL[i.status]?.text ?? i.status})`).join(", ")}`}</p>}
      </Card>
      {msg && <p className="pk-note" role="alert">{msg}</p>}
    </>
  );
}

// ─── Approvals ───────────────────────────────────────────────────────────────

function ApprovalsTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const { data, err, reload } = useLoad<Proposal[]>(() => calls.proposals(id), [id]);
  const [msg, setMsg] = useState("");
  if (!data) return <Loading err={err} retry={reload} what="approvals" />;
  const pending = data.filter((p) => p.status === "pending"), done = data.filter((p) => p.status !== "pending");
  const STATUS: Record<string, string> = { applied: "Applied and verified", rejected: "Rejected", failed: "Not applied" };
  return (
    <>
      <p className="pk-note">Changes Amara proposes to this business's material wait here. Approve opens a system dialog with the full text; only then is it applied, and it is checked afterwards.</p>
      {msg && <p className="pk-note" role="status">{msg}</p>}
      {pending.length === 0 ? <p className="pk-note">Nothing is waiting for your approval.</p> : (
        <ul className="pk-list" aria-label="Waiting for approval">
          {pending.map((p) => (
            <li key={p.proposalId}><b>{p.title}</b> <span className="pk-dim">· proposed by {p.proposedBy === "amara" ? "Amara" : "a background job"} · {whenText(p.proposedAt)}</span>
              {p.rationale && <p className="pk-note">Why: {p.rationale}</p>}
              <pre className="pk-note" style={{ whiteSpace: "pre-wrap", margin: "4px 0" }}>{p.body}</pre>
              <Button size="small" variant="primary" onClick={() => void calls.approveProposal(id, p.proposalId).then((r) => { setMsg(`${STATUS[r.status] ?? r.status}: ${p.title}${r.detail ? ` (${r.detail})` : ""}`); reload(); }, (e: unknown) => setMsg(errText(e)))}>Approve…</Button>{" "}
              <Button size="small" variant="plain" onClick={() => void calls.rejectProposal(id, p.proposalId).then(() => { setMsg(`Rejected: ${p.title}`); reload(); }, (e: unknown) => setMsg(errText(e)))}>Reject</Button>
            </li>
          ))}
        </ul>
      )}
      {done.length > 0 && <Card title="Decided" headingLevel={3}><ul className="pk-list" aria-label="Decided approvals">{done.map((p) => <li key={p.proposalId}>{p.title} · {STATUS[p.status] ?? p.status} · {whenText(p.decidedAt)}{p.detail ? ` · ${p.detail}` : ""}</li>)}</ul></Card>}
    </>
  );
}

// ─── Activity ────────────────────────────────────────────────────────────────

function ActivityTab({ id, calls }: { id: string; calls: WorkspaceCalls }) {
  const a = useLoad<Activity[]>(() => calls.activity(id), [id]);
  const r = useLoad<Recommendation[]>(() => calls.recommendations(id), [id]);
  return (
    <>
      <Card title="Activity" headingLevel={3} accessory={<span className="pk-acc">This business only</span>}>
        {!a.data ? <Loading err={a.err} retry={a.reload} what="activity" /> : a.data.length === 0 ? <p className="pk-note">No activity yet.</p> : (
          <ul className="pk-list" aria-label="Business activity">{a.data.map((x) => <li key={x.activityId}>{x.summary ?? x.kind} <span className="pk-dim">· {x.actor === "taisun" ? "you" : x.actor === "amara" ? "Amara" : "the app"} · {whenText(x.occurredAt)}</span></li>)}</ul>
        )}
      </Card>
      <Card title="Recommendations for this business" headingLevel={3}>
        {!r.data ? <Loading err={r.err} retry={r.reload} what="recommendations" /> : r.data.length === 0 ? <p className="pk-note">None yet. Recommendations about this business will be listed here with their evidence.</p> : (
          <ul className="pk-list" aria-label="Business recommendations">{r.data.map((x) => <li key={x.recommendationId}><b>{x.title}</b> · {x.status} · {whenText(x.createdAt)}</li>)}</ul>
        )}
      </Card>
    </>
  );
}

// ─── Host ────────────────────────────────────────────────────────────────────

export function WorkspaceView({ calls = liveWorkspaceCalls }: { calls?: WorkspaceCalls }) {
  const [list, setList] = useState<Business[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState(""); const [desc, setDesc] = useState(""); const [msg, setMsg] = useState("");
  const load = useCallback(() => {
    calls.list().then((l) => { setList(l); setErr(null); setSel((s) => (s && l.some((b) => b.businessId === s) ? s : (l.find((b) => b.status === "active") ?? l[0])?.businessId ?? null)); }, (e: unknown) => setErr(errText(e)));
  }, [calls]);
  useEffect(() => { load(); }, [load]);
  const create = () => calls.create(name, desc.trim() || null).then((b) => { setAdding(false); setName(""); setDesc(""); setMsg(`Created: ${b.name}`); setSel(b.businessId); setTab("overview"); load(); }, (e: unknown) => setMsg(errText(e)));
  const current = list?.find((b) => b.businessId === sel) ?? null;
  let body: ReactNode = null;
  if (current) {
    const id = current.businessId;
    body = (
      <>
        <SegmentedControl label="Workspace sections" options={TABS} value={tab} onChange={setTab} />
        {tab === "overview" && <OverviewTab key={id} id={id} calls={calls} onChanged={load} />}
        {tab === "briefing" && <BusinessBriefingTab key={id} id={id} />}
        {tab === "context" && <ContextTab key={id} id={id} calls={calls} />}
        {tab === "documents" && <DocumentsTab key={id} id={id} calls={calls} />}
        {tab === "integrations" && <IntegrationsTab key={id} id={id} calls={calls} />}
        {tab === "intelligence" && <IntelligenceTab key={id} id={id} calls={calls} />}
        {tab === "website" && <SiteOpsTab key={id} id={id} calls={calls} />}
        {tab === "permissions" && <PermissionsTab key={id} id={id} calls={calls} />}
        {tab === "approvals" && <ApprovalsTab key={id} id={id} calls={calls} />}
        {tab === "activity" && <ActivityTab key={id} id={id} calls={calls} />}
      </>
    );
  }
  return (
    <div className="v-page pk-host" id="workspaces">
      <PageHero title="Workspaces" statement={current ? current.name : "Business workspaces"} detail="Each business keeps its own context, documents, memory, integrations and history, apart from your personal data and from every other business." />
      {err && <p className="pk-note" role="alert"><StatusText kind="fail">{`Couldn't load workspaces: ${err}`}</StatusText> <Button size="small" variant="plain" onClick={load}>Retry</Button></p>}
      {!list && !err && <p className="pk-note" role="status">Loading workspaces…</p>}
      {list && (
        <p className="pk-note">
          {list.length > 1 && <Select label="Business" options={list.map((b) => ({ value: b.businessId, label: b.status === "archived" ? `${b.name} (archived)` : b.name }))} value={sel ?? ""} onChange={(e) => { setSel(e.target.value); setTab("overview"); }} />}
          {!adding && <Button size="small" variant="plain" onClick={() => setAdding(true)}>New business</Button>}
        </p>
      )}
      {adding && (
        <Card title="New business workspace" headingLevel={3}>
          <TextField label="Business name" value={name} onChange={(e) => setName(e.target.value)} />
          <TextField label="Description (optional)" value={desc} onChange={(e) => setDesc(e.target.value)} />
          <p className="pk-note"><Button size="small" variant="primary" disabled={!name.trim()} onClick={() => void create()}>Create</Button> <Button size="small" variant="plain" onClick={() => setAdding(false)}>Cancel</Button></p>
        </Card>
      )}
      {msg && <p className="pk-note" role="status">{msg}</p>}
      {list && list.length === 0 && <p className="pk-note">No workspaces yet.</p>}
      {body}
    </div>
  );
}

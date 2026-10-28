import { useCallback, useEffect, useState, type FormEvent } from "react";
import { correctTerrenceContextItem, deleteTerrenceContextItem, listTerrenceContextItems } from "../bridge";
import { date } from "../lib/format";
import type { TerrenceContextItem } from "../types";
import { Check, Pencil, Trash2, X } from "lucide-react";

export type ContextCorrectionDraft = {
  item: TerrenceContextItem;
  title: string;
  content: string;
  status: TerrenceContextItem["status"];
  confidence: string;
};

export type ContextScope = "current" | "history";

export function ContextInspector({ visible, onClose, onChanged }: { visible: boolean; onClose: () => void; onChanged: () => Promise<void> }) {
  const [items, setItems] = useState<TerrenceContextItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<ContextCorrectionDraft | null>(null);
  const [pendingDeletion, setPendingDeletion] = useState<string | null>(null);
  const [scope, setScope] = useState<ContextScope>("current");
  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setItems(await listTerrenceContextItems(scope === "current"
        ? { statuses: ["active", "blocked", "paused"], limit: 120 }
        : { statuses: ["historical", "superseded", "completed", "archived"], limit: 120 }));
    } catch {
      setError("Context is unavailable right now.");
    } finally {
      setLoading(false);
    }
  }, [scope]);

  useEffect(() => {
    if (!visible) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- reset the inspector's local state when it closes; moving this to a key/remount would change load timing
      setEditing(null);
      setPendingDeletion(null);
      setScope("current");
      return;
    }
    void load();
  }, [visible, load]);

  if (!visible) return null;

  const startCorrection = (item: TerrenceContextItem) => {
    setPendingDeletion(null);
    setEditing({ item, title: item.title, content: item.content || "", status: item.status, confidence: String(item.confidence) });
  };

  const selectScope = (nextScope: ContextScope) => {
    setScope(nextScope);
    setEditing(null);
    setPendingDeletion(null);
  };

  const saveCorrection = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!editing || saving) return;
    const confidence = Number(editing.confidence);
    if (!Number.isFinite(confidence) || confidence < 0 || confidence > 1 || !editing.title.trim()) {
      setError("Enter a title and a confidence from 0 to 1.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await correctTerrenceContextItem({
        originalId: editing.item.id,
        replacement: {
          kind: editing.item.kind,
          title: editing.title.trim(),
          content: editing.content.trim() || null,
          status: editing.status,
          confidence,
          provenanceSource: "user_correction",
          provenanceRef: `context:${editing.item.id}`,
          occurredAt: editing.item.occurredAt,
          metadata: { ...editing.item.metadata, correctedFrom: editing.item.id },
        },
      });
      setEditing(null);
      await Promise.all([load(), onChanged()]);
    } catch {
      setError("The correction could not be saved.");
    } finally {
      setSaving(false);
    }
  };

  const hideItem = async (item: TerrenceContextItem) => {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      await deleteTerrenceContextItem(item.id);
      setPendingDeletion(null);
      await Promise.all([load(), onChanged()]);
    } catch {
      setError("The context item could not be hidden.");
    } finally {
      setSaving(false);
    }
  };

  return <div className="context-inspector-backdrop">
    <section className="context-inspector" role="dialog" aria-modal="true" aria-label="Terrence context">
      <header><div><span className="system-kicker">Terrence context</span><h2>What Terrence knows</h2></div><button type="button" onClick={onClose} title="Close context"><X size={18} /></button></header>
      <div className={`context-inspector-body${editing ? " is-editing" : ""}`}>
        <div className="context-item-list" aria-live="polite">
          <div className="context-scope-tabs" role="tablist" aria-label="Terrence context view"><button type="button" role="tab" aria-selected={scope === "current"} className={scope === "current" ? "is-selected" : ""} onClick={() => selectScope("current")}>Current</button><button type="button" role="tab" aria-selected={scope === "history"} className={scope === "history" ? "is-selected" : ""} onClick={() => selectScope("history")}>History</button></div>
          {loading && <p className="context-empty">Loading context.</p>}
          {!loading && !error && items.length === 0 && <p className="context-empty">No {scope === "current" ? "current" : "historical"} context is recorded.</p>}
          {items.map((item) => <article key={item.id} className={editing?.item.id === item.id ? "is-editing" : ""}>
            <div className="context-item-heading"><strong>{item.title}</strong><span>{item.kind}</span></div>
            {item.content && <p>{item.content}</p>}
            <footer><span>{item.status} · {Math.round(item.confidence * 100)}% confidence</span><span>{item.provenanceSource}</span><time>{date(item.updatedAt)}</time></footer>
            {scope === "current" && <><div className="context-item-actions"><button type="button" onClick={() => startCorrection(item)} disabled={saving} title={`Correct ${item.title}`}><Pencil size={14} /></button><button type="button" onClick={() => setPendingDeletion(item.id)} disabled={saving} title={`Hide ${item.title}`}><Trash2 size={14} /></button></div>{pendingDeletion === item.id && <div className="context-delete-confirm"><span>Hide this from Terrence?</span><button type="button" onClick={() => void hideItem(item)} disabled={saving}>Hide</button><button type="button" onClick={() => setPendingDeletion(null)} disabled={saving} title="Cancel"><X size={14} /></button></div>}</>}
          </article>)}
          {error && <p className="context-inspector-error">{error}</p>}
        </div>
        {editing && <aside className="context-editor">
          <form onSubmit={(event) => void saveCorrection(event)}>
            <span className="system-kicker">Correcting {editing.item.kind}</span>
            <label>Title<input value={editing.title} onChange={(event) => setEditing((current) => current && { ...current, title: event.target.value })} maxLength={300} /* eslint-disable-line jsx-a11y/no-autofocus -- focus moves into the edit field the user just opened */ autoFocus /></label>
            <label>Details<textarea value={editing.content} onChange={(event) => setEditing((current) => current && { ...current, content: event.target.value })} maxLength={12000} rows={7} /></label>
            <div className="context-editor-row"><label>Status<select value={editing.status} onChange={(event) => setEditing((current) => current && { ...current, status: event.target.value as TerrenceContextItem["status"] })}><option value="active">Active</option><option value="blocked">Blocked</option><option value="paused">Paused</option><option value="completed">Completed</option><option value="archived">Archived</option><option value="historical">Historical</option></select></label><label>Confidence<input type="number" min="0" max="1" step="0.05" value={editing.confidence} onChange={(event) => setEditing((current) => current && { ...current, confidence: event.target.value })} /></label></div>
            <div className="context-editor-actions"><button type="button" onClick={() => setEditing(null)} disabled={saving}>Cancel</button><button type="submit" disabled={saving}><Check size={15} />Save correction</button></div>
          </form>
        </aside>}
      </div>
    </section>
  </div>;
}

import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode, type RefObject } from "react";
import {
  addFollowupItem, addMutedTopic, deleteFollowupItem, deleteMutedTopic, listFollowups, markFollowupDone, reopenFollowupItem, updateFollowupItem,
} from "../bridge";
import {
  CLEANED_NOTE, FOLLOWUP_ERROR_COPY, INCOMPLETE_DATE, changedBySave, dayLabel, draftFrom, dueState, followupErrorKind, isIsoDay, itemPatch, nextError, textError, todayIso, topicError,
  type FollowupErrorKind, type ItemDraft,
} from "../lib/followups";
import type { FollowupItem, FollowupLists, MutedTopic } from "../types";
import { Badge, Button, Card, Dialog, InsetGroup, ListRow, More, PageHero, TextField } from "../ui";
import "./ListsScreen.css";

type FieldErrors = { text?: string; due?: string; next?: string; form?: string };
type Edit = { base: FollowupItem; draft: ItemDraft; errors: FieldErrors; saving: boolean };
type Section = "open" | "done" | "muted";
type Confirm = { kind: "item"; item: FollowupItem; section: Section } | { kind: "muted"; topic: string };
type FocusTarget = Section | { elementId: string };
/** Where the edited item is now, compared with when the edit began. */
type EditState = "same" | "changed" | "done" | "gone";

/** A date input holding a half-typed date reads as "", which would look like clearing it. */
const hasIncompleteDate = (form: HTMLFormElement) => Array.from(form.querySelectorAll<HTMLInputElement>('input[type="date"]')).some((input) => input.validity.badInput);

const hasErrors = (errors: FieldErrors) => Object.values(errors).some(Boolean);

const sameContent = (a: FollowupItem, b: FollowupItem) => a.text === b.text && a.due === b.due && a.next === b.next;

/** Amara changes these files herself, so re-read when the window comes back. */
function useWindowFocus(onFocus: () => void) {
  useEffect(() => {
    const onVisible = () => { if (document.visibilityState === "visible") onFocus(); };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if ("__TAURI_INTERNALS__" in window) {
      try {
        getCurrentWindow()
          .onFocusChanged(({ payload }) => { if (payload) onFocus(); })
          .then((fn) => { if (disposed) fn(); else unlisten = fn; })
          .catch(() => { /* the DOM events stay in charge */ });
      } catch {
        /* the DOM events stay in charge */
      }
    }
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [onFocus]);
}

const DUE_BADGE = { overdue: { tone: "warning", label: "Overdue" }, today: { tone: "accent", label: "Due today" } } as const;

function ItemMeta({ item }: { item: FollowupItem }) {
  const parts = [
    item.due && `Due ${dayLabel(item.due)}`,
    item.next && `Next: ${item.next}`,
    item.notes.length > 0 && `Notes: ${item.notes.join("; ")}`,
  ].filter(Boolean);
  return parts.length ? <>{parts.join(" · ")}</> : null;
}

function ItemEditor({ edit, state, onChange, onSave, onCancel }: { edit: Edit; state: EditState; onChange: (draft: Partial<ItemDraft>) => void; onSave: (incompleteDate: boolean) => void; onCancel: () => void }) {
  const textRef = useRef<HTMLInputElement>(null);
  useEffect(() => { textRef.current?.focus(); }, []);
  const { draft, errors, saving } = edit;
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); onSave(hasIncompleteDate(event.currentTarget)); };
  const oddDate = draft.due !== "" && !isIsoDay(draft.due);
  return (
    <li className="ui-list-row lists__editor">
      <form className="lists__form" onSubmit={submit} noValidate aria-label={`Edit: ${edit.base.text}`}>
        {state !== "same" && (
          <p className="lists__notice" role="status">
            {state === "changed" && "Amara updated this while you were editing. Saving keeps only the fields you changed."}
            {state === "done" && "Amara marked this done while you were editing. Saving still updates it."}
            {state === "gone" && "This is no longer on the list. Your edit is still here, so you can save it as a new item."}
          </p>
        )}
        <TextField ref={textRef} label="Text" value={draft.text} error={errors.text} onChange={(e) => onChange({ text: e.target.value })} />
        <div className="lists__form-row">
          <TextField label="Due date (optional)" type="date" value={isIsoDay(draft.due) ? draft.due : ""} error={errors.due}
            description={oddDate ? <>Saved as “{draft.due}”. <button type="button" className="lists__link" onClick={() => onChange({ due: "" })}>Clear it</button></> : undefined}
            onChange={(e) => onChange({ due: e.target.value })} />
          <TextField label="Next step (optional)" value={draft.next} error={errors.next} onChange={(e) => onChange({ next: e.target.value })} />
        </div>
        {errors.form && <p className="lists__error" role="alert">{errors.form}</p>}
        <div className="lists__form-actions">
          <Button size="small" variant="plain" onClick={onCancel} disabled={saving}>Cancel</Button>
          <Button size="small" variant="primary" type="submit" loading={saving}>{state === "gone" ? "Save as new item" : "Save"}</Button>
        </div>
      </form>
    </li>
  );
}

function AddItemForm({ onAdd, onCancel }: { onAdd: (draft: ItemDraft) => Promise<string | null>; onCancel: () => void }) {
  const [draft, setDraft] = useState<ItemDraft>({ text: "", due: "", next: "" });
  const [errors, setErrors] = useState<FieldErrors>({});
  const [saving, setSaving] = useState(false);
  const textRef = useRef<HTMLInputElement>(null);
  useEffect(() => { textRef.current?.focus(); }, []);
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const found: FieldErrors = { text: textError(draft.text), next: nextError(draft.next), due: hasIncompleteDate(event.currentTarget) ? INCOMPLETE_DATE : undefined };
    setErrors(found);
    if (hasErrors(found)) return;
    setSaving(true);
    const form = await onAdd(draft);
    setSaving(false);
    if (form) setErrors({ form });
  };
  return (
    <form className="lists__form lists__add" onSubmit={(e) => { void submit(e); }} noValidate aria-label="Add item">
      <TextField ref={textRef} label="Text" value={draft.text} error={errors.text} onChange={(e) => setDraft({ ...draft, text: e.target.value })} />
      <div className="lists__form-row">
        <TextField label="Due date (optional)" type="date" value={draft.due} error={errors.due} onChange={(e) => setDraft({ ...draft, due: e.target.value })} />
        <TextField label="Next step (optional)" value={draft.next} error={errors.next} onChange={(e) => setDraft({ ...draft, next: e.target.value })} />
      </div>
      {errors.form && <p className="lists__error" role="alert">{errors.form}</p>}
      <div className="lists__form-actions">
        <Button size="small" variant="plain" onClick={onCancel} disabled={saving}>Cancel</Button>
        <Button size="small" variant="primary" type="submit" loading={saving}>Add item</Button>
      </div>
    </form>
  );
}

function AddTopicForm({ onAdd }: { onAdd: (topic: string, until: string) => Promise<string | null> }) {
  const [topic, setTopic] = useState("");
  const [until, setUntil] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [saving, setSaving] = useState(false);
  const topicRef = useRef<HTMLInputElement>(null);
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const found: FieldErrors = { text: topicError(topic), due: hasIncompleteDate(event.currentTarget) ? INCOMPLETE_DATE : undefined };
    setErrors(found);
    if (hasErrors(found)) return;
    setSaving(true);
    const form = await onAdd(topic.trim(), until);
    setSaving(false);
    if (form) { setErrors({ form }); return; }
    setTopic("");
    setUntil("");
    topicRef.current?.focus();
  };
  return (
    <form className="lists__form lists__add" onSubmit={(e) => { void submit(e); }} noValidate aria-label="Add topic">
      <div className="lists__form-row lists__form-row--end">
        <TextField ref={topicRef} label="Topic" value={topic} error={errors.text} onChange={(e) => setTopic(e.target.value)} />
        <TextField label="Until (optional)" type="date" value={until} error={errors.due} onChange={(e) => setUntil(e.target.value)} />
        <Button variant="secondary" type="submit" loading={saving}>Add topic</Button>
      </div>
      {errors.form && <p className="lists__error" role="alert">{errors.form}</p>}
    </form>
  );
}

function SectionTitle({ headingRef, children }: { headingRef: RefObject<HTMLSpanElement | null>; children: ReactNode }) {
  return <span ref={headingRef} tabIndex={-1} className="lists__section-title">{children}</span>;
}

function mutedSubtitle(muted: MutedTopic) {
  if (!muted.until) return "No end date";
  return muted.active ? `Until ${dayLabel(muted.until)}` : `Ended ${dayLabel(muted.until)}. Amara can bring this up again.`;
}

/** Chat & Agents › Lists: Amara's follow-ups (~/.hermes/amara), which she and the app both change. */
const FIRST_GLANCE = 4;

export function ListsField({ refreshSignal }: { refreshSignal?: number }) {
  const [lists, setLists] = useState<FollowupLists | null>(null);
  const [loadError, setLoadError] = useState<FollowupErrorKind | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [pending, setPending] = useState<string[]>([]);
  // Drafts live here rather than in the rows, so a row Amara closes keeps its edit on screen.
  const [edits, setEdits] = useState<Record<number, Edit>>({});
  const [adding, setAdding] = useState(false);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [focusRequest, setFocusRequest] = useState<{ target: FocusTarget } | null>(null);
  const loadSeq = useRef(0);
  const openHeading = useRef<HTMLSpanElement>(null);
  const doneHeading = useRef<HTMLSpanElement>(null);
  const mutedHeading = useRef<HTMLSpanElement>(null);

  const load = useCallback(() => {
    const seq = ++loadSeq.current;
    return listFollowups().then((next) => {
      if (seq !== loadSeq.current) return;
      setLists(next);
      setLoadError(null);
    }, (error: unknown) => {
      if (seq !== loadSeq.current) return;
      const kind = followupErrorKind(error);
      console.warn(`[lists] read failed: ${kind}`);
      setLoadError(kind);
    });
  }, []);

  const refresh = useCallback(() => { void load(); }, [load]);
  useEffect(() => { void load(); }, [load, refreshSignal]);
  useWindowFocus(refresh);

  // Runs after the refreshed rows commit. A removed row's button is gone, so focus would fall to <body>.
  useEffect(() => {
    if (!focusRequest) return;
    const { target } = focusRequest;
    if (typeof target === "object") (document.getElementById(target.elementId) ?? openHeading.current)?.focus();
    else if (document.activeElement === document.body || document.activeElement === null) ({ open: openHeading, done: doneHeading, muted: mutedHeading })[target].current?.focus();
  }, [focusRequest]);

  const setEdit = (id: number, change: Partial<Edit>) => setEdits((all) => (all[id] ? { ...all, [id]: { ...all[id], ...change } } : all));
  const closeEdit = (id: number) => setEdits((all) => { const rest = { ...all }; delete rest[id]; return rest; });

  // What the backend stored is what shows, even if the re-read that follows fails.
  const showSaved = (saved: FollowupItem) => setLists((all) => all && {
    ...all,
    open: all.open.some((item) => item.id === saved.id) ? all.open.map((item) => (item.id === saved.id ? saved : item)) : saved.done ? all.open : [...all.open, saved],
    recentlyDone: all.recentlyDone.map((item) => (item.id === saved.id ? saved : item)),
  });
  const showSavedTopic = (saved: MutedTopic) => setLists((all) => all && {
    ...all,
    muted: all.muted.some((row) => row.topic.toLowerCase() === saved.topic.toLowerCase())
      ? all.muted.map((row) => (row.topic.toLowerCase() === saved.topic.toLowerCase() ? saved : row))
      : [...all.muted, saved],
  });

  const retry = async () => {
    setPending((keys) => [...keys, "load"]);
    await load();
    setPending((keys) => keys.filter((key) => key !== "load"));
  };

  const runAction = async (key: string, work: () => Promise<unknown>, success: string, focus: FocusTarget, goneIsFine = false) => {
    setActionError(null);
    setNote(null);
    setPending((keys) => [...keys, key]);
    let failed = false;
    try {
      await work();
      setStatus(success);
    } catch (error) {
      const kind = followupErrorKind(error);
      console.warn(`[lists] change failed: ${kind}`);
      if (goneIsFine && kind === "not_found") setStatus("That was already gone.");
      else { failed = true; setActionError(FOLLOWUP_ERROR_COPY[kind]); }
    }
    setPending((keys) => keys.filter((other) => other !== key));
    setConfirm(null);
    await load();
    if (!failed) setFocusRequest({ target: focus });
  };

  const addItem = async (draft: ItemDraft) => {
    const input = { text: draft.text.trim(), due: draft.due || undefined, next: draft.next.trim() || undefined };
    let saved: FollowupItem;
    try {
      saved = await addFollowupItem(input);
    } catch (error) {
      await load();
      return FOLLOWUP_ERROR_COPY[followupErrorKind(error)];
    }
    setAdding(false);
    showSaved(saved);
    setNote(changedBySave(input, saved) ? CLEANED_NOTE : null);
    setStatus("Added.");
    await load();
    setFocusRequest({ target: { elementId: "lists-add" } });
    return null;
  };

  const addTopic = async (topic: string, until: string) => {
    let saved: MutedTopic;
    try {
      saved = await addMutedTopic(topic, until || undefined);
    } catch (error) {
      await load();
      return FOLLOWUP_ERROR_COPY[followupErrorKind(error)];
    }
    showSavedTopic(saved);
    setNote(saved.topic === topic ? null : CLEANED_NOTE);
    setStatus(`Amara won't mention ${saved.topic}.`);
    await load();
    return null;
  };

  const saveEdit = async (edit: Edit, state: EditState, incompleteDate: boolean) => {
    const { base, draft } = edit;
    const errors: FieldErrors = { text: textError(draft.text), next: nextError(draft.next), due: incompleteDate ? INCOMPLETE_DATE : undefined };
    if (hasErrors(errors)) { setEdit(base.id, { errors }); return; }
    const patch = itemPatch(base, draft);
    const leaveTo = { target: { elementId: `lists-edit-${base.id}` } };
    if (state !== "gone" && Object.keys(patch).length === 0) { closeEdit(base.id); setFocusRequest(leaveTo); return; }
    setEdit(base.id, { saving: true, errors: {} });
    setNote(null);
    const input = { text: draft.text.trim(), due: isIsoDay(draft.due) ? draft.due : undefined, next: draft.next.trim() || undefined };
    let saved: FollowupItem;
    try {
      saved = await (state === "gone" ? addFollowupItem(input) : updateFollowupItem(base.id, patch));
    } catch (error) {
      const kind = followupErrorKind(error);
      console.warn(`[lists] save failed: ${kind}`);
      setEdit(base.id, { saving: false, errors: { form: FOLLOWUP_ERROR_COPY[kind] } });
      await load();
      return;
    }
    closeEdit(base.id);
    showSaved(saved);
    setNote(changedBySave(state === "gone" ? input : patch, saved) ? CLEANED_NOTE : null);
    setStatus("Saved.");
    await load();
    setFocusRequest(leaveTo);
  };

  const confirmed = () => {
    if (!confirm) return;
    if (confirm.kind === "item") void runAction("confirm", () => deleteFollowupItem(confirm.item.id), "Deleted.", confirm.section, true);
    else void runAction("confirm", () => deleteMutedTopic(confirm.topic), `Amara can mention ${confirm.topic} again.`, "muted", true);
  };

  const isPending = (key: string) => pending.includes(key);
  const banners = [...new Set([loadError ? FOLLOWUP_ERROR_COPY[loadError] : null, actionError].filter((text): text is string => !!text))];
  const count = lists?.open.length ?? 0;
  const header = <PageHero title="Lists" statement={!lists ? "Keeping track" : count === 0 ? "Nothing open" : `${count} open`} detail="What Amara is keeping track of for you. She updates these too." />;
  const live = <p className="ui-visually-hidden" role="status">{status}</p>;

  if (!lists) {
    return (
      <div className="lists page-inset">
        {header}
        {live}
        {loadError
          ? <Card title="Lists unavailable"><div className="lists__unavailable" role="alert"><p>{FOLLOWUP_ERROR_COPY[loadError]}</p><Button size="small" loading={isPending("load")} onClick={() => { void retry(); }}>Try again</Button></div></Card>
          : <Card title="Open"><p className="lists__empty" role="status">Loading lists…</p></Card>}
      </div>
    );
  }

  const stateOf = (edit: Edit): EditState => {
    const open = lists.open.find((item) => item.id === edit.base.id);
    if (open) return sameContent(open, edit.base) ? "same" : "changed";
    return lists.recentlyDone.some((item) => item.id === edit.base.id) ? "done" : "gone";
  };
  const openIds = new Set(lists.open.map((item) => item.id));
  // An item that left Open while being edited stays in its place until the edit is saved or cancelled.
  const openRows = [...lists.open, ...Object.values(edits).filter((edit) => !openIds.has(edit.base.id)).map((edit) => edit.base)];
  const doneRows = lists.recentlyDone.filter((item) => !edits[item.id]);
  const today = todayIso();
  const later = openRows.slice(FIRST_GLANCE);

  const openRow = (item: FollowupItem) => {
    const edit = edits[item.id];
    if (edit) {
      const state = stateOf(edit);
      return <ItemEditor key={item.id} edit={edit} state={state}
        onChange={(change) => setEdit(item.id, { draft: { ...edit.draft, ...change } })}
        onSave={(incompleteDate) => { void saveEdit(edit, state, incompleteDate); }}
        onCancel={() => { closeEdit(item.id); setFocusRequest({ target: { elementId: `lists-edit-${item.id}` } }); }} />;
    }
    const due = dueState(item.due, today);
    return (
      <ListRow key={item.id} className="lists__row" title={item.text} subtitle={<ItemMeta item={item} />}
        value={due === "overdue" || due === "today" ? <Badge tone={DUE_BADGE[due].tone}>{DUE_BADGE[due].label}</Badge> : undefined}
        accessory={<>
          <Button size="small" variant="secondary" aria-label={`Done: ${item.text}`} loading={isPending(`done:${item.id}`)} onClick={() => { void runAction(`done:${item.id}`, () => markFollowupDone(item.id), "Marked done.", "open"); }}>Done</Button>
          <Button id={`lists-edit-${item.id}`} size="small" variant="secondary" aria-label={`Edit: ${item.text}`} onClick={() => setEdits((all) => ({ ...all, [item.id]: { base: item, draft: draftFrom(item), errors: {}, saving: false } }))}>Edit</Button>
          <Button size="small" variant="plain" aria-label={`Delete: ${item.text}`} onClick={() => setConfirm({ kind: "item", item, section: "open" })}>Delete</Button>
        </>} />
    );
  };

  return (
    <div className="lists page-inset">
      {header}
      {live}
      {banners.map((text) => (
        <div key={text} className="lists__banner" role="alert">
          <p>{text}{loadError && text === FOLLOWUP_ERROR_COPY[loadError] ? " This is the last version that loaded." : ""}</p>
          {loadError && text === FOLLOWUP_ERROR_COPY[loadError] && <Button size="small" loading={isPending("load")} onClick={() => { void retry(); }}>Try again</Button>}
        </div>
      ))}
      {note && <p className="lists__notice" role="status">{note}</p>}

      <Card title={<SectionTitle headingRef={openHeading}>Open</SectionTitle>}
        accessory={!adding && <Button id="lists-add" size="small" variant="secondary" onClick={() => setAdding(true)}>Add item</Button>}>
                {adding && <AddItemForm onAdd={addItem} onCancel={() => { setAdding(false); setFocusRequest({ target: { elementId: "lists-add" } }); }} />}
        {openRows.length > 0
          ? <InsetGroup aria-label="Open items">{openRows.slice(0, FIRST_GLANCE).map(openRow)}</InsetGroup>
          : <p className="lists__empty">Nothing open. Amara isn't following up on anything.</p>}
      </Card>

      <More label={later.length > 0 ? `${later.length} more, done and not mentioning` : "Done and not mentioning"}>
      {later.length > 0 && <InsetGroup title="More open" aria-label="More open items">{later.map(openRow)}</InsetGroup>}
      <Card title={<SectionTitle headingRef={doneHeading}>Done recently</SectionTitle>}>
        <p className="lists__description">Finished in the last 30 days.</p>
        {doneRows.length > 0
          ? (
            <InsetGroup aria-label="Done recently">
              {doneRows.map((item) => (
                <ListRow key={item.id} className="lists__row" title={item.text} subtitle={item.done ? `Done ${dayLabel(item.done)}` : "Done"}
                  accessory={<>
                    <Button size="small" variant="secondary" aria-label={`Reopen: ${item.text}`} loading={isPending(`reopen:${item.id}`)} onClick={() => { void runAction(`reopen:${item.id}`, () => reopenFollowupItem(item.id), "Reopened.", "done"); }}>Reopen</Button>
                    <Button size="small" variant="plain" aria-label={`Delete: ${item.text}`} onClick={() => setConfirm({ kind: "item", item, section: "done" })}>Delete</Button>
                  </>} />
              ))}
            </InsetGroup>
          )
          : <p className="lists__empty">Nothing done in the last 30 days.</p>}
      </Card>

      <Card title={<SectionTitle headingRef={mutedHeading}>Not mentioning</SectionTitle>}>
        <p className="lists__description">Topics Amara won't bring up. With no date, that lasts until you remove it.</p>
        {lists.muted.length > 0
          ? (
            <InsetGroup aria-label="Not mentioning">
              {lists.muted.map((muted) => (
                <ListRow key={muted.topic} className="lists__row" title={muted.topic} subtitle={mutedSubtitle(muted)}
                  accessory={<Button size="small" variant="plain" aria-label={`Remove: ${muted.topic}`} onClick={() => setConfirm({ kind: "muted", topic: muted.topic })}>Remove</Button>} />
              ))}
            </InsetGroup>
          )
          : <p className="lists__empty">No muted topics. Amara can bring up anything.</p>}
        <AddTopicForm onAdd={addTopic} />
      </Card>
      </More>

      <Dialog open={confirm !== null} title={confirm?.kind === "muted" ? "Remove this topic?" : "Delete this item?"}
        confirmLabel={confirm?.kind === "muted" ? "Remove" : "Delete"} destructive busy={isPending("confirm")}
        onConfirm={confirmed} onCancel={() => setConfirm(null)}>
        {confirm?.kind === "item" && `“${confirm.item.text}” will be gone from Amara's list. This can't be undone.`}
        {confirm?.kind === "muted" && `Amara will be able to bring up “${confirm.topic}” again.`}
      </Dialog>
    </div>
  );
}

import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { MessageCircle } from "lucide-react";
import { HOST_VIEWS, SYSTEMS, TODAY_LABEL, TODAY_ROUTE, type Route } from "../app/routes";
import { Dialog } from "../ui";
import "./CommandPalette.css";

export type PaletteCommand = { id: string; label: string; hint?: string; route?: Route; run?: () => void };

/** Navigation commands: the 6 sections (⌘1–6) and their sub-views / anchors. */
export function paletteCommands(extra: PaletteCommand[] = []): PaletteCommand[] {
  const sections = SYSTEMS.map((s, i) => ({ id: s.id, label: s.label, hint: `⌘${i + 1}`, route: { id: s.id, view: "main" } }));
  const subs: PaletteCommand[] = [];
  for (const s of SYSTEMS) {
    for (const v of HOST_VIEWS[s.id] ?? []) if (v.view !== "main") subs.push({ id: `${s.id}:${v.view}`, label: `${s.label} › ${v.label}`, route: { id: s.id, view: v.view } });
  }
  subs.push(
    { id: "terrance:today", label: `Home › ${TODAY_LABEL}`, route: TODAY_ROUTE },
    { id: "terrance:goals", label: "Home › Goals", route: { id: "terrance", view: "goals" } },
    { id: "settings:devices", label: "Settings › Devices", route: { id: "settings", view: "devices" } },
    { id: "settings:about", label: "About Amara OS", route: { id: "settings", view: "about" } },
  );
  return [...sections, ...subs, ...extra];
}

export function filterCommands(commands: PaletteCommand[], query: string): PaletteCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  return commands.filter((c) => c.label.toLowerCase().includes(q) || c.id.toLowerCase().includes(q));
}

type Props = {
  open: boolean;
  onClose: () => void;
  onNavigate: (route: Route) => void;
  /** No-match fallback: open Chat & Agents with the text pre-filled. */
  onAsk: (text: string) => void;
  extra?: PaletteCommand[];
};

/** ⌘K command palette on ui/Dialog (Decision 15). Esc closes; ↑/↓ + Return; never offers destructive actions. */
export function CommandPalette({ open, onClose, onNavigate, onAsk, extra }: Props) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const all = useMemo(() => paletteCommands(extra), [extra]);
  const matches = filterCommands(all, query);
  const text = query.trim();
  const rows: PaletteCommand[] = text ? [...matches, { id: "ask", label: `Ask Amara: “${text}”`, run: () => onAsk(text) }] : matches;
  const active = Math.min(index, rows.length - 1);

  useEffect(() => { if (open) input.current?.focus(); }, [open]);

  const close = () => { setQuery(""); setIndex(0); onClose(); };
  const choose = (cmd: PaletteCommand | undefined) => {
    if (!cmd) return;
    close();
    if (cmd.route) onNavigate(cmd.route);
    cmd.run?.();
  };
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setIndex((i) => Math.min(rows.length - 1, i + 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setIndex((i) => Math.max(0, i - 1)); }
    else if (e.key === "Enter") { e.preventDefault(); choose(rows[active]); }
  };

  return (
    <Dialog open={open} title="Command palette" hideTitle onCancel={close} className="palette">
      <input ref={input} className="palette__input" type="text" role="combobox" aria-expanded="true" aria-controls={listId} aria-autocomplete="list"
        aria-label="Search or ask Amara" aria-activedescendant={rows[active] ? `${listId}-${rows[active].id}` : undefined}
        placeholder="Search or ask Amara…" value={query} onChange={(e) => { setQuery(e.target.value); setIndex(0); }} onKeyDown={onKeyDown} />
      <ul id={listId} role="listbox" aria-label="Commands" className="palette__list">
        {rows.map((cmd, i) => (
          // Options are picked with the combobox keys (↑/↓, Return) or a click.
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events
          <li key={cmd.id} id={`${listId}-${cmd.id}`} role="option" aria-selected={i === active} className="palette__row" data-active={i === active || undefined}
            onMouseEnter={() => setIndex(i)} onClick={() => choose(cmd)}>
            {cmd.id === "ask" && <MessageCircle className="palette__icon" aria-hidden="true" />}
            <span className="palette__label">{cmd.label}</span>
            {cmd.hint && <kbd className="keycap">{cmd.hint}</kbd>}
          </li>
        ))}
      </ul>
      <p className="palette__footer"><kbd className="keycap">↑</kbd><kbd className="keycap">↓</kbd> to move · <kbd className="keycap">↩</kbd> to open · <kbd className="keycap">esc</kbd> to close</p>
    </Dialog>
  );
}

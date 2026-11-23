import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { MessageCircle } from "lucide-react";
import { HOST_VIEWS, SYSTEMS, TODAY_LABEL, TODAY_ROUTE, type Route } from "../app/routes";
import { Dialog } from "../ui";
import "./CommandPalette.css";

export type PaletteCommand = { id: string; label: string; hint?: string; route?: Route; run?: () => void; keywords?: string };

/** design-1111v: old names still find the renamed pages (typing "revenue" finds Finances). */
const OLD_NAMES: Record<string, string> = {
  "enterprise:main": "business", "enterprise": "business", "enterprise:finances": "revenue money business", "enterprise:tasks": "work business", "enterprise:decisions": "votes business",
  "enterprise:ventures": "business", "enterprise:content": "pipeline business", "life:dates": "life admin deadlines", "life:money": "personal business", "life:goals": "trajectory", "terrance:lists": "lists",
};

/** Navigation commands: the 7 sections (⌘1–7) and their sub-views / anchors. */
export function paletteCommands(extra: PaletteCommand[] = []): PaletteCommand[] {
  const sections = SYSTEMS.map((s, i) => ({ id: s.id, label: s.label, hint: `⌘${i + 1}`, route: { id: s.id, view: "main" }, keywords: OLD_NAMES[s.id] }));
  const subs: PaletteCommand[] = [];
  for (const s of SYSTEMS) {
    for (const v of HOST_VIEWS[s.id] ?? []) if (v.view !== "main") subs.push({ id: `${s.id}:${v.view}`, label: `${s.label} › ${v.label}`, route: { id: s.id, view: v.view }, keywords: OLD_NAMES[`${s.id}:${v.view}`] });
  }
  subs.push(
    { id: "terrance:today", label: `Home › ${TODAY_LABEL}`, route: TODAY_ROUTE },
    // design-1111v: Goals, Life admin and Weekly review moved to Life (the host tabs above); Lists is To-dos.
    { id: "terrance:needs-you", label: `${TODAY_LABEL} › Needs you today`, route: { id: "terrance", view: "needs-you" } },
    { id: "terrance:checkin", label: `${TODAY_LABEL} › Check-in`, route: { id: "terrance", view: "checkin" } },
    { id: "terrance:lists", label: `${TODAY_LABEL} › To-dos`, route: { id: "terrance", view: "lists" }, keywords: OLD_NAMES["terrance:lists"] },
    { id: "chat:activity", label: "Agents › Activity", route: { id: "chat", view: "activity" } },
    { id: "settings:about", label: "About Amara OS", route: { id: "settings", view: "about" } },
    { id: "settings:updates", label: "Settings › General › Updates", route: { id: "settings", view: "updates" } },
  );
  return [...sections, ...subs, ...extra];
}

export function filterCommands(commands: PaletteCommand[], query: string): PaletteCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  const hits = commands.filter((c) => c.label.toLowerCase().includes(q) || c.id.toLowerCase().includes(q) || (c.keywords ?? "").includes(q));
  // design-1111v: an old page name typed from habit ("rev" for Revenue) still lands where that page went,
  // so a command whose old name starts with the query comes first. The rest keep their order.
  const oldName = (c: PaletteCommand) => q.length >= 3 && (c.keywords ?? "").split(/\s+/).some((w) => w.startsWith(q));
  return [...hits.filter(oldName), ...hits.filter((c) => !oldName(c))];
}

type Props = {
  open: boolean;
  onClose: () => void;
  onNavigate: (route: Route) => void;
  /** No-match fallback: open the Home chat with the text pre-filled. */
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

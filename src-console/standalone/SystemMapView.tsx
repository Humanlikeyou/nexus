import { useCallback, useEffect, useId, useMemo, useRef, useState, type RefObject } from "react";
import { Button, Card, DataTag, StatusText } from "../ui";
import type { Wired } from "../lib/sampleAdapters";
import { WireNote } from "./parts";
import type { MapNode, MapNodeKind, MapPath, PathStatus, SystemMap as SystemMapData, SystemMapAdapter } from "./systemMap";
import { systemMapAdapter } from "./systemMapLive";
import "./SystemMapView.css";

/** Settings › Services › System map (decision 0011). Pure helpers first, then the screen. */

export const REFRESH_MS = 10_000;
const NARROW_PX = 540;

export const STATUS_WORD: Record<PathStatus, string> = { ok: "Flowing", broken: "Broken", off: "Off on purpose", unknown: "Not checked" };
const KIND_WORD: Record<MapNodeKind, string> = { app: "App", backend: "Backend", service: "Service", device: "Device", store: "Store", external: "Outside" };
const KIND_ORDER: MapNodeKind[] = ["app", "backend", "service", "store", "device", "external"];
const STATUS_KIND = { ok: "ok", broken: "fail", off: "off", unknown: "wait" } as const;

/** "just now", "3 min ago", "2 hours ago", "yesterday", "5 days ago". */
export function relativeTime(iso: string | null, now: number = Date.now()): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} hour${h === 1 ? "" : "s"} ago`;
  const d = Math.round(h / 24);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

function clock(iso: string): string | null {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? null : new Date(t).toLocaleTimeString([], { hour: "numeric", minute: "2-digit", second: "2-digit" });
}

/** What to call a path in a sentence: the end that is not Amara OS, or "A to B". */
export function pathName(p: MapPath, nodes: MapNode[]): string {
  const label = (id: string) => nodes.find((n) => n.id === id)?.label ?? id;
  const isApp = (id: string) => nodes.find((n) => n.id === id)?.kind === "app";
  if (isApp(p.from)) return label(p.to);
  if (isApp(p.to)) return label(p.from);
  return `${label(p.from)} to ${label(p.to)}`;
}

export function summarize(map: SystemMapData): string {
  const { paths, nodes } = map;
  const n = (s: PathStatus) => paths.filter((p) => p.status === s);
  const broken = n("broken");
  if (paths.length === 0) return "No paths to show yet";
  if (broken.length) return `${broken.length} broken: ${broken.map((p) => pathName(p, nodes)).join(", ")}`;
  if (n("ok").length === paths.length) return `All ${paths.length} path${paths.length === 1 ? "" : "s"} flowing`;
  const parts = [`${n("ok").length} flowing`];
  if (n("off").length) parts.push(`${n("off").length} off on purpose`);
  if (n("unknown").length) parts.push(`${n("unknown").length} not checked`);
  return parts.join(", ");
}

type Placed = { node: MapNode; col: number; x: number; y: number };

/** Columns by distance from Amara OS; rows ordered by where their neighbours sit, then by kind. */
export function layout(map: SystemMapData): { placed: Map<string, Placed>; cols: number; rows: number } {
  const { nodes, paths } = map;
  const root = nodes.find((n) => n.kind === "app") ?? nodes[0];
  const rank = new Map<string, number>();
  if (root) {
    rank.set(root.id, 0);
    const queue = [root.id];
    while (queue.length) {
      const id = queue.shift()!;
      for (const p of paths) {
        const other = p.from === id ? p.to : p.to === id ? p.from : null;
        if (other && !rank.has(other)) { rank.set(other, rank.get(id)! + 1); queue.push(other); }
      }
    }
  }
  const reached = Math.max(0, ...rank.values());
  for (const n of nodes) if (!rank.has(n.id)) rank.set(n.id, reached + 1);
  const cols = Math.max(1, ...[...rank.values()].map((r) => r + 1));
  const byCol: MapNode[][] = Array.from({ length: cols }, () => []);
  const pos = new Map<string, number>();
  for (let c = 0; c < cols; c++) {
    const list = nodes.filter((n) => rank.get(n.id) === c);
    const key = (n: MapNode) => {
      const nb = paths.flatMap((p) => (p.from === n.id ? [p.to] : p.to === n.id ? [p.from] : [])).filter((id) => pos.has(id) && rank.get(id)! < c).map((id) => pos.get(id)!);
      return nb.length ? nb.reduce((a, b) => a + b, 0) / nb.length : 1e6;
    };
    list.sort((a, b) => key(a) - key(b) || KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind));
    list.forEach((n, i) => pos.set(n.id, (i + 0.5) / list.length));
    byCol[c] = list;
  }
  const placed = new Map<string, Placed>();
  byCol.forEach((list, col) => list.forEach((node, i) => placed.set(node.id, { node, col, x: ((col + 0.5) / cols) * 100, y: ((i + 0.5) / list.length) * 100 })));
  return { placed, cols, rows: Math.max(1, ...byCol.map((l) => l.length)) };
}

/** True while the user has asked the app (or the OS) for less motion. */
export function useReducedMotion(): boolean {
  const read = () => {
    try {
      return document.documentElement.dataset.reduceMotion === "true" || (typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches);
    } catch { return false; }
  };
  const [reduced, setReduced] = useState(read);
  useEffect(() => {
    const mq = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;
    const on = () => setReduced(read());
    mq?.addEventListener?.("change", on);
    const mo = new MutationObserver(on);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-reduce-motion"] });
    return () => { mq?.removeEventListener?.("change", on); mo.disconnect(); };
  }, []);
  return reduced;
}

/** On screen = intersecting the viewport AND the page is visible. Without IntersectionObserver, treat as on screen. */
export function useOnScreen(ref: RefObject<Element | null>): boolean {
  const [inView, setInView] = useState(typeof IntersectionObserver === "undefined");
  const [pageVisible, setPageVisible] = useState(() => typeof document === "undefined" || document.visibilityState !== "hidden");
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver((entries) => { const last = entries[entries.length - 1]; if (last) setInView(last.isIntersecting); });
    io.observe(el);
    return () => io.disconnect();
  }, [ref]);
  useEffect(() => {
    const on = () => setPageVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, []);
  return inView && pageVisible;
}

function Dot({ path, motion, open, onOpen, style }: { path: MapPath; motion: "blink" | "still"; open: boolean; onOpen: (id: string) => void; style?: React.CSSProperties }) {
  return (
    <button type="button" className="sysmap__dot" style={style} data-status={path.status} data-motion={path.status === "broken" ? motion : undefined}
      data-path={path.id} aria-haspopup="true" aria-expanded={open} aria-label={`${path.carries}: ${STATUS_WORD[path.status]}`} title={`${path.carries}: ${STATUS_WORD[path.status]}`}
      onClick={() => onOpen(path.id)}>
      <span className="sysmap__pip" aria-hidden="true" />
    </button>
  );
}

function Detail({ path, nodes, now, onClose }: { path: MapPath; nodes: MapNode[]; now: number; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    el?.focus();
    const on = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } };
    el?.addEventListener("keydown", on);
    return () => el?.removeEventListener("keydown", on);
  }, [path.id, onClose]);
  const label = (id: string) => nodes.find((n) => n.id === id)?.label ?? id;
  const last = relativeTime(path.lastOkAt, now);
  const whyTitle = path.status === "broken" ? "What's wrong" : path.status === "off" ? "Why it's off" : "Why not checked";
  return (
    <div className="sysmap__detail" ref={ref} tabIndex={-1} role="group" aria-label={`${path.carries}, details`}>
      <div className="sysmap__detail-head">
        <div><strong>{path.carries}</strong><small>{label(path.from)} to {label(path.to)}</small></div>
        <Button size="small" variant="secondary" onClick={onClose} aria-label="Close details">Close</Button>
      </div>
      <dl>
        <dt>Status</dt><dd><StatusText kind={STATUS_KIND[path.status]}>{STATUS_WORD[path.status]}</StatusText></dd>
        <dt>Last worked</dt><dd>{last ?? "Not seen working yet"}</dd>
        {path.reason && <><dt>{whyTitle}</dt><dd>{path.reason}</dd></>}
        {path.fix && <><dt>The usual fix</dt><dd>{path.fix}</dd></>}
        {path.status === "ok" && !path.reason && !path.fix && <><dt>Nothing to do</dt><dd>This path is working.</dd></>}
      </dl>
    </div>
  );
}

export function SystemMap({ adapter: given, now: nowFn = Date.now }: { adapter?: SystemMapAdapter; now?: () => number }) {
  const adapter = useMemo(() => given ?? systemMapAdapter(), [given]);
  const rootRef = useRef<HTMLDivElement>(null);
  const onScreen = useOnScreen(rootRef);
  const reduced = useReducedMotion();
  const motion = reduced ? "still" : "blink";
  const [, setTick] = useState(0);
  const [stale, setStale] = useState(false);
  const [openId, setOpenId] = useState<string | null>(null);
  const [narrow, setNarrow] = useState(false);
  const lastDot = useRef<string | null>(null);
  const titleId = useId();

  useEffect(() => {
    if (!onScreen) return;
    let live = true;
    const run = async () => {
      try { await adapter.refresh(); if (live) setStale(false); } catch { if (live) setStale(true); }
      if (live) setTick((t) => t + 1);
    };
    void run();
    const id = setInterval(() => { void run(); }, REFRESH_MS);
    return () => { live = false; clearInterval(id); };
  }, [adapter, onScreen]);

  useEffect(() => {
    const el = rootRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([e]) => { if (e) setNarrow(e.contentRect.width < NARROW_PX); });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const w: Wired<SystemMapData> = adapter.map();
  const open = useCallback((id: string) => { lastDot.current = id; setOpenId((cur) => (cur === id ? null : id)); }, []);
  const close = useCallback(() => {
    setOpenId(null);
    const id = lastDot.current;
    if (id) requestAnimationFrame(() => rootRef.current?.querySelector<HTMLElement>(`[data-path="${CSS.escape(id)}"]`)?.focus());
  }, []);

  if (w.state === "unwired") return (
    <Card title="System map" id="system-map" ref={rootRef as RefObject<HTMLElement>} accessory={<DataTag kind="wire" />} className="sysmap">
      <WireNote>Needs wiring: Amara&apos;s status check feeds this map once the backend lands (decision 0011). Nothing here is made up.</WireNote>
    </Card>
  );

  const map = w.value;
  const now = nowFn();
  const laid = layout(map);
  const opened = map.paths.find((p) => p.id === openId) ?? null;
  const stamp = clock(map.checkedAt);
  const anyBroken = map.paths.some((p) => p.status === "broken");

  return (
    <Card title="System map" className="sysmap" aria-labelledby={titleId} accessory={w.state === "sample" ? <DataTag kind="sample" /> : undefined} id="system-map">
      <div ref={rootRef} className="sysmap__root" data-narrow={narrow || undefined} data-motion={motion}>
        <div className="sysmap__top">
          <p className="sysmap__summary" role="status" data-broken={anyBroken || undefined} id={titleId}>{summarize(map)}</p>
          <p className="sysmap__stamp">{stale ? "Couldn't re-check just now. " : ""}{stamp ? `Checked ${stamp}` : "Checked just now"}</p>
        </div>
        <ul className="sysmap__legend" aria-label="What the dots mean">
          {(["ok", "broken", "off", "unknown"] as PathStatus[]).map((s) => <li key={s}><span className="sysmap__dot sysmap__dot--legend" data-status={s} data-motion={s === "broken" ? motion : undefined}><span className="sysmap__pip" aria-hidden="true" /></span>{STATUS_WORD[s]}</li>)}
        </ul>
        {narrow ? (
          <ul className="sysmap__list">
            {map.paths.map((p) => (
              <li key={p.id}>
                <Dot path={p} motion={motion} open={openId === p.id} onOpen={open} />
                <span className="sysmap__list-main"><span>{p.carries}</span><small>{map.nodes.find((n) => n.id === p.from)?.label} to {map.nodes.find((n) => n.id === p.to)?.label}</small></span>
                <span className="sysmap__list-st">{STATUS_WORD[p.status]}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="sysmap__canvas" style={{ ["--cols" as string]: laid.cols, ["--rows" as string]: laid.rows }}>
            <svg className="sysmap__lines" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true" focusable="false">
              {map.paths.map((p) => {
                const a = laid.placed.get(p.from), b = laid.placed.get(p.to);
                if (!a || !b) return null;
                const bend = a.col === b.col ? 40 / laid.cols : 0;
                const d = bend ? `M${a.x} ${a.y} Q${a.x + bend * 2} ${(a.y + b.y) / 2} ${b.x} ${b.y}` : `M${a.x} ${a.y} L${b.x} ${b.y}`;
                return <path key={p.id} d={d} className="sysmap__line" data-status={p.status} fill="none" />;
              })}
            </svg>
            {[...laid.placed.values()].map(({ node, x, y }) => (
              <div key={node.id} className="sysmap__node" data-kind={node.kind} style={{ left: `${x}%`, top: `${y}%` }}>
                <span>{node.label}</span><small>{KIND_WORD[node.kind]}</small>
              </div>
            ))}
            {map.paths.map((p) => {
              const a = laid.placed.get(p.from), b = laid.placed.get(p.to);
              if (!a || !b) return null;
              const bend = a.col === b.col ? 40 / laid.cols : 0;
              return <Dot key={p.id} path={p} motion={motion} open={openId === p.id} onOpen={open} style={{ left: `${(a.x + b.x) / 2 + bend}%`, top: `${(a.y + b.y) / 2}%` }} />;
            })}
          </div>
        )}
        {opened && <Detail path={opened} nodes={map.nodes} now={now} onClose={close} />}
      </div>
    </Card>
  );
}

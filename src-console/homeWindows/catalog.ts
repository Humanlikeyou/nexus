/** Sample window catalog and prompt parser. Comments and page bodies are sample data only. */
import type { OpenRequest, OpenSource, WindowKind, Signed } from "./types";

export type CatalogEntry = {
  id: string;
  kind: WindowKind;
  title: string;
  source: string;
  siteId: string;
  fav: { color: string; letter: string };
  signed: Signed;
  chip: (query: string) => string;
  /** Sample comment, shown only in sample mode. Never a trading call. */
  comment: string;
  pdfBullets?: string[];
  pdfDateHint?: string;
  match: RegExp;
};

export const CATALOG: CatalogEntry[] = [
  {
    id: "instagram",
    kind: "social",
    title: "Instagram · Following",
    source: "instagram.com",
    siteId: "instagram",
    fav: { color: "#a8285a", letter: "IG" },
    signed: "in",
    chip: () => "→ Opening Instagram · Following",
    comment: "<b>3 new posts</b> since yesterday. The Taiyo Skin teaser has the most comments; two ask about the launch date.",
    match: /instagram|\binsta\b|\bfeed\b/i,
  },
  {
    id: "youtube",
    kind: "video",
    title: "Label approval, step by step",
    source: "youtube.com",
    siteId: "youtube",
    fav: { color: "#e24a3b", letter: "▶" },
    signed: null,
    chip: () => "→ Opening YouTube · Label approval, step by step",
    comment: "A 12-minute walkthrough of label approval. The part on ingredient lists starts at <b>4:10</b>, which is where you are now.",
    match: /youtube|\bvideo\b|label approval/i,
  },
  {
    id: "lease",
    kind: "pdf",
    title: "lease-renewal-2026.pdf",
    source: "Local file · Documents",
    siteId: "local-pdf",
    fav: { color: "#8b5a2b", letter: "PDF" },
    signed: null,
    chip: () => "→ Opening lease-renewal-2026.pdf",
    comment: "Your lease renews on <b>Apr 1, 2027</b>. Notice is due one month before, by <b>Mar 1</b>, and the renewal fee is one month's rent. Early exit is clause 9 on page 4.",
    pdfBullets: ["Renews 1 April 2027", "Notice due by 1 March", "Renewal fee: one month's rent"],
    pdfDateHint: "notice by Mar 1",
    match: /\bpdf\b|\blease\b|renewal/i,
  },
  {
    id: "tradingview",
    kind: "chart",
    title: "BTCUSD · 4h",
    source: "tradingview.com",
    siteId: "tradingview",
    fav: { color: "#2962ff", letter: "TV" },
    signed: "in",
    chip: () => "→ Opening TradingView · BTCUSD, 4 hours",
    comment: "Bitcoin's 4-hour chart is up about <b>3%</b> since this morning and just moved above Monday's high near <b>62,100</b> (the copper line). Volume is thinner than yesterday's push. That's what's on screen, not a trading call.",
    match: /tradingview|bitcoin|btcusd|\bchart\b|\bbtc\b/i,
  },
  {
    id: "akiya",
    kind: "web",
    title: "Japan's akiya market, explained",
    source: "example-news.jp",
    siteId: "example-news",
    fav: { color: "#3f6e62", letter: "N" },
    signed: null,
    chip: () => "→ Opening example-news.jp · Japan's akiya market, explained",
    comment: "An explainer on empty homes (akiya): listings are cheap, but repairs and taxes add up. It suggests a survey before any offer, the same step on your Akiya list.",
    match: /akiya|article|example-news|\bweb\b|empty homes/i,
  },
];

export const entryById = (id: string): CatalogEntry => {
  const e = CATALOG.find((c) => c.id === id);
  if (!e) throw new Error(`unknown window catalog id: ${id}`);
  return e;
};

const OPEN = /(?:^|\b)(pull up|show me|open|bring up)\b/i;

export function parseWindowPrompt(text: string, openedBy: OpenSource): OpenRequest | null {
  const q = text.trim();
  if (!q) return null;
  if (!OPEN.test(q)) return null;
  const hit = CATALOG.find((c) => c.match.test(q));
  if (!hit) return null;
  return {
    catalogId: hit.id,
    kind: hit.kind,
    title: hit.title,
    source: hit.source,
    siteId: hit.siteId,
    fav: hit.fav,
    signed: hit.signed,
    chip: hit.chip(q),
    query: q,
    openedBy,
    target: hit.kind === "pdf" ? { localPath: hit.title } : { query: q, url: `https://${hit.source}` },
  };
}

export function requestFromCatalog(id: string, openedBy: OpenSource, query?: string): OpenRequest {
  const hit = entryById(id);
  const q = query ?? `open ${hit.title}`;
  return {
    catalogId: hit.id,
    kind: hit.kind,
    title: hit.title,
    source: hit.source,
    siteId: hit.siteId,
    fav: hit.fav,
    signed: hit.signed,
    chip: hit.chip(q),
    query: q,
    openedBy,
    target: hit.kind === "pdf" ? { localPath: hit.title } : { query: q, url: `https://${hit.source}` },
  };
}

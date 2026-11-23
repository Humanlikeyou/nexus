import type { StoredTake, TakePage } from "./types";

export const MAX_ATTEMPTS_PER_DAY = 2; // first take + one refresh
export const takeKey = (page: TakePage) => `amara_take_${page}`;

export type KvDeps = { load: (key: string) => Promise<string | null>; save: (key: string, value: string) => Promise<void> };

const isTake = (v: unknown): v is StoredTake => !!v && typeof v === "object" && typeof (v as StoredTake).day === "string" && typeof (v as StoredTake).attempts === "number";

/** The stored record for this page and day; another day's record, or a damaged one, reads as empty. */
export async function loadStored(page: TakePage, day: string, kv: KvDeps): Promise<StoredTake> {
  const empty: StoredTake = { day, attempts: 0, fingerprint: null, take: null };
  try {
    const raw = await kv.load(takeKey(page));
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    return isTake(parsed) && parsed.day === day ? parsed : empty;
  } catch { return empty; }
}

export async function saveStored(page: TakePage, rec: StoredTake, kv: KvDeps): Promise<void> {
  try { await kv.save(takeKey(page), JSON.stringify(rec)); } catch { /* not persisted: next launch regenerates at most the day's allowance */ }
}

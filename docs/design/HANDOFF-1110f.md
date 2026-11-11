# HANDOFF 1110f: "Amara's take" backend contract (for Claude)

The UI is a proposal only. Nothing has been built yet. Build the UI behind a `Wired<T>` adapter like 1110d/1110e: sample data only when `localStorage nexus.sampleAdapters=1`, otherwise `unwired` until the generator exists.

## Inputs per page (only these; the note may cite nothing else)
| Page | Inputs (for the note's day, JST) | Trigger: "today's data exists" when |
|---|---|---|
| fitness | recovery %, effort/strain (0–100), sleep duration + efficiency, resting HR, HRV, the user's 28-day usual for each | last night's sleep has synced |
| neuro | the day's sessions (or the latest if none today): length, avg focus, avg calm, protocol; recovery and sleep that day | a Muse session has ended |
| mindbody | fitness inputs + neuro inputs + readiness drivers | sleep synced (neuro optional; the combined note only if a session exists for the day) |
| business | 30-day net, revenue, costs, this week, open decisions (titles), trading risk index + kill switch, ventures at risk | the business snapshot loaded today |
| home | top 1–3 facts from the other four pages' notes for today (recovery, sleep, decisions count) | any one of the above exists |

## Generation and caching
- **One note per page per day.** Generate on the first visit to that page once its trigger is met. Never generate on a timer, in the background, or before the data exists.
- **Cache key:** `take:{page}:{YYYY-MM-DD in Asia/Tokyo}:{inputsHash}`. `inputsHash` is a stable hash of the exact input values.
- **Refresh rule:** at most **one** regeneration per page per day, only when a new session lands (neuro/mindbody/home) after the note was written. A refresh replaces the cached note and keeps `refreshedAt`. Later changes that day don't regenerate.
- Store it locally (UI kv), not in Letta/memory. Delete nothing; keep old notes for history.
- Generation goes through the existing Amara chat backend with a fixed system prompt. It must cite only the provided inputs, and every number in `text` must appear in `citations`. Reject the note (state `failed`) if a number in the text isn't in the inputs.

## Typed props
```ts
type TakePage = "home" | "fitness" | "neuro" | "mindbody" | "business";
type Lane = "health" | "neuro" | "business";
interface TakeCitation { label: string; value: string; raw: number; unit?: string; lane: Lane; sourceId: string /* element id the chip scrolls to */ }
interface AmaraTake { page: TakePage; day: string /* YYYY-MM-DD JST */; text: string; citations: TakeCitation[]; writtenAt: string /* ISO */; refreshedAt?: string; lanes: Lane[]; sourceLine: string }
type TakeState =
  | { state: "fresh"; take: AmaraTake }
  | { state: "generating" }
  | { state: "nodata"; waitingFor: "sleep" | "session" | "business"; yesterday?: AmaraTake }
  | { state: "failed"; reason: string; retry: true }
  | { state: "unwired"; reason?: string };
interface AmaraTakeProps { page: TakePage; take: Wired<TakeState>; compact?: boolean; onAskMore(take: AmaraTake): void; onCite(c: TakeCitation): void }
```

## Adapter
```ts
interface TakeAdapter {
  get(page: TakePage, now: Date): Wired<TakeState>;      // reads the cache; never generates
  ensure(page: TakePage, now: Date): Promise<Wired<TakeState>>; // first visit: generate if trigger met and no cache for today
  onSessionLanded(day: string): void;                      // marks one allowed refresh for neuro/mindbody/home
}
```
- `liveTakes()` returns `unwired` for everything until the generator exists. `sampleTakes()` returns the mockup notes, labelled Sample data.
- Ask more: `onAskMore` opens Home chat with `{ contextFrom: page, citations }` prefilled and doesn't send until Taisun presses send.

## Failure states
- Backend offline or outbound disabled (`NEXUS_OUTBOUND_DISABLED=1`): `unwired`, reason "Amara is offline". No retries in a loop.
- Generation error or a validation fail (an uncited number): `failed`, with one manual "Try again". There's no automatic retry beyond 1.
- Inputs missing for the day: `nodata` with `waitingFor`.
- Never show a stale note as today's: notes from a previous day show only as "Yesterday's note".

## Built (design-build-1111, approved by Taisun 2026-11-11 12:52 JST)
- `src-console/amaraTake/AmaraTakeCard.tsx` (`AmaraTake`, `AmaraTakeCard`, `TakeEmber`) on `useAmaraTakeStatus` (new in `useAmaraTake.ts`; `useAmaraTake` wraps it). View model in `amaraTake/takeView.ts`.
- Placed on Fitness, Neuro (hidden while a session runs), Mind–Body, Business (after the strip) and Home (compact, on-glass; hidden on windows shorter than 761px so the lens keeps its room).
- States: fresh · generating (while the first check runs) · nodata (with "waiting for") · failed (one "Try again") · unwired ("Needs wiring"). The live hook can't yet tell nodata from failed; both are covered by sample overrides (`localStorage nexus.sampleTakeState`) and unit tests.
- Ask more: dispatches `amara:ask-more`; App opens Home chat with the question in the draft and a context strip above the composer. Nothing sends until Taisun presses send; live context is appended to the message, sample context never is.
- Sample notes show only in mock mode (`nexus.sampleAdapters=1`), labelled Sample data. Preview mode makes no router call.
- Gap: chips are static (scroll-to-source isn't wired).

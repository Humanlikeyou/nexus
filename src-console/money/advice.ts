import { median } from "./chartData";
import { CATEGORY_LABEL, formatMinor, type Currency, type WireCurrencyView, type WireMonth } from "./entries";

/**
 * Amara's money read (decision 0045). Deterministic and pure: every claim is computed from recorded month totals,
 * lists the numbers it used with where each came from, and lists its assumptions. One currency at a time (no conversion).
 * Wording is facts plus "setting aside / would" only; it never uses causal words, and a missing value is "–".
 * With fewer than two complete months or no income recorded, there is no advice: `needs` says what to enter.
 */

export type Confidence = "low" | "medium" | "high";
export type AdviceKind = "savings" | "runway" | "bills" | "categories" | "goal" | "afford";
export type AdviceNumber = { label: string; /** Text exactly as it appears in the claim. */ shown: string; /** Where it came from, e.g. "median of Aug, Sep, Oct income". */ source: string; minor?: number };
export type AdviceItem = { id: string; kind: AdviceKind; claim: string; numbers: AdviceNumber[]; assumptions: string[]; confidence: Confidence; /** Proposes something to do (refused while data is partial). */ suggestion: boolean };
export type AdviceResult = { currency: Currency; status: "ok" | "needs_data"; needs: string[]; items: AdviceItem[]; completeMonths: number; hints: string[] };

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const monthWord = (ym: string) => MONTHS[Number(ym.slice(5, 7)) - 1] ?? ym;
const monthYear = (ym: string) => `${monthWord(ym)} ${ym.slice(0, 4)}`;
const mlist = (ms: WireMonth[]) => ms.map((m) => monthWord(m.month)).join(", ");
const addMonths = (ym: string, n: number) => { const t = Number(ym.slice(0, 4)) * 12 + (Number(ym.slice(5, 7)) - 1) + n; return `${Math.floor(t / 12)}-${String((t % 12) + 1).padStart(2, "0")}`; };
const money = (minor: number, c: Currency) => formatMinor(Math.round(minor), c);
const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;
const med = (xs: number[]) => median(xs) ?? 0;

const COMMON = ["Amounts are what you recorded or imported; anything not recorded counts as unknown, not zero.", "No exchange rate is used: this is one currency only."];

type Basis = { complete: WireMonth[]; income: number; spend: number; saved: number; confidence: Confidence; label: string };
function basisOf(v: WireCurrencyView, today: string): Basis | null {
  const complete = v.months.filter((m) => m.month < today.slice(0, 7) && m.has_data).slice(-3);
  if (complete.length < 2) return null;
  const spendS = complete.map((m) => m.expense_minor);
  const mid = med(spendS);
  const steady = mid > 0 && (Math.max(...spendS) - Math.min(...spendS)) / mid <= 0.25;
  return { complete, income: med(complete.map((m) => m.income_minor)), spend: mid, saved: med(complete.map((m) => m.savings_minor)), confidence: complete.length >= 3 ? (steady ? "high" : "medium") : "low", label: mlist(complete) };
}

/** What to enter before there is anything to say. */
export function adviceNeeds(v: WireCurrencyView, today: string): string[] {
  const done = v.months.filter((m) => m.month < today.slice(0, 7) && m.has_data);
  const needs: string[] = [];
  if (done.length < 2) needs.push(`Record income and spending for at least two full months (${done.length} so far in ${v.currency}).`);
  if (!done.some((m) => m.income_minor > 0)) needs.push(`Add your income in ${v.currency}.`);
  return needs;
}

export function buildAdvice(v: WireCurrencyView, today: string): AdviceResult {
  const c = v.currency;
  const needs = adviceNeeds(v, today);
  const b = needs.length ? null : basisOf(v, today);
  if (!b) return { currency: c, status: "needs_data", needs: needs.length ? needs : [`Record income and spending for at least two full months in ${c}.`], items: [], completeMonths: v.months.filter((m) => m.month < today.slice(0, 7) && m.has_data).length, hints: [] };
  const items: AdviceItem[] = [];
  const hints: string[] = [];
  const committed = v.committed_monthly_minor;
  const incomeN: AdviceNumber = { label: "Usual income", shown: money(b.income, c), source: `median of ${b.label} income`, minor: b.income };
  const spendN: AdviceNumber = { label: "Usual spending", shown: money(b.spend, c), source: `median of ${b.label} spending`, minor: b.spend };
  const commitN: AdviceNumber = { label: "Committed bills a month", shown: money(committed, c), source: "your recorded bills and commitments, as a monthly amount", minor: committed };

  // 1. Suggested savings this month.
  const left = b.income - committed - b.spend;
  const savAssume = [...COMMON, "Usual income and spending are the median of your last complete months (up to three).", "Committed bills are assumed not to be inside your recorded spending.", "This month's income is assumed to match the usual income."];
  items.push(left > 0
    ? { id: "savings", kind: "savings", claim: `On your usual income, committed bills and usual spending, about ${money(left, c)} is left to set aside this month.`, numbers: [{ label: "Left to set aside", shown: money(left, c), source: "usual income minus committed bills minus usual spending", minor: left }, incomeN, commitN, spendN], assumptions: savAssume, confidence: b.confidence, suggestion: true }
    : { id: "savings", kind: "savings", claim: `Your usual income is ${money(b.income, c)}, and committed bills plus usual spending come to ${money(committed + b.spend, c)}, so no amount is left to set aside on these figures.`, numbers: [incomeN, { label: "Committed bills plus usual spending", shown: money(committed + b.spend, c), source: "committed bills plus the median spending", minor: committed + b.spend }], assumptions: savAssume, confidence: b.confidence, suggestion: false });

  // 2. Runway.
  const burn = b.spend + committed;
  if (v.available && burn > 0) {
    const availN: AdviceNumber = { label: "Available now", shown: money(v.available.amount_minor, c), source: `balance of ${v.available.base_date} plus entries since`, minor: v.available.amount_minor };
    if (v.available.amount_minor < 0) items.push({ id: "runway", kind: "runway", claim: `The recorded available amount is below zero (${availN.shown}), so there is no buffer on these figures.`, numbers: [availN], assumptions: COMMON, confidence: "low", suggestion: false });
    else {
      const months = (v.available.amount_minor / burn).toFixed(1);
      items.push({ id: "runway", kind: "runway", claim: `Your available money covers about ${months} months of usual spending plus committed bills.`, numbers: [{ label: "Buffer", shown: `${months} months`, source: "available now divided by usual spending plus committed bills" }, availN, { label: "Monthly outgo", shown: money(burn, c), source: "usual spending plus committed bills", minor: burn }], assumptions: [...COMMON, "Spending stays at the usual level and no income arrives.", "The available amount is the latest balance you entered plus entries since."], confidence: b.confidence === "high" ? "medium" : b.confidence, suggestion: false });
    }
  } else hints.push(`Enter the money you have now in ${c} to see how many months it covers.`);

  // 3. Upcoming bills.
  v.upcoming.slice(0, 3).forEach((u, i) => {
    const weeks = Math.max(1, Math.ceil(u.days_until / 7));
    const per = Math.ceil(u.amount_minor / weeks);
    items.push({ id: `bill-${i}`, kind: "bills", claim: `A bill of ${money(u.amount_minor, c)} is due in ${plural(u.days_until, "day")}. Setting aside ${money(per, c)} a week for ${plural(weeks, "week")} would cover it.`, numbers: [{ label: `Bill: ${u.label}`, shown: money(u.amount_minor, c), source: `bill due ${u.due}`, minor: u.amount_minor }, { label: "Days until due", shown: plural(u.days_until, "day"), source: `from ${today} to ${u.due}` }, { label: "Per week", shown: money(per, c), source: "bill amount divided by whole weeks until due, rounded up", minor: per }, { label: "Weeks", shown: plural(weeks, "week"), source: "days until due divided by 7, rounded up" }], assumptions: [...COMMON, "Weeks are counted in whole weeks, with at least one.", "The bill is paid in full on its due date."], confidence: "high", suggestion: true });
  });

  // 4. Top categories against earlier months.
  const cur = v.months[v.months.length - 1];
  const earlier = v.months.slice(0, -1).filter((m) => m.has_data).slice(-3);
  if (cur?.has_data && cur.by_category.length && earlier.length >= 1) {
    const top = [...cur.by_category].sort((a, z) => z.minor - a.minor).slice(0, 3);
    const nums: AdviceNumber[] = [];
    const parts = top.map((t) => {
      const usual = med(earlier.map((m) => m.by_category.find((x) => x.category === t.category)?.minor ?? 0));
      const name = CATEGORY_LABEL[t.category] ?? t.category;
      nums.push({ label: `${name} this month`, shown: money(t.minor, c), source: `${monthWord(cur.month)} so far, category ${name}`, minor: t.minor }, { label: `${name} usual`, shown: money(usual, c), source: `median of ${mlist(earlier)}`, minor: usual });
      return `${name} ${money(t.minor, c)} (usual ${money(usual, c)})`;
    });
    items.push({ id: "categories", kind: "categories", claim: `Top spending so far in ${monthWord(cur.month)}: ${parts.join(", ")}.`, numbers: nums, assumptions: [...COMMON, `This month is not finished, so its totals can still grow.`, `A category with no entry in an earlier month counts as nothing spent there.`, "Only expenses with a category are listed."], confidence: earlier.length >= 3 ? "medium" : "low", suggestion: false });
  }

  // 5. Savings goal pace.
  const g = v.goal;
  if (g) {
    const pace = b.saved;
    const goalAssume = [...COMMON, "Pace is the median monthly savings of your last complete months (up to three).", "Saved so far is the total of your savings entries."];
    const paceN: AdviceNumber = { label: "Recorded pace a month", shown: money(pace, c), source: `median of ${b.label} savings`, minor: pace };
    if (g.remaining_minor <= 0) items.push({ id: "goal", kind: "goal", claim: `Your savings goal of ${money(g.target_minor, c)} is reached on the recorded entries.`, numbers: [{ label: "Target", shown: money(g.target_minor, c), source: "your savings goal", minor: g.target_minor }], assumptions: goalAssume, confidence: "high", suggestion: false });
    else if (g.per_month_needed_minor != null && g.deadline) {
      const need = g.per_month_needed_minor;
      items.push({ id: "goal", kind: "goal", claim: `To reach ${money(g.target_minor, c)} by ${g.deadline}, about ${money(need, c)} a month is needed. Your recorded pace is ${money(pace, c)} a month.`, numbers: [{ label: "Needed a month", shown: money(need, c), source: `remaining ${money(g.remaining_minor, c)} spread to the deadline`, minor: need }, paceN, { label: "Target", shown: money(g.target_minor, c), source: "your savings goal", minor: g.target_minor }, { label: "Deadline", shown: g.deadline, source: "your savings goal" }], assumptions: goalAssume, confidence: b.confidence, suggestion: false });
    } else if (pace > 0) {
      const n = Math.ceil(g.remaining_minor / pace);
      items.push({ id: "goal", kind: "goal", claim: `${money(g.remaining_minor, c)} remains toward ${money(g.target_minor, c)}. At your recorded pace of ${money(pace, c)} a month that is about ${plural(n, "month")}.`, numbers: [{ label: "Remaining", shown: money(g.remaining_minor, c), source: "target minus saved so far", minor: g.remaining_minor }, paceN, { label: "Months at this pace", shown: plural(n, "month"), source: "remaining divided by pace, rounded up" }, { label: "Target", shown: money(g.target_minor, c), source: "your savings goal", minor: g.target_minor }], assumptions: goalAssume, confidence: b.confidence, suggestion: false });
    }
  } else hints.push(`Set a savings goal in ${c} to see the pace it needs.`);

  return { currency: c, status: "ok", needs: [], items, completeMonths: b.complete.length, hints };
}

/** "Can I afford it?": the effect of one amount on this month's savings figure, the buffer and the goal date. Needs the same basis as the advice. */
export function canAfford(v: WireCurrencyView, today: string, amountMinor: number): { status: "ok" | "needs_data"; needs: string[]; items: AdviceItem[] } {
  const c = v.currency;
  const b = adviceNeeds(v, today).length ? null : basisOf(v, today);
  if (!b || !Number.isFinite(amountMinor) || amountMinor <= 0) return { status: "needs_data", needs: b ? ["Enter an amount above zero."] : adviceNeeds(v, today), items: [] };
  const committed = v.committed_monthly_minor;
  const amt: AdviceNumber = { label: "Amount", shown: money(amountMinor, c), source: "the amount you entered", minor: amountMinor };
  const assume = [...COMMON, "The amount is spent once, this month, out of money that would otherwise be saved.", "Usual income and spending are the median of your last complete months (up to three).", "Committed bills are assumed not to be inside your recorded spending."];
  const items: AdviceItem[] = [];
  const left = b.income - committed - b.spend;
  const after = left - amountMinor;
  items.push({ id: "afford-savings", kind: "afford", claim: `If ${amt.shown} is spent this month, the figure left to set aside goes from ${money(left, c)} to ${money(after, c)}.`, numbers: [amt, { label: "Left to set aside now", shown: money(left, c), source: "usual income minus committed bills minus usual spending", minor: left }, { label: "Left to set aside after", shown: money(after, c), source: "the figure above minus the amount", minor: after }], assumptions: assume, confidence: b.confidence, suggestion: false });
  const burn = b.spend + committed;
  if (v.available && burn > 0 && v.available.amount_minor >= 0) {
    const now = (v.available.amount_minor / burn).toFixed(1);
    const then = ((v.available.amount_minor - amountMinor) / burn).toFixed(1);
    items.push({ id: "afford-buffer", kind: "afford", claim: `Your buffer goes from ${now} months to ${then} months.`, numbers: [{ label: "Buffer now", shown: `${now} months`, source: "available now divided by usual spending plus committed bills" }, { label: "Buffer after", shown: `${then} months`, source: "available now minus the amount, divided by the same monthly outgo" }, amt], assumptions: [...COMMON, "The amount comes out of the available balance.", "Spending stays at the usual level and no income arrives."], confidence: b.confidence === "high" ? "medium" : b.confidence, suggestion: false });
  }
  const g = v.goal;
  if (g && g.remaining_minor > 0 && b.saved > 0) {
    const today0 = today.slice(0, 7);
    const m0 = Math.ceil(g.remaining_minor / b.saved), m1 = Math.ceil((g.remaining_minor + amountMinor) / b.saved);
    const d0 = monthYear(addMonths(today0, m0)), d1 = monthYear(addMonths(today0, m1));
    items.push({ id: "afford-goal", kind: "afford", claim: m0 === m1 ? `The goal date stays at ${d0} at your recorded pace.` : `At your recorded pace the goal date moves from ${d0} to ${d1}.`, numbers: [{ label: "Goal date now", shown: d0, source: "remaining divided by recorded monthly pace, rounded up to whole months" }, { label: "Goal date after", shown: d1, source: "remaining plus the amount, divided by the same pace" }, { label: "Recorded pace a month", shown: money(b.saved, c), source: `median of ${b.label} savings`, minor: b.saved }, amt], assumptions: [...COMMON, "The amount is added to what is still to save.", "Saving continues at the recorded pace and starts this month."], confidence: b.confidence, suggestion: false });
  }
  return { status: "ok", needs: [], items };
}

/** Plain text of the facts for the chat: claims, their numbers with sources, and assumptions. */
export function adviceContext(r: AdviceResult): string {
  if (r.status !== "ok") return `Money read (${r.currency}): not enough recorded data. ${r.needs.join(" ")}`;
  return [`Money read (${r.currency}), computed from recorded entries only:`, ...r.items.map((i) => `- ${i.claim} [${i.numbers.map((n) => `${n.label}: ${n.shown} (${n.source})`).join("; ")}] Assumptions: ${i.assumptions.join(" ")}`)].join("\n");
}

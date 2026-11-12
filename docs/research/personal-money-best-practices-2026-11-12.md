# Personal money best practices (research, 2026-11-12)

Scope: design input for a read-only, local-first personal-money section (one person in Japan; US checking/brokerage/IRA, SMBC and Japan Post Bank via CSV, watch-only crypto).
Method: ~9 web searches, snippets only (no deep page reads, 20-minute cap). Most app-comparison sources are affiliate blogs; treat feature claims as "reported", not verified against vendor sites. Items marked **unverified** come from general knowledge or a single weak source. Not tax or financial advice.

## 1. Leading apps and what each is loved for

Comparison roundup (affiliate-style, conflicting prices; verify on vendor sites): https://www.youngju.dev/blog/culture/2026-05-16-personal-finance-budgeting-apps-2026-ynab-monarch-money-copilot-actual-budget-lunch-money-firefly-iii-deep-dive.en and https://www.fincomparelab.com/best-tools/best-ynab-alternatives/

| App | Loved for (1-2 features) | Source |
|---|---|---|
| YNAB | Zero-based "give every dollar a job" method; behaviour change; "age of money" metric. Reported ~$109/yr. | https://envelopebudgeting.com/articles/monarch-vs-ynab ; https://www.ynab.com/blog/the-key-to-reducing-your-money-stress |
| Monarch Money | Net worth and investments alongside budget; good for couples/households. Core reported ~$99.99/yr. | https://envelopebudgeting.com/articles/monarch-vs-ynab |
| Copilot Money | Automation-first, polished design, adaptive budgets that flex with actual patterns; Apple-only (no Android). ~$95/yr reported. | https://www.asianefficiency.com/technology/copilot-money-vs-monarch-vs-ynab/ ; https://getfinny.app/blog/ynab-vs-copilot-money-2026 |
| Rocket Money | Subscription detection and cancellation. **Unverified** (search did not return a Rocket Money source; general knowledge). | none retrieved |
| Lunch Money | Developer/power-user flexibility, minimalist. Little detail found; **unverified** beyond the roundup. | https://toolradar.com/guides/best-ynab-alternatives |
| Actual Budget | Free, open source (MIT), local-first (SQLite on device), YNAB-style envelopes, YNAB import, rules that apply retroactively, CSV/QIF/OFX/QFX/CAMT import, "Add Multiplier" for approximate currency conversion. | https://actualbudget.org/docs/transactions/importing ; https://actualbudget.org/ ; https://www.youngju.dev/blog/culture/2026-05-16-personal-finance-budgeting-apps-2026-ynab-monarch-money-copilot-actual-budget-lunch-money-firefly-iii-deep-dive.en |
| Firefly III | Self-hosted; multi-currency (each administration has a primary currency, foreign amounts per transaction); rules to clean up bank CSVs; recurring transactions. Docs say recurring cannot be split or have a dynamic amount. | https://docs.firefly-iii.org/how-to/firefly-iii/features/currencies/ ; https://docs.firefly-iii.org/how-to/firefly-iii/finances/recurring/ |
| Empower (ex-Personal Capital) | Free net-worth dashboard, retirement planner, investment fee analyzer (benchmark 0.50%). Paid wealth management is the upsell. | https://support-personalwealth.empower.com/hc/en-us/articles/201169600-Retirement-Fee-Analyzer-Calculations-Overview ; https://choosefi.com/personal-capital-review-the-ultimate-net-worth-tracker/ |
| Tiller | Bank feeds into your own Google/Excel sheet (full customisability). **Unverified**, no source retrieved. | none retrieved |
| Money Forward ME (JP) | Widest account linking (~2,500 services), strong analysis charts and automatic investment valuation, same/next-day reflection; free tier capped at 4 linked accounts; standard plan 540 yen/mo or 5,940 yen/yr. | https://appllio.com/household-account-book-app-zaim-vs-moneyforward ; https://japanlivinglife.com/articles/money-management-apps-japan |
| Zaim (JP) | Daily-spend tracking, receipt scanning (long-receipt mode), cheaper premium; sync can lag up to a week. | https://appllio.com/household-account-book-app-zaim-vs-moneyforward ; https://app-liv.jp/articles/153119/ |

Takeaway for this app: the loved features cluster into (a) a method that forces decisions (YNAB), (b) net worth in one view (Monarch/Empower), (c) automation of categorisation and bills (Copilot, rules in Actual/Firefly), (d) subscription audit (Rocket Money, unverified). Local-first CSV tools (Actual, Firefly) prove rules + CSV import is a workable base without bank aggregation. Note Money Forward ME's limits (4 free links) and aggregator dependence are exactly what a private read-only local tool avoids; whether MF ME/Zaim link to US institutions is **unverified** (assume no).

## 2. Budgeting methods and fit for irregular entrepreneur income

- Zero-based / envelope ("give every dollar a job"): YNAB's four rules are give every dollar a job, embrace true expenses, roll with the punches, age your money; target age of money ~30 days. https://www.ynab.com/blog/the-key-to-reducing-your-money-stress ; https://cdn.jsdelivr.net/npm/@maro-org/ynab-mcp@0.8.0/build/methodology/terminology.md
- A third-party blog claims YNAB shifted in May 2025 to "Five Key Questions" with the four rules still foundational; **unverified** against YNAB: https://joingerald.com/learn/financial-wellness/ynab
- YNAB publishes an 8-part irregular-income series (aging money, non-monthly expenses): https://www.ynab.com/say-goodbye-to-crazytown-mastering-your-variable-income-once-for-all-part-8
- Sinking funds (monthly set-asides for known irregular costs): closest YNAB equivalent is "embrace true expenses" (break large expenses into monthly amounts with target dates). https://cheatography.com/pfife/cheat-sheets/ynab-you-need-a-budget/pdf
- 50/30/20 and pay-yourself-first: no source retrieved; description (needs/wants/savings split; automate savings first) is general knowledge, **unverified here**.
- Irregular income practice: budget from the lowest recent month (review 6-12 months), not the average; route all income to a holding account and pay yourself a fixed baseline into an operating account; surplus months fill a buffer (1-2 months of baseline expenses suggested, varies); set aside tax before counting income as spendable. https://pocketguard.com/blog/budget-with-irregular-income/ ; https://www.key.com/personal/financial-wellness/articles/irregular-income-budget.html ; https://www.capitalone.com/bank/money-management/ways-to-save/irregular-income-budget/
- Another source suggests saving 2-3 months of expenses when income varies: https://joingerald.com/learn/financial-wellness/ynab (weak source).

Recommendation (design judgement): for an entrepreneur, a "baseline salary + buffer + sinking funds" model beats percentage rules. Use zero-based thinking only for the baseline salary, sinking funds for annual/irregular costs (tax, insurance, resident tax, travel, renewals), and treat percentages (50/30/20) as a sanity ratio only.

## 3. Most useful views and alerts

Sources for these are mostly product feature lists; the framing below is design judgement unless cited.
- Net worth over time (all currencies, snapshots): the headline feature of Monarch and Empower. https://envelopebudgeting.com/articles/monarch-vs-ynab ; https://choosefi.com/personal-capital-review-the-ultimate-net-worth-tracker/
- Cash-flow forecast (30/60/90 days from known recurring items + sinking-fund contributions + expected income range): Empower lists cash-flow views; forecasting depth is **unverified** for all apps. https://www.mybanktracker.com/blog/investing/personal-capital-review-310564
- Safe-to-spend (liquid cash minus upcoming bills minus sinking-fund commitments minus tax reserve): design judgement; Copilot's adaptive budgets are the nearest reported analogue. https://getfinny.app/blog/ynab-vs-copilot-money-2026
- Upcoming bills and recurring detection: Firefly III recurring transactions. https://docs.firefly-iii.org/how-to/firefly-iii/finances/recurring/
- Subscription audit (list, annualised cost, last-charged, price-change flag): Rocket Money's signature feature, **unverified** here.
- Savings rate (trailing 3/12-month, income-variance aware).
- Emergency-fund runway in months = liquid reserves / baseline monthly outflow (use baseline, not average).
- Goal funding (sinking funds with target date and monthly need): per YNAB target-date funding. https://cheatography.com/pfife/cheat-sheets/ynab-you-need-a-budget/pdf
- Fee analyzer for brokerage/IRA holdings (expense ratio vs 0.50% benchmark): https://support-personalwealth.empower.com/hc/en-us/articles/201169600-Retirement-Fee-Analyzer-Calculations-Overview
- Alerts worth having (all local): bill due in N days with insufficient balance in that account; balance below buffer floor; a CSV/balance snapshot older than N days (staleness); unusual charge or price increase on a subscription; deadline reminders (section 5).

## 4. Multi-currency (JPY/USD) best practice

- Keep every transaction in its native currency and amount; store a converted value separately. Firefly III does this (primary currency plus foreign amount per transaction; a manually set primary-currency amount overrides a calculated one). https://docs.firefly-iii.org/how-to/firefly-iii/features/currencies/ ; https://docs.firefly-iii.org/explanation/financial-concepts/exchange-rates/
- Actual Budget's CSV import only offers a blunt multiplier for approximate conversion, a sign of weak multi-currency support in local-first tools; do better. https://actualbudget.org/docs/transactions/importing
- Design recommendations (judgement): store rate history locally (date, pair, source); show net worth in a chosen display currency with a one-click toggle JPY/USD; separate "FX gain/loss" from real spending change in net-worth deltas; budget in the currency you earn/spend in (yen living costs vs USD income) and show a "USD income needed to cover JPY baseline" line; a manual rate override per day for offline use.
- For tax-reporting conversion, rates differ from market rates: FBAR uses the Treasury Reporting Rate of Exchange for December 31 applied to each account's peak balance; Form 8938 uses the year-end rate. So keep the Dec 31 rate and per-account peak balance as stored fields. https://www.taxesforexpats.com/articles/fbar-fatca/fbar-exchange-rate.html ; https://www.irs.gov/businesses/comparison-of-form-8938-and-fbar-requirements
- Which Treasury rate to use and how the annual figure is published: confirm from Treasury (Bureau of the Fiscal Service) directly; **unverified** (not fetched).

## 5. US-expat items to surface read-only (awareness reminders, not advice)

- FBAR (FinCEN Form 114): required if aggregate foreign financial accounts exceed $10,000 at any time in the year (aggregate, not per account); due April 15 with automatic extension to October 15; filed electronically with FinCEN, not with the 1040; keep records 5 years. Report each account's maximum value converted at the Dec 31 Treasury rate. Japanese bank accounts (SMBC, Japan Post Bank) count; crypto held on foreign exchanges may be a grey area, **unverified**. https://www.irs.gov/fbar ; https://www.taxesforexpats.com/articles/fbar/a-detailed-look-at-the-foreign-bank-account-report-fbar-form.html
- A claimed June 15 FBAR date for expats is disputed by sources; plan around Oct 15. https://www.taxesforexpats.com/articles/fbar/a-detailed-look-at-the-foreign-bank-account-report-fbar-form.html
- Form 8938 (FATCA), separate from FBAR: abroad thresholds $200,000 year-end / $300,000 any-time (single; $400,000 / $600,000 joint). https://www.irs.gov/businesses/comparison-of-form-8938-and-fbar-requirements ; https://www.taxesforexpats.com/guides/FBAR-8938.html
- FEIE: 2026 maximum exclusion $132,900 per IRS Notice 2026-25 (2025 was $130,000); housing base 16% ($21,264) and cap 30% ($39,870) for 2026. Qualifying via physical presence test = 330 full days in any 12 consecutive months abroad, reported on Form 2555 Part III; bona fide residence is Part II. https://www.irs.gov/pub/irs-drop/n-26-25.pdf ; https://kpmg.com/xx/en/our-insights/gms-flash-alert/2026/flash-alert-2026-090.html ; https://www.greenbacktaxservices.com/knowledge-center/physical-presence-test/
- Design: a "US expat calendar" card with read-only reminders (FBAR Apr 15/Oct 15; US return due date and expat automatic extension to June 15 **unverified here**; Japan tax-year items **unverified**), a running aggregate-foreign-balance vs $10,000 and 8938 thresholds, per-account peak balance for the year, a day-count of days outside the US toward 330 (user-entered or from calendar), and the FEIE limit vs YTD earned income. Always label "awareness, not advice; confirm with a preparer".
- IRA contributions while using FEIE, foreign tax credit alternatives, PFIC treatment of Japanese funds, Japan-side tax: **unverified**, out of scope of this research; flag as "ask a preparer" prompts only.

## 6. What makes such tools stick

Little hard evidence retrieved; the following are design inferences plus cited tool behaviour.
- Low-friction categorisation via rules: both Actual and Firefly emphasise rules that auto-categorise on payee patterns and can be applied retroactively. https://actualbudget.org/docs/transactions/importing ; https://docs.firefly-iii.org/how-to/firefly-iii/finances/recurring/
- Import quality matters: Actual's release notes show constant fixes for CSV edge cases (date formats, UTF-16 encodings), highly relevant to Japanese bank exports (Shift-JIS and similar encodings are **unverified** for SMBC/Japan Post; test with real exports). https://actualbudget.org/docs/releases
- Duplicate/reconciliation handling on re-import is essential for CSV workflows: https://medium.com/actualbudget/how-transaction-reconciliation-works-8dc5749bbd21
- Weekly review ritual (10 minutes): import new CSVs, update manual balances, clear uncategorised queue, glance at safe-to-spend and upcoming 14 days, tick sinking-fund contributions. Cadence is practitioner convention; no source retrieved (**unverified**).
- Reduce guilt: show runway and progress, not shame; surface a single "next action" per session.
- Staleness indicator per account (last updated) so manual data is never silently trusted.

## 7. Ranked top-10 features for this app

"Local-first now" = buildable from CSV + manual balances with no network.

1. Net worth over time (multi-currency, JPY/USD toggle, snapshots from manual balances). Why: the single most-cited high-value view; anchors everything. Local-first now: yes.
2. CSV import with per-bank profiles (SMBC, Japan Post, US checking), de-duplication, encoding handling. Why: without clean data nothing else works. Local-first now: yes.
3. Rules-based auto-categorisation with retroactive apply. Why: the main stickiness lever (low friction). Local-first now: yes.
4. Baseline salary + buffer + sinking funds model (irregular-income fit). Why: matches an entrepreneur's cash reality better than percentage rules. Local-first now: yes.
5. Safe-to-spend number (liquid minus upcoming bills, sinking-fund commitments, tax reserve). Why: the one daily answer people actually want. Local-first now: yes, needs recurring list from item 6.
6. Recurring bills and subscription audit (detect from CSV history, annualised cost, price-change flag). Why: quick win, real savings. Local-first now: yes.
7. Cash-flow forecast 30/60/90 days with income as a range (low/expected). Why: prevents cash surprises under irregular income. Local-first now: yes (from recurring + manual expected income).
8. US expat awareness card (FBAR/8938 aggregate vs thresholds, per-account peak, Dec 31 rate field, FEIE limit and 330-day count, deadline reminders). Why: costly to miss, rarely handled by mainstream apps. Local-first now: yes (manual rates and dates); official Treasury rate lookup is optional network.
9. Emergency-fund runway and savings rate (baseline-based). Why: simple, motivating, robust to income swings. Local-first now: yes.
10. Weekly review ritual screen with staleness alerts and a short checklist. Why: habit formation and data trust. Local-first now: yes.

Deferred (need network or heavy lifting): live FX rates and crypto watch-only prices (public price API or address watch), brokerage/IRA holdings with fee analyzer (CSV positions possible, expense ratios need lookup), automatic bank feeds (conflicts with read-only/local-first goals; not recommended).

## Gaps to verify before building
Rocket Money, Tiller and Lunch Money feature claims; Treasury Reporting Rate source and format; Japanese bank CSV encodings and column layouts; FBAR treatment of crypto and of Japan Post Bank accounts; US return extension dates for expats; Japan-side tax reminders; any evidence for weekly-review efficacy.

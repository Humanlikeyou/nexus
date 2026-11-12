# Money view research spike (2026-11-12)

Scope: read-only research. No account, key or service was contacted; only public docs and web search were read.
Method note: web search was shallow. Anything tagged **unverified** was not confirmed on a primary page. Items tagged
"(spec, from memory)" are well-known standards whose page I did not fetch in this spike; re-check before coding.
Hard constraints assumed: read-only, no seed/private key/wallet login, data stays on the Mac, outbound network off by default.

## Bottom line
1. Start with file import (CSV/PDF statements) for every institution. It needs no new host and no sign-up.
2. US aggregators are possible but add a cloud intermediary; Plaid is the only one I could partly verify for individuals.
3. Japan: no read API is realistically open to an individual. Use CSV downloads.
4. Ledger: watch-only via your own Bitcoin Core plus an Electrum server on the Mac; no public explorer.
5. FX: ECB reference rates (one file, daily) with a manual rate entry fallback.
6. Seed guard: pure local rules, no network. Rules in section 5.

---
## 1. US aggregation (Chime, Fidelity)

### 1a. Aggregators
| Option | Chime | Fidelity | Individual use | Source |
|---|---|---|---|---|
| Plaid | Listed as a Plaid-powered app/institution (Chime also is a Plaid customer) | Contested: moved to OAuth in 2023; a forum post says it is back via OAuth, undated | Plaid pricing page says "Pay as You Go" is "Best for individuals and developers"; business entity requirement not stated there | https://plaid.com/pricing/ ; https://plaid.com/en-eu/what-is-plaid/ ; https://plaid.com/customer-stories/chime/ ; https://feedback.lunchmoney.app/institution-status-requests/p/fidelity-investments |
| Finicity (Mastercard) | unverified | A commenter says Finicity still connects Fidelity for a budgeting app (anecdotal) | unverified | https://lr.eu.psf.lt/r/fidelityinvestments/comments/16vl2cp/fidelity_no_longer_supported_by_plaid |
| MX | unverified | unverified | unverified | none found |
| Akoya | not applicable (Akoya is Fidelity-origin, bank-owned data network) | Akoya was built from Fidelity Access, spun out 2019, owned by Fidelity, TCH and 11 banks; uses OAuth 2.0 / OIDC tokens | Developer eligibility for an individual: unverified (likely requires a data-recipient application) | https://www.americanbanker.com/news/fidelity-data-sharing-hub-aims-to-end-screen-scraping ; https://www.pymnts.com/data/2020/fidelity-teams-with-tch-fis-to-launch-personal-data-startup/ |
| Fidelity own data sharing | n/a | Not found beyond Akoya/OAuth route: unverified | unverified | none found |

Plaid details found:
- Plans: Free (Sandbox free; Limited Production up to 200 API calls per product on live data), Pay as You Go, Growth (12-month commit), Custom. Source: https://plaid.com/pricing/
- Per-product unit prices for Transactions/Investments/Balance are not shown on the page ("Included" or contact sales). Single-user cost: **unverified**.
- A third-party brief says Plaid Investments needs Growth or Custom plan (itself unverified): https://raxx-internal-docs.pages.dev/legal/research/fidelity-api-integration-2026-05-05.html
- Plaid says OAuth institutions need dashboard registration and per-institution approval: https://plaid.com/docs/link/oauth/index.html.md
- Read-only product names (from Plaid's public product naming, from memory, unverified here): Transactions, Balance, Auth, Investments (Holdings/Investments Transactions). Request only Transactions and Investments scopes; never Transfer/Payment Initiation.
- Token held locally: the access_token is a string; Plaid's API is server to server and needs your client_id/secret, so a desktop-only design means the secret sits on the Mac and calls go to a Plaid host. Whether Plaid permits a no-backend personal app: **unverified**. Link (the consent UI) is a hosted web flow, so it also needs an outbound host.

Recommendation: do not integrate an aggregator in v1. If later wanted, trial Plaid Pay as You Go for Chime only (owner decision, new host `production.plaid.com` plus Link host, plus secret on Mac). Treat Fidelity via aggregator as unreliable until proven.
Trade-offs: aggregator gives automatic sync but a third party sees all balances/transactions, needs sign-up and a network host, and Fidelity coverage has flip-flopped.

### 1b. No-aggregator fallback (file import)
- Chime: no native CSV/Excel export per several sources; only monthly PDF statements. Paths reported: app Settings > Documents > Monthly Statements, or Profile > Documents > Account statements (export icon); web Profile > Documents. Sources are third-party converter sites, not Chime: https://www.docuclipper.com/blog/convert-chime-bank-statement-to-excel/ ; https://ledgerbridge-2ja.pages.dev/convert/chime . Confirm in Chime's help center (unverified).
  Implication: parse PDF text locally (do NOT upload to converter sites).
- Fidelity: CSV from Activity & Orders > History, choose period, download; file name like `History_for_Account_ending_in_XXXX.csv`; guides disagree on max span (90 days vs 93-day windows back 5 years) so verify in app. Sources: https://www.fidelity.com/customer-service/faqs-exporting-account-information ; https://traderfyles.helpscoutdocs.com/article/11-how-to-export-trade-history-from-fidelity
- Fidelity Quicken/QuickBooks download: official FAQ says first download returns last 90 days, then up to 90 days of new history since last download (same FAQ URL). A Quicken forum user suggests QFX may no longer be offered (forum opinion, unverified): https://community.quicken.com/discussion/7904506/fidelity-investment-account-transactions-not-downloading-correctly
  Positions/balances CSV ("Portfolio positions" download): unverified.
- IRA is a Fidelity account so the same History CSV applies (unverified for IRA-specific menu).
Recommendation: manual monthly import of Fidelity CSV and Chime PDF into local SQLite. Trade-off: manual, monthly lag, PDF parsing is brittle.

---
## 2. Japanese banks

### 2a. Statement CSV
Search found no primary spec for personal-account CSVs at any of the eight banks. Do not hard-code formats; ship a "sniff and map" importer (detect encoding, header row, sign convention) and have the owner supply one sample file per bank he picks.

| Bank | CSV available | Encoding / headers / formats | Source |
|---|---|---|---|
| MUFG | unverified for personal; business formats exist (corporate, different) | unverified | https://www.bk.mufg.jp/houjin/dente/pdf/csvFormat.pdf (corporate) |
| SMBC | unverified | unverified | none |
| Mizuho | unverified | unverified | none |
| Japan Post Bank | unverified | unverified | none |
| Rakuten Bank | Yes, PC web only (app has no CSV); CSV or PDF from account opening date to day before request | encoding and headers unverified (my guess of headers from the query is not confirmed) | https://support.freee.co.jp/hc/ja/articles/4414077747481 ; https://www.rakuten-bank.co.jp/info/2021/211029.html |
| SBI Sumishin Net Bank (now NEOBANK/docomo-branded in places) | Yes, CSV and PDF on web and app (2021 notice); older notice: "明細をダウンロード(CSV形式)" link, max 999 rows per download (old); statement screen redesigned summer 2024, so format may have changed | encoding and headers unverified | https://www.netbk.co.jp/contents/company/info/2021/mg_notice_210219_info_2.html ; https://www.netbk.co.jp/contents/company/info/2013/mg_notice_130917_2_info.html ; https://www.netbk.co.jp/contents/company/info/2024/0612_002662.html |
| PayPay Bank | unverified | unverified | none |
| Sony Bank | unverified | unverified | none |

General facts found:
- Japanese accounting tools commonly require Shift-JIS CSV with 日付/入金/出金 columns (Yayoi): https://support.yayoi-kk.co.jp/subcontents.html?page_id=27075 . This is the import-side requirement, not proof about what banks emit. Expect Shift-JIS (CP932) with some banks possibly UTF-8; handle both. Unverified per bank.
- Corporate/regional bank formats can be fixed-width JIS (Zengin style) which is a different thing; the importer should not assume it.
Recommendation: build importer with encoding auto-detect (try UTF-8 with BOM, then CP932), tolerate quoted fields, 全角 digits, comma thousands, "yyyy/mm/dd" and "yyyy年mm月dd日" dates, and either a signed amount or separate 入金/出金 columns. Trade-off: guessing wrong silently corrupts totals, so show a preview and require owner confirmation on first import per bank.

### 2b. Open banking / read APIs
- 電子決済等代行業 (EPSP) is a registered status under Japan's Banking Act amendments (in force June 2018); MUFG and SMBC publish lists of connected providers that include Moneytree: https://www.bk.mufg.jp/innovation/api/partner/direct/directapi_externalserviceproviders.pdf ; https://www.smbc.co.jp/collaboration/keiyakunaiyou.html
- Moneytree's B2B platform "MT LINK" is sold to financial institutions and companies, not individuals: https://www.atpress.ne.jp/news/125163 ; https://ascii.jp/elem/000/001/486/1486913/
- Money Forward appears as a chained-connection (連鎖接続) party in Sony Bank's API docs: https://sonybank.jp/services/api/01.html . Money Forward ME API for individuals: **unverified, likely not offered**.
- Individual developer access to any bank read API: **unverified, assume no**. EPSP registration is a business licence.
Recommendation: skip open banking. Optional later: let the owner use Moneytree or Money Forward ME apps separately; do not integrate.

---
## 3. Ledger watch-only (BTC and ETH)
Found:
- An xpub lets anyone derive every address in the account and see balances and all future transactions; sending it to a third-party server deanonymizes the whole wallet: https://vault12.com/help/xpub-watch-only/ ; https://www.spark.money/glossary/watch-only-wallet
- Ledger Live can export xpub (without fingerprint, with path): https://node40.atlassian.net/wiki/spaces/NKB/pages/2162360396/How+to+Export+your+Ledger+Hardware+Wallet+Extended+Public+Key+x+y+zPub
- Sparrow supports watch-only import from xpub plus derivation path, warns before exporting an xpub: https://support.bitbox.swiss/en_US/create-sparrow-watch-only-wallet
- Tor/VPN is suggested as a mitigation (same vault12 page).
Not verified in this spike: Fulcrum/Electrs address logging, Electrum Personal Server details, Bitcoin Core descriptor import (single unverified table says descriptor-based only in v30), Helios/ETH light client behaviour. Cite and check before building: Fulcrum and Bitcoin Core docs.

Options (privacy ranked):
1. Own node on the Mac: Bitcoin Core (pruned ok for balances via descriptors with `importdescriptors`, spec/doc unverified here) querying only localhost. Zero outside leak. Cost: initial sync and disk. The user's Mac has 500 GB (memory note); pruned node is about a few GB to tens of GB plus time (size unverified). Note full address index needs Fulcrum/Electrs, which needs an unpruned node (unverified).
2. Own Electrum server (Fulcrum/electrs) over unpruned Core: best wallet UX, heavy disk (unverified size).
3. Public Electrum server or explorer: server learns all your addresses and IP. Only acceptable over Tor and per-address, never by xpub.
ETH: query your own address against your own node (e.g. Geth/Reth, heavy) or a light client (Helios claims trustless sync, unverified here). Public RPC reveals the address and IP; an ETH address is a single public identifier, so the leak is smaller than an xpub but still links IP to holdings.
Recommendation: v1 stores nothing about the wallet on a server: owner pastes an xpub or address list that the app keeps encrypted locally, and balances come from Bitcoin Core on localhost (pruned, descriptors, rescan) if the owner OKs running it; otherwise manual balance entry. Defer ETH: manual entry, or own light client later. Trade-off: node sync takes days and disk; manual entry is stale.
Hard rule: accept only xpub/ypub/zpub/addresses. Never accept a seed, xprv, WIF, or key (guard in section 5).

---
## 4. JPY/USD reference rate
Findings:
- ECB: reference rates usually updated about 16:00 CET each working day except TARGET closing days; "for information purposes only", transaction use "strongly discouraged"; averages of buy/sell, not trade prices; if no direct quote exists a cross rate via a major liquid currency is used: https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html ; https://www.ecb.europa.eu/stats/pdf/exchange/Frameworkfortheeuroforeignexchangereferencerates.en.pdf . Reuse licence: unverified (check ECB legal notice).
  JPY/USD cross: USD per EUR = U, JPY per EUR = J; JPY per USD = J / U. Both are in the same daily ECB file (JPY and USD in the 30-currency set: unverified here but widely known). Cost: one host `www.ecb.europa.eu`.
- Federal Reserve H.10: released Mondays at 4:15 pm for the previous week; the Board is retiring the Data Download Program and points to FRED; terms not stated on the page: https://www.federalreserve.gov/releases/h10/default.htm . Weekly cadence makes it poor for daily values.
- Bank of Japan: publishes 外国為替市況 daily after business hours (since 2007-01-04), recent 70 business days on page, long series on its time-series site, data may be corrected: https://www.boj.or.jp/statistics/market/forex/fxdaily/
- MUFG (via MURC site): 公表仲値 TTM = (TTS+TTB)/2; yearly past data and downloads; site disclaims accuracy vs bank-published rates; publication time unverified: https://www.murc-kawasesouba.jp/fx/past_3month.php ; https://www.murc-kawasesouba.jp/fx/index.php?id=261016 . TTM is what Japanese practice (e.g., yen conversion of foreign income) often uses; confirm with tax adviser (unverified).
Recommendation: ECB daily file as the default (one host, daily, timestamped, stable XML/CSV). Store each rate with source, publication date and fetched-at. Optional second source MUFG TTM for tax-style conversion.
Offline/manual fallback: the owner types a rate, source and date; stored as "manual". If no rate exists for a date the UI shows "no rate" and blank converted totals; the app never interpolates, extrapolates or uses a hard-coded default. Show rate age and flag if older than 5 business days.

---
## 5. Seed-phrase guard (detector rules; all local, no network)
Facts:
- BIP-39: ENT 128/160/192/224/256 bits give 12/15/18/21/24 words; checksum = first ENT/32 bits of SHA-256(entropy); 11 bits per word, 2048-word list; mnemonic is UTF-8 NFKD; checksum catches about 255/256 random errors, so a random 12 word BIP-39 string passes with prob about 1/16 (4 checksum bits): https://bips.dev/39/
- Wordlists: English is the default; non-English lists exist but BIP-39 says they are strongly discouraged; Japanese list is in the BIP wordlists doc (separator U+3000 ideographic space is from memory, unverified here): https://bips.dev/39/ ; https://github.com/bitcoin/bips/blob/master/bip-0039/bip-0039-wordlists.md
- Bech32/bech32m: v0 witness uses bech32, v1-v16 use bech32m (constant 0x2bc830a3), HRP `bc` mainnet, mixed case invalid, all upper valid: https://bips.dev/350/
- EIP-55: mixed-case checksum, 40 hex chars, keccak256 over lowercase hex ASCII; example `0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed`: https://eips.ethereum.org/EIPS/eip-55

Rules (BLOCK = refuse to store/send/log; ALLOW = fine):
| Class | Rule | Action |
|---|---|---|
| BIP-39 words | Normalize NFKD, lowercase, split on whitespace/U+3000; if token count in {12,15,18,21,24} AND at least 11 of every 12 tokens (or all) are in the English or Japanese list -> BLOCK. Do not require a valid checksum (typos still leak). Also BLOCK runs of >= 8 consecutive list words anywhere in text | BLOCK |
| xprv family | `\b[xyztuvYZTUV]prv[1-9A-HJ-NP-Za-km-z]{100,112}\b` (SLIP-132 prefixes; length 111 base58 normally; spec from memory, unverified) | BLOCK |
| WIF | `\b[5KL][1-9A-HJ-NP-Za-km-z]{50,51}\b` (mainnet uncompressed 51 chars starts 5; compressed 52 chars starts K/L) (from memory) | BLOCK |
| Raw hex key | `\b(?:0x)?[0-9a-fA-F]{64}\b` | BLOCK (also catches tx hashes: false positives accepted; allow only if preceded by label `txid`) |
| xpub family | `\b[xyzXYZ]pub[1-9A-HJ-NP-Za-km-z]{100,112}\b` | ALLOW (public); mark sensitive, never send out |
| BTC legacy | `\b1[1-9A-HJ-NP-Za-km-z]{25,34}\b` | ALLOW |
| BTC P2SH | `\b3[1-9A-HJ-NP-Za-km-z]{25,34}\b` | ALLOW |
| bech32 v0 | `\bbc1q[02-9ac-hj-np-z]{38,58}\b` (case-insensitive; 42 chars P2WPKH, 62 P2WSH) | ALLOW |
| bech32m v1 | `\bbc1p[02-9ac-hj-np-z]{58}\b` (62 chars taproot) | ALLOW |
| ETH address | `\b0x[0-9a-fA-F]{40}\b`; if mixed case, verify EIP-55 checksum, reject on mismatch | ALLOW |
Notes: validate Base58Check / bech32 checksums in code, not regex alone. Run the guard on every text input, paste, import file, log line and clipboard read. Version prefixes for xprv/WIF above are from BIP-32/SLIP-132/WIF knowledge, not fetched here; add unit tests from the BIP-32 and BIP-39 test vectors (https://bips.dev/32/ , https://bips.dev/39/).

---
## 6. Owner OK list and build order

Needs the owner's explicit OK:
- Which Japanese banks (choose before building importers; supply one sample CSV per bank, with account numbers removed).
- Any new outbound host: `www.ecb.europa.eu` (FX), and later `production.plaid.com` + Plaid Link host (aggregator), a Bitcoin peer network or public RPC if not using own node.
- Any sign-up: Plaid account/dashboard, Akoya or Finicity developer access. None done in this spike.
- Running Bitcoin Core (disk, bandwidth, background process, Tor or not).
- Storing the xpub locally (encrypted, sensitive).
- Whether to feed Chime PDFs to a local parser only (no cloud converters).

Suggested build order (local first):
1. Seed/key guard (section 5) with tests; wire it into every input path first.
2. Local encrypted SQLite store: accounts, transactions, positions, rates; no network.
3. File importers: Fidelity History CSV, bank CSV sniffer (CP932/UTF-8), Chime PDF text parser. Preview + confirm.
4. Manual FX rate entry and manual crypto balance entry (no network).
5. Read-only views: net worth by currency, converted only when a stored rate exists.
6. With owner OK: ECB daily rate fetch (single host, fixed URL, response size cap).
7. With owner OK: Bitcoin Core localhost watch-only balances (xpub/descriptor); ETH manual or own light client.
8. Last, optional: Plaid for Chime only, after a cost/privacy review.

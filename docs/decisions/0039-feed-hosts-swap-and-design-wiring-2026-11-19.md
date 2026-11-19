# 0039: Swap the broken feed hosts, and Claude does the Design wiring (Taisun, 2026-11-19, in session)

Context: a live check of decision 0038's feeds found CoinGecko working. Stooq returns 404 and serves a JavaScript bot wall. GDELT rate-limits even a single request. Three options were offered.

His words: **"1"**. That approves option 1:
- `stooq.com` is replaced by **`query1.finance.yahoo.com:443`**, the keyless chart API, unofficial and may change, for stock prices.
- `api.gdeltproject.org` is replaced by **`news.google.com:443`**, keyless RSS search; Google's terms allow personal, non-commercial feed-reader use.
- CoinGecko stays. Both new hosts were checked live on 2026-11-19 and return the expected data.
- All the 0038 rules still apply: outbound gate per request, no proxy, no redirects, caps, watch-only, local tier only, and nothing about Taisun sent.

His words (next message): **"you can continue with the design too and i think you start on stage h no?"**

- **Design:** Taisun now lets Claude Code (via the Design Lead agent) wire the new backends into the screens. This lifts hard rule 10 / design-boundary for this wiring. The list is in `tasks/design-1118-coordination.md`:
  - Path accept/decline and Undo
  - trading Record/Dismiss/Undo and the watchlist panels
  - Smart home devices
  - Finances / Ventures (incl. linked goals) / Content stages and saved drafts
  - the Treasury screen
  - the Home lens hooks
  - the strap Needs-you note
- Changes stay small and consistent with the existing design system. The Grok bot's design commits are never rewritten, and each touch is noted for Design.
- **Stage H:** already done as far as it goes. Open at login is built. Signing and auto-update were dropped by decision 0038. Nothing more to build.

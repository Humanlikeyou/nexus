# 0038: Temporary ledger, watchlist feeds, no Apple signing (Taisun, 2026-11-19, in session)

His words (verbatim): **"1. Make a temporary ledger file 2. The business is still in the budding fase so we don't have any sales report yet. 3. Ok 4.up to you 5. Amara picks the best ones to watch based on extensive deep research and those panels stay up and track the stock and crypto until Amara ask if I want a new pick a couple weeks later 6 … I don't need this "Get an Apple Developer certificate. It lets the app be properly signed and update itself.""**

1. **Ledger:** a temporary `<data>/business/ledger.json` (0600, folder 0700) was created by Claude on 2026-11-19. It has one venture, `taiyo-skin` "Taiyo Skin", stage Budding, health searching, and no money figures (no fx, funds or targets). It is checked by the app's own reader.
2. **No sales exports yet:** Treasury keeps only the generic column mapper until real exports exist.
3. **Keychain moves:** he will choose "Allow" when asked. No screen calls the move yet.
4. **Stale data window:** Claude's choice, 7 days (resolves 0032 #7's "77").
5. **Watchlist feeds: approved (new outbound hosts, hard rule 3).**
   - Amara researches and picks a few stocks and crypto to watch.
   - The panels track live prices until, about every 14 days, Amara asks whether he wants new picks.
   - Watch-only as before: no orders, no keys, no broker, and "Not financial advice" on every pick.
   - Hosts (Claude's choice: free, keyless, read-only):
     - `api.coingecko.com:443`: crypto prices
     - `stooq.com:443`: stock prices (CSV)
     - `api.gdeltproject.org:443`: news headlines for the research
   - The research and picking run on the local deep tier, with no Claude tier.
   - All feed calls respect `NEXUS_OUTBOUND_DISABLED`, navy/local-only and Settings › Outbound.
   - Any other host stays gated.
6. **No Apple Developer ID:**
   - Signing, notarization and auto-update (Stage H Phases 14–15) are dropped.
   - The app stays a local ad-hoc install.
   - The built code stays inert: `UPDATER_SWITCH = false` and `release-local.sh` refuses without an identity.
   - The release docs mark these "not needed".

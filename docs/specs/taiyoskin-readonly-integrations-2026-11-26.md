# TaiyoSkin read-only integrations and intelligence (Phase 12)

Saved 2026-11-26 from Taisun's prompt in session. This is Phase 12 in `AMARA_PROJECT_HANDOFF.md` §5 (spec 2, "Amara Enterprise"). It builds on the workspace foundation (decision 0063).

## His prompt (verbatim)

> Implement the first read-only TaiyoSkin connector integrations using the approved Enterprise architecture.
>
> First inspect the actual TaiyoSkin deployment, Lovable project, Supabase database, Hostinger configuration and available Google account/API access.
>
> Do not assume credentials or integrations exist.
>
> Prioritize:
>
> 1. Supabase data access.
> 2. Google Analytics 4.
> 3. Google Search Console.
> 4. Website and deployment metadata.
> 5. Business documents.
>
> For each connector:
> - Use supported authenticated APIs.
> - Keep secrets out of frontend code.
> - Support secure credential storage.
> - Implement pagination and rate-limit handling.
> - Track synchronization timestamps.
> - Preserve source provenance.
> - Handle missing access and disconnected states.
> - Prevent duplicate ingestion.
> - Support explicit manual synchronization.
> - Do not silently upload customer information elsewhere.
>
> Customer Intelligence:
>
> Analyze permitted customer information in aggregated and appropriately anonymized form.
>
> Identify:
> - Frequent customer concerns.
> - Product preferences.
> - Customer-reported needs.
> - Frequently asked questions.
> - Product interest patterns.
> - Content gaps.
> - Potential product opportunities.
>
> Website Intelligence:
>
> Analyze available:
> - Page performance.
> - Traffic sources.
> - Entry and exit pages.
> - Product engagement.
> - Conversion events.
> - Search queries.
> - Search impressions.
> - Search clicks.
> - Search positions.
> - Indexed pages.
>
> Do not fabricate unavailable metrics.
>
> Every insight must distinguish:
> - Measured information.
> - Calculated metrics.
> - AI interpretation.
> - Unverified hypothesis.
>
> Keep TaiyoSkin analytics separate from ARM trading and personal financial information.
>
> Start with read-only queries and reports. Do not publish, modify or delete website content in this phase.

## Inspection (2026-11-26, read-only; no row contents and no key values read)

| Source | Found |
|---|---|
| Deployment | `taiyoskin.com` is served by Hostinger's CDN (`platform: hostinger`). The live files' last-modified date is 2026-08-09. Deploys are by **manual upload** to the File Manager (DEPLOYMENT-GUIDE.md); no Hostinger API is in use. The live sitemap has 25 URLs; the repo's has 24 |
| Lovable | Project `7afce6c7-…` (README). It is linked to the private GitHub repo `Humanlikeyou/taiyo-skin-craft` (`main` pushed 2026-10-21). Lovable has no read API this app could use; GitHub is the deployment-history source |
| Supabase | Project `taiyoskin` (`hghluspjxlfhymjmlmcx`, Tokyo). The CLI on this Mac is logged in and linked. Only the publishable (anon) key is on disk; **no service-role key exists locally**. Estimated rows: page_views ~2,279, sessions ~824, analytics_events ~593, checkout_events ~32, waitlist ~10, quiz_results ~10, customer_profiles ~3, contact_requests ~1, orders 0. A second, older project (`TaiyoSkinAgent`) exists |
| Live site and Supabase | The live bundle (2026-08-09) contains **no Supabase project URL**. It does not appear to send anything to Supabase now (not verified at runtime) |
| GA4 | Property `G-VJ6E2TSRR8` was added on `main` on 2026-03-22 (it loads at runtime from `VITE_GA4_MEASUREMENT_ID`). The **live bundle has no GA4 ID and no tag script**. Branch `cursor/remove-third-party-tracking` (2026-10-21) removes third-party analytics. So GA4 is very likely collecting nothing now |
| Search Console | There is no verification tag in the source or the live HTML. A DNS-verified property may exist; that can't be known without Google API access |
| Google API access | `gcloud` isn't installed, and no Google OAuth client or service account exists for this app |
| Business documents | The Google Drive desktop folder is synced at `~/Library/CloudStorage/GoogleDrive-<account>`. It holds many TaiyoSkin documents ("00 Index - Taiyo Skin", Knowledge Base, FORGE label, compliance, pre-order and email packs, the cost model) |

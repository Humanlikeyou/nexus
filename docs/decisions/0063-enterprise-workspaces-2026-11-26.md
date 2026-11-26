# 0063: Amara Enterprise business workspaces, Phase 11 foundation (Taisun, 2026-11-26, in session)

Brief: `docs/specs/enterprise-foundation-2026-11-26.md`. It builds on the approved data architecture (0053): the business scope, `business/amara_business.db` and the permission table in §6.

Claude asked three questions. Taisun answered:
- **Design:** "Override for Enterprise". The design boundary is lifted for the Enterprise host only, as 0055 did for Life. One new Enterprise subview, **Workspaces**, sits after Ventures. Overview, Decisions, Ventures, Finances, Treasury, Tasks and Content are unchanged, and Overview gains one link card.
- **Seed data:** "Import as unverified". TaiyoSkin's material comes from his local files, and every item is `unverified` with its source, until he approves it in the app.
- **Connectors:** Supabase, Hostinger, Instagram and the local site repository.

## What landed

**Store** `src-tauri/src/commands/business_workspace.rs`: `<data>/business/workspaces.db` (business scope), with document copies under `<data>/business/workspaces/<id>/documents/` (0600).

| Brief item | How |
|---|---|
| 1. Identity | `business` row: id, name, description, the linked ledger venture, active or archived. A new file gets **TaiyoSkin** (`taiyo-skin`, linked to the ledger venture `taiyo-skin`) |
| 2. Multiple businesses | Create, rename, archive. The picker appears when there are two or more |
| 3. Context and memory | Ten sections (the brief's list). Items are `draft`, `unverified`, `approved` or `archived`, with a version history. Only the user approves; editing approved text makes it a draft again. Memory notes: the user's are kept, and Amara's wait for the user |
| 4. Documents and knowledge | Files are copied in, hashed and read back. Text documents can be searched together with context; each hit shows its status, and archived material is left out |
| 5. Integration registry | `connectors.rs` `DEFINITIONS`: Supabase, Hostinger, Instagram and the website source on this Mac. Each business gets each one, disconnected |
| 6. Status and configuration | Settings are validated per field. Secrets go **only to the macOS Keychain** (`com.taisun.nexus.biz.<business>.<connector>.<field>`), are write-only and zeroized; the view says `stored` true or false. States: `disconnected`, `auth_required`, `connected` or `error`, worked out on every read. **Nothing is ever shown as connected without a passing check.** Phase 11 has no network, so only the local folder can be checked; a remote connector with its key stored is "Authentication required: not verified" |
| 7. Permissions | Per business, per actor (Amara, background jobs, integrations), per capability. Defaults: Amara may read approved context and propose changes, and nothing else is on. Approving material, publishing and external writes are not capabilities at all. `business_context_for_amara` returns exactly what Amara may be given |
| 8. Synchronization | `sync_local.rs`: a read-only scan of the site folder (routes, products, ingredients, markdown docs). Each run is recorded with seen, changed and conflicts. **A sync never changes approved, draft or archived material**; a difference counts as a conflict. A run left running by a crash reads as interrupted. Remote sync: "not available yet" |
| 9. Activity history | Every change writes a business activity row to `amara_business.db`, tagged with its business (`biz:` refs, below) |
| 10. Action approvals | Amara's proposals are inert `proposal` rows. Approving opens the native dialog with the full text. The change then runs as the controller operation `business.context.apply_proposal` (origin model, user approval bound to the text's SHA-256, reversible local, business scope), and the controller marks it verified by reading it back |

**`amara_store` v3** (additive, the 0053 migration rules): `business_id` on `activity` and `recommendation`. It is derived from a new `biz:<business>/…` record reference and allowed on business rows only (CHECK). A file with rows is backed up first, and an open takes at most one backup. Business recommendation evidence must stay in the same business. New queries list one business's activity and recommendations.

**Keychain:** the trait gains `delete` (a default that refuses, plus the macOS and test implementations) for Disconnect.

## Tenant isolation and separation (each has a test)

- Every workspace table has `business_id` in its primary key, with composite foreign keys, so a cross-business reference is refused by the database. Triggers refuse any change to `business_id`.
- Rust reaches rows only through a `Tenant` for one validated id. A source-pin test checks that **every** statement on a workspace table binds `business_id = ?1`. It inspects 30+ statements, and a mutation test showed it fails when a bind is dropped. One listed exception: the startup step that marks stale runs as interrupted.
- The workspace code never names personal stores, Letta, chat context, ARM, Treasury, money or any network client (source pin). The chat context builders (`terrence_runtime.rs`, `continuity.rs`, `amara_turn.rs`) never read workspaces. The personal file refuses a `business_id`.
- No revenue is shown in Workspaces. Business income stays in Finances and Treasury, and ARM stays apart.

## Verification

- Rust: 19 workspace tests, plus 1 opt-in live test (`NEXUS_LIVE_SITE=<folder> cargo test --lib business_workspace::tests::live -- --ignored`). The amara_store suite has 24 tests including the v3 migration; 2 existing assertions were updated (the version is now 3, and one backup per open).
- **Live run against `~/projects/taiyoskin/pr11-fix`, read-only, into a temp data folder:** connected; 12 items found (3 products, 3 ingredient lists, the site's pages and 5 documents), all unverified. The real app data was not touched.
- vitest: 11 view tests. Two nav pins were updated for the new tab: `routes.test.ts` and the ⌘K snapshot.
- e2e `test/e2e/workspaces.spec.ts`: 12 passed (light and dark), including axe with no serious or critical violations on four sections, with colour contrast included.

## Not done, and why

- **No network checks, remote sync or external writes.** Those belong to Phase 12 (authenticated, read-only) and Phase 13 (publishing).
- **Amara's chat does not read workspaces yet.** `context_for_model` is the boundary for it; wiring it into chat is a separate step.
- **Brand voice and formulations** aren't in the site source. Voice can be added as a document (for example a voice file); formulations stay empty until Taisun adds the approved ones.
- **Not run:** an independent security review, a launch of the packaged app, and an install.

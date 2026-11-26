# 0065: Website operations workflow, Phase 13 (Taisun, 2026-11-26, in session)

Brief: `docs/specs/website-operations-2026-11-26.md`.

## Inspection

- **Deployment:** the site source is built locally (`npm run build`), then `dist/` is uploaded by hand to `public_html` through Hostinger's File Manager (DEPLOYMENT-GUIDE.md). No SSH, SFTP, FTP or CI is set up on this Mac.
- **The live build matches no local source.** Live serves `assets/index-L4HtXu4M.js`. The four local builds are `CRL9hxaz`, `BHoh61mC` and `CyOw-rw4` (June) and `C6Q8J1SP` (12 August). Which branch produced live is unknown.
- **Hostinger API:** as found, its deploy-from-archive operation is documented for Agency (h5g) plans. It overwrites the site with no undo, and there's no general shared-hosting file API. The supported routes for a normal plan are SFTP/FTP or hPanel Git deployment. Not verified against Taisun's actual plan.

## Taisun's answers

| Question | Answer |
|---|---|
| How to publish | **Amara prepares, Taisun uploads** in File Manager; Amara verifies |
| Source of truth | **taiyoskin.com**: the live site itself |
| Safe test target | **Local preview only** |
| Backup | **Download the live public files, plus an hPanel backup** |

## Consequences

- Changes are computed from a **snapshot of the live files**.
- Content compiled into the JavaScript bundle can't be edited safely without a source that reproduces live. That covers product pages, product descriptions, ingredients, FAQs, blog posts and landing pages. These types are registered and refused with that reason ("as they become available").
- **Available now:** SEO title and description (index.html, plus og/twitter tags), robots.txt, sitemap.xml, replacing an image or asset with an approved document, and deleting a file.
- **Preview** is rendered in the app: a search-result card for SEO metadata, the text for robots and the sitemap. **Flag:** the "local preview on 127.0.0.1" option would have needed a new listener, which `.claude/rules/rust.md` forbids, so none was added. Previewing the whole built site needs a decision.

## What landed

`business_workspace/site_ops.rs`, workspace store **v3** (additive): `site_snapshot`, `site_change`, and `site_change_event` (append-only: update and delete are refused). Tenant pins and triggers cover all three.

| Stage | How |
|---|---|
| 1–3 Locate, approved information, current content | A live snapshot over `net.rs` (approved hosts and the outbound switch): index.html, robots, the sitemap and every same-site file they and their CSS link to, hashed and read back. A search runs across the snapshot and approved context; a bundled match is labelled "not editable yet" |
| 4 Propose | Computed from the snapshot's bytes. Cited sources must be **approved**, otherwise it's refused. Asset replacement only from an **approved** document of the same file type, with its hash checked |
| 5 Compare | A line diff with context |
| 6 Validate | Lengths; exactly one `<title>`/description tag; HTTPS links; same-site links exist in the snapshot or sitemap; sitemap URLs on the site; robots directives. **Flags:** pricing, formulation, customer communication, deletion. Pricing without an approved product item, or formulation and ingredient wording without an approved ingredient or formulation item, is an **error**, which blocks approval |
| 7 Preview | Rendered in the app |
| 8 Approve | Only through the native dialog, which spells out every flag ("THIS CHANGES PRICING"), each warning and the diff. Bound to the exact text shown; errors can't be approved. Drafting needs no publishing permission, and a model draft needs `propose_changes` |
| 9 Back up | A fresh live snapshot right before packaging. If the live file changed since the draft, the change is withdrawn. Requires Taisun's "I took an hPanel backup" confirmation |
| 10 Publish | `upload.tar.gz`, `rollback.tar.gz` and a README. **Taisun uploads.** Nothing in the code can write to the website (source pin: no SFTP, FTP, POST, PUT or HTTP client of its own) |
| 11 Verify | The live file is fetched and compared by hash with the approved bytes (or checked to be gone, for a deletion) |
| 12 Audit | Every transition is a `site_change_event` plus a business activity row |
| 13 Roll back | Upload `rollback.tar.gz`, then Verify rollback compares the live file with the backup |

UI: Enterprise › Workspaces › **Website changes**.

## Verification

- **Rust:** 5 workflow tests against a fake live site:
  - draft → approved → packaged → verify_failed (before the upload) → verified → rolled_back, with append-only history;
  - a stale live file withdraws the change;
  - formulation, pricing and deletion flags and their required sources; sitemap, robots, unapproved sources, bundled types and model permission;
  - asset replacement from an approved document of the same type;
  - no write path to the website.
- **vitest:** 3 view tests.
- **e2e:** the new tab is covered, including axe.
- **Not run:** a live snapshot of taiyoskin.com through the workflow, any real upload, `.tar.gz` extraction in Hostinger's File Manager (assumed supported, not checked), an independent security review, and an install.

## Next decisions for Taisun

1. Confirm the source that reproduces the live build, which would unlock product, ingredient, FAQ, blog and landing-page edits.
2. Whether a full-site local preview may add a loopback listener (a rule exception).
3. Whether to add SFTP to a staging subdomain later.

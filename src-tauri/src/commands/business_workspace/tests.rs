//! 0063 workspace tests: real SQLite files and folders in a temp directory; rows checked on disk.

use super::connectors::{
    definition, derive_state, service_name, validate_config, CheckResult, TEST_HOME,
};
use super::*;
use crate::commands::amara_store::{store_for, Scope};
use crate::commands::execution::{Controller, ExecRequest, ExecStatus, Ledger, Origin, Registry};
use crate::commands::keychain::{FakeKeychain, Keychain};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(tag: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("nexus-biz-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    Tmp(p)
}
fn input(section: &str, title: &str, body: &str) -> BusinessContextInput {
    BusinessContextInput {
        item_id: None,
        section: section.into(),
        title: title.into(),
        body: body.into(),
        status: "draft".into(),
        expected_version: None,
    }
}
fn two(data: &Path) -> (WorkspaceStore, Tenant, Tenant) {
    let s = store_in(data);
    s.create_business("Second Brand", None, None, None).unwrap();
    let a = s.tenant("taiyo-skin").unwrap();
    let b = s.tenant("second-brand").unwrap();
    (s, a, b)
}

// ─── Identity and multiple businesses ────────────────────────────────────────

#[test]
fn a_new_store_holds_taiyoskin_linked_to_its_venture_with_every_connector_disconnected() {
    let d = tmp("seed");
    let s = store_in(&d.0);
    let list = s.list_businesses().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].business_id, "taiyo-skin");
    assert_eq!(list[0].name, "TaiyoSkin");
    assert_eq!(list[0].venture_id.as_deref(), Some("taiyo-skin"));
    let t = s.tenant("taiyo-skin").unwrap();
    let kc = FakeKeychain::default();
    let cs = t.connectors(&kc).unwrap();
    assert_eq!(
        cs.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(),
        [
            "supabase",
            "website",
            "github",
            "google_analytics",
            "search_console",
            "instagram",
            "local_repo",
            "drive_folder"
        ]
    );
    assert!(cs
        .iter()
        .all(|c| c.state == "disconnected" && !c.external_writes));
    // Reopening never seeds twice, and the file is private.
    assert_eq!(store_in(&d.0).list_businesses().unwrap().len(), 1);
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(d.0.join("business/workspaces.db"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn more_businesses_can_be_created_and_ids_are_validated() {
    let d = tmp("multi");
    let s = store_in(&d.0);
    let b = s
        .create_business("Second Brand", None, Some("Test"), None)
        .unwrap();
    assert_eq!(b.business_id, "second-brand");
    assert!(s
        .create_business("Second Brand", None, None, None)
        .unwrap_err()
        .contains("already exists"));
    assert!(s.create_business("", None, None, None).is_err());
    assert!(BusinessId::parse("../x").is_err());
    assert!(BusinessId::parse("A").is_err());
    assert!(s.tenant("nope").err().unwrap().contains("no such business"));
    assert!(s.tenant("Bad Id").is_err());
    assert_eq!(s.list_businesses().unwrap().len(), 2);
    let t = s.tenant("second-brand").unwrap();
    let up = t
        .update_identity("Second Brand Co", None, Some("second"), "archived")
        .unwrap();
    assert_eq!(
        (up.name.as_str(), up.status.as_str()),
        ("Second Brand Co", "archived")
    );
}

// ─── Tenant isolation ────────────────────────────────────────────────────────

#[test]
fn one_business_never_sees_or_changes_another_businesss_records() {
    let d = tmp("iso");
    let (_s, a, b) = two(&d.0);
    let item = a
        .context_save(
            &input("positioning", "Secret plan", "Only for A"),
            "taisun",
            "user",
            None,
        )
        .unwrap();
    a.memory_add("A remembers this", "user").unwrap();
    a.document_store("A doc", "a.md", b"alpha text", "draft", "user", None, None)
        .unwrap();

    assert!(b.context_list(None).unwrap().is_empty());
    assert!(b.context_get(&item.item_id).unwrap().is_none());
    assert!(b.memory_list().unwrap().is_empty());
    assert!(b.documents().unwrap().is_empty());
    assert!(b.knowledge_search("alpha").unwrap().is_empty());
    assert!(b.knowledge_search("Secret").unwrap().is_empty());
    assert_eq!(a.knowledge_search("Secret").unwrap().len(), 1);
    // B can't edit, approve, history or propose against A's item id.
    let edit = BusinessContextInput {
        item_id: Some(item.item_id.clone()),
        expected_version: Some(1),
        ..input("positioning", "x", "y")
    };
    assert!(b.context_save(&edit, "taisun", "user", None).is_err());
    assert!(b
        .context_set_status(&item.item_id, "approved", 1, "taisun")
        .is_err());
    assert!(b.context_history(&item.item_id).unwrap().is_empty());
    assert!(b
        .proposal_create(&item.item_id, "x", "y", None, "amara")
        .unwrap_err()
        .contains("no such item"));
    // A is untouched.
    assert_eq!(
        a.context_get(&item.item_id).unwrap().unwrap().title,
        "Secret plan"
    );
}

#[test]
fn the_database_itself_refuses_cross_business_links_and_business_id_changes() {
    let d = tmp("isodb");
    let (_s, a, b) = two(&d.0);
    let item = a
        .context_save(&input("objectives", "Goal", "Grow"), "taisun", "user", None)
        .unwrap();
    // A proposal row in B that points at A's item fails the composite foreign key.
    let r = b.conn().execute(
        "INSERT INTO proposal (business_id, proposal_id, kind, item_id, base_version, title, body, status, proposed_by, proposed_at)
         VALUES ('second-brand', 'p1', 'context_update', ?1, 1, 't', 'b', 'pending', 'amara', 'now')",
        params![item.item_id],
    );
    assert!(r.is_err(), "cross-business reference must be refused");
    // Moving a row to another business is refused by trigger, on every table.
    for sql in [
        "UPDATE context_item SET business_id = 'second-brand'",
        "UPDATE connector SET business_id = 'second-brand'",
        "UPDATE permission SET business_id = 'second-brand'",
        "UPDATE business SET business_id = 'x2' WHERE business_id = 'taiyo-skin'",
    ] {
        let e = a.conn().execute(sql, []).unwrap_err().to_string();
        assert!(e.contains("business_id is fixed"), "{sql}: {e}");
    }
}

/// Source pin: every statement that reads or changes a business's records binds `business_id = ?1`.
/// From each `FROM <table>` / `UPDATE <table>` in production code, the rest of that SQL string must
/// bind the tenant. The only exception is listed with its reason. The test also checks it really
/// inspected the statements (a pin that matches nothing would pass vacuously).
#[test]
fn every_tenant_statement_binds_its_business_id() {
    let tables = [
        "context_item",
        "context_version",
        "memory",
        "document",
        "connector",
        "sync_run",
        "permission",
        "proposal",
        // 0064
        "connector_check",
        "source_cursor",
        "site_page_view",
        "site_session",
        "site_event",
        "site_checkout",
        "aggregate_snapshot",
        "site_fact",
        "business_salt",
        "insight_note",
        // 0065
        "site_snapshot",
        "site_change",
        "site_change_event",
    ];
    // Startup maintenance: marks stale runs interrupted in every business; reads nothing out.
    let exceptions = [
        "SET status = 'interrupted'",
        // The v2 schema migration renames a connector id in every business; it reads nothing out.
        "SET connector_id = 'website' WHERE connector_id = 'hostinger'",
    ];
    let mut checked = 0;
    for (name, src) in [
        (
            "business_workspace.rs",
            include_str!("../business_workspace.rs"),
        ),
        ("connectors.rs", include_str!("connectors.rs")),
        ("sync_local.rs", include_str!("sync_local.rs")),
        ("supabase.rs", include_str!("supabase.rs")),
        ("website.rs", include_str!("website.rs")),
        ("intelligence.rs", include_str!("intelligence.rs")),
        ("site_ops.rs", include_str!("site_ops.rs")),
    ] {
        // Tests live in tests.rs, so each of these files is production code from top to bottom.
        let prod = src;
        for t in tables {
            for kw in ["FROM", "UPDATE"] {
                for pat in [format!("{kw} {t} "), format!("{kw} {t}\n")] {
                    for (at, _) in prod.match_indices(&pat) {
                        // The statement text up to the end of its string literal.
                        let rest = &prod[at..];
                        let end = rest
                            .char_indices()
                            .find(|&(i, c)| c == '"' && !rest[..i].ends_with('\\'))
                            .map(|(i, _)| i)
                            .unwrap_or(rest.len());
                        let stmt = &rest[..end];
                        let before = &prod[at.saturating_sub(40)..at];
                        if before.contains("CREATE")
                            || before.contains("REFERENCES")
                            || exceptions.iter().any(|e| stmt.contains(e))
                        {
                            continue;
                        }
                        checked += 1;
                        assert!(
                            stmt.contains("business_id = ?1"),
                            "{name}: statement without the tenant bind:\n{stmt}"
                        );
                    }
                }
            }
        }
    }
    assert!(checked >= 30, "only {checked} statements inspected");
}

// ─── Approved, draft, unverified ─────────────────────────────────────────────

#[test]
fn only_the_user_approves_and_editing_approved_text_makes_it_a_draft_again() {
    let d = tmp("approve");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let i = t
        .context_save(
            &input("identity", "Voice", "Calm, direct."),
            "taisun",
            "user",
            None,
        )
        .unwrap();
    assert_eq!((i.status.as_str(), i.version), ("draft", 1));
    assert!(t
        .context_set_status(&i.item_id, "approved", 1, "amara")
        .unwrap_err()
        .contains("only you"));
    let a = t
        .context_set_status(&i.item_id, "approved", 1, "taisun")
        .unwrap();
    assert_eq!(a.status, "approved");
    assert!(a.approved_at.is_some());
    // A stale version is refused.
    assert!(t
        .context_set_status(&i.item_id, "archived", 1, "taisun")
        .is_err());
    let e = t
        .context_save(
            &BusinessContextInput {
                item_id: Some(i.item_id.clone()),
                expected_version: Some(2),
                ..input("identity", "Voice", "Calm, direct, warm.")
            },
            "taisun",
            "user",
            None,
        )
        .unwrap();
    assert_eq!((e.status.as_str(), e.version), ("draft", 3));
    assert!(e.approved_at.is_none());
    let h = t.context_history(&i.item_id).unwrap();
    assert_eq!(h.iter().map(|v| v.version).collect::<Vec<_>>(), [3, 2, 1]);
    assert_eq!(h[1].status, "approved");
    // Saving can never set approved directly.
    let mut bad = input("identity", "x", "y");
    bad.status = "approved".into();
    assert!(t.context_save(&bad, "taisun", "user", None).is_err());
    // History is append-only.
    assert!(t
        .conn()
        .execute("UPDATE context_version SET body = 'x'", [])
        .is_err());
    let counts = t.section_counts().unwrap();
    let id = counts.iter().find(|c| c.section == "identity").unwrap();
    assert_eq!((id.approved, id.draft, id.unverified), (0, 1, 0));
    assert_eq!(counts.len(), 10);
}

#[test]
fn amara_gets_approved_context_only_unless_the_user_allows_more() {
    let d = tmp("model");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let a = t
        .context_save(
            &input("positioning", "Approved", "yes"),
            "taisun",
            "user",
            None,
        )
        .unwrap();
    t.context_set_status(&a.item_id, "approved", 1, "taisun")
        .unwrap();
    t.context_save(&input("positioning", "Draft", "no"), "taisun", "user", None)
        .unwrap();
    let seen = t.context_for_model().unwrap();
    assert_eq!(
        seen.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
        ["Approved"]
    );
    t.set_permission("model", "read_unverified_context", true)
        .unwrap();
    assert_eq!(t.context_for_model().unwrap().len(), 2);
    t.set_permission("model", "read_approved_context", false)
        .unwrap();
    assert!(t.context_for_model().unwrap_err().contains("not permitted"));
    // Defaults: only model read-approved and propose; scheduler and integrations nothing.
    let other = store_in(&d.0)
        .create_business("Fresh Co", None, None, None)
        .unwrap();
    let p = store_in(&d.0)
        .tenant(&other.business_id)
        .unwrap()
        .permissions()
        .unwrap();
    let on: Vec<_> = p
        .iter()
        .filter(|x| x.allowed)
        .map(|x| format!("{}:{}", x.actor, x.capability))
        .collect();
    assert_eq!(on, ["model:read_approved_context", "model:propose_changes"]);
    assert!(t.set_permission("model", "write_website", true).is_err());
}

#[test]
fn memory_from_the_user_is_kept_and_from_amara_waits_for_the_user() {
    let d = tmp("mem");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    assert_eq!(
        t.memory_add("Launch is FORGE first", "user")
            .unwrap()
            .status,
        "kept"
    );
    let m = t.memory_add("Customers ask about scent", "amara").unwrap();
    assert_eq!(m.status, "proposed");
    assert_eq!(
        t.memory_decide(&m.memory_id, "dismissed").unwrap().status,
        "dismissed"
    );
    t.set_permission("model", "propose_changes", false).unwrap();
    assert!(t
        .memory_add("x", "amara")
        .unwrap_err()
        .contains("not permitted"));
}

#[test]
fn documents_are_copied_privately_hashed_and_searchable_by_status() {
    let d = tmp("docs");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let doc = t
        .document_store(
            "Brand book",
            "../../evil name.md",
            b"# Brand\nOur tone is calm.",
            "draft",
            "user",
            None,
            None,
        )
        .unwrap();
    assert_eq!(doc.kind, "markdown");
    assert!(doc.searchable);
    assert!(!doc.file_name.contains('/') && !doc.file_name.contains(".."));
    let path =
        d.0.join("business/workspaces/taiyo-skin/documents")
            .join(&doc.file_name);
    assert_eq!(sha256_hex(&std::fs::read(&path).unwrap()), doc.sha256);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let hits = t.knowledge_search("TONE").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].status, "draft");
    assert!(hits[0].excerpt.contains("tone is calm"));
    assert!(t
        .document_set_status(&doc.doc_id, "approved", 1, "amara")
        .is_err());
    let ap = t
        .document_set_status(&doc.doc_id, "approved", 1, "taisun")
        .unwrap();
    assert_eq!(ap.status, "approved");
    t.document_set_status(&doc.doc_id, "archived", 2, "taisun")
        .unwrap();
    assert!(
        t.knowledge_search("tone").unwrap().is_empty(),
        "archived material is left out"
    );
    let pdf = t
        .document_store("Spec", "spec.pdf", b"%PDF-1.4", "draft", "user", None, None)
        .unwrap();
    assert!(!pdf.searchable);
    assert!(t
        .document_store("Empty", "e.md", b"", "draft", "user", None, None)
        .is_err());
    assert!(t.knowledge_search("a").is_err());
    // Multi-byte text around a match is cut on character boundaries.
    t.context_save(
        &input("identity", "日本語", "太陽の肌 ケア calm 日本"),
        "taisun",
        "user",
        None,
    )
    .unwrap();
    assert!(t
        .knowledge_search("calm")
        .unwrap()
        .iter()
        .any(|h| h.excerpt.contains("太陽")));
}

// ─── Connectors and secrets ──────────────────────────────────────────────────

#[test]
fn connector_states_are_never_connected_without_a_passing_check() {
    let supa = definition("supabase").unwrap();
    let site = definition("website").unwrap();
    let local = definition("local_repo").unwrap();
    let ga = definition("google_analytics").unwrap();
    let ok = CheckResult {
        outcome: "ok".into(),
        detail: "fine".into(),
        at: "t".into(),
    };
    let auth = CheckResult {
        outcome: "auth".into(),
        detail: "refused".into(),
        at: "t".into(),
    };
    let bad = CheckResult {
        outcome: "error".into(),
        detail: "gone".into(),
        at: "t".into(),
    };
    assert_eq!(
        derive_state(supa, false, &Ok(vec![false]), None).0,
        "disconnected"
    );
    assert_eq!(
        derive_state(supa, true, &Ok(vec![false]), None).0,
        "auth_required"
    );
    let (s, why) = derive_state(supa, true, &Ok(vec![true]), None);
    assert_eq!(
        s, "auth_required",
        "a stored key with no check is not a connection"
    );
    assert!(why.contains("not checked"));
    assert_eq!(
        derive_state(supa, true, &Ok(vec![true]), Some(&ok)).0,
        "connected"
    );
    assert_eq!(
        derive_state(supa, true, &Ok(vec![true]), Some(&auth)).0,
        "auth_required"
    );
    assert_eq!(
        derive_state(supa, true, &Ok(vec![true]), Some(&bad)).0,
        "error"
    );
    assert_eq!(
        derive_state(supa, true, &Err("locked".into()), Some(&ok)).0,
        "error"
    );
    assert_eq!(
        derive_state(site, true, &Ok(vec![]), None).0,
        "disconnected"
    );
    assert_eq!(
        derive_state(local, true, &Ok(vec![]), Some(&ok)).0,
        "connected"
    );
    assert_eq!(
        derive_state(local, true, &Ok(vec![]), Some(&bad)).0,
        "error"
    );
    assert_eq!(
        derive_state(local, false, &Ok(vec![]), None).0,
        "disconnected"
    );
    let (s, why) = derive_state(ga, true, &Ok(vec![]), Some(&ok));
    assert_eq!(s, "disconnected", "a deferred connector is never connected");
    assert!(why.contains("Deferred"));
}

#[test]
fn secrets_go_to_the_keychain_per_business_and_never_come_back() {
    let d = tmp("secret");
    let (_s, a, b) = two(&d.0);
    let kc = FakeKeychain::default();
    let v = a
        .connector_configure(
            &kc,
            "github",
            &BTreeMap::from([("repo".into(), "owner/site".into())]),
        )
        .unwrap();
    assert_eq!(v.state, "auth_required");
    let v = a
        .connector_secret_set(&kc, "github", "token", "  ghp_test-VALUE-123  ".into())
        .unwrap();
    assert!(v.secrets[0].stored);
    assert_eq!(v.state, "auth_required", "stored but not checked");
    let json = serde_json::to_string(&v).unwrap();
    assert!(
        !json.contains("ghp_test-VALUE-123"),
        "no secret in any view"
    );
    assert_eq!(
        kc.get(&service_name(a.id(), "github", "token")).unwrap(),
        b"ghp_test-VALUE-123"
    );
    assert!(!b.connector_view(&kc, "github").unwrap().secrets[0].stored);
    let raw = std::fs::read(d.0.join("business/workspaces.db")).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("ghp_test-VALUE-123"));
    assert!(a
        .connector_secret_set(&kc, "github", "other", "x".into())
        .is_err());
    assert!(a
        .connector_secret_set(&kc, "local_repo", "path", "x".into())
        .is_err());
    assert!(a
        .connector_secret_set(&kc, "github", "token", "has space".into())
        .is_err());
    // A passing check, then a new key: the check no longer applies.
    a.record_check("github", "ok", "readable").unwrap();
    assert_eq!(a.connector_view(&kc, "github").unwrap().state, "connected");
    a.connector_secret_set(&kc, "github", "token", "ghp_new".into())
        .unwrap();
    assert_eq!(
        a.connector_view(&kc, "github").unwrap().state,
        "auth_required"
    );
    // Disconnect removes the key, the settings and the check.
    let v = a.connector_disconnect(&kc, "github").unwrap();
    assert_eq!(v.state, "disconnected");
    assert!(kc.get(&service_name(a.id(), "github", "token")).is_err());
    assert!(a.last_check("github").unwrap().is_none());
}

#[test]
fn connector_settings_are_validated_and_unknown_fields_refused() {
    assert!(validate_config(
        "supabase",
        "pooler_host",
        "aws-1-ap-northeast-1.pooler.supabase.com"
    )
    .is_ok());
    assert!(
        validate_config(
            "supabase",
            "pooler_host",
            "aws-1-us-east-1.pooler.supabase.com"
        )
        .is_err(),
        "only the approved region"
    );
    assert!(validate_config("supabase", "pooler_host", "evil.com").is_err());
    assert!(validate_config("supabase", "project_ref", "hghluspjxlfhymjmlmcx").is_ok());
    assert!(validate_config("supabase", "project_ref", "short").is_err());
    assert!(validate_config("website", "domain", "taiyoskin.com").is_ok());
    assert!(
        validate_config("website", "domain", "example.com").is_err(),
        "an unapproved domain is refused"
    );
    assert!(validate_config("website", "domain", "api.github.com").is_err());
    assert!(validate_config("github", "repo", "owner/site").is_ok());
    assert!(validate_config("github", "repo", "owner/../x").is_err());
    assert!(validate_config("github", "repo", "a/b/c").is_err());
    assert!(validate_config("local_repo", "path", "relative/path").is_err());
    assert!(validate_config("drive_folder", "path", "/etc").is_err());
    let d = tmp("cfg");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let kc = FakeKeychain::default();
    assert!(t
        .connector_configure(
            &kc,
            "supabase",
            &BTreeMap::from([("db_password".into(), "x".into())])
        )
        .is_err());
    assert!(t
        .connector_configure(&kc, "google_analytics", &BTreeMap::new())
        .unwrap_err()
        .contains("isn't available"));
    assert!(t.connector_view(&kc, "stripe").is_err());
}

// ─── Sync from the local repository ──────────────────────────────────────────

const PRODUCTS: &str = r#"import x from "@/assets/x.jpg";
export type Product = { id: string; slug: "shield" | "core"; name: string };
export const products: Product[] = [
  {
    id: "shield",
    slug: "shield",
    name: "SHIELD — Future Sun-Care Research",
    tagline: "No formula has been selected.",
    price: null,
    purchasable: false,
    benefits: [
      "No formula has been selected"
    ],
    ingredients: [
      "Cajeput",
      "Borneol"
    ],
    detailedIngredients: [
      { name: "Cajeput Oil", benefit: "A botanical reference only." }
    ],
    seo: { title: "SHIELD", description: "x" }
  },
  {
    id: "forge",
    slug: "forge",
    name: "FORGE",
    tagline: "Say \"hi\"",
    price: FORGE_PRICE_USD,
    ingredients: ["Water"],
    seo: { title: "FORGE", description: "y" }
  }
];
"#;

fn fake_site(home: &Path) -> PathBuf {
    let root = home.join("site");
    std::fs::create_dir_all(root.join("src/data")).unwrap();
    std::fs::create_dir_all(root.join("src/content")).unwrap();
    std::fs::write(root.join("package.json"), "{}").unwrap();
    std::fs::write(root.join("src/App.tsx"), r#"<Route path="shop" /><Route path="about" /><Route path="/admin/orders" /><Route path="*" /><Route path="shop" />"#).unwrap();
    std::fs::write(root.join("src/data/products.ts"), PRODUCTS).unwrap();
    std::fs::write(root.join("README.md"), "# Site\nHow to edit.").unwrap();
    std::fs::write(root.join("src/content/privacy.en.md"), "Privacy text").unwrap();
    std::fs::write(home.join("outside.md"), "should never be read").unwrap();
    std::os::unix::fs::symlink(home.join("outside.md"), root.join("LINKED.md")).unwrap();
    root
}

#[test]
fn products_and_routes_are_read_from_the_source_text() {
    let ps = sync_local::parse_products(PRODUCTS);
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].name, "SHIELD — Future Sun-Care Research");
    assert_eq!(ps[0].price, None);
    assert_eq!(ps[0].ingredients, ["Cajeput", "Borneol"]);
    assert_eq!(
        ps[0].detailed,
        [(
            "Cajeput Oil".to_string(),
            "A botanical reference only.".to_string()
        )]
    );
    assert_eq!(ps[1].tagline.as_deref(), Some("Say \"hi\""));
    assert_eq!(ps[1].price.as_deref(), Some("FORGE_PRICE_USD"));
    let r = sync_local::parse_routes(
        r#"<Route path="shop" /><Route path="/admin/x" /><Route path="*" /><Route path="shop" />"#,
    );
    assert_eq!(r, ["/shop", "/admin/x"]);
}

#[test]
fn a_sync_writes_unverified_material_and_never_changes_approved_material() {
    let d = tmp("sync");
    TEST_HOME.with(|h| *h.borrow_mut() = Some(d.0.clone()));
    let root = fake_site(&d.0);
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let kc = FakeKeychain::default();
    assert!(sync_local::run(&t, "local_repo", "user")
        .unwrap_err()
        .contains("folder"));
    let v = t
        .connector_configure(
            &kc,
            "local_repo",
            &BTreeMap::from([("path".into(), root.to_string_lossy().into_owned())]),
        )
        .unwrap();
    assert_eq!(v.state, "connected", "{}", v.detail);
    assert_eq!(v.sync.state, "never");

    let run = sync_local::run(&t, "local_repo", "user").unwrap();
    assert_eq!(run.status, "succeeded", "{:?}", run.detail);
    let items = t.context_list(None).unwrap();
    assert!(items
        .iter()
        .all(|i| i.status == "unverified" && i.source == "sync" && i.source_ref.is_some()));
    let sections: Vec<&str> = items.iter().map(|i| i.section.as_str()).collect();
    assert_eq!(sections.iter().filter(|s| **s == "products").count(), 2);
    assert_eq!(sections.iter().filter(|s| **s == "ingredients").count(), 2);
    assert_eq!(sections.iter().filter(|s| **s == "website").count(), 1);
    assert!(
        !sections.contains(&"formulations"),
        "formulations are never guessed"
    );
    let docs = t.documents().unwrap();
    let names: Vec<&str> = docs.iter().map(|d| d.title.as_str()).collect();
    assert!(names.contains(&"README.md") && names.contains(&"src/content/privacy.en.md"));
    assert!(
        !names.contains(&"LINKED.md"),
        "symlinked files are never read"
    );
    assert!(docs.iter().all(|d| d.status == "unverified"));
    let site = items.iter().find(|i| i.section == "website").unwrap();
    assert!(site.body.contains("- /shop") && site.body.contains("Admin pages:\n- /admin/orders"));

    // Unchanged source: nothing changes.
    let again = sync_local::run(&t, "local_repo", "user").unwrap();
    assert_eq!((again.items_changed, again.conflicts), (0, 0));

    // Approve one product; then the source changes both products.
    let shield = items
        .iter()
        .find(|i| i.title.starts_with("SHIELD"))
        .unwrap();
    t.context_set_status(&shield.item_id, "approved", shield.version, "taisun")
        .unwrap();
    std::fs::write(
        root.join("src/data/products.ts"),
        PRODUCTS
            .replace("No formula has been selected.", "Changed.")
            .replace("Say \\\"hi\\\"", "New tagline"),
    )
    .unwrap();
    let third = sync_local::run(&t, "local_repo", "user").unwrap();
    assert_eq!(third.conflicts, 1, "{:?}", third.detail);
    assert_eq!(third.items_changed, 1);
    let after = t.context_get(&shield.item_id).unwrap().unwrap();
    assert_eq!(after.status, "approved");
    assert!(
        !after.body.contains("Changed."),
        "approved material is never overwritten by a sync"
    );
    assert!(t
        .context_list(Some("products"))
        .unwrap()
        .iter()
        .any(|i| i.body.contains("New tagline")));
    let v = t.connector_view(&kc, "local_repo").unwrap();
    assert_eq!(v.sync.state, "succeeded");
    assert_eq!(t.sync_runs(None, 10).unwrap().len(), 3);

    // A folder that disappears is an error, not "connected".
    std::fs::remove_file(root.join("package.json")).unwrap();
    assert_eq!(t.connector_view(&kc, "local_repo").unwrap().state, "error");
    let failed = sync_local::run(&t, "local_repo", "user").unwrap();
    assert_eq!(failed.status, "failed");
    // A remote connector can't be synchronized in this phase; a scheduler needs the permission.
    assert!(sync_local::run(&t, "supabase", "user").is_err());
    assert!(sync_local::run(&t, "local_repo", "scheduler")
        .unwrap_err()
        .contains("not permitted"));
    TEST_HOME.with(|h| *h.borrow_mut() = None);
}

#[test]
fn a_sync_left_running_by_a_crash_reads_as_interrupted() {
    let d = tmp("crash");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    t.conn()
        .execute(
            "INSERT INTO sync_run (business_id, run_id, connector_id, started_at, status) VALUES ('taiyo-skin', 'r1', 'local_repo', '2026-01-01T00:00:00Z', 'running')",
            [],
        )
        .unwrap();
    drop(t);
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    assert_eq!(t.sync_runs(None, 5).unwrap()[0].status, "interrupted");
}

// ─── Business action approvals ───────────────────────────────────────────────

fn controller(data: &Path) -> Controller {
    let mut ops: Registry = HashMap::new();
    let op: Arc<dyn crate::commands::execution::Operation> = Arc::new(ops::ApplyProposal {
        data: data.to_path_buf(),
    });
    ops.insert(ops::APPLY_ACTION, op);
    Controller::new(Ledger::at(data.join("execution.db")), ops)
}

#[test]
fn an_amara_proposal_applies_only_with_approval_and_is_verified_by_read_back() {
    let d = tmp("prop");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let i = t
        .context_save(
            &input("positioning", "Positioning", "Old"),
            "taisun",
            "user",
            None,
        )
        .unwrap();
    let p = t
        .proposal_create(
            &i.item_id,
            "Positioning",
            "New, clearer",
            Some("Shorter"),
            "amara",
        )
        .unwrap();
    assert_eq!(p.status, "pending");
    assert!(t
        .proposal_create(&i.item_id, "Positioning", "Old", None, "amara")
        .unwrap_err()
        .contains("changes nothing"));

    // Submitted as the model without an approval: refused, nothing changes.
    let c = controller(&d.0);
    let params = serde_json::json!({"business_id": "taiyo-skin", "proposal_id": p.proposal_id, "content_sha256": ops::content_sha(&p.title, &p.body)});
    let r = c
        .submit(&ExecRequest {
            action_id: ops::APPLY_ACTION.into(),
            workflow_id: "w".into(),
            idempotency_key: "k-no-approval".into(),
            origin: Origin::Model,
            params,
            approval_id: None,
        })
        .unwrap();
    assert_eq!(r.record().status, ExecStatus::NotAuthorized);
    assert_eq!(t.context_get(&i.item_id).unwrap().unwrap().body, "Old");

    let text = commands::proposal_dialog_text(&d.0, "taiyo-skin", &p.proposal_id).unwrap();
    assert!(text.contains("New, clearer") && text.contains("TaiyoSkin"));
    let done = commands::approve_proposal_in(&d.0, &c, "taiyo-skin", &p.proposal_id).unwrap();
    assert_eq!(done.status, "applied");
    let rec = Ledger::at(d.0.join("execution.db")).recent(5).unwrap();
    assert_eq!(rec[0].status, ExecStatus::Verified);
    assert_eq!(rec[0].data_scope, "business");
    assert_eq!(rec[0].origin, "model");
    let item = t.context_get(&i.item_id).unwrap().unwrap();
    assert_eq!(
        (item.body.as_str(), item.status.as_str()),
        ("New, clearer", "approved")
    );
    assert!(
        commands::approve_proposal_in(&d.0, &c, "taiyo-skin", &p.proposal_id)
            .unwrap_err()
            .contains("already decided")
    );
}

#[test]
fn a_proposal_fails_cleanly_when_the_item_moved_on_or_the_text_changed() {
    let d = tmp("prop2");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let c = controller(&d.0);
    let i = t
        .context_save(
            &input("objectives", "Objective", "A"),
            "taisun",
            "user",
            None,
        )
        .unwrap();
    let p = t
        .proposal_create(&i.item_id, "Objective", "B", None, "amara")
        .unwrap();
    t.context_save(
        &BusinessContextInput {
            item_id: Some(i.item_id.clone()),
            expected_version: Some(1),
            ..input("objectives", "Objective", "C by hand")
        },
        "taisun",
        "user",
        None,
    )
    .unwrap();
    let done = commands::approve_proposal_in(&d.0, &c, "taiyo-skin", &p.proposal_id).unwrap();
    assert_eq!(done.status, "failed");
    assert!(done.detail.unwrap_or_default().contains("changed after"));
    assert_eq!(
        t.context_get(&i.item_id).unwrap().unwrap().body,
        "C by hand"
    );

    // The text is changed after the dialog showed it: the approval no longer matches.
    let p2 = t
        .proposal_create(&i.item_id, "Objective", "D", None, "amara")
        .unwrap();
    let params = serde_json::json!({"business_id": "taiyo-skin", "proposal_id": p2.proposal_id, "content_sha256": ops::content_sha("Objective", "D")});
    let approval = c
        .grant_user_approval(
            ops::APPLY_ACTION,
            &params,
            std::time::Duration::from_secs(60),
        )
        .unwrap();
    t.conn()
        .execute(
            "UPDATE proposal SET body = 'E' WHERE business_id = ?1 AND proposal_id = ?2",
            params!["taiyo-skin", p2.proposal_id],
        )
        .unwrap();
    let r = c
        .submit(&ExecRequest {
            action_id: ops::APPLY_ACTION.into(),
            workflow_id: "w".into(),
            idempotency_key: "k2".into(),
            origin: Origin::Model,
            params,
            approval_id: Some(approval),
        })
        .unwrap();
    assert_ne!(r.record().status, ExecStatus::Verified);
    assert_eq!(
        t.context_get(&i.item_id).unwrap().unwrap().body,
        "C by hand"
    );
    // Proposing needs permission; a rejected proposal is final.
    assert_eq!(
        t.proposal_decide(&p2.proposal_id, "rejected", None, None)
            .unwrap()
            .status,
        "rejected"
    );
    t.set_permission("model", "propose_changes", false).unwrap();
    assert!(t
        .proposal_create(&i.item_id, "Objective", "F", None, "amara")
        .unwrap_err()
        .contains("not permitted"));
}

// ─── Activity history and separation ─────────────────────────────────────────

#[test]
fn activity_goes_to_the_business_file_tagged_with_its_business_only() {
    let d = tmp("act");
    let (_s, a, b) = two(&d.0);
    a.context_save(&input("identity", "Voice", "x"), "taisun", "user", None)
        .unwrap();
    b.memory_add("B note", "user").unwrap();
    let store = store_for(&d.0, Scope::Business);
    let ha = store.list_business_activity("taiyo-skin", 50).unwrap();
    let hb = store.list_business_activity("second-brand", 50).unwrap();
    assert!(ha.iter().any(|r| r.kind == "business.context_changed"));
    assert!(ha
        .iter()
        .all(|r| r.subject_ref.starts_with("biz:taiyo-skin/")));
    assert!(hb
        .iter()
        .all(|r| r.subject_ref.starts_with("biz:second-brand/")));
    assert!(hb.iter().any(|r| r.kind == "business.memory_changed"));
    assert!(
        !d.0.join("amara.db").exists(),
        "nothing touches the personal store"
    );
    // The personal file refuses a business id at all.
    let personal = store_for(&d.0, Scope::Personal);
    let conn = personal.open().unwrap();
    let e = conn.execute(
        "INSERT INTO activity (activity_id, scope, kind, subject_ref, occurred_at, recorded_at, source, actor, schema_version, business_id)
         VALUES ('x', 'personal', 'goal.created', 'goals:g', 'now', 'now', 'user', 'taisun', 3, 'taiyo-skin')",
        [],
    );
    assert!(e.is_err());
    assert!(personal.list_business_activity("taiyo-skin", 5).is_err());
}

#[test]
fn workspace_code_stays_out_of_personal_memory_and_arm_and_only_net_rs_reaches_the_network() {
    let srcs = [
        (
            "business_workspace.rs",
            include_str!("../business_workspace.rs"),
        ),
        ("connectors.rs", include_str!("connectors.rs")),
        ("sync_local.rs", include_str!("sync_local.rs")),
        ("ops.rs", include_str!("ops.rs")),
        ("commands.rs", include_str!("commands.rs")),
        ("net.rs", include_str!("net.rs")),
        ("supabase.rs", include_str!("supabase.rs")),
        ("website.rs", include_str!("website.rs")),
        ("drive_folder.rs", include_str!("drive_folder.rs")),
        ("intelligence.rs", include_str!("intelligence.rs")),
        ("site_ops.rs", include_str!("site_ops.rs")),
    ];
    for (name, src) in srcs {
        // Code only: comments may name the neighbouring stores to say they are not used.
        let prod: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in [
            "Scope::Personal",
            "amara.db",
            "letta",
            "terrence_context",
            "money",
            "arm::",
            "treasury",
            "TcpStream",
            "Command::new",
            "chat_history",
            "ModelClass::Cloud",
            "CloudIfAuthorized",
        ] {
            assert!(!prod.contains(banned), "{name} mentions {banned}");
        }
        // HTTP only in net.rs; Postgres only in supabase.rs.
        if name != "net.rs" {
            assert!(!prod.contains("reqwest"), "{name} uses reqwest directly");
        }
        if name != "supabase.rs" {
            assert!(
                !prod.contains("tokio_postgres"),
                "{name} opens Postgres directly"
            );
        }
    }
    // The approved hosts are exactly these (decision 0064); adding one is a decision, not a code edit.
    assert_eq!(
        super::net::APPROVED_HTTP_HOSTS,
        ["taiyoskin.com", "www.taiyoskin.com", "api.github.com"]
    );
    // And the personal chat context never reads a workspace.
    for (name, src) in [
        (
            "terrence_runtime.rs",
            include_str!("../terrence_runtime.rs"),
        ),
        ("continuity.rs", include_str!("../continuity.rs")),
        ("amara_turn.rs", include_str!("../amara_turn.rs")),
    ] {
        assert!(
            !src.contains("business_workspace") && !src.contains("workspaces.db"),
            "{name} reads business workspaces"
        );
    }
}

/// Opt-in check against a real site folder, read-only, into a temp data folder:
/// `NEXUS_LIVE_SITE=<folder> cargo test --lib business_workspace::tests::live -- --ignored --nocapture`.
#[test]
#[ignore]
fn live_sync_of_a_real_site_folder() {
    let Ok(site) = std::env::var("NEXUS_LIVE_SITE") else {
        return;
    };
    let d = tmp("live");
    let t = store_in(&d.0).tenant("taiyo-skin").unwrap();
    let kc = FakeKeychain::default();
    let v = t
        .connector_configure(&kc, "local_repo", &BTreeMap::from([("path".into(), site)]))
        .unwrap();
    println!("state: {} ({})", v.state, v.detail);
    let run = sync_local::run(&t, "local_repo", "user").unwrap();
    println!("run: {} · {}", run.status, run.detail.unwrap_or_default());
    for i in t.context_list(None).unwrap() {
        println!(
            "[{}] {} · {} lines · {}",
            i.section,
            i.title,
            i.body.lines().count(),
            i.status
        );
    }
    for doc in t.documents().unwrap() {
        println!("[doc] {} · {} bytes · {}", doc.title, doc.bytes, doc.status);
    }
}

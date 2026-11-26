//! 0065 tests: the website workflow against a fake live site (no network, nothing published).

use super::net::{Request, Response, Transport, TEST_ALLOW_NET};
use super::site_ops::{self, BusinessSiteChangeInput};
use super::*;
use crate::commands::keychain::FakeKeychain;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(tag: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("nexus-biz13-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    Tmp(p)
}
const HTML: &str = "<html><head>\n<title>Taiyo Skin</title>\n<meta name=\"description\" content=\"Old description of the brand.\">\n<meta property=\"og:description\" content=\"Old description of the brand.\">\n<link rel=\"stylesheet\" href=\"/assets/index.css\">\n</head><body><img src=\"/logo.png\"><script src=\"/assets/index.js\"></script></body></html>\n";
const SITEMAP: &str = "<urlset>\n<url><loc>https://taiyoskin.com/</loc></url>\n<url><loc>https://taiyoskin.com/shop</loc></url>\n</urlset>\n";

#[derive(Default)]
struct Site {
    files: RefCell<HashMap<String, Vec<u8>>>,
}
impl Site {
    fn new() -> Self {
        let s = Site::default();
        for (p, b) in [
            ("/", HTML.as_bytes()),
            (
                "/robots.txt",
                b"User-agent: *\nSitemap: https://taiyoskin.com/sitemap.xml\n".as_slice(),
            ),
            ("/sitemap.xml", SITEMAP.as_bytes()),
            (
                "/assets/index.css",
                b"body{background:url(/bg.webp)}".as_slice(),
            ),
            ("/assets/index.js", b"console.log(1)".as_slice()),
            ("/logo.png", b"PNG-OLD".as_slice()),
            ("/bg.webp", b"WEBP".as_slice()),
        ] {
            s.files.borrow_mut().insert(p.into(), b.to_vec());
        }
        s
    }
    fn set(&self, p: &str, b: &[u8]) {
        self.files.borrow_mut().insert(p.into(), b.to_vec());
    }
}
impl Transport for Site {
    fn get(&self, req: &Request, _max: usize) -> Result<Response, String> {
        let path = req
            .url
            .trim_start_matches("https://taiyoskin.com")
            .to_string();
        Ok(match self.files.borrow().get(&path) {
            Some(b) => Response {
                status: 200,
                headers: vec![],
                body: b.clone(),
            },
            None => Response {
                status: 404,
                headers: vec![],
                body: vec![],
            },
        })
    }
}
fn no_sleep(_: Duration) {}
fn setup(d: &Path) -> Tenant {
    TEST_ALLOW_NET.with(|a| a.set(true));
    let t = store_in(d).tenant("taiyo-skin").unwrap();
    t.connector_configure(
        &FakeKeychain::default(),
        "website",
        &BTreeMap::from([("domain".into(), "taiyoskin.com".into())]),
    )
    .unwrap();
    t
}
fn approved(t: &Tenant, section: &str, title: &str, body: &str) -> String {
    let i = t
        .context_save(
            &BusinessContextInput {
                item_id: None,
                section: section.into(),
                title: title.into(),
                body: body.into(),
                status: "draft".into(),
                expected_version: None,
            },
            "taisun",
            "user",
            None,
        )
        .unwrap();
    t.context_set_status(&i.item_id, "approved", 1, "taisun")
        .unwrap()
        .item_id
}
fn input(kind: &str, value: &str, refs: Vec<String>) -> BusinessSiteChangeInput {
    BusinessSiteChangeInput {
        content_type: kind.into(),
        value: value.into(),
        path: None,
        document_id: None,
        source_refs: refs,
    }
}

#[test]
fn a_change_goes_from_draft_to_verified_with_a_backup_and_an_append_only_history() {
    let d = tmp("happy");
    let t = setup(&d.0);
    let site = Site::new();
    let snap = site_ops::snapshot_live(&t, &site, no_sleep).unwrap();
    let paths: Vec<&str> = snap.files.iter().map(|f| f.path.as_str()).collect();
    for p in [
        "/index.html",
        "/robots.txt",
        "/sitemap.xml",
        "/assets/index.css",
        "/assets/index.js",
        "/logo.png",
        "/bg.webp",
    ] {
        assert!(paths.contains(&p), "{p} missing from {paths:?}");
    }
    assert!(snap.limits.contains("hPanel"));
    assert!(site_ops::locate(&t, "old description")
        .unwrap()
        .iter()
        .any(|m| m.reference == "/index.html" && m.status.as_deref() == Some("editable")));

    let pos = approved(
        &t,
        "positioning",
        "Positioning",
        "Calm daily skin care made in small batches.",
    );
    let c = site_ops::draft(
        &t,
        &input(
            "seo_description",
            "Calm daily skin care made in small batches, from Taiyo Skin.",
            vec![pos],
        ),
        "taisun",
    )
    .unwrap();
    assert_eq!(c.status, "draft");
    assert!(c
        .diff
        .iter()
        .any(|l| l.op == "-" && l.text.contains("Old description")));
    assert!(
        c.diff.iter().filter(|l| l.op == "+").count() == 2,
        "description and og:description: {:?}",
        c.diff
    );
    assert!(c.flags.is_empty(), "{:?}", c.flags);
    assert!(!c.checks.iter().any(|x| x.level == "error"));

    let text = site_ops::approval_text(&c).unwrap();
    assert!(
        site_ops::approve(&t, &c.change_id, "something else").is_err(),
        "approval is bound to the text shown"
    );
    site_ops::approve(&t, &c.change_id, &text).unwrap();
    assert!(site_ops::package(&t, &c.change_id, &site, no_sleep)
        .unwrap_err()
        .contains("hPanel"));
    site_ops::confirm_hpanel_backup(&t, &c.change_id).unwrap();
    let p = site_ops::package(&t, &c.change_id, &site, no_sleep).unwrap();
    assert_eq!(p.status, "packaged");
    let dir = PathBuf::from(p.package_dir.unwrap());
    for f in ["upload.tar.gz", "rollback.tar.gz", "README.txt"] {
        assert!(dir.join(f).is_file(), "{f}");
    }
    // Before the upload the live file still differs: verification fails, honestly.
    let v = site_ops::verify(&t, &c.change_id, &site, no_sleep, false).unwrap();
    assert_eq!(v.status, "verify_failed");
    // After the upload (simulated): verified.
    let after = std::fs::read(
        d.0.join("business/workspaces/taiyo-skin/site/changes")
            .join(&c.change_id)
            .join("after"),
    )
    .unwrap();
    site.set("/", &after);
    assert_eq!(
        site_ops::verify(&t, &c.change_id, &site, no_sleep, false)
            .unwrap()
            .status,
        "verified"
    );
    let ev: Vec<String> = t
        .site_events(&c.change_id)
        .unwrap()
        .into_iter()
        .map(|e| e.to_status)
        .collect();
    assert_eq!(
        ev,
        ["draft", "approved", "packaged", "verify_failed", "verified"]
    );
    assert!(t
        .conn()
        .execute("UPDATE site_change_event SET detail = 'x'", [])
        .is_err());
    assert!(t
        .conn()
        .execute("DELETE FROM site_change_event", [])
        .is_err());
    // Rollback: upload the backup, then verify it.
    site.set("/", HTML.as_bytes());
    assert_eq!(
        site_ops::verify(&t, &c.change_id, &site, no_sleep, true)
            .unwrap()
            .status,
        "rolled_back"
    );
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn a_live_file_that_changed_since_the_draft_withdraws_the_change() {
    let d = tmp("stale");
    let t = setup(&d.0);
    let site = Site::new();
    site_ops::snapshot_live(&t, &site, no_sleep).unwrap();
    let c = site_ops::draft(
        &t,
        &input("seo_title", "Taiyo Skin: calm daily care", vec![]),
        "taisun",
    )
    .unwrap();
    site_ops::approve(&t, &c.change_id, &site_ops::approval_text(&c).unwrap()).unwrap();
    site_ops::confirm_hpanel_backup(&t, &c.change_id).unwrap();
    site.set(
        "/",
        b"<html><head><title>Someone else edited this</title></head></html>",
    );
    assert!(site_ops::package(&t, &c.change_id, &site, no_sleep)
        .unwrap_err()
        .contains("changed since"));
    assert_eq!(t.site_change(&c.change_id).unwrap().status, "rejected");
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn sensitive_changes_are_flagged_and_need_approved_sources() {
    let d = tmp("flags");
    let t = setup(&d.0);
    let site = Site::new();
    site_ops::snapshot_live(&t, &site, no_sleep).unwrap();
    // Formulation wording with no approved ingredient item: an error, so it can't be approved.
    let c = site_ops::draft(
        &t,
        &input(
            "seo_description",
            "Now with 5% niacinamide for calmer skin every day, made in Japan.",
            vec![],
        ),
        "taisun",
    )
    .unwrap();
    assert!(c.flags.contains(&"formulation".to_string()));
    assert!(c.checks.iter().any(|x| x.level == "error"));
    assert!(site_ops::approval_text(&c).unwrap_err().contains("errors"));
    // With an approved ingredient item it can go to the dialog, which names the flag.
    let ing = approved(&t, "ingredients", "Ingredients: FORGE", "Niacinamide 5%");
    let c = site_ops::draft(
        &t,
        &input(
            "seo_description",
            "Now with 5% niacinamide for calmer skin every day, made in Japan.",
            vec![ing],
        ),
        "taisun",
    )
    .unwrap();
    assert!(site_ops::approval_text(&c).unwrap().contains("FORMULATION"));
    // Pricing needs an approved product item.
    let c = site_ops::draft(
        &t,
        &input("seo_title", "FORGE cleanser, $48 launch price", vec![]),
        "taisun",
    )
    .unwrap();
    assert!(
        c.flags.contains(&"pricing".to_string()) && c.checks.iter().any(|x| x.level == "error")
    );
    // Removing a sitemap URL is a deletion; a foreign URL is an error.
    let c = site_ops::draft(
        &t,
        &input(
            "sitemap",
            "<urlset>\n<url><loc>https://taiyoskin.com/</loc></url>\n</urlset>\n",
            vec![],
        ),
        "taisun",
    )
    .unwrap();
    assert!(c.flags.contains(&"deletion".to_string()));
    assert!(site_ops::approval_text(&c).unwrap().contains("DELETES"));
    let c = site_ops::draft(
        &t,
        &input(
            "sitemap",
            "<urlset><url><loc>https://evil.example/</loc></url></urlset>",
            vec![],
        ),
        "taisun",
    )
    .unwrap();
    assert!(c.checks.iter().any(|x| x.level == "error"));
    let c = site_ops::draft(
        &t,
        &input("robots", "User-agent: *\nNonsense here\n", vec![]),
        "taisun",
    )
    .unwrap();
    assert!(c.checks.iter().any(|x| x.level == "error"));
    // Deleting a file is flagged.
    let mut del = input("asset_delete", "", vec![]);
    del.path = Some("/bg.webp".into());
    assert!(site_ops::draft(&t, &del, "taisun")
        .unwrap()
        .flags
        .contains(&"deletion".to_string()));
    // A draft source must be approved; bundled content isn't editable; a model needs permission.
    let draft_item = t
        .context_save(
            &BusinessContextInput {
                item_id: None,
                section: "products".into(),
                title: "Draft".into(),
                body: "x".into(),
                status: "draft".into(),
                expected_version: None,
            },
            "taisun",
            "user",
            None,
        )
        .unwrap();
    assert!(site_ops::draft(
        &t,
        &input(
            "seo_title",
            "Taiyo Skin daily care",
            vec![draft_item.item_id]
        ),
        "taisun"
    )
    .unwrap_err()
    .contains("only approved"));
    assert!(
        site_ops::draft(&t, &input("product_description", "x", vec![]), "taisun")
            .unwrap_err()
            .contains("compiled into")
    );
    t.set_permission("model", "propose_changes", false).unwrap();
    assert!(site_ops::draft(
        &t,
        &input("seo_title", "Taiyo Skin daily care", vec![]),
        "amara"
    )
    .unwrap_err()
    .contains("not permitted"));
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn an_image_is_replaced_only_by_an_approved_document_of_the_same_type() {
    let d = tmp("asset");
    let t = setup(&d.0);
    let site = Site::new();
    site_ops::snapshot_live(&t, &site, no_sleep).unwrap();
    let doc = t
        .document_store(
            "New logo", "logo.png", b"PNG-NEW", "draft", "user", None, None,
        )
        .unwrap();
    let mut i = input("asset_replace", "", vec![]);
    i.path = Some("/logo.png".into());
    i.document_id = Some(doc.doc_id.clone());
    assert!(site_ops::draft(&t, &i, "taisun")
        .unwrap_err()
        .contains("only an approved document"));
    t.document_set_status(&doc.doc_id, "approved", 1, "taisun")
        .unwrap();
    let c = site_ops::draft(&t, &i, "taisun").unwrap();
    assert_eq!(c.target, "/logo.png");
    let pdf = t
        .document_store("Spec", "spec.pdf", b"%PDF", "draft", "user", None, None)
        .unwrap();
    t.document_set_status(&pdf.doc_id, "approved", 1, "taisun")
        .unwrap();
    i.document_id = Some(pdf.doc_id);
    assert!(site_ops::draft(&t, &i, "taisun")
        .unwrap_err()
        .contains(".pdf"));
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn site_operations_have_no_write_path_to_the_website() {
    let src = include_str!("site_ops.rs");
    for banned in [
        "sftp",
        "ftp://",
        "POST",
        "PUT",
        "reqwest",
        "Command::new",
        "hostinger_api",
    ] {
        assert!(!src.contains(banned), "site_ops mentions {banned}");
    }
    assert!(site_ops::diff("a\nb\nc", "a\nB\nc")
        .iter()
        .any(|l| l.op == "+" && l.text == "B"));
}

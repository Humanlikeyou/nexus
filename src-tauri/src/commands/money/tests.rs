use super::super::local_store::tests::temp_dir;
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

const PHRASE_12: &str =
    "abandon ability able about above absent absorb abstract absurd abuse access accident";
const PHRASE_JA: &str = "あいこくしん あいさつ あいだ あおぞら あかちゃん あきる あけがた あける あこがれる あさい あさひ あしあと";

fn xpub() -> String {
    format!("xpub6{}", "C".repeat(106))
}

// ── seed guard (ported from src-console/money/money.test.tsx) ──

#[test]
fn guard_refuses_phrases_and_private_keys() {
    let p24 = format!("{PHRASE_12} {PHRASE_12}");
    for s in [
        PHRASE_12.to_string(),
        p24,
        PHRASE_JA.to_string(),
        format!("  {}\n", PHRASE_12.to_uppercase()),
        PHRASE_JA.replace(' ', "\u{3000}"),
    ] {
        assert_eq!(guard_input(&s), Guard::Secret(SecretKind::Phrase));
    }
    for s in [
        format!("xprv9{}", "s".repeat(106)),
        format!("5{}", "H".repeat(50)),
        "a".repeat(64),
        format!("0x{}", "b".repeat(64)),
    ] {
        assert_eq!(guard_input(&s), Guard::Secret(SecretKind::PrivateKey));
    }
}

#[test]
fn guard_accepts_public_values_only() {
    assert_eq!(guard_input(&xpub()), Guard::Public(Chain::Btc, Form::Xpub));
    for a in [
        "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
        "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
        "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
        "BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ",
    ] {
        assert_eq!(
            guard_input(a),
            Guard::Public(Chain::Btc, Form::Address),
            "{a}"
        );
    }
    assert_eq!(
        guard_input(&format!("0x{}", "a".repeat(40))),
        Guard::Public(Chain::Eth, Form::Address)
    );
    assert_eq!(guard_input("hello there"), Guard::Invalid);
    assert_eq!(guard_input("   "), Guard::Empty);
    // Ten words is not a phrase; eleven is.
    assert_eq!(guard_input(&["word"; 10].join(" ")), Guard::Invalid);
    assert_eq!(
        guard_input(&["word"; 11].join(" ")),
        Guard::Secret(SecretKind::Phrase)
    );
    assert_eq!(
        guard_input(&format!("zpub{}", "D".repeat(108))),
        Guard::Public(Chain::Btc, Form::Xpub)
    );
}

// ── accounts ──

fn draft(nick: &str) -> MoneyAccountDraft {
    MoneyAccountDraft {
        nickname: nick.into(),
        institution: "smbc".into(),
        kind: "ordinary".into(),
        route: "csv".into(),
        currency: "JPY".into(),
        scope: "personal".into(),
    }
}

fn ledger_draft(route: &str, cur: &str) -> MoneyAccountDraft {
    MoneyAccountDraft {
        nickname: "Cold".into(),
        institution: "ledger".into(),
        kind: "btc".into(),
        route: route.into(),
        currency: cur.into(),
        scope: "personal".into(),
    }
}

#[test]
fn account_add_validates_and_lists() {
    let d = temp_dir("money-acct");
    let a = account_add_in(&d, &draft("  Main  ")).unwrap();
    assert_eq!(a.nickname, "Main");
    assert!(a.id.starts_with("acct-"));
    assert_eq!(accounts_list_in(&d).unwrap(), vec![a]);
    for bad in [
        MoneyAccountDraft {
            institution: "paypal".into(),
            ..draft("x")
        },
        MoneyAccountDraft {
            kind: "x".into(),
            ..draft("x")
        },
        MoneyAccountDraft {
            route: "api".into(),
            ..draft("x")
        },
        MoneyAccountDraft {
            currency: "EUR".into(),
            ..draft("x")
        },
        MoneyAccountDraft {
            scope: "all".into(),
            ..draft("x")
        },
        MoneyAccountDraft {
            institution: "chime".into(),
            route: "csv".into(),
            currency: "USD".into(),
            ..draft("x")
        },
        MoneyAccountDraft {
            currency: "BTC".into(),
            ..draft("x")
        },
        draft(""),
        draft(&format!(
            "{} {} {}",
            "a".repeat(20),
            "b".repeat(20),
            "c".repeat(20)
        )),
    ] {
        assert_eq!(account_add_in(&d, &bad).unwrap_err(), "invalid_input");
    }
    assert_eq!(accounts_list_in(&d).unwrap().len(), 1);
    assert!(account_add_in(
        &d,
        &draft(&format!(
            "{} {} {}",
            "a".repeat(19),
            "b".repeat(20),
            "c".repeat(19)
        ))
    )
    .is_ok());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn phrase_nickname_is_refused_without_echo() {
    let d = temp_dir("money-nick");
    let err = account_add_in(&d, &draft(PHRASE_12)).unwrap_err();
    assert_eq!(err, "refused_secret");
    assert!(!err.contains("abandon"));
    assert_eq!(
        account_add_in(&d, &draft(&format!("5{}", "H".repeat(50)))).unwrap_err(),
        "refused_secret"
    );
    assert!(accounts_list_in(&d).unwrap().is_empty());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn watch_add_stores_public_only() {
    let d = temp_dir("money-watch");
    let acct = account_add_in(&d, &ledger_draft("watch_only", "BTC")).unwrap();
    let added = watch_add_in(&d, &acct.id, &format!("  {}  ", xpub())).unwrap();
    assert_eq!((added.chain.as_str(), added.form.as_str()), ("btc", "xpub"));
    // Same value twice: stored once.
    watch_add_in(&d, &acct.id, &xpub()).unwrap();
    let w: Vec<WatchEntry> = read_json(&d.join("money/watch.json"), MAX_STORE_BYTES).unwrap();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].value, xpub());
    // Secrets: fixed error, nothing more written.
    let before = fs::read_to_string(d.join("money/watch.json")).unwrap();
    for s in [
        PHRASE_12.to_string(),
        format!("xprv9{}", "s".repeat(106)),
        "c".repeat(64),
    ] {
        assert_eq!(
            watch_add_in(&d, &acct.id, &s).unwrap_err(),
            "refused_secret"
        );
    }
    assert_eq!(
        watch_add_in(&d, &acct.id, "hello").unwrap_err(),
        "invalid_input"
    );
    // An ETH address on a BTC account, and an unknown account, are refused.
    assert_eq!(
        watch_add_in(&d, &acct.id, &format!("0x{}", "a".repeat(40))).unwrap_err(),
        "invalid_input"
    );
    assert_eq!(
        watch_add_in(&d, "acct-nope", &xpub()).unwrap_err(),
        "not_found"
    );
    assert_eq!(
        fs::read_to_string(d.join("money/watch.json")).unwrap(),
        before
    );
    let _ = fs::remove_dir_all(d);
}

#[test]
fn manual_balance_only_for_ledger_or_manual_accounts() {
    let d = temp_dir("money-bal");
    let ledger = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
    let bank = account_add_in(&d, &draft("Main")).unwrap();
    balance_set_manual_in(&d, &ledger.id, 0.5, "2026-11-12").unwrap();
    balance_set_manual_in(&d, &ledger.id, 0.75, "2026-11-13").unwrap();
    let all: Vec<ManualBalance> =
        read_json(&d.join("money/balances.json"), MAX_STORE_BYTES).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].native_amount, all[0].source.as_str()),
        (0.75, "manual")
    );
    for (id, amt, day) in [
        (bank.id.as_str(), 1.0, "2026-11-12"),
        (ledger.id.as_str(), f64::NAN, "2026-11-12"),
        (ledger.id.as_str(), f64::INFINITY, "2026-11-12"),
        (ledger.id.as_str(), 1.0, "2026-13-40"),
        (ledger.id.as_str(), 1.0, "yesterday"),
    ] {
        assert_eq!(
            balance_set_manual_in(&d, id, amt, day).unwrap_err(),
            "invalid_input"
        );
    }
    assert_eq!(
        balance_set_manual_in(&d, "acct-nope", 1.0, "2026-11-12").unwrap_err(),
        "not_found"
    );
    let _ = fs::remove_dir_all(d);
}

// ── statement import ──
//
// ASSUMED FORMATS. The real SMBC and Japan Post Bank (Yucho) statement CSVs are UNVERIFIED (spike §2:
// no primary spec was found). Every fixture below is a synthetic file built in the test from the
// column names in the task, hence the `assumed_` prefix. REPLACE these with Taisun's redacted
// sample files for each bank before trusting the importer on real statements.

const SPLIT_HEADER: &str = "年月日,お引出し,お預入れ,お取り扱い内容,残高\n";

fn csv_account(d: &Path) -> MoneyAccount {
    account_add_in(d, &draft("SMBC main")).unwrap()
}

fn write_file(d: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = d.join(name);
    fs::write(&p, bytes).unwrap();
    p
}

fn tx_file(d: &Path, id: &str) -> PathBuf {
    d.join("money/tx").join(format!("{id}.jsonl"))
}

fn txs_of(d: &Path, id: &str) -> Vec<TxRecord> {
    parse_tx_records(&read_tx_text(&tx_file(d, id)).unwrap()).unwrap()
}

#[test]
fn assumed_utf8_bom_split_columns() {
    let d = temp_dir("money-bom");
    let a = csv_account(&d);
    let body = format!(
            "\u{feff}口座情報,0000\n{SPLIT_HEADER}2026/11/02,\"3,000\",,ｺﾝﾋﾞﾆ,\"97,000\"\n2026/11/03,,\"50,000\",給与,\"147,000\"\n,,,,\n合計,,,,\n"
        );
    let p = write_file(&d, "s.csv", body.as_bytes());
    let r = import_statement_in(&d, &a.id, &p).unwrap();
    assert_eq!(
        r,
        ImportSummary {
            added: 2,
            duplicates: 0,
            skipped_rows: 1
        }
    );
    let t = txs_of(&d, &a.id);
    assert_eq!(
        (t[0].date.as_str(), t[0].amount, t[0].balance),
        ("2026-11-02", -3000, Some(97000))
    );
    assert_eq!((t[1].amount, t[1].description.as_str()), (50000, "給与"));
    let _ = fs::remove_dir_all(d);
}

#[test]
fn assumed_cp932_bytes() {
    let d = temp_dir("money-sjis");
    let a = csv_account(&d);
    let text = format!("{SPLIT_HEADER}2026/11/02,\"1,200\",,スーパー,\"8,800\"\n");
    let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(&text);
    assert!(!bad);
    assert!(std::str::from_utf8(&bytes).is_err());
    let p = write_file(&d, "s.csv", &bytes);
    let r = import_statement_in(&d, &a.id, &p).unwrap();
    assert_eq!(r.added, 1);
    let t = txs_of(&d, &a.id);
    assert_eq!(
        (t[0].amount, t[0].description.as_str()),
        (-1200, "スーパー")
    );
    let _ = fs::remove_dir_all(d);
}

#[test]
fn assumed_era_dates_and_fullwidth_digits() {
    assert_eq!(parse_date("令和6年1月5日").as_deref(), Some("2024-01-05"));
    assert_eq!(parse_date("令和元年5月1日").as_deref(), Some("2019-05-01"));
    assert_eq!(parse_date("R6.01.05").as_deref(), Some("2024-01-05"));
    assert_eq!(
        parse_date("平成30年12月31日").as_deref(),
        Some("2018-12-31")
    );
    assert_eq!(parse_date("2026/11/2").as_deref(), Some("2026-11-02"));
    assert_eq!(parse_date("2026-11-02").as_deref(), Some("2026-11-02"));
    assert_eq!(parse_date("20261102").as_deref(), Some("2026-11-02"));
    assert_eq!(
        parse_date("２０２６／１１／０２").as_deref(),
        Some("2026-11-02")
    );
    assert_eq!(
        parse_date("令和８年１１月２日").as_deref(),
        Some("2026-11-02")
    );
    assert_eq!(parse_date("2026/13/02"), None);
    assert_eq!(parse_date("hello"), None);
    assert_eq!(parse_date(""), None);
    assert_eq!(parse_yen("１２，３４５円"), Ok(Some(12345)));
    assert_eq!(parse_yen("¥1,000"), Ok(Some(1000)));
    assert_eq!(parse_yen("△500"), Ok(Some(-500)));
    assert_eq!(parse_yen("-７"), Ok(Some(-7)));
    assert_eq!(parse_yen(""), Ok(None));
    assert_eq!(parse_yen("12.5"), Err(()));
    assert_eq!(parse_yen("abc"), Err(()));

    let d = temp_dir("money-era");
    let a = csv_account(&d);
    let body = format!(
            "{SPLIT_HEADER}令和8年11月2日,\"１，２００\",,店,\"９，８００円\"\nR8.11.03,,5000,入金,14800\n"
        );
    let p = write_file(&d, "s.csv", body.as_bytes());
    assert_eq!(import_statement_in(&d, &a.id, &p).unwrap().added, 2);
    let t = txs_of(&d, &a.id);
    assert_eq!(
        (t[0].date.as_str(), t[0].amount, t[0].balance),
        ("2026-11-02", -1200, Some(9800))
    );
    assert_eq!((t[1].date.as_str(), t[1].amount), ("2026-11-03", 5000));
    let _ = fs::remove_dir_all(d);
}

#[test]
fn assumed_signed_amount_with_and_without_kubun() {
    let d = temp_dir("money-signed");
    let a = csv_account(&d);
    let signed =
        "日付,取引金額,差引残高,摘要\n2026/11/02,-1500,98500,A\n2026/11/03,2500,101000,B\n";
    let p = write_file(&d, "a.csv", signed.as_bytes());
    assert_eq!(import_statement_in(&d, &a.id, &p).unwrap().added, 2);
    let t = txs_of(&d, &a.id);
    assert_eq!((t[0].amount, t[1].amount), (-1500, 2500));

    let b = account_add_in(&d, &draft("Yucho")).unwrap();
    let kubun = "お取引日,金額,受払区分,残高,内容\n2026/11/02,\"1,500\",支払,98500,A\n2026/11/03,\"2,500\",入金,101000,B\n2026/11/04,300,不明,101000,C\n";
    let p = write_file(&d, "b.csv", kubun.as_bytes());
    let r = import_statement_in(&d, &b.id, &p).unwrap();
    assert_eq!((r.added, r.skipped_rows), (2, 1));
    let t = txs_of(&d, &b.id);
    assert_eq!((t[0].amount, t[1].amount), (-1500, 2500));
    let _ = fs::remove_dir_all(d);
}

#[test]
fn assumed_unknown_header_is_refused_and_writes_nothing() {
    let d = temp_dir("money-unk");
    let a = csv_account(&d);
    for body in [
        "col1,col2,col3\n2026/11/02,100,200\n",
        "年月日,メモ\n2026/11/02,x\n",
        "年月日,お引出し,お引出し,お預入れ\n2026/11/02,1,2,3\n",
        "",
    ] {
        let p = write_file(&d, "u.csv", body.as_bytes());
        assert_eq!(
            import_statement_in(&d, &a.id, &p).unwrap_err(),
            "unrecognised_format"
        );
    }
    // Bytes that are neither UTF-8 nor Shift-JIS.
    let p = write_file(&d, "u.csv", &[0x81, 0x20, 0xff, 0xfe, 0xfd]);
    assert_eq!(
        import_statement_in(&d, &a.id, &p).unwrap_err(),
        "unrecognised_format"
    );
    assert!(!d.join("money/tx").exists());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn assumed_reimport_dedupes_and_same_day_twins_are_kept() {
    let d = temp_dir("money-dedupe");
    let a = csv_account(&d);
    let body = format!(
            "{SPLIT_HEADER}2026/11/02,500,,コーヒー,9500\n2026/11/02,500,,コーヒー,9500\n2026/11/03,,1000,入金,10500\n"
        );
    let p = write_file(&d, "s.csv", body.as_bytes());
    assert_eq!(
        import_statement_in(&d, &a.id, &p).unwrap(),
        ImportSummary {
            added: 3,
            duplicates: 0,
            skipped_rows: 0
        }
    );
    let before = fs::read_to_string(tx_file(&d, &a.id)).unwrap();
    assert_eq!(
        import_statement_in(&d, &a.id, &p).unwrap(),
        ImportSummary {
            added: 0,
            duplicates: 3,
            skipped_rows: 0
        }
    );
    assert_eq!(fs::read_to_string(tx_file(&d, &a.id)).unwrap(), before);
    // An overlapping later file adds only the new row.
    let more = format!("{SPLIT_HEADER}2026/11/03,,1000,入金,10500\n2026/11/04,200,,バス,10300\n");
    let p2 = write_file(&d, "s2.csv", more.as_bytes());
    let r = import_statement_in(&d, &a.id, &p2).unwrap();
    assert_eq!((r.added, r.duplicates), (1, 1));
    let after = fs::read_to_string(tx_file(&d, &a.id)).unwrap();
    assert!(after.starts_with(&before));
    assert_eq!(txs_of(&d, &a.id).len(), 4);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn import_refuses_symlink_directory_oversize_relative_and_wrong_route() {
    let d = temp_dir("money-path");
    let a = csv_account(&d);
    let real = write_file(
        &d,
        "real.csv",
        format!("{SPLIT_HEADER}2026/11/02,1,,x,2\n").as_bytes(),
    );
    #[cfg(unix)]
    {
        let link = d.join("link.csv");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            import_statement_in(&d, &a.id, &link).unwrap_err(),
            "bad_file"
        );
    }
    assert_eq!(import_statement_in(&d, &a.id, &d).unwrap_err(), "bad_file");
    assert_eq!(
        import_statement_in(&d, &a.id, &d.join("missing.csv")).unwrap_err(),
        "bad_file"
    );
    assert_eq!(
        import_statement_in(&d, &a.id, Path::new("real.csv")).unwrap_err(),
        "invalid_input"
    );
    assert_eq!(
        import_statement_in(&d, &a.id, &d.join("..").join("real.csv")).unwrap_err(),
        "invalid_input"
    );
    let mut big = SPLIT_HEADER.as_bytes().to_vec();
    big.resize(MAX_IMPORT_BYTES as usize + 1, b'x');
    let p = write_file(&d, "big.csv", &big);
    assert_eq!(import_statement_in(&d, &a.id, &p).unwrap_err(), "too_large");
    let ledger = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
    assert_eq!(
        import_statement_in(&d, &ledger.id, &real).unwrap_err(),
        "invalid_input"
    );
    assert_eq!(
        import_statement_in(&d, "acct-nope", &real).unwrap_err(),
        "not_found"
    );
    assert!(!d.join("money/tx").exists());
    let _ = fs::remove_dir_all(d);
}

// ── seed sentinel: never on disk, never in an error ──

fn all_file_bytes(dir: &Path, out: &mut Vec<u8>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            all_file_bytes(&p, out);
        } else {
            out.extend(fs::read(&p).unwrap());
            out.push(b'\n');
        }
    }
}

#[test]
fn seed_sentinel_never_reaches_disk_or_errors() {
    let words = [
        "zorblax", "quindle", "marvomp", "tessiqua", "flunder", "gorpwick", "yamblet", "crundle",
        "wopsel", "drivane", "plimzot", "hexabar",
    ];
    let phrase = words.join(" ");
    let data = temp_dir("money-sentinel");
    let acct = account_add_in(&data, &ledger_draft("watch_only", "BTC")).unwrap();
    let bank = csv_account(&data);
    let mut errors: Vec<String> = vec![
        account_add_in(&data, &draft(&phrase)).unwrap_err(),
        account_add_in(
            &data,
            &MoneyAccountDraft {
                institution: phrase.clone(),
                ..draft("x")
            },
        )
        .unwrap_err(),
        watch_add_in(&data, &acct.id, &phrase).unwrap_err(),
        watch_add_in(&data, &phrase, &xpub()).unwrap_err(),
        balance_set_manual_in(&data, &acct.id, 1.0, &phrase).unwrap_err(),
        fx_set_manual_in(&data, 150.0, &phrase).unwrap_err(),
    ];
    // A statement whose description holds the phrase: the field is replaced, the row is kept.
    let body = format!(
        "{SPLIT_HEADER}2026/11/02,100,,\"{phrase}\",900\n2026/11/03,200,,{},700\n",
        phrase.to_uppercase()
    );
    let src_dir = temp_dir("money-sentinel-src");
    let p = write_file(&src_dir, "src.csv", body.as_bytes());
    let r = import_statement_in(&data, &bank.id, &p).unwrap();
    assert_eq!(r.added, 2);
    assert!(txs_of(&data, &bank.id)
        .iter()
        .all(|x| x.description == REMOVED));
    errors.push(import_statement_in(&data, &bank.id, &data.join(&phrase)).unwrap_err());
    for e in &errors {
        for w in words {
            assert!(!e.to_lowercase().contains(w), "error echoed input: {e}");
        }
    }
    // Every file written under the data dir (the fixture lives outside it).
    let mut bytes = Vec::new();
    all_file_bytes(&data, &mut bytes);
    let blob = String::from_utf8_lossy(&bytes).to_lowercase();
    for w in words {
        assert!(!blob.contains(w), "sentinel word reached disk");
    }
    let _ = fs::remove_dir_all(data);
    let _ = fs::remove_dir_all(src_dir);
}

// ── modes ──

#[cfg(unix)]
#[test]
fn money_files_are_0600_and_dirs_0700() {
    use std::os::unix::fs::PermissionsExt;
    let d = temp_dir("money-modes");
    let bank = csv_account(&d);
    let ledger = account_add_in(&d, &ledger_draft("watch_only", "BTC")).unwrap();
    watch_add_in(&d, &ledger.id, &xpub()).unwrap();
    balance_set_manual_in(&d, &ledger.id, 1.0, "2026-11-12").unwrap();
    fx_set_manual_in(&d, 150.5, "2026-11-12").unwrap();
    let p = write_file(
        &d,
        "s.csv",
        format!("{SPLIT_HEADER}2026/11/02,1,,x,2\n").as_bytes(),
    );
    import_statement_in(&d, &bank.id, &p).unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&d.join("money")), 0o700);
    assert_eq!(mode(&d.join("money/tx")), 0o700);
    for f in ["accounts.json", "watch.json", "balances.json", "fx.json"] {
        assert_eq!(mode(&d.join("money").join(f)), 0o600, "{f}");
    }
    assert_eq!(mode(&tx_file(&d, &bank.id)), 0o600);
    let _ = fs::remove_dir_all(d);
}

// ── FX ──

const ECB_FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope><gesmes:subject>Reference rates</gesmes:subject>
<Cube>
  <Cube time='2026-11-12'>
    <Cube currency='USD' rate='1.0000'/>
    <Cube currency='JPY' rate='155.50'/>
    <Cube currency='BGN' rate='1.9558'/>
  </Cube>
</Cube></gesmes:Envelope>"#;

fn cube(body: &str) -> String {
    format!("<Cube><Cube time='2026-11-12'>{body}</Cube></Cube>")
}

#[test]
fn ecb_parse_cross_rate_and_errors() {
    let (date, rate) = parse_ecb_daily(ECB_FIXTURE).unwrap();
    assert_eq!((date.as_str(), rate), ("2026-11-12", 155.5));
    let (_, r2) = parse_ecb_daily(
            r#"<Cube><Cube time="2026-11-12"><Cube currency="USD" rate="1.1620"/><Cube currency="JPY" rate="177.12"/></Cube></Cube>"#,
        )
        .unwrap();
    assert_eq!(r2, ((177.12f64 / 1.1620) * 10_000.0).round() / 10_000.0);
    let usd = |r: &str| format!("<Cube currency='USD' rate='{r}'/>");
    let jpy = |r: &str| format!("<Cube currency='JPY' rate='{r}'/>");
    for bad in [
        String::new(),
        cube(&jpy("155.5")),
        cube(&usd("1.0")),
        cube(&format!("{}{}", usd("0"), jpy("155"))),
        cube(&format!("{}{}", usd("x"), jpy("155"))),
        cube(&format!("{}{}", usd("0.0001"), jpy("155"))),
        format!("<Cube>{}{}</Cube>", usd("1.0"), jpy("155")),
        cube(&format!("{}{}", usd("1.0"), jpy("155"))).replace("2026-11-12", "soon"),
    ] {
        assert_eq!(
            parse_ecb_daily(&bad).unwrap_err(),
            "ecb_parse_failed",
            "{bad}"
        );
    }
}

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

#[test]
fn refresh_is_blocked_by_kill_switch_and_navy_before_any_fetch() {
    let d = temp_dir("money-fxblock");
    for (off, navy) in [(true, false), (false, true), (true, true)] {
        let called = AtomicBool::new(false);
        let r = block_on(fx_refresh_in(&d, off, navy, now_jst(), || async {
            called.store(true, Ordering::SeqCst);
            Ok(ECB_FIXTURE.to_string())
        }));
        assert_eq!(r.unwrap_err(), "outbound_disabled");
        assert!(!called.load(Ordering::SeqCst), "fetch ran while blocked");
    }
    assert_eq!(fx_get_in(&d).unwrap(), None);
    assert!(!d.join("money").exists());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn refresh_stores_ecb_rate_and_failures_store_nothing() {
    let d = temp_dir("money-fxok");
    assert_eq!(fx_get_in(&d).unwrap(), None);
    let t0 = now_jst();
    let later = t0 + chrono::Duration::minutes(11);
    let r = block_on(fx_refresh_in(&d, false, false, t0, || async {
        Ok(ECB_FIXTURE.to_string())
    }))
    .unwrap();
    assert_eq!(
        (r.rate, r.date.as_str(), r.source.as_str()),
        (155.5, "2026-11-12", "ecb")
    );
    assert_eq!(fx_get_in(&d).unwrap(), Some(r.clone()));
    // A failed fetch or a bad body leaves the stored rate as it was.
    let e = block_on(fx_refresh_in(&d, false, false, later, || async {
        Err("fetch_failed".to_string())
    }))
    .unwrap_err();
    assert_eq!(e, "fetch_failed");
    let e = block_on(fx_refresh_in(&d, false, false, later, || async {
        Ok("<Cube/>".to_string())
    }))
    .unwrap_err();
    assert_eq!(e, "ecb_parse_failed");
    assert_eq!(fx_get_in(&d).unwrap(), Some(r));
    let _ = fs::remove_dir_all(d);
}

#[test]
fn l4_refresh_waits_ten_minutes_after_a_successful_fetch() {
    let d = temp_dir("money-fxgap");
    let t0 = now_jst();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let run = |now| {
        block_on(fx_refresh_in(&d, false, false, now, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(ECB_FIXTURE.to_string())
        }))
    };
    let first = run(t0).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // Inside the window: the stored rate comes back and nothing is fetched.
    let again = run(t0 + chrono::Duration::minutes(9)).unwrap();
    assert_eq!(again, first);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // After it: a fetch.
    run(t0 + chrono::Duration::minutes(10)).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    // A manual rate never blocks a refresh, and the kill switch still wins inside the window.
    fx_set_manual_in(&d, 150.0, "2026-11-12").unwrap();
    run(t0 + chrono::Duration::minutes(11)).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let blocked = block_on(fx_refresh_in(&d, true, false, t0, || async {
        Ok(String::new())
    }));
    assert_eq!(blocked.unwrap_err(), "outbound_disabled");
    let _ = fs::remove_dir_all(d);
}

#[test]
fn l4_rows_past_the_cap_are_counted_and_wide_rows_skipped() {
    let mut text = String::from(SPLIT_HEADER);
    for _ in 0..(MAX_ROWS + 6) {
        text.push_str("2026/11/02,1,,x,2\n");
    }
    let (rows, overflow) = parse_csv(&text);
    assert_eq!((rows.len(), overflow), (MAX_ROWS, 7));
    let (txs, skipped) = parse_statement(text.as_bytes()).unwrap();
    assert_eq!((txs.len(), skipped), (MAX_ROWS - 1, 7));
    // More than 64 cells: the row is skipped, and a wide row is never a header.
    let wide = vec!["1"; MAX_CELLS + 1].join(",");
    let body = format!("{wide}\n{SPLIT_HEADER}2026/11/02,1,,x,2\n2026/11/03,1,,x,2,{wide}\n");
    let (txs, skipped) = parse_statement(body.as_bytes()).unwrap();
    assert_eq!((txs.len(), skipped), (1, 1));
    let ok64 = vec!["1"; MAX_CELLS].join(",");
    let (rows, _) = parse_csv(&format!("{ok64}\n"));
    assert_eq!(rows[0].len(), MAX_CELLS);
}

#[cfg(unix)]
#[test]
fn l2_a_fifo_is_refused_without_hanging() {
    let d = temp_dir("money-fifo");
    let fifo = d.join("pipe.csv");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    // The open itself must not block and must be refused on the handle.
    assert_eq!(open_regular_nofollow(&fifo).unwrap_err(), "bad_file");
    let a = csv_account(&d);
    assert_eq!(
        import_statement_in(&d, &a.id, &fifo).unwrap_err(),
        "bad_file"
    );
    // A regular file still opens.
    let ok = write_file(&d, "ok.csv", b"x");
    assert!(open_regular_nofollow(&ok).is_ok());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn manual_rate_bounds() {
    let d = temp_dir("money-fxman");
    let r = fx_set_manual_in(&d, 151.25, "2026-11-12").unwrap();
    assert_eq!((r.source.as_str(), r.rate), ("manual", 151.25));
    assert_eq!(fx_get_in(&d).unwrap(), Some(r));
    for rate in [0.0, -1.0, 1000.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            fx_set_manual_in(&d, rate, "2026-11-12").unwrap_err(),
            "invalid_input"
        );
    }
    assert_eq!(
        fx_set_manual_in(&d, 150.0, "11/12").unwrap_err(),
        "invalid_input"
    );
    assert_eq!(fx_get_in(&d).unwrap().unwrap().rate, 151.25);
    let _ = fs::remove_dir_all(d);
}

// ── security review fixes (M1, M2, L1, L4) ──

const SHORT: [&str; 12] = [
    "zoo", "act", "add", "age", "air", "all", "and", "arm", "art", "ask", "bag", "bar",
];

fn bypass_phrases() -> Vec<(&'static str, String)> {
    let w = SHORT;
    let accented = [
        "ábaco", "ñandú", "épico", "óleo", "útil", "íntimo", "ñoño", "año", "café", "señal",
        "avión", "árbol",
    ];
    let zh = [
        "的", "一", "是", "在", "不", "了", "有", "和", "人", "这", "中", "大",
    ];
    let ko = [
        "가격",
        "가끔",
        "가난",
        "가능",
        "가득",
        "가르침",
        "가방",
        "가위",
        "가을",
        "가이드",
        "가입",
        "가족",
    ];
    let fullwidth: Vec<String> = w
        .iter()
        .map(|x| {
            x.chars()
                .map(|c| char::from_u32(c as u32 + 0xfee0).unwrap())
                .collect()
        })
        .collect();
    let mut stray: Vec<String> = w.iter().map(|x| x.to_string()).collect();
    stray.insert(6, "x9#7".to_string());
    vec![
        ("commas", w.join(", ")),
        ("hyphens", w.join("-")),
        ("dots", w.join(".")),
        ("underscores", w.join("_")),
        ("zwsp", w.join("\u{200b}")),
        ("word joiner", w.join("\u{2060}")),
        ("bom", w.join("\u{feff}")),
        ("zwsp plus space", w.join(" \u{200b}")),
        ("number prefix", format!("1 {}", w.join(" "))),
        ("stray token", stray.join(" ")),
        ("accented", accented.join(" ")),
        ("chinese", zh.join(" ")),
        ("korean", ko.join(" ")),
        ("fullwidth latin", fullwidth.join("\u{3000}")),
    ]
}

#[test]
fn m1_phrase_bypasses_are_refused_as_nicknames_and_by_the_guard() {
    let d = temp_dir("money-m1");
    for (name, phrase) in bypass_phrases() {
        assert_eq!(
            guard_input(&phrase),
            Guard::Secret(SecretKind::Phrase),
            "guard: {name}"
        );
        let err = account_add_in(&d, &draft(&phrase)).unwrap_err();
        assert_eq!(err, "refused_secret", "nickname: {name}");
    }
    // Eight or more words is refused as a nickname even when it is not a phrase.
    assert_eq!(
        account_add_in(&d, &draft("zoo act add age air all and arm")).unwrap_err(),
        "refused_secret"
    );
    assert!(account_add_in(&d, &draft("Ledger cold storage")).is_ok());
    assert!(account_add_in(&d, &draft("Yucho_main-2 o'neil.1")).is_ok());
    for odd in ["a,b", "Main\u{200b}", "a/b", "名前#1", "x@y"] {
        assert_eq!(
            account_add_in(&d, &draft(odd)).unwrap_err(),
            "invalid_input",
            "{odd:?}"
        );
    }
    assert_eq!(accounts_list_in(&d).unwrap().len(), 2);
    let on_disk = fs::read_to_string(d.join("money/accounts.json")).unwrap();
    assert!(!on_disk.contains("zoo"));
    // The same phrases in a statement description are removed.
    let bank = csv_account(&d);
    let mut body = String::from(SPLIT_HEADER);
    for (i, (_, phrase)) in bypass_phrases().iter().enumerate() {
        body.push_str(&format!("2026/11/{:02},1,,\"{phrase}\",9\n", i + 1));
    }
    let p = write_file(&d, "b.csv", body.as_bytes());
    let r = import_statement_in(&d, &bank.id, &p).unwrap();
    assert_eq!(r.added, bypass_phrases().len());
    assert!(txs_of(&d, &bank.id)
        .iter()
        .all(|t| t.description == REMOVED));
    let _ = fs::remove_dir_all(d);
}

#[test]
fn l1_private_key_tokens_inside_text_are_refused() {
    let wif = format!("5{}", "H".repeat(50));
    let hex = "ab".repeat(32);
    let b58 = "K".repeat(60);
    let xprv = format!("xprv9{}", "s".repeat(106));
    let d = temp_dir("money-l1");
    for text in [
        format!("my {wif}"),
        format!("key:{wif}"),
        format!("tx {hex}"),
        format!("a 0x{hex}"),
        format!("x {b58}"),
        format!("see {xprv} ok"),
    ] {
        assert_eq!(
            account_add_in(&d, &draft(&text)).unwrap_err(),
            "refused_secret",
            "{text}"
        );
        assert_eq!(clean_description(&text), REMOVED, "{text}");
    }
    assert_eq!(clean_description("Konbini 500 yen"), "Konbini 500 yen");
    let _ = fs::remove_dir_all(d);
}

#[test]
fn l4_a_minus_sign_only_leads() {
    assert_eq!(parse_yen("1-2"), Err(()));
    assert_eq!(parse_yen("12-"), Err(()));
    assert_eq!(parse_yen("--5"), Err(()));
    assert_eq!(parse_yen("5△"), Err(()));
    assert_eq!(parse_yen("-12"), Ok(Some(-12)));
    assert_eq!(parse_yen("△12"), Ok(Some(-12)));
    assert_eq!(parse_yen("▲1,200円"), Ok(Some(-1200)));
    assert_eq!(parse_yen(" -３ "), Ok(Some(-3)));
}

#[test]
fn m2_and_l4_source_assertions() {
    let src = production_source();
    assert_eq!(src.matches(".no_proxy()").count(), 1);
    assert_eq!(src.matches(".https_only(true)").count(), 1);
    let client = src.split("async fn fetch_ecb").nth(1).unwrap();
    let client = client.split("// ──").next().unwrap();
    assert!(client.contains(".no_proxy()") && client.contains(".https_only(true)"));
}

// ── balances (read-only) ──

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn balances_empty_store_is_empty_with_null_fx() {
    let d = temp_dir("money-bal-empty");
    let r = balances_list_in(&d, day("2026-11-12")).unwrap();
    assert!(r.balances.is_empty());
    assert!(r.fx.is_none());
    // A read creates nothing.
    assert!(!d.join("money").exists());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn balances_manual_has_source_and_as_of_and_fx_when_stored() {
    let d = temp_dir("money-bal-manual");
    let ledger = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
    let empty = account_add_in(&d, &ledger_draft("manual", "ETH")).unwrap();
    balance_set_manual_in(&d, &ledger.id, 0.75, "2026-11-10").unwrap();
    let r = balances_list_in(&d, day("2026-11-12")).unwrap();
    assert!(r.fx.is_none());
    // An account with no stored balance is absent, never 0.
    assert_eq!(r.balances.len(), 1);
    assert!(r.balances.iter().all(|b| b.account_id != empty.id));
    let b = &r.balances[0];
    assert_eq!(
        (
            b.account_id.as_str(),
            b.currency.as_str(),
            b.amount,
            b.as_of.as_str(),
            b.source.as_str()
        ),
        (ledger.id.as_str(), "BTC", 0.75, "2026-11-10", "manual")
    );
    assert_eq!(b.history.len(), 1);
    fx_set_manual_in(&d, 150.5, "2026-11-11").unwrap();
    let fx = balances_list_in(&d, day("2026-11-12")).unwrap().fx.unwrap();
    assert_eq!(
        (fx.rate, fx.date.as_str(), fx.source.as_str()),
        (150.5, "2026-11-11", "manual")
    );
    // Stale manual balance: still reported, but no history point in the window.
    let r = balances_list_in(&d, day("2026-12-31")).unwrap();
    assert_eq!(r.balances[0].amount, 0.75);
    assert!(r.balances[0].history.is_empty());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn balances_statement_current_and_history_window() {
    let d = temp_dir("money-bal-stmt");
    let a = csv_account(&d);
    let text = format!(
            "{SPLIT_HEADER}2026/09/01,\"1,000\",,old,\"50,000\"\n2026/10/20,\"1,000\",,a,\"49,000\"\n2026/11/02,\"3,000\",,b,\"46,000\"\n2026/11/02,\"1,000\",,c,\"45,000\"\n2026/11/10,,\"5,000\",d,\"50,000\"\n"
        );
    let p = write_file(&d, "s.csv", text.as_bytes());
    import_statement_in(&d, &a.id, &p).unwrap();
    let r = balances_list_in(&d, day("2026-11-12")).unwrap();
    assert_eq!(r.balances.len(), 1);
    let b = &r.balances[0];
    assert_eq!(
        (
            b.amount,
            b.as_of.as_str(),
            b.source.as_str(),
            b.currency.as_str()
        ),
        (50_000.0, "2026-11-10", "statement", "JPY")
    );
    // Window is 2026-10-13..: the September row is out, same-day rows collapse to the last one.
    let pts: Vec<(&str, f64)> = b
        .history
        .iter()
        .map(|p| (p.date.as_str(), p.amount))
        .collect();
    assert_eq!(
        pts,
        vec![
            ("2026-10-20", 49_000.0),
            ("2026-11-02", 45_000.0),
            ("2026-11-10", 50_000.0)
        ]
    );
    let _ = fs::remove_dir_all(d);
}

#[test]
fn balances_statement_without_running_balance_is_absent_not_zero() {
    let d = temp_dir("money-bal-nobal");
    let a = csv_account(&d);
    let p = write_file(
        &d,
        "s.csv",
        "年月日,お引出し,お預入れ,お取り扱い内容\n2026/11/02,\"1,200\",,x\n".as_bytes(),
    );
    assert_eq!(import_statement_in(&d, &a.id, &p).unwrap().added, 1);
    assert!(balances_list_in(&d, day("2026-11-12"))
        .unwrap()
        .balances
        .is_empty());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn balances_bad_fx_is_null_and_caps_are_fixed_errors() {
    let d = temp_dir("money-bal-caps");
    let a = csv_account(&d);
    let mdir = d.join("money");
    fs::create_dir_all(&mdir).unwrap();
    fs::write(
        mdir.join("fx.json"),
        r#"{"rate":0,"date":"2026-11-11","source":"ecb","fetched_at":"x"}"#,
    )
    .unwrap();
    assert!(balances_list_in(&d, day("2026-11-12"))
        .unwrap()
        .fx
        .is_none());
    // Oversize balances.json and oversize tx file are refused with a fixed code.
    fs::write(
        mdir.join("balances.json"),
        vec![b' '; MAX_STORE_BYTES as usize + 1],
    )
    .unwrap();
    assert_eq!(
        balances_list_in(&d, day("2026-11-12")).unwrap_err(),
        "store_error"
    );
    fs::write(mdir.join("balances.json"), "[]").unwrap();
    fs::create_dir_all(mdir.join("tx")).unwrap();
    let f = fs::File::create(tx_file(&d, &a.id)).unwrap();
    f.set_len(MAX_TX_FILE_BYTES + 1).unwrap();
    assert_eq!(
        balances_list_in(&d, day("2026-11-12")).unwrap_err(),
        "store_error"
    );
    // Corrupt JSON: fixed code, nothing echoed.
    fs::write(tx_file(&d, &a.id), "SECRET-ish {not json\n").unwrap();
    let e = balances_list_in(&d, day("2026-11-12")).unwrap_err();
    assert_eq!(e, "store_error");
    let _ = fs::remove_dir_all(d);
}

#[cfg(unix)]
#[test]
fn balances_read_changes_no_file_or_permission() {
    use std::os::unix::fs::PermissionsExt;
    let d = temp_dir("money-bal-perms");
    let a = csv_account(&d);
    let l = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
    balance_set_manual_in(&d, &l.id, 1.0, "2026-11-10").unwrap();
    fx_set_manual_in(&d, 150.0, "2026-11-11").unwrap();
    let p = write_file(
        &d,
        "s.csv",
        format!("{SPLIT_HEADER}2026/11/02,\"1,000\",,x,\"9,000\"\n").as_bytes(),
    );
    import_statement_in(&d, &a.id, &p).unwrap();
    let snap = || {
        let mut v: Vec<(PathBuf, u32, u64)> = Vec::new();
        let mut stack = vec![d.join("money")];
        while let Some(dir) = stack.pop() {
            v.push((
                dir.clone(),
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0,
            ));
            for e in fs::read_dir(&dir).unwrap() {
                let e = e.unwrap().path();
                if e.is_dir() {
                    stack.push(e);
                } else {
                    let m = fs::metadata(&e).unwrap();
                    v.push((e, m.permissions().mode() & 0o777, m.len()));
                }
            }
        }
        v.sort();
        v
    };
    let before = snap();
    assert!(before.iter().any(|(_, m, _)| *m == 0o600));
    balances_list_in(&d, day("2026-11-12")).unwrap();
    assert_eq!(before, snap());
    let _ = fs::remove_dir_all(d);
}

// ── native-picker import (money_import_pick) ──

#[test]
fn pick_import_creates_one_account_imports_and_reports_only_counts() {
    let d = temp_dir("money-pick");
    let f = write_file(
        &d,
        "s.csv",
        format!(
            "{SPLIT_HEADER}2026/11/02,1000,,coffee,9000\n2026/11/03,,5000,pay,14000\nbad,row\n"
        )
        .as_bytes(),
    );
    let r = import_pick_in(&d, Some(&f)).unwrap();
    assert_eq!((r.imported, r.skipped), (2, 1));
    assert_eq!(load_accounts(&d).unwrap().len(), 1);
    // Same file again: all duplicates, still one account, nothing new imported.
    let r2 = import_pick_in(&d, Some(&f)).unwrap();
    assert_eq!((r2.imported, r2.skipped), (0, 3));
    assert_eq!(load_accounts(&d).unwrap().len(), 1);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn pick_import_errors_are_fixed_codes_and_a_refused_file_creates_nothing() {
    let d = temp_dir("money-pick-err");
    assert_eq!(import_pick_in(&d, None).unwrap_err(), "cancelled");
    let junk = write_file(&d, "j.csv", b"hello,world\n1,2\n");
    assert_eq!(
        import_pick_in(&d, Some(&junk)).unwrap_err(),
        "unrecognised_format"
    );
    assert_eq!(
        import_pick_in(&d, Some(&d.join("nope.csv"))).unwrap_err(),
        "bad_file"
    );
    assert_eq!(
        import_pick_in(&d, Some(Path::new("rel.csv"))).unwrap_err(),
        "invalid_input"
    );
    assert!(load_accounts(&d).unwrap().is_empty());
    assert_eq!(import_pick_code("anything with /Users/x path"), "failed");
    let _ = fs::remove_dir_all(d);
}

#[test]
fn pick_import_reuses_an_existing_yucho_account() {
    let d = temp_dir("money-pick-reuse");
    let mut dr = draft("Yucho");
    dr.institution = "japan_post".into();
    let a = account_add_in(&d, &dr).unwrap();
    let f = write_file(
        &d,
        "s.csv",
        format!("{SPLIT_HEADER}2026/11/02,1,,x,2\n").as_bytes(),
    );
    assert_eq!(import_pick_in(&d, Some(&f)).unwrap().imported, 1);
    assert_eq!(load_accounts(&d).unwrap().len(), 1);
    assert!(tx_file(&d, &a.id).exists());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn pick_command_takes_no_path_from_the_renderer() {
    let src = include_str!("../money_import_pick.rs");
    let sig = src
        .split("pub async fn money_import_pick(")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    assert_eq!(sig.trim(), "app: tauri::AppHandle");
    assert!(src.contains("blocking_pick_file"));
}

#[test]
fn the_renderer_path_import_is_not_a_command_any_more() {
    let src = production_source();
    assert!(!src.contains("pub async fn money_import_statement"));
    assert!(src.contains("pub(crate) fn import_statement_in"));
}

#[test]
fn a_failed_import_takes_back_the_account_the_pick_created_but_never_an_old_one() {
    // Fail the write by making the tx directory path a plain file.
    let d = temp_dir("money-pick-rollback");
    let f = write_file(
        &d,
        "s.csv",
        format!("{SPLIT_HEADER}2026/11/02,1,,x,2\n").as_bytes(),
    );
    fs::create_dir_all(d.join("money")).unwrap();
    fs::write(d.join("money/tx"), b"not a directory").unwrap();
    assert_eq!(import_pick_in(&d, Some(&f)).unwrap_err(), "store_error");
    assert!(
        load_accounts(&d).unwrap().is_empty(),
        "the new account is rolled back"
    );
    // An account that already existed stays when the import fails.
    let a = csv_account(&d);
    assert_eq!(import_pick_in(&d, Some(&f)).unwrap_err(), "store_error");
    assert_eq!(load_accounts(&d).unwrap().len(), 1);
    assert_eq!(load_accounts(&d).unwrap()[0].id, a.id);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn import_parsed_in_writes_the_rows_it_is_given_without_reading_a_path() {
    let d = temp_dir("money-parsed");
    let a = csv_account(&d);
    let (txs, skipped) =
        parse_statement(format!("{SPLIT_HEADER}2026/11/02,1,,x,2\nbad\n").as_bytes()).unwrap();
    let s = import_parsed_in(&d, &a, txs.clone(), skipped).unwrap();
    assert_eq!((s.added, s.duplicates, s.skipped_rows), (1, 0, 1));
    let s2 = import_parsed_in(&d, &a, txs, skipped).unwrap();
    assert_eq!((s2.added, s2.duplicates), (0, 1));
    let _ = fs::remove_dir_all(d);
}

// ── source scans ──

fn production_source() -> String {
    let src = include_str!("../money.rs");
    let marker = concat!("#[cfg(test)]", "\nmod tests;");
    src.split(marker).next().unwrap().to_string()
}

#[test]
fn no_command_is_named_like_a_money_mover() {
    let src = production_source();
    let mut names = Vec::new();
    let mut lines = src.lines();
    while let Some(l) = lines.next() {
        if l.trim() == "#[tauri::command]" {
            // `#[specta::specta]` sits between the attribute and the fn line.
            let f = lines.find(|l| !l.trim_start().starts_with("#[")).unwrap();
            let name = f
                .split("fn ")
                .nth(1)
                .and_then(|r| r.split('(').next())
                .unwrap()
                .trim()
                .to_string();
            names.push(name);
        }
    }
    assert_eq!(names.len(), 8, "{names:?}");
    for n in &names {
        for banned in [
            "transfer",
            "pay",
            "trade",
            "convert",
            "withdraw",
            "send",
            "buy",
            "sell",
            "move",
            "wire",
            "remit",
            "swap",
            "sign",
            "broadcast",
            "order",
            "deposit",
            "exchange",
        ] {
            assert!(!n.contains(banned), "command {n} contains {banned}");
        }
    }
}

#[test]
fn ecb_is_the_only_url_and_refresh_checks_the_gates_first() {
    let src = production_source();
    let https = concat!("https", "://");
    let http = concat!("http", "://");
    assert_eq!(
        src.matches(https).count(),
        1,
        "exactly one URL in the module"
    );
    assert_eq!(src.matches(http).count(), 0);
    assert!(ECB_DAILY_URL.starts_with("https://www.ecb.europa.eu/"));
    let cmd = src.split("pub async fn fx_rate_refresh").nth(1).unwrap();
    assert!(cmd.contains("super::telegram::outbound_disabled()"));
    assert!(cmd.contains("super::telegram::local_only_on()"));
    // The gate sits before the fetch in `fx_refresh_in`, and only `fetch_ecb` builds a client.
    let gate = src.find("if outbound_disabled || local_only").unwrap();
    let body = src.find("let body = fetch().await?").unwrap();
    assert!(gate < body);
    assert_eq!(src.matches("reqwest::Client::builder()").count(), 1);
}

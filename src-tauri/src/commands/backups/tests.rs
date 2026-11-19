use super::*;
use std::collections::HashSet;
use std::sync::Mutex as M;

fn fake_home(tag: &str) -> (PathBuf, PathBuf) {
    let home = std::env::temp_dir().join(format!(
        "nexus-backups-test-{tag}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(home.join("Desktop")).unwrap();
    let data = home.join("Library-data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("goals.json"), "[1]").unwrap();
    (home.canonicalize().unwrap(), data.canonicalize().unwrap())
}

fn patch_dir(d: &str) -> BackupSettingsPatch {
    BackupSettingsPatch {
        dir: Some(d.into()),
        ..Default::default()
    }
}

fn entries(path: &Path) -> Vec<String> {
    let f = fs::File::open(path).unwrap();
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(f));
    a.entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
        .collect()
}

fn all_contents(path: &Path) -> String {
    use std::io::Read;
    let f = fs::File::open(path).unwrap();
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let mut out = String::new();
    for e in a.entries().unwrap() {
        let mut e = e.unwrap();
        let mut b = Vec::new();
        e.read_to_end(&mut b).unwrap();
        out.push_str(&String::from_utf8_lossy(&b));
    }
    out
}

const PLENTY: &(dyn Fn(&Path) -> Result<u64, String> + Send + Sync) = &|_| Ok(u64::MAX);

// ---- settings ----

#[test]
fn defaults_match_the_ts_defaults_and_serialise_camel_case() {
    let (home, data) = fake_home("defaults");
    let s = load_settings(&data).unwrap();
    assert_eq!(
        serde_json::to_value(&s).unwrap(),
        serde_json::json!({"nightly":true,"at":"03:00","dir":"~/Amara Backups","keep":14,"beforeUpdate":true,"skipOnBatteryBelow":20,"autoPrune":false,"lastRunAt":null,"lastRunResult":null,"lastOkAt":null,"gitBundle":false,"lastBundleResult":null,"failedAttempts":0,"failedSlot":null})
    );
    assert!(!settings_path(&data).exists(), "a read writes nothing");
    let _ = home;
}

#[test]
fn a_valid_patch_persists_atomically_0600() {
    let (home, data) = fake_home("persist");
    let p = BackupSettingsPatch {
        at: Some("23:59".into()),
        keep: Some(60),
        skip_on_battery_below: Some(0),
        nightly: Some(false),
        dir: Some("~/Custom Backups".into()),
        ..Default::default()
    };
    let s = apply_patch(&home, &data, p).unwrap();
    assert_eq!(load_settings(&data).unwrap(), s);
    assert_eq!(s.dir, "~/Custom Backups");
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(settings_path(&data))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let junk = fs::read_dir(&data)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(junk, 0);
}

#[test]
fn bad_values_are_refused_and_change_nothing() {
    let (home, data) = fake_home("bad");
    for at in ["24:00", "3:00", "03:60", "ab:cd", "03-00", "", "03:000"] {
        let p = BackupSettingsPatch {
            at: Some(at.into()),
            ..Default::default()
        };
        assert_eq!(
            apply_patch(&home, &data, p).unwrap_err(),
            "invalid_time",
            "{at}"
        );
    }
    for k in [0, 61, -1] {
        let p = BackupSettingsPatch {
            keep: Some(k),
            ..Default::default()
        };
        assert_eq!(apply_patch(&home, &data, p).unwrap_err(), "invalid_keep");
    }
    for b in [-1, 101] {
        let p = BackupSettingsPatch {
            skip_on_battery_below: Some(b),
            ..Default::default()
        };
        assert_eq!(apply_patch(&home, &data, p).unwrap_err(), "invalid_battery");
    }
    // A bad field next to a good one: all-or-nothing.
    let p = BackupSettingsPatch {
        nightly: Some(false),
        keep: Some(0),
        ..Default::default()
    };
    assert!(apply_patch(&home, &data, p).is_err());
    assert!(!settings_path(&data).exists());
    for ok in ["00:00", "23:59", "03:00"] {
        let p = BackupSettingsPatch {
            at: Some(ok.into()),
            ..Default::default()
        };
        assert!(apply_patch(&home, &data, p).is_ok(), "{ok}");
    }
}

#[test]
fn dir_must_be_inside_home_and_outside_app_data() {
    let (home, data) = fake_home("dir");
    let outside = std::env::temp_dir().join(format!("outside-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&outside).unwrap();
    let outside_s = outside.to_string_lossy().to_string();
    for (d, code) in [
        (outside_s.as_str(), "dir_not_in_home"),
        ("/etc", "dir_not_in_home"),
        ("~", "dir_not_in_home"),
        ("relative/path", "dir_invalid"),
        ("~/../escape", "dir_invalid"),
        ("", "dir_invalid"),
    ] {
        assert_eq!(
            apply_patch(&home, &data, patch_dir(d)).unwrap_err(),
            code,
            "{d}"
        );
    }
    // inside app data, the app data dir itself, below it, or a parent of it
    let inside = data.join("backups").to_string_lossy().to_string();
    assert_eq!(
        apply_patch(&home, &data, patch_dir(&inside)).unwrap_err(),
        "dir_in_app_data"
    );
    assert_eq!(
        apply_patch(&home, &data, patch_dir(&data.to_string_lossy())).unwrap_err(),
        "dir_in_app_data"
    );
    let nested_data = home.join("a").join("b");
    fs::create_dir_all(&nested_data).unwrap();
    assert_eq!(
        apply_patch(&home, &nested_data, patch_dir("~/a")).unwrap_err(),
        "dir_in_app_data"
    );
    assert!(apply_patch(&home, &data, patch_dir("~/Amara Backups")).is_ok());
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn a_symlinked_dir_or_parent_is_refused() {
    let (home, data) = fake_home("symlink");
    let real = home.join("real");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, home.join("link")).unwrap();
    assert_eq!(
        apply_patch(&home, &data, patch_dir("~/link")).unwrap_err(),
        "dir_is_symlink"
    );
    assert_eq!(
        apply_patch(&home, &data, patch_dir("~/link/sub")).unwrap_err(),
        "dir_is_symlink"
    );
    let file = home.join("afile");
    fs::write(&file, "x").unwrap();
    assert_eq!(
        apply_patch(&home, &data, patch_dir("~/afile")).unwrap_err(),
        "dir_not_a_directory"
    );
}

// ---- Muse folders are byte-for-byte; narrowed redaction ----

#[test]
fn muse_files_are_byte_identical_unless_a_json_has_a_secret_field_then_left_out_and_listed() {
    let (home, data) = fake_home("muse-bytes");
    // A clean JSON (odd spacing, a float a parse/re-print would change) and one with secret-looking fields.
    let clean: &[u8] = b"{ \"eeg\" : [1,2],  \"x\": 13.935167180141983 }\n";
    let secret: &[u8] = b"{ \"auth\" : \"MUSE-FIELD\",  \"x\": 1 }\n";
    let bare_key: &[u8] = b"{\"meta\":{\"key\":\"abc\"}}";
    let broken: &[u8] = b"{ not json 13.935167180141983";
    // Recording formats are always copied as is, even with secret-looking content.
    let ndjson: &[u8] = b"{\"auth\":\"MUSE-ND\"}\n{\"x\":1.50}\n";
    let bands: &[u8] = b"{\"api_key\":\"MUSE-BANDS\",\"x\":13.935167180141983}";
    let put = |d: &str, f: &str, b: &[u8]| {
        fs::create_dir_all(data.join(d)).unwrap();
        fs::write(data.join(d).join(f), b).unwrap();
    };
    put("sessions", "clean.json", clean);
    put("muse", "m_clean.json", clean);
    put("sessions", "secret.json", secret);
    put("muse", "m_secret.json", secret);
    put("sessions", "bare_key.json", bare_key);
    put("sessions", "broken.json", broken);
    put("replays", "r.ndjson", ndjson);
    put("replays", "r.ndjson.bands.json", bands);
    put("sessions", "my_token.json", b"{}");
    // Outside the Muse folders the field is still redacted.
    put("kv", "other.json", secret);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let busy = AtomicBool::new(false);
    let r = rt
        .block_on(run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            PLENTY,
            &(|_: &str, _: u8| {}),
            &busy,
        ))
        .unwrap();
    let arch = home.join("Amara Backups").join(&r.entry.id);
    let f = fs::File::open(&arch).unwrap();
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let mut got: std::collections::HashMap<String, Vec<u8>> = Default::default();
    for e in a.entries().unwrap() {
        use std::io::Read;
        let mut e = e.unwrap();
        let n = e.path().unwrap().to_string_lossy().into_owned();
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).unwrap();
        got.insert(n, buf);
    }
    let exact = |rel: &str| {
        got.iter()
            .find(|(k, _)| k.split_once('/').is_some_and(|(_, r)| r == rel))
            .map(|(_, v)| v.clone())
    };
    let manifest: Value = serde_json::from_slice(&exact("manifest.json").unwrap()).unwrap();
    // byte-identical, and the manifest hash is the original file's sha256
    for (d, f, want) in [
        ("sessions", "clean.json", clean),
        ("muse", "m_clean.json", clean),
        ("replays", "r.ndjson", ndjson),
        ("replays", "r.ndjson.bands.json", bands),
        ("sessions", "broken.json", broken),
    ] {
        let mut keys = vec![format!("nexus-config/{d}/{f}")];
        if d == "sessions" {
            keys.push(format!("{d}/{f}"));
        }
        for key in keys {
            assert_eq!(exact(&key).unwrap_or_default(), want, "{key}");
            assert_eq!(
                manifest["sha256"][&key].as_str().unwrap(),
                sha_hex(want),
                "{key}"
            );
        }
    }
    // a Muse JSON with a secret-looking field (including a bare `key`) is left out, in both copies
    for f in [
        "secret.json",
        "m_secret.json",
        "bare_key.json",
        "my_token.json",
    ] {
        assert!(
            !got.keys().any(|k| k.ends_with(&format!("/{f}"))),
            "{f} was packed"
        );
    }
    for f in ["secret.json", "m_secret.json", "bare_key.json"] {
        assert!(
            r.skipped
                .iter()
                .any(|s| s.contains(f) && s.contains("left out")),
            "{f} not listed: {:?}",
            r.skipped
        );
    }
    assert!(r
        .skipped
        .iter()
        .any(|s| s.contains("broken.json") && s.contains("not valid JSON")));
    // no value of a left-out file reached the archive, and the originals are untouched
    let all: Vec<u8> = got.values().flatten().copied().collect();
    let text = String::from_utf8_lossy(&all);
    assert!(!text.contains("MUSE-FIELD"));
    assert_eq!(fs::read(data.join("sessions/secret.json")).unwrap(), secret);
    assert_eq!(fs::read(data.join("muse/m_secret.json")).unwrap(), secret);
    // outside Muse folders: redacted as before
    let other = String::from_utf8(exact("nexus-config/kv/other.json").unwrap()).unwrap();
    assert!(!other.contains("MUSE-FIELD") && other.contains("\"x\""));
}

#[test]
fn the_open_recording_and_its_sidecar_are_left_out_and_listed() {
    let (home, data) = fake_home("active-rec");
    fs::create_dir_all(data.join("replays")).unwrap();
    for f in [
        "eeg_live.ndjson",
        "eeg_live.ndjson.bands.json",
        "eeg_done.ndjson",
        "eeg_done.ndjson.bands.json",
    ] {
        fs::write(data.join("replays").join(f), b"x").unwrap();
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let out = home.join("out");
    fs::create_dir_all(&out).unwrap();
    let info = rt
        .block_on(pack_to(
            &data,
            &out,
            "t.tar.gz",
            &PackOpts {
                exclude_secrets: true,
                progress: None,
                active_recording: Some("eeg_live.ndjson".into()),
            },
        ))
        .unwrap();
    let names = entries(&out.join("t.tar.gz"));
    assert!(!names.iter().any(|n| n.contains("eeg_live")), "{names:?}");
    assert!(names.iter().any(|n| n.ends_with("replays/eeg_done.ndjson")));
    assert!(names
        .iter()
        .any(|n| n.ends_with("replays/eeg_done.ndjson.bands.json")));
    let skipped = info.skipped.join("|");
    assert!(skipped.contains("eeg_live.ndjson ") && skipped.contains("eeg_live.ndjson.bands.json"));
    // the originals are untouched
    assert_eq!(
        fs::read(data.join("replays/eeg_live.ndjson")).unwrap(),
        b"x"
    );
}

#[test]
fn redaction_keeps_token_counts_and_identifier_keys_but_not_real_secrets() {
    let input = br#"{"prompt_tokens":12,"completion_tokens":7,"total_tokens":19,
          "take":{"cited":[{"key":"hrv","label":"HRV","display":"1","baseline":null}]},
          "worker_api_keys":[{"name":"w","key":"SENTINEL-ARM"}],
          "key":"SENTINEL-TOP","access_tokens":"SENTINEL-STR","telegram_bot_token":"SENTINEL-TG",
          "creds":{"key":"hrv2","password":"SENTINEL-PW"}}"#;
    let (out, removed) = redact_json_bytes(input).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("\"prompt_tokens\":12")
            && text.contains("\"completion_tokens\":7")
            && text.contains("\"total_tokens\":19")
    );
    assert!(text.contains("\"key\":\"hrv\""));
    assert!(!text.contains("SENTINEL"), "{text}");
    assert!(
        removed.contains(&"telegram_bot_token".to_string())
            && removed.contains(&"password".to_string())
    );
    // a string under a *_tokens name is still a secret
    assert!(removed.contains(&"access_tokens".to_string()));
}

#[test]
fn bare_key_is_removed_except_in_a_cited_point_and_camel_keys_are_secrets() {
    let s = "SENTINEL-S";
    let cases = [
        format!(r#"{{"anthropic":{{"key":"{s}"}}}}"#),
        format!(r#"[{{"id":"a","key":"{s}"}}]"#),
        format!(r#"{{"headers":{{"key":"{s}"}}}}"#),
        // identifier-looking and token-looking values are NOT kept by their look
        r#"{"headers":{"key":"abc"}}"#.to_string(),
        r#"{"x":{"key":"hrv"}}"#.to_string(),
        r#"[{"id":"a","key":"hrv"}]"#.to_string(),
        r#"{"x":{"key":"sk-abcdef"}}"#.to_string(),
        r#"{"x":{"key":"eyJhbGciOi"}}"#.to_string(),
        r#"{"x":{"key":12345}}"#.to_string(),
        r#"{"key":"hrv"}"#.to_string(),
        // `cited` by name alone, or with a partial shape, or under another name: removed
        r#"{"cited":[{"key":"hrv"}]}"#.to_string(),
        r#"{"cited":[{"key":"hrv","label":"HRV"}]}"#.to_string(),
        r#"{"cited":[{"key":"hrv","label":"HRV","display":"1"}]}"#.to_string(),
        r#"{"cited":[{"key":7,"label":"HRV","display":"1","baseline":null}]}"#.to_string(),
        r#"{"cites":[{"key":"hrv","label":"HRV","display":"1","baseline":null}]}"#.to_string(),
        r#"{"cited":{"key":"hrv"}}"#.to_string(),
    ];
    for c in &cases {
        let (out, removed) = redact_json_bytes(c.as_bytes()).unwrap();
        assert!(
            removed.contains(&"key".to_string()),
            "{c} -> {}",
            String::from_utf8_lossy(&out)
        );
    }
    // the one allowed context: take.cited[] with the full CitedPoint shape
    let ok = br#"{"day":"d","take":{"cited":[{"key":"hrv","label":"HRV","display":"52 ms","baseline":"48 ms"},{"key":"sleep_total","label":"Sleep","display":"7h","baseline":null}]}}"#;
    let (out, removed) = redact_json_bytes(ok).unwrap();
    assert!(removed.is_empty(), "{removed:?}");
    let parsed = |b: &[u8]| serde_json::from_slice::<Value>(b).unwrap();
    assert_eq!(parsed(&out), parsed(ok));
    // ...and a secret-looking value there is still just an identifier slot, a secret name next to it goes
    let mixed = br#"{"cited":[{"key":"hrv","label":"HRV","display":"1","baseline":null,"api_key":"SENTINEL-X"}]}"#;
    let (out, removed) = redact_json_bytes(mixed).unwrap();
    assert_eq!(removed, vec!["api_key".to_string()]);
    assert!(!String::from_utf8(out).unwrap().contains("SENTINEL"));
    // camelCase secrets, at any depth
    for k in [
        "privateKey",
        "accessKey",
        "secretKey",
        "signingKey",
        "sessionKey",
        "encryptionKey",
        "clientKey",
        "apiKey",
    ] {
        let c = format!(r#"{{"a":{{"{k}":"SENTINEL-CAMEL","fine":1}}}}"#);
        let (out, removed) = redact_json_bytes(c.as_bytes()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            !text.contains("SENTINEL") && text.contains("\"fine\""),
            "{k}"
        );
        assert_eq!(removed, vec![k.to_string()]);
    }
    // ordinary camelCase fields stay
    let (_, removed) =
        redact_json_bytes(br#"{"keyboard":1,"monkey":2,"keyCount":3,"turkey":4}"#).unwrap();
    assert!(removed.is_empty(), "{removed:?}");
}

// ---- backup_now ----

fn seed_secrets(data: &Path) {
    fs::write(data.join("amara-n8n.key"), "SENTINEL-KEYFILE").unwrap();
    fs::write(data.join("crew_service_token"), "SENTINEL-CREW").unwrap();
    fs::write(data.join("n8n_api_key"), "SENTINEL-N8N").unwrap();
    fs::write(data.join(".env"), "HA_TOKEN=SENTINEL-HA").unwrap();
    fs::write(data.join(".amara-n8n.key.abc.tmp"), "SENTINEL-TMP").unwrap();
    fs::write(data.join("my-Token.json"), "SENTINEL-TOKENFILE").unwrap();
    fs::write(data.join("app_secret.txt"), "SENTINEL-SECRETFILE").unwrap();
    fs::create_dir_all(data.join("sessions/sub")).unwrap();
    fs::write(data.join("sessions/sub/amara-n8n.key"), "SENTINEL-NESTED").unwrap();
    fs::write(data.join("sessions/sub/x.pem"), "SENTINEL-PEM").unwrap();
    fs::write(data.join("sessions/rec.ndjson"), "keepme").unwrap();
    // JSON stores whose secret field is a plain name (arm_config.json worker_api_keys[].key)
    fs::write(
        data.join("arm_config.json"),
        r#"{"poll_interval_secs":5,"worker_api_keys":[{"name":"w1","key":"SENTINEL-ARM"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(data.join("kv")).unwrap();
    fs::write(
        data.join("kv/s1.json"),
        r#"{"auth":"SENTINEL-AUTH","eeg":[1,2]}"#,
    )
    .unwrap();
    fs::write(data.join("broken.json"), "{SENTINEL-BROKEN").unwrap();
    fs::write(
            data.join("config.json"),
            r#"{"theme":"dark","telegram_bot_token":"SENTINEL-TG","anthropic_api_key":"SENTINEL-AK","n8n_webhook_secret":"SENTINEL-WH","gateway_auth_token":"SENTINEL-GW","llm_token_threshold":512,"nested":{"x_password":"SENTINEL-PW","ok":1}}"#,
        )
        .unwrap();
}

fn quiet() -> impl Fn(&str, u8) + Send + Sync {
    |_, _| {}
}

#[tokio::test]
async fn backup_now_writes_0600_into_a_new_0700_dir_without_any_secret() {
    let (home, data) = fake_home("run");
    seed_secrets(&data);
    let s = BackupSettings::default();
    let busy = AtomicBool::new(false);
    let r = run_backup(&home, &data, &s, PLENTY, &quiet(), &busy)
        .await
        .unwrap();
    let dir = home.join("Amara Backups");
    let arch = dir.join(&r.entry.id);
    assert!(r.entry.id.starts_with("amara-backup-") && r.entry.id.ends_with(".tar.gz"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&arch).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(!busy.load(Ordering::SeqCst), "the flag is released");

    let names = entries(&arch);
    for bad in [
        "amara-n8n.key",
        "crew_service_token",
        "n8n_api_key",
        ".env",
        ".amara-n8n.key.",
        "my-Token",
        "app_secret",
        ".pem",
    ] {
        assert!(!names.iter().any(|n| n.contains(bad)), "{bad} in {names:?}");
    }
    let text = all_contents(&arch);
    assert!(
        !text.contains("SENTINEL"),
        "a secret value reached the archive"
    );
    // ...but ordinary data and the non-secret config fields did.
    assert!(names.iter().any(|n| n.ends_with("/goals.json")));
    assert!(names.iter().any(|n| n.ends_with("sessions/rec.ndjson")));
    assert!(text.contains("keepme") && text.contains("\"theme\": \"dark\""));
    // non-secret fields of redacted JSON stores survive (worker_api_keys goes whole: its name holds api_key)
    assert!(text.contains("poll_interval_secs") && text.contains("\"eeg\":[1,2]"));
    assert!(text.contains("llm_token_threshold") && text.contains("\"ok\": 1"));
    assert!(!text.contains("\"telegram_bot_token\""));
    assert!(r.skipped.iter().any(|x| x.contains("telegram_bot_token")));
    assert!(!format!("{:?}", r.skipped).contains("SENTINEL"));
    // The source files are untouched.
    assert!(data.join("amara-n8n.key").is_file() && data.join(".env").is_file());
}

/// Fail-before: with the filter off (what `pack_it_up` still does) the same data dir puts
/// the secrets in the archive, so the scan in the test above really proves the filter.
#[tokio::test]
async fn without_the_filter_the_same_scan_would_fail() {
    let (home, data) = fake_home("nofilter");
    seed_secrets(&data);
    let out = home.join("out");
    fs::create_dir_all(&out).unwrap();
    let info = pack_to(&data, &out, "x.tar.gz", &PackOpts::default())
        .await
        .unwrap();
    let text = all_contents(Path::new(&info.path));
    assert!(text.contains("SENTINEL-HA") && text.contains("SENTINEL-TG"));
}

#[tokio::test]
async fn the_desktop_pack_it_up_archive_leaves_secrets_out_too() {
    let (home, data) = fake_home("packitup");
    fs::create_dir_all(home.join("Desktop")).unwrap();
    seed_secrets(&data);
    let info = super::super::packager::pack_at(&home, &data).await.unwrap();
    let text = all_contents(Path::new(&info.path));
    assert!(
        !text.contains("SENTINEL"),
        "a secret reached the Desktop archive"
    );
    assert!(text.contains("keepme") && text.contains("poll_interval_secs"));
}

/// Home windows: per-site browser profile records never reach a backup or the Desktop archive.
/// Fail-before: with "webviews" removed from the exclude list the sentinel is in the archive.
#[tokio::test]
async fn webviews_profiles_never_reach_a_backup_archive() {
    let (home, data) = fake_home("webviews");
    fs::create_dir_all(home.join("Desktop")).unwrap();
    seed_secrets(&data);
    let site = data.join("webviews").join("instagram_com");
    fs::create_dir_all(&site).unwrap();
    fs::write(site.join("store.bin"), "WEBVIEWS-SENTINEL").unwrap();
    let info = super::super::packager::pack_at(&home, &data).await.unwrap();
    let text = all_contents(Path::new(&info.path));
    assert!(
        !text.contains("WEBVIEWS-SENTINEL"),
        "a webview profile reached the archive"
    );
    assert!(text.contains("keepme"), "ordinary data is still packed");
    let out = home.join("out");
    fs::create_dir_all(&out).unwrap();
    let opts = PackOpts {
        exclude_secrets: true,
        ..PackOpts::default()
    };
    let info = pack_to(&data, &out, "y.tar.gz", &opts).await.unwrap();
    assert!(!all_contents(Path::new(&info.path)).contains("WEBVIEWS-SENTINEL"));
    let info = pack_to(
        &data,
        &out,
        "z.tar.gz",
        &PackOpts {
            exclude_secrets: false,
            ..PackOpts::default()
        },
    )
    .await
    .unwrap();
    assert!(all_contents(Path::new(&info.path)).contains("WEBVIEWS-SENTINEL"));
}

#[test]
fn a_restore_keeps_the_live_secrets_the_backup_left_out() {
    let restored = br#"{"theme":"light","nested":{"ok":2},"worker_api_keys":[]}"#;
    let current = br#"{"theme":"dark","telegram_bot_token":"LIVE-TG","nested":{"ok":1,"x_password":"LIVE-PW"},"worker_api_keys":[{"key":"LIVE-ARM"}],"plain":"gone"}"#;
    let v: Value = serde_json::from_slice(&merge_kept_secrets(restored, current).unwrap()).unwrap();
    assert_eq!(v["theme"], "light"); // the backup wins for normal fields
    assert_eq!(v["nested"]["ok"], 2);
    assert_eq!(v["telegram_bot_token"], "LIVE-TG"); // a secret it lacks is kept
    assert_eq!(v["nested"]["x_password"], "LIVE-PW");
    assert_eq!(v["worker_api_keys"], serde_json::json!([])); // present in the backup: backup wins
    assert!(v.get("plain").is_none()); // non-secret live-only fields are not resurrected
    assert!(merge_kept_secrets(b"not json", current).is_none());
}

#[test]
fn a_restore_that_moves_an_endpoint_does_not_get_the_live_token() {
    let current = br#"{"worker_base_url":"http://127.0.0.1:9000","gateway_auth_token":"LIVE-GW","telegram_bot_token":"LIVE-TG","n8n":{"url":"http://127.0.0.1:5678","webhook_secret":"LIVE-WH"}}"#;
    // top-level endpoint moved: no top-level or nested secret is carried over
    let moved =
        br#"{"worker_base_url":"https://evil.example","n8n":{"url":"http://127.0.0.1:5678"}}"#;
    let v: Value = serde_json::from_slice(&merge_kept_secrets(moved, current).unwrap()).unwrap();
    assert!(v.get("gateway_auth_token").is_none() && v.get("telegram_bot_token").is_none());
    assert!(v["n8n"].get("webhook_secret").is_none());
    // only the nested endpoint moved: top-level secrets stay, the nested one is dropped
    let nested =
        br#"{"worker_base_url":"http://127.0.0.1:9000","n8n":{"url":"https://evil.example"}}"#;
    let v: Value = serde_json::from_slice(&merge_kept_secrets(nested, current).unwrap()).unwrap();
    assert_eq!(v["gateway_auth_token"], "LIVE-GW");
    assert!(v["n8n"].get("webhook_secret").is_none());
    // a removed endpoint counts as a change too
    let gone = br#"{"n8n":{"url":"http://127.0.0.1:5678"}}"#;
    let v: Value = serde_json::from_slice(&merge_kept_secrets(gone, current).unwrap()).unwrap();
    assert!(v.get("gateway_auth_token").is_none());
}

#[tokio::test]
async fn a_backup_still_passes_the_restore_allow_list() {
    use super::super::packager::{
        find_pack_root, plan_restore, restore_target, unpack_archive_checked,
    };
    let (home, data) = fake_home("allow");
    seed_secrets(&data);
    let busy = AtomicBool::new(false);
    let r = run_backup(
        &home,
        &data,
        &BackupSettings::default(),
        PLENTY,
        &quiet(),
        &busy,
    )
    .await
    .unwrap();
    // every file the archive holds under its top folder is either restorable or manifest.json
    let arch = home.join("Amara Backups").join(&r.entry.id);
    for n in entries(&arch) {
        if n.ends_with('/') || n.contains("/._") || n.starts_with("._") {
            continue;
        }
        let rel: Vec<String> = n.split('/').skip(1).map(str::to_string).collect();
        if rel == ["manifest.json"] {
            continue;
        }
        assert!(restore_target(&rel).is_some(), "not on the allow-list: {n}");
    }
    let staging = home.join("staging");
    fs::create_dir(&staging).unwrap();
    unpack_archive_checked(&arch, &staging, 2 * 1024 * 1024 * 1024, 50_000).unwrap();
    let plan = plan_restore(&find_pack_root(&staging).unwrap()).unwrap();
    assert!(plan.items.iter().any(|i| i.rel.ends_with("goals.json")));
}

#[tokio::test]
async fn a_second_concurrent_call_is_refused() {
    let (home, data) = fake_home("busy");
    let busy = AtomicBool::new(false);
    let _held = BusyGuard::acquire(&busy).unwrap();
    let e = run_backup(
        &home,
        &data,
        &BackupSettings::default(),
        PLENTY,
        &quiet(),
        &busy,
    )
    .await
    .unwrap_err();
    assert_eq!(e, "already_running");
    assert!(
        busy.load(Ordering::SeqCst),
        "the refused call does not clear the holder's flag"
    );
    assert!(!home.join("Amara Backups").exists());
}

#[tokio::test]
async fn too_little_free_space_refuses_before_writing() {
    let (home, data) = fake_home("space");
    fs::write(data.join("big.bin"), vec![0u8; 4096]).unwrap();
    let need = dir_size(&data) + FREE_SPACE_MARGIN;
    let busy = AtomicBool::new(false);
    let seen: M<HashSet<u64>> = M::new(HashSet::new());
    let low = |_: &Path| {
        seen.lock().unwrap().insert(need - 1);
        Ok(need - 1)
    };
    let e = run_backup(
        &home,
        &data,
        &BackupSettings::default(),
        &low,
        &quiet(),
        &busy,
    )
    .await
    .unwrap_err();
    assert_eq!(e, "not_enough_space");
    let made = fs::read_dir(home.join("Amara Backups")).unwrap().count();
    assert_eq!(made, 0, "no archive was started");
    let exact = |_: &Path| Ok(need);
    assert!(run_backup(
        &home,
        &data,
        &BackupSettings::default(),
        &exact,
        &quiet(),
        &busy
    )
    .await
    .is_ok());
}

#[tokio::test]
async fn progress_events_run_up_to_100() {
    let (home, data) = fake_home("progress");
    let log: M<Vec<(String, u8)>> = M::new(Vec::new());
    let rec = |p: &str, n: u8| log.lock().unwrap().push((p.to_string(), n));
    let busy = AtomicBool::new(false);
    run_backup(
        &home,
        &data,
        &BackupSettings::default(),
        PLENTY,
        &rec,
        &busy,
    )
    .await
    .unwrap();
    let log = log.lock().unwrap();
    assert!(log.windows(2).all(|w| w[0].1 <= w[1].1), "{log:?}");
    assert_eq!(log.last().unwrap(), &("Done".to_string(), 100));
    assert!(log.iter().any(|(p, _)| p == "Compressing"));
}

#[tokio::test]
async fn a_symlinked_destination_is_refused() {
    let (home, data) = fake_home("symdest");
    let real = home.join("real");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, home.join("Amara Backups")).unwrap();
    let busy = AtomicBool::new(false);
    let e = run_backup(
        &home,
        &data,
        &BackupSettings::default(),
        PLENTY,
        &quiet(),
        &busy,
    )
    .await
    .unwrap_err();
    assert_eq!(e, "dir_is_symlink");
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

#[test]
fn free_bytes_reads_a_real_volume() {
    assert!(free_bytes(&std::env::temp_dir()).unwrap() > 0);
    assert!(free_bytes(Path::new("/definitely/not/here")).is_err());
}

// ---- list ----

#[test]
fn list_shows_only_backup_files_newest_first_and_never_follows_links() {
    let (home, data) = fake_home("list");
    let dir = home.join("Amara Backups");
    fs::create_dir_all(&dir).unwrap();
    let put = |n: &str, c: &str| fs::write(dir.join(n), c).unwrap();
    put("amara-backup-20261110-030000.tar.gz", "aaaa");
    put("amara-backup-20261112-030000.tar.gz", "bb");
    put("nexus-backup-20261111_101500.tar.gz", "c");
    put("notes.txt", "x");
    put("amara-backup-2026-bad.tar.gz", "x");
    put("amara-backup-20261112-030000.tar.gz.part", "x");
    put("amara-n8n.key", "SENTINEL");
    fs::create_dir(dir.join("amara-backup-20261113-030000.tar.gz")).unwrap();
    std::os::unix::fs::symlink(
        dir.join("amara-backup-20261110-030000.tar.gz"),
        dir.join("amara-backup-20261114-030000.tar.gz"),
    )
    .unwrap();
    let now = Local.with_ymd_and_hms(2026, 11, 12, 12, 0, 0).unwrap();
    let l = list_in(&home, &data, "~/Amara Backups", now).unwrap();
    let ids: Vec<&str> = l.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "amara-backup-20261112-030000.tar.gz",
            "nexus-backup-20261111_101500.tar.gz",
            "amara-backup-20261110-030000.tar.gz"
        ]
    );
    assert_eq!(l[0].days_old, 0);
    assert_eq!(l[1].days_old, 1);
    assert_eq!(l[2].days_old, 2);
    assert_eq!(l[0].size_bytes, 2);
    assert_eq!(l[0].label, "Nov 12, 3:00 AM");
    let j = serde_json::to_value(&l[0]).unwrap();
    for k in ["id", "label", "madeAt", "sizeBytes", "daysOld"] {
        assert!(j.get(k).is_some(), "{k}");
    }
    // a missing folder is an empty list, not an error
    assert!(list_in(&home, &data, "~/Nope", now).unwrap().is_empty());
}

// ---- exclusions ----

#[test]
fn exclusion_rules() {
    for n in [
        "amara-n8n.key",
        "crew_service_token",
        "n8n_api_key",
        ".env",
        ".env.local",
        "A.KEY",
        "x_token",
        "client_secret.json",
        "cert.pem",
        ".amara-n8n.key.9f.tmp",
        "config.json",
    ] {
        assert!(is_excluded_name(n, 0), "{n}");
    }
    assert!(
        !is_excluded_name("config.json", 1),
        "only the top-level config is redacted"
    );
    for n in [
        "goals.json",
        "events.db",
        "kv",
        "eeg_20260217_100521.ndjson",
        "sessions",
    ] {
        assert!(!is_excluded_name(n, 0), "{n}");
    }
    assert!(is_secret_field("telegram_bot_token") && is_secret_field("anthropic_api_key"));
    assert!(!is_secret_field("llm_token_threshold") && !is_secret_field("theme"));
}

// ---- module source ----

#[test]
fn this_module_has_no_delete_or_prune_and_no_scheduler() {
    let src = include_str!("../backups.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    for bad in [
        "remove_file",
        "remove_dir",
        "fs::remove",
        "tokio::time",
        "interval(",
        "thread::sleep",
        "spawn(",
    ] {
        assert!(!body.contains(bad), "backups.rs must not contain {bad}");
    }
}

#[test]
fn nothing_in_the_data_dir_or_the_backup_dir_is_deleted_by_a_run() {
    // Fail-before shape: a run over a populated backup dir leaves every old backup alone.
    let (home, data) = fake_home("nodelete");
    let dir = home.join("Amara Backups");
    fs::create_dir_all(&dir).unwrap();
    for i in 0..20 {
        fs::write(
            dir.join(format!("amara-backup-202610{:02}-030000.tar.gz", i + 1)),
            "old",
        )
        .unwrap();
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let s = BackupSettings {
        keep: 1,
        ..Default::default()
    };
    let busy = AtomicBool::new(false);
    rt.block_on(run_backup(&home, &data, &s, PLENTY, &quiet(), &busy))
        .unwrap();
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        21,
        "keep=1 deleted nothing"
    );
}

// ---- backup_verify_last ----

fn make_archive(path: &Path, files: &[(&str, &[u8])]) {
    let gz =
        flate2::write::GzEncoder::new(fs::File::create(path).unwrap(), flate2::Compression::fast());
    let mut b = tar::Builder::new(gz);
    for (n, data) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o600);
        h.set_cksum();
        b.append_data(&mut h, format!("amara-pack-1/{n}"), *data)
            .unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
}

fn snapshot(dir: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            let m = fs::symlink_metadata(e.path()).unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                m.len(),
                m.modified().unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

fn sha_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(b))
}

const NEWEST: &str = "amara-backup-20261112-030000.tar.gz";

#[test]
fn verify_none_when_there_is_no_archive() {
    let (home, data) = fake_home("vnone");
    assert_eq!(
        verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false)
        )
        .unwrap(),
        None
    );
    fs::create_dir_all(home.join("Amara Backups")).unwrap();
    fs::write(home.join("Amara Backups/notes.txt"), "x").unwrap();
    assert_eq!(
        verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false)
        )
        .unwrap(),
        None
    );
}

#[test]
fn verify_a_good_archive_reads_every_entry_and_changes_nothing() {
    let (home, data) = fake_home("vgood");
    let dir = home.join("Amara Backups");
    fs::create_dir_all(&dir).unwrap();
    make_archive(
        &dir.join("amara-backup-20261101-030000.tar.gz"),
        &[("manifest.json", b"{}")],
    );
    make_archive(
        &dir.join(NEWEST),
        &[
            ("manifest.json", b"{\"version\":\"0.1.0\"}"),
            ("goals.json", b"[1]"),
        ],
    );
    let before = snapshot(&dir);
    let r = verify_last_in(
        &home,
        &data,
        "~/Amara Backups",
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(r.file, NEWEST);
    assert!(r.ok, "{r:?}");
    assert_eq!((r.checked, r.mode.as_str()), (2, "structure"));
    assert!(r.mismatched.is_empty() && r.size > 0 && !r.at.is_empty());
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn verify_with_hashes_flags_a_tampered_entry() {
    let (home, data) = fake_home("vtamper");
    let dir = home.join("Amara Backups");
    fs::create_dir_all(&dir).unwrap();
    let man = |h: &str| format!("{{\"sha256\":{{\"goals.json\":\"{h}\"}}}}").into_bytes();
    make_archive(
        &dir.join(NEWEST),
        &[
            ("manifest.json", &man(&sha_hex(b"[1]"))),
            ("goals.json", b"[1]"),
        ],
    );
    let r = verify_last_in(
        &home,
        &data,
        "~/Amara Backups",
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(r.ok && r.mode == "sha256", "{r:?}");
    make_archive(
        &dir.join(NEWEST),
        &[
            ("manifest.json", &man(&sha_hex(b"[1]"))),
            ("goals.json", b"[2]"),
        ],
    );
    let before = snapshot(&dir);
    let r = verify_last_in(
        &home,
        &data,
        "~/Amara Backups",
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(!r.ok);
    assert_eq!(r.mismatched, vec!["goals.json".to_string()]);
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn verify_flags_secret_names_and_a_truncated_archive() {
    let (home, data) = fake_home("vsecret");
    let dir = home.join("Amara Backups");
    fs::create_dir_all(&dir).unwrap();
    make_archive(
        &dir.join(NEWEST),
        &[("manifest.json", b"{}"), ("telegram_token.txt", b"x")],
    );
    let r = verify_last_in(
        &home,
        &data,
        "~/Amara Backups",
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(!r.ok);
    assert_eq!(
        r.mismatched,
        vec!["amara-pack-1/telegram_token.txt".to_string()]
    );
    let bytes = fs::read(dir.join(NEWEST)).unwrap();
    fs::write(dir.join(NEWEST), &bytes[..bytes.len() / 2]).unwrap();
    let r = verify_last_in(
        &home,
        &data,
        "~/Amara Backups",
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(!r.ok && !r.mismatched.is_empty());
}

#[test]
fn verify_refuses_a_symlinked_archive() {
    let (home, data) = fake_home("vsym");
    let dir = home.join("Amara Backups");
    fs::create_dir_all(&dir).unwrap();
    let real = home.join("elsewhere.tar.gz");
    make_archive(&real, &[("manifest.json", b"{}")]);
    std::os::unix::fs::symlink(&real, dir.join(NEWEST)).unwrap();
    let before = snapshot(&dir);
    assert_eq!(
        verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        "symlink_refused"
    );
    assert_eq!(snapshot(&dir), before);
}

// ---- verify hardening ----

fn vopen(home: &Path, data: &Path) -> Result<Option<VerifyReport>, String> {
    verify_last_in(
        home,
        data,
        "~/Amara Backups",
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
}

fn setup(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (home, data) = fake_home(tag);
    let dir = home.join("Amara Backups");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(NEWEST);
    (home, data, p)
}

#[test]
fn verify_is_refused_while_a_backup_runs_or_another_verify_is_running() {
    let (home, data, p) = setup("vbusy");
    make_archive(&p, &[("manifest.json", b"{}")]);
    let on = AtomicBool::new(true);
    let off = AtomicBool::new(false);
    assert_eq!(
        verify_last_in(&home, &data, "~/Amara Backups", &on, &off).unwrap_err(),
        "backup_running"
    );
    assert_eq!(
        verify_last_in(&home, &data, "~/Amara Backups", &off, &on).unwrap_err(),
        "busy"
    );
    assert!(
        on.load(Ordering::SeqCst),
        "a refused call must not release the other flag"
    );
    assert!(vopen(&home, &data).unwrap().unwrap().ok);
}

#[test]
fn verify_checks_the_gzip_trailer() {
    let (home, data, p) = setup("vtrailer");
    make_archive(&p, &[("manifest.json", b"{}"), ("goals.json", b"[1]")]);
    let mut b = fs::read(&p).unwrap();
    let n = b.len();
    b[n - 8] ^= 0xff; // CRC32 in the trailer
    fs::write(&p, &b).unwrap();
    let r = vopen(&home, &data).unwrap().unwrap();
    assert!(!r.ok, "{r:?}");
    assert!(r.mismatched.contains(&"archive_unreadable".to_string()));
}

#[test]
fn verify_aborts_on_streamed_size_ratio_deadline_and_entry_caps() {
    let (home, data, p) = setup("vcaps");
    make_archive(
        &p,
        &[("manifest.json", b"{}"), ("big.bin", &vec![0u8; 1 << 20])],
    );
    let go = |caps: VerifyCaps| verify_archive(&p, NEWEST, String::new(), caps);
    let loose = VerifyCaps {
        entries: 100,
        total: u64::MAX,
        ratio_floor: u64::MAX,
        deadline: std::time::Duration::from_secs(60),
    };
    assert!(go(loose).unwrap().ok);
    assert_eq!(
        go(VerifyCaps {
            total: 1000,
            ..loose
        })
        .unwrap_err(),
        "archive_too_large"
    );
    assert_eq!(
        go(VerifyCaps {
            ratio_floor: 1000,
            ..loose
        })
        .unwrap_err(),
        "compression_ratio"
    );
    assert_eq!(
        go(VerifyCaps {
            deadline: std::time::Duration::ZERO,
            ..loose
        })
        .unwrap_err(),
        "verify_timeout"
    );
    assert_eq!(
        go(VerifyCaps {
            entries: 1,
            ..loose
        })
        .unwrap_err(),
        "too_many_entries"
    );
    let _ = (home, data);
}

#[test]
fn verify_counts_directories_toward_the_entry_cap() {
    let (_h, _d, p) = setup("vdirs");
    let gz =
        flate2::write::GzEncoder::new(fs::File::create(&p).unwrap(), flate2::Compression::fast());
    let mut b = tar::Builder::new(gz);
    for i in 0..5 {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Directory);
        h.set_size(0);
        h.set_mode(0o700);
        h.set_cksum();
        b.append_data(&mut h, format!("amara-pack-1/d{i}/"), &b""[..])
            .unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
    let caps = VerifyCaps {
        entries: 3,
        ..VERIFY_CAPS
    };
    assert_eq!(
        verify_archive(&p, NEWEST, String::new(), caps).unwrap_err(),
        "too_many_entries"
    );
}

#[test]
fn verify_treats_a_bad_manifest_as_broken_not_structure_mode() {
    let hex = sha_hex(b"[1]");
    type Case = (&'static str, Vec<(&'static str, Vec<u8>)>);
    let cases: Vec<Case> = vec![
        ("unparseable", vec![("manifest.json", b"{nope".to_vec())]),
        ("not_object", vec![("manifest.json", b"[1]".to_vec())]),
        (
            "bad_hash",
            vec![("manifest.json", b"{\"sha256\":{\"a\":\"zz\"}}".to_vec())],
        ),
        (
            "bad_map",
            vec![("manifest.json", b"{\"sha256\":5}".to_vec())],
        ),
        (
            "duplicate",
            vec![
                ("manifest.json", b"{}".to_vec()),
                ("manifest.json", b"{}".to_vec()),
            ],
        ),
        (
            "oversized",
            vec![("manifest.json", {
                let mut v = b"{\"x\":\"".to_vec();
                v.extend(vec![b'a'; (VERIFY_MAX_MANIFEST_BYTES as usize) + 10]);
                v.extend(b"\"}");
                v
            })],
        ),
        (
            "missing_listed",
            vec![(
                "manifest.json",
                format!("{{\"sha256\":{{\"gone.json\":\"{hex}\"}}}}").into_bytes(),
            )],
        ),
    ];
    for (tag, files) in cases {
        let (home, data, p) = setup(&format!("vman-{tag}"));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        make_archive(&p, &refs);
        let r = vopen(&home, &data).unwrap().unwrap();
        assert!(!r.ok, "{tag}: {r:?}");
        if tag == "missing_listed" {
            assert_eq!(r.mismatched, vec!["missing: gone.json".to_string()]);
        } else {
            assert!(
                r.mismatched.contains(&"manifest_invalid".to_string()),
                "{tag}: {r:?}"
            );
        }
    }
    let (home, data, p) = setup("vman-none");
    make_archive(&p, &[("goals.json", b"[1]")]);
    let r = vopen(&home, &data).unwrap().unwrap();
    assert!(!r.ok && r.mismatched == vec!["manifest_missing".to_string()]);
}

#[test]
fn returned_names_are_short_and_free_of_control_and_bidi_characters() {
    let dirty = format!("a\u{202E}b\u{200B}c\nd\u{0}{}", "x".repeat(500));
    let c = clean_name(&dirty);
    assert!(c.starts_with("abcd") && c.chars().count() == 200);
    assert!(!c
        .chars()
        .any(|ch| ch.is_control() || ch == '\u{202E}' || ch == '\u{200B}'));
    let (home, data, p) = setup("vnames");
    make_archive(&p, &[("manifest.json", b"{}"), ("tok\u{202E}en.txt", b"x")]);
    let r = vopen(&home, &data).unwrap().unwrap();
    assert_eq!(r.mismatched, vec!["amara-pack-1/token.txt".to_string()]);
}

#[tokio::test]
async fn a_fresh_pack_verifies_in_sha256_mode_and_a_tampered_one_does_not() {
    let (home, data) = fake_home("vpack");
    fs::write(data.join("notes.txt"), "hello").unwrap();
    let busy = AtomicBool::new(false);
    let r = run_backup(
        &home,
        &data,
        &BackupSettings::default(),
        PLENTY,
        &quiet(),
        &busy,
    )
    .await
    .unwrap();
    let v = vopen(&home, &data).unwrap().unwrap();
    assert_eq!(v.file, r.entry.id);
    assert!(v.ok && v.mode == "sha256" && v.checked >= 3, "{v:?}");
    // Rewrite the archive with one entry's bytes changed and the manifest left alone.
    let path = home.join("Amara Backups").join(&r.entry.id);
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&path).unwrap()));
    let mut items: Vec<(String, Vec<u8>)> = Vec::new();
    for e in a.entries().unwrap() {
        let mut e = e.unwrap();
        if e.header().entry_type().is_file() {
            let mut b = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut b).unwrap();
            if e.path().unwrap().ends_with("goals.json") {
                b = b"[666]".to_vec();
            }
            items.push((e.path().unwrap().to_string_lossy().into_owned(), b));
        }
    }
    let gz = flate2::write::GzEncoder::new(
        fs::File::create(&path).unwrap(),
        flate2::Compression::fast(),
    );
    let mut b = tar::Builder::new(gz);
    for (n, d) in &items {
        let mut h = tar::Header::new_gnu();
        h.set_size(d.len() as u64);
        h.set_mode(0o600);
        h.set_cksum();
        b.append_data(&mut h, n, d.as_slice()).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
    let v = vopen(&home, &data).unwrap().unwrap();
    assert!(
        !v.ok && v.mismatched.contains(&"goals.json".to_string()),
        "{v:?}"
    );
}

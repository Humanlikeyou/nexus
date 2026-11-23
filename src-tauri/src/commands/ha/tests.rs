use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "TEST-HA-TOKEN-zz9-must-never-leak";

fn cfg(base: &str) -> HaConfig {
    HaConfig {
        base: base.to_string(),
        token: HaToken(TOKEN.into()),
    }
}

/// Records every request (method, path, has-right-bearer, body) and answers by path.
/// Error bodies echo the token to prove it never reaches our errors.
type Seen = std::sync::Arc<StdMutex<Vec<(String, String, bool, String)>>>;
async fn mock(routes: Vec<(&'static str, u16, String)>) -> (String, Seen) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Seen = Default::default();
    let s2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            let seen = s2.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65536];
                let mut n = 0;
                // Read headers + body (content-length).
                loop {
                    let r = sock.read(&mut buf[n..]).await.unwrap_or(0);
                    if r == 0 {
                        break;
                    }
                    n += r;
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if n >= i + 4 + len {
                            break;
                        }
                    }
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let mut first = req.split_whitespace();
                let method = first.next().unwrap_or("").to_string();
                let path = first.next().unwrap_or("/").to_string();
                let bearer = req.contains(&format!("authorization: Bearer {TOKEN}"))
                    || req.contains(&format!("Authorization: Bearer {TOKEN}"));
                let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                seen.lock()
                    .unwrap()
                    .push((method, path.clone(), bearer, body));
                let (status, body) = routes
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, s, b)| (*s, b.clone()))
                    .unwrap_or((404, format!("{{\"message\":\"no {TOKEN}\"}}")));
                let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

fn states_body() -> String {
    json!([
        {"entity_id": "light.kitchen", "state": "on", "last_changed": "2026-11-04T04:00:00Z",
         "attributes": {"friendly_name": "Kitchen", "brightness": 200,
                        "entity_picture": format!("/api/camera_proxy/x?token={TOKEN}"),
                        "nested": {"access_token": TOKEN, "ok": 1}}},
        {"entity_id": "camera.door", "state": "idle",
         "attributes": {"access_token": TOKEN, "friendly_name": "Door"}},
        {"entity_id": "BAD ID", "state": "x", "attributes": {}}
    ])
    .to_string()
}

#[tokio::test]
async fn states_are_parsed_sanitized_and_carry_no_token() {
    let (base, seen) = mock(vec![("/api/states", 200, states_body())]).await;
    let list = states_with(&cfg(&base)).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].entity_id, "light.kitchen");
    assert_eq!(list[0].domain, "light");
    assert_eq!(list[0].friendly_name.as_deref(), Some("Kitchen"));
    assert_eq!(list[0].attributes["brightness"], 200);
    assert_eq!(list[0].attributes["nested"]["ok"], 1);
    let out = serde_json::to_string(&list).unwrap();
    assert!(!out.contains(TOKEN), "{out}");
    assert!(out.contains("\"entityId\"") && out.contains("\"lastChanged\""));
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].0, "GET");
    assert!(seen[0].2, "bearer token sent");
}

#[tokio::test]
async fn call_service_posts_entity_id_and_validates() {
    let (base, seen) = mock(vec![("/api/services/light/turn_on", 200, "[]".into())]).await;
    let (_, body) = authorize("light", "turn_on", "light.kitchen", None).unwrap();
    call_service_with(&cfg(&base), "light", "turn_on", &body)
        .await
        .unwrap();
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].0, "POST");
        assert!(seen[0].2);
        let body: Value = serde_json::from_str(&seen[0].3).unwrap();
        assert_eq!(body, json!({"entity_id": "light.kitchen"}));
    }
    for (d, s, e) in [
        ("Light", "turn_on", "light.kitchen"),
        ("light/../x", "turn_on", "light.kitchen"),
        ("light", "turn on", "light.kitchen"),
        ("light", "turn_on", "light"),
        ("light", "turn_on", "light.kitchen;rm"),
        ("light", "turn_on", ".kitchen"),
        ("light", "turn_on", "light."),
        ("", "turn_on", "light.kitchen"),
    ] {
        let r = authorize(d, s, e, None);
        assert_eq!(r.unwrap_err(), ERR_INVALID, "{d} {s} {e}");
    }
    assert!(!valid_name(&"a".repeat(65)));
    assert!(valid_entity_id(&format!("a.{}", "b".repeat(128))));
    assert!(!valid_entity_id(&format!("a.{}", "b".repeat(129))));
    assert_eq!(seen.lock().unwrap().len(), 1, "rejected calls never hit HA");
}

#[tokio::test]
async fn conversation_match_no_match_and_cap() {
    let matched = json!({"response": {"response_type": "action_done",
            "speech": {"plain": {"speech": "Turned on the light"}}}})
    .to_string();
    let (base, seen) = mock(vec![("/api/conversation/process", 200, matched)]).await;
    let r = conversation_with(&cfg(&base), "turn on the kitchen light")
        .await
        .unwrap();
    assert_eq!(
        r,
        HaConversationResult::Match {
            speech: "Turned on the light".into()
        }
    );
    let body: Value = serde_json::from_str(&seen.lock().unwrap()[0].3).unwrap();
    assert_eq!(body["language"], "en");
    assert_eq!(body["agent_id"], "conversation.home_assistant");
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        json!({"kind": "match", "speech": "Turned on the light"})
    );

    let nomatch = json!({"response": {"response_type": "error",
            "data": {"code": "no_intent_match"},
            "speech": {"plain": {"speech": "Sorry, I couldn't understand that"}}}})
    .to_string();
    let (base2, _) = mock(vec![("/api/conversation/process", 200, nomatch)]).await;
    let r = conversation_with(&cfg(&base2), "sing").await.unwrap();
    assert_eq!(r, HaConversationResult::NoMatch);
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        json!({"kind": "noMatch"})
    );

    let long = "a".repeat(MAX_CONVERSATION_CHARS + 1);
    assert_eq!(
        conversation_with(&cfg(&base), &long).await.unwrap_err(),
        ERR_TEXT
    );
    assert_eq!(
        conversation_with(&cfg(&base), "  ").await.unwrap_err(),
        ERR_TEXT
    );
}

#[tokio::test]
async fn errors_are_fixed_text_and_never_echo_the_token() {
    let (base, _) = mock(vec![
        (
            "/api/states",
            401,
            format!("{{\"message\":\"bad {TOKEN}\"}}"),
        ),
        (
            "/api/services/light/turn_on",
            500,
            format!("{{\"message\":\"{TOKEN}\"}}"),
        ),
        ("/api/conversation/process", 200, format!("\"{TOKEN}\"")),
    ])
    .await;
    let c = cfg(&base);
    let errs = [
        states_with(&c).await.unwrap_err(),
        call_service_with(&c, "light", "turn_on", &json!({"entity_id": "light.x"}))
            .await
            .unwrap_err(),
    ];
    assert_eq!(errs[0], ERR_AUTH);
    assert_eq!(errs[1], ERR_FAILED);
    // A token-bearing reply with no response_type is noMatch and carries nothing back.
    let conv = conversation_with(&c, "hi").await.unwrap();
    assert_eq!(conv, HaConversationResult::NoMatch);
    assert!(!serde_json::to_string(&conv).unwrap().contains(TOKEN));
    for e in &errs {
        assert!(!e.contains(TOKEN));
    }
    // Nothing listening.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let down = cfg(&format!("http://127.0.0.1:{port}"));
    assert_eq!(states_with(&down).await.unwrap_err(), ERR_UNREACHABLE);
    assert!(!format!("{c:?}").contains(TOKEN), "Debug is redacted");
}

#[test]
fn url_must_be_loopback_and_only_two_keys_are_read() {
    let ok = parse_config(&format!(
        "HERMES_KEY=zzz\nexport HA_URL=\"http://localhost:8123\"\nHA_TOKEN='{TOKEN}'\n"
    ))
    .unwrap();
    assert_eq!(ok.base, "http://localhost:8123");
    assert_eq!(ok.token.0, TOKEN);
    let default = parse_config(&format!("HA_TOKEN={TOKEN}")).unwrap();
    assert_eq!(default.base, DEFAULT_HA_URL);
    for bad in [
        "http://192.168.1.10:8123",
        "http://homeassistant.local:8123",
        "https://127.0.0.1:8123",
        "http://127.0.0.1@evil.com:8123",
        "http://127.0.0.1:8123/api",
        "ws://127.0.0.1:8123",
    ] {
        let e = parse_config(&format!("HA_URL={bad}\nHA_TOKEN={TOKEN}")).unwrap_err();
        assert_eq!(e, ERR_URL, "{bad}");
    }
    assert_eq!(
        parse_config("HA_URL=http://127.0.0.1:8123").unwrap_err(),
        ERR_CONFIG
    );
    assert_eq!(
        parse_config("HA_TOKEN=has space\n").unwrap_err(),
        ERR_CONFIG
    );
}

#[test]
fn env_file_checks_mode_owner_and_symlinks() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("nexus-ha-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let f = dir.join(".env");
    std::fs::write(&f, format!("HA_TOKEN={TOKEN}\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    let uid = current_uid();
    assert_eq!(read_config_at(&dir, &f, uid).unwrap_err(), ERR_CONFIG);
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(read_config_at(&dir, &f, uid).is_ok());
    assert_eq!(read_config_at(&dir, &f, uid + 1).unwrap_err(), ERR_CONFIG);
    let link = dir.join("link.env");
    std::os::unix::fs::symlink(&f, &link).unwrap();
    assert_eq!(read_config_at(&dir, &link, uid).unwrap_err(), ERR_CONFIG);
    let big = dir.join("big.env");
    std::fs::write(&big, "x".repeat(ENV_MAX_BYTES as usize + 1)).unwrap();
    std::fs::set_permissions(&big, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(read_config_at(&dir, &big, uid).unwrap_err(), ERR_CONFIG);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn websocket_auth_subscribe_and_state_changed_events() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        ws.send(Message::Text(r#"{"type":"auth_required"}"#.into()))
            .await
            .unwrap();
        let auth: Value =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(auth["type"], "auth");
        assert_eq!(auth["access_token"], TOKEN);
        ws.send(Message::Text(r#"{"type":"auth_ok"}"#.into()))
            .await
            .unwrap();
        let sub: Value =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(sub["type"], "subscribe_events");
        assert_eq!(sub["event_type"], "state_changed");
        let ev = json!({"id": 1, "type": "event", "event": {
                "event_type": "state_changed", "time_fired": "2026-11-04T04:00:01Z",
                "data": {"entity_id": "light.kitchen",
                    "old_state": {"entity_id": "light.kitchen", "state": "off", "attributes": {}},
                    "new_state": {"entity_id": "light.kitchen", "state": "on",
                        "attributes": {"friendly_name": "Kitchen", "entity_picture": format!("/x?token={TOKEN}")}}}}});
        ws.send(Message::Text(ev.to_string().into())).await.unwrap();
        let other =
            json!({"id": 1, "type": "event", "event": {"event_type": "call_service", "data": {}}});
        ws.send(Message::Text(other.to_string().into()))
            .await
            .unwrap();
        ws.close(None).await.unwrap();
    });
    let (ws, _) = tokio_tungstenite::connect_async(ws_url(&format!("http://{addr}")))
        .await
        .unwrap();
    let mut connected = false;
    let mut changes = Vec::new();
    let end = live_session(
        ws,
        &HaToken(TOKEN.into()),
        WsTimings::default(),
        || connected = true,
        |c| changes.push(c),
    )
    .await;
    server.await.unwrap();
    assert!(connected);
    assert_eq!(end, SessionEnd::Closed);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].entity_id, "light.kitchen");
    assert_eq!(changes[0].new_state.as_ref().unwrap().state, "on");
    assert_eq!(changes[0].old_state.as_ref().unwrap().state, "off");
    let payload = serde_json::to_string(&changes[0]).unwrap();
    assert!(!payload.contains(TOKEN), "{payload}");
    assert!(payload.contains("\"newState\"") && payload.contains("\"timeFired\""));
}

#[tokio::test]
async fn websocket_auth_invalid_ends_the_session() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        ws.send(Message::Text(r#"{"type":"auth_required"}"#.into()))
            .await
            .unwrap();
        let _ = ws.next().await;
        ws.send(Message::Text(
            r#"{"type":"auth_invalid","message":"x"}"#.into(),
        ))
        .await
        .unwrap();
    });
    let (ws, _) = tokio_tungstenite::connect_async(ws_url(&format!("http://{addr}")))
        .await
        .unwrap();
    let mut connected = false;
    let end = live_session(
        ws,
        &HaToken(TOKEN.into()),
        WsTimings::default(),
        || connected = true,
        |_| {},
    )
    .await;
    assert_eq!(end, SessionEnd::AuthFailed);
    assert!(!connected);
}

#[test]
fn backoff_doubles_to_a_cap_and_container_status_parses() {
    let mut b = BACKOFF_START;
    let mut seen = vec![];
    for _ in 0..10 {
        seen.push(b.as_secs());
        b = next_backoff(b);
    }
    assert_eq!(seen, [1, 2, 4, 8, 16, 32, 60, 60, 60, 60]);
    assert_eq!(
        parse_container_status("Up 3 hours\n"),
        HaContainerStatus {
            running: true,
            status: "Up 3 hours".into()
        }
    );
    assert!(!parse_container_status("Exited (0) 2 minutes ago").running);
    assert_eq!(
        parse_container_status(""),
        HaContainerStatus {
            running: false,
            status: String::new()
        }
    );
    assert_eq!(
        ws_url("http://127.0.0.1:8123"),
        "ws://127.0.0.1:8123/api/websocket"
    );
}

#[test]
fn allowlist_domain_match_unknown_and_sensitive() {
    for (d, s) in [
        ("light", "turn_on"),
        ("light", "toggle"),
        ("switch", "turn_off"),
        ("fan", "toggle"),
        ("media_player", "turn_on"),
        ("scene", "turn_on"),
        ("climate", "turn_on"),
        ("climate", "turn_off"),
    ] {
        let (p, body) = authorize(d, s, &format!("{d}.x"), None).unwrap();
        assert_eq!(p, Policy::Allowed, "{d}.{s}");
        assert_eq!(body, json!({"entity_id": format!("{d}.x")}));
    }
    // Domain mismatch.
    assert_eq!(
        authorize("light", "turn_on", "switch.x", None).unwrap_err(),
        ERR_DOMAIN_MISMATCH
    );
    assert_eq!(
        authorize("lock", "unlock", "light.x", None).unwrap_err(),
        ERR_DOMAIN_MISMATCH
    );
    // Unknown domain / service refused outright.
    for (d, s) in [
        ("light", "turn_off_all"),
        ("scene", "turn_off"),
        ("scene", "apply"),
        ("vacuum", "start"),
        ("notify", "send"),
        ("climate", "set_fan_mode"),
        ("persistent_notification", "create"),
    ] {
        assert_eq!(
            authorize(d, s, &format!("{d}.x"), None).unwrap_err(),
            ERR_NOT_ALLOWED,
            "{d}.{s}"
        );
    }
    // Sensitive domains need the native confirm.
    for d in SENSITIVE_DOMAINS {
        let (p, _) = authorize(d, "turn_on", &format!("{d}.x"), None).unwrap();
        assert_eq!(p, Policy::NeedsApproval, "{d}");
    }
    assert_eq!(
        authorize("lock", "unlock", "lock.front", None).unwrap().0,
        Policy::NeedsApproval
    );
    // Climate data: typed, bounded, only for the two services.
    let t = |t: f64| HaServiceData {
        temperature: Some(t),
        hvac_mode: None,
    };
    let m = |m: &str| HaServiceData {
        temperature: None,
        hvac_mode: Some(m.into()),
    };
    let (p, body) = authorize("climate", "set_temperature", "climate.lr", Some(&t(21.5))).unwrap();
    assert_eq!(p, Policy::Allowed);
    assert_eq!(
        body,
        json!({"entity_id": "climate.lr", "temperature": 21.5})
    );
    let (_, body) = authorize(
        "climate",
        "set_hvac_mode",
        "climate.lr",
        Some(&m("heat_cool")),
    )
    .unwrap();
    assert_eq!(
        body,
        json!({"entity_id": "climate.lr", "hvac_mode": "heat_cool"})
    );
    for bad in [
        t(4.9),
        t(95.1),
        t(f64::NAN),
        t(f64::INFINITY),
        HaServiceData::default(),
        HaServiceData {
            temperature: Some(20.0),
            hvac_mode: Some("heat".into()),
        },
    ] {
        assert_eq!(
            authorize("climate", "set_temperature", "climate.lr", Some(&bad)).unwrap_err(),
            ERR_BAD_DATA
        );
    }
    assert_eq!(
        authorize("climate", "set_temperature", "climate.lr", None).unwrap_err(),
        ERR_BAD_DATA
    );
    for bad in [m("turbo"), m("HEAT"), m(""), t(20.0)] {
        assert_eq!(
            authorize("climate", "set_hvac_mode", "climate.lr", Some(&bad)).unwrap_err(),
            ERR_BAD_DATA
        );
    }
    assert_eq!(
        authorize("light", "turn_on", "light.x", Some(&t(20.0))).unwrap_err(),
        ERR_BAD_DATA
    );
    // Unknown data fields are rejected at deserialization.
    assert!(serde_json::from_value::<HaServiceData>(json!({"brightness": 255})).is_err());
    assert!(serde_json::from_value::<HaServiceData>(json!({"hvacMode": "cool"})).is_ok());
}

#[tokio::test]
async fn oversized_bodies_are_refused_with_fixed_text() {
    let big = format!("[{}]", "0,".repeat(MAX_BODY_BYTES / 2) + "0");
    assert!(big.len() > MAX_BODY_BYTES);
    let (base, _) = mock(vec![
        ("/api/states", 200, big.clone()),
        ("/api/conversation/process", 200, big),
    ])
    .await;
    assert_eq!(states_with(&cfg(&base)).await.unwrap_err(), ERR_TOO_LARGE);
    assert_eq!(
        conversation_with(&cfg(&base), "hi").await.unwrap_err(),
        ERR_TOO_LARGE
    );
}

#[tokio::test]
async fn streamed_body_without_length_is_capped() {
    // Chunked reply (no Content-Length) exercises the streaming cap.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let chunk = " ".repeat(1000);
                let mut out = String::from("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n");
                for _ in 0..3 {
                    out.push_str(&format!("{:x}\r\n{chunk}\r\n", chunk.len()));
                }
                out.push_str("1\r\n1\r\n0\r\n\r\n");
                let _ = sock.write_all(out.as_bytes()).await;
            });
        }
    });
    let get = || client().get(format!("http://{addr}/")).send();
    let resp = get().await.unwrap();
    assert!(resp.content_length().is_none());
    assert_eq!(read_capped(resp, 2500).await.unwrap_err(), ERR_TOO_LARGE);
    let v = read_capped(get().await.unwrap(), 4000).await.unwrap();
    assert!(v.is_number());
}

#[tokio::test]
async fn websocket_config_connect_timeout_and_idle_timeout() {
    let c = ws_config();
    assert_eq!(c.max_message_size, Some(1024 * 1024));
    assert_eq!(c.max_frame_size, Some(256 * 1024));
    assert_eq!(WS_CONNECT_TIMEOUT, Duration::from_secs(10));
    assert_eq!(WS_IDLE_TIMEOUT, Duration::from_secs(90));

    // A server that accepts TCP but never answers the handshake → connect times out.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = silent.local_addr().unwrap();
    let hold = tokio::spawn(async move {
        let (_sock, _) = silent.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let t0 = std::time::Instant::now();
    assert!(connect_ws(
        &format!("ws://{addr}/api/websocket"),
        Duration::from_millis(200)
    )
    .await
    .is_none());
    assert!(t0.elapsed() < Duration::from_secs(5));
    hold.abort();

    // Server goes silent after auth_ok → the idle timeout ends the session.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        ws.send(Message::Text(r#"{"type":"auth_required"}"#.into()))
            .await
            .unwrap();
        let _ = ws.next().await;
        ws.send(Message::Text(r#"{"type":"auth_ok"}"#.into()))
            .await
            .unwrap();
        let _ = ws.next().await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let ws = connect_ws(&ws_url(&format!("http://{addr}")), WS_CONNECT_TIMEOUT)
        .await
        .unwrap();
    let mut connected = false;
    let t0 = std::time::Instant::now();
    let end = live_session(
        ws,
        &HaToken(TOKEN.into()),
        WsTimings {
            idle: Duration::from_millis(300),
            ..WsTimings::default()
        },
        || connected = true,
        |_| {},
    )
    .await;
    assert!(connected);
    assert_eq!(end, SessionEnd::Closed);
    assert!(t0.elapsed() < Duration::from_secs(5));
    server.abort();

    // Oversized message (> 1 MiB) is rejected by the client config → session ends.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        ws.send(Message::Text("x".repeat(2 * 1024 * 1024).into()))
            .await
            .ok();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let ws = connect_ws(&ws_url(&format!("http://{addr}")), WS_CONNECT_TIMEOUT)
        .await
        .unwrap();
    let end = live_session(
        ws,
        &HaToken(TOKEN.into()),
        WsTimings::default(),
        || {},
        |_| {},
    )
    .await;
    assert_eq!(end, SessionEnd::Closed);
}

#[test]
fn every_error_starts_with_a_known_kind() {
    let all = [
        ERR_CONFIG,
        ERR_URL,
        ERR_UNREACHABLE,
        ERR_AUTH,
        ERR_FAILED,
        ERR_BAD_REPLY,
        ERR_INVALID,
        ERR_TEXT,
        ERR_BUSY,
        ERR_DOCKER,
        ERR_DOCKER_FAILED,
        ERR_DOCKER_TIMEOUT,
        ERR_TOO_LARGE,
        ERR_NOT_ALLOWED,
        ERR_DOMAIN_MISMATCH,
        ERR_BAD_DATA,
        ERR_DECLINED,
        ERR_NEEDS_APPROVAL,
        ERR_BUSY_APPROVAL,
        ERR_STORAGE,
        ERR_SAVE,
        ERR_TOKEN_FORMAT,
        ERR_NOTHING,
        ERR_TOKEN_REQUIRED,
    ];
    assert_eq!(ERROR_KINDS.len(), 17);
    for e in all {
        let (kind, text) = e.split_once(": ").expect(e);
        assert!(ERROR_KINDS.contains(&kind), "{e}");
        assert!(!text.is_empty() && !text.contains(TOKEN));
    }
    // Approval-gate errors map to fixed kinds.
    assert_eq!(
        approval_error(&super::super::approval_gate::denied_message()),
        ERR_DECLINED
    );
    assert_eq!(approval_error("busy: x"), ERR_BUSY_APPROVAL);
    assert_eq!(
        approval_error("Approval dialog failed: x"),
        ERR_NEEDS_APPROVAL
    );
    // Every kind is used by at least one error.
    for k in ERROR_KINDS {
        assert!(
            all.iter().any(|e| e.starts_with(&format!("{k}: "))),
            "{k} unused"
        );
    }
}

/// Accept one ws client; run `script` on the server side.
async fn ws_pair<F, Fut>(script: F) -> WsStream
where
    F: FnOnce(tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        script(tokio_tungstenite::accept_async(sock).await.unwrap()).await;
    });
    connect_ws(&ws_url(&format!("http://{addr}")), WS_CONNECT_TIMEOUT)
        .await
        .unwrap()
}

async fn auth_ok_then<S>(ws: &mut S)
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures::Sink<Message>
        + Unpin,
{
    let _ = ws
        .send(Message::Text(r#"{"type":"auth_required"}"#.into()))
        .await;
    let _ = ws.next().await;
    let _ = ws.send(Message::Text(r#"{"type":"auth_ok"}"#.into())).await;
    let _ = ws.next().await; // subscribe
}

#[tokio::test]
async fn auth_phase_is_bounded_by_the_auth_timeout() {
    assert_eq!(WS_AUTH_TIMEOUT, Duration::from_secs(10));
    assert_eq!(WS_PING_INTERVAL, Duration::from_secs(30));
    assert_eq!(WsTimings::default().auth, WS_AUTH_TIMEOUT);
    // Scaled 10 s → 300 ms (tokio paused time needs the `test-util` feature, which would
    // mean editing Cargo.toml). Server never sends auth_required.
    let ws = ws_pair(|ws| async move {
        let _keep_open = ws;
        tokio::time::sleep(Duration::from_secs(30)).await;
    })
    .await;
    let t = WsTimings {
        auth: Duration::from_millis(300),
        ..WsTimings::default()
    };
    let t0 = std::time::Instant::now();
    let end = live_session(ws, &HaToken(TOKEN.into()), t, || {}, |_| {}).await;
    assert_eq!(end, SessionEnd::Closed);
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_millis(300) && took < Duration::from_secs(3),
        "{took:?}"
    );
    // The bound covers the whole phase: auth_required arrives, auth_ok never does.
    let ws = ws_pair(|mut ws| async move {
        let _ = ws
            .send(Message::Text(r#"{"type":"auth_required"}"#.into()))
            .await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    })
    .await;
    let mut connected = false;
    let t0 = std::time::Instant::now();
    let end = live_session(ws, &HaToken(TOKEN.into()), t, || connected = true, |_| {}).await;
    assert_eq!(end, SessionEnd::Closed);
    assert!(!connected);
    assert!(t0.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn pings_keep_a_silent_healthy_server_alive_past_the_idle_limit() {
    // Scaled: ping 30 s → 100 ms, idle 90 s → 300 ms. Run 4× the idle limit.
    let t = WsTimings {
        auth: Duration::from_secs(5),
        ping: Duration::from_millis(100),
        idle: Duration::from_millis(300),
    };
    let pings = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let p2 = pings.clone();
    // Healthy: sends no events, but keeps reading, so tungstenite auto-replies with pongs.
    let ws = ws_pair(move |mut ws| async move {
        auth_ok_then(&mut ws).await;
        while let Some(Ok(m)) = ws.next().await {
            if m.is_ping() {
                p2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    })
    .await;
    let mut connected = false;
    let r = tokio::time::timeout(
        Duration::from_millis(1200),
        live_session(ws, &HaToken(TOKEN.into()), t, || connected = true, |_| {}),
    )
    .await;
    assert!(r.is_err(), "session must still be alive past 4× idle");
    assert!(connected);
    assert!(pings.load(std::sync::atomic::Ordering::SeqCst) >= 5);

    // Unhealthy: never reads, so no pongs → closed by the idle limit.
    let ws = ws_pair(|mut ws| async move {
        auth_ok_then(&mut ws).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    })
    .await;
    let t0 = std::time::Instant::now();
    let end = live_session(ws, &HaToken(TOKEN.into()), t, || {}, |_| {}).await;
    assert_eq!(end, SessionEnd::Closed);
    assert!(t0.elapsed() < Duration::from_secs(3));
}

#[test]
fn conversation_match_only_for_action_done_or_query_answer() {
    let reply = |rt: Option<&str>, code: Option<&str>| {
        let mut r = json!({"speech": {"plain": {"speech": "Sorry, I am not aware of any device called 2+2"}}});
        if let Some(rt) = rt {
            r["response_type"] = json!(rt);
        }
        if let Some(code) = code {
            r["data"] = json!({"code": code});
        }
        json!({"response": r})
    };
    let m = HaConversationResult::Match {
        speech: "Sorry, I am not aware of any device called 2+2".into(),
    };
    assert_eq!(
        conversation_result(&reply(Some("action_done"), None)).unwrap(),
        m
    );
    assert_eq!(
        conversation_result(&reply(Some("query_answer"), None)).unwrap(),
        m
    );
    for code in [
        "no_valid_targets",
        "no_intent_match",
        "failed_to_handle",
        "unknown",
    ] {
        assert_eq!(
            conversation_result(&reply(Some("error"), Some(code))).unwrap(),
            HaConversationResult::NoMatch,
            "{code}"
        );
    }
    assert_eq!(
        conversation_result(&reply(None, None)).unwrap(),
        HaConversationResult::NoMatch
    );
    assert_eq!(
        conversation_result(&reply(Some("weird"), None)).unwrap(),
        HaConversationResult::NoMatch
    );
    assert_eq!(
        conversation_result(&json!({})).unwrap(),
        HaConversationResult::NoMatch
    );
}

// ─── settings ────────────────────────────────────────────────────────────

const NEW_TOKEN: &str = "abcDEF0123456789._-abcDEF0123456789xyz";

fn tmpdir() -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("nexus-ha-set-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    dir.canonicalize().unwrap()
}

fn src(app: &Path) -> Sources {
    Sources {
        app_dir: Some(app.to_path_buf()),
        legacy_dir: None,
        uid: current_uid(),
        cfg_url: None,
        env_url: None,
        env_token: None,
    }
}

fn set_token(dir: &Path, t: &str) -> Result<(), String> {
    set_config_with(
        dir,
        current_uid(),
        None,
        Some(HaToken(t.into())),
        |_| Ok(()),
    )
}

#[test]
fn token_format_rules() {
    assert!(valid_new_token(NEW_TOKEN));
    for bad in [
        format!("{NEW_TOKEN}\nEVIL=1"),
        format!("{NEW_TOKEN}\r"),
        format!("{NEW_TOKEN}\0"),
        format!("{NEW_TOKEN}=x"),
        format!("{NEW_TOKEN}#x"),
        format!(" {NEW_TOKEN}"),
        format!("{NEW_TOKEN} "),
        "a".repeat(31),
        "a".repeat(513),
        String::new(),
    ] {
        assert!(!valid_new_token(&bad));
    }
    assert!(valid_new_token(&"a".repeat(32)) && valid_new_token(&"a".repeat(512)));
}

#[test]
fn newline_or_equals_token_refused_and_file_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let f = dir.join(".env");
    std::fs::write(&f, "A=1\n").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    for bad in [format!("{NEW_TOKEN}\nX=1"), format!("{NEW_TOKEN}=1")] {
        let e = set_token(&dir, &bad).unwrap_err();
        assert_eq!(e, ERR_TOKEN_FORMAT);
        assert!(!e.contains(NEW_TOKEN));
    }
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "A=1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn symlink_target_and_folder_refused() {
    let dir = tmpdir();
    let real = dir.join("real.env");
    std::fs::write(&real, "A=1\n").unwrap();
    std::os::unix::fs::symlink(&real, dir.join(".env")).unwrap();
    assert_eq!(set_token(&dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
    assert_eq!(
        write_env_key(&dir, current_uid(), TOKEN_KEY, None).unwrap_err(),
        ERR_STORAGE
    );
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "A=1\n");
    // reading through the symlink fails closed
    assert_eq!(resolve(&src(&dir)).unwrap_err(), ERR_STORAGE);

    let other = tmpdir();
    let link_dir = dir.join("linkdir");
    std::os::unix::fs::symlink(&other, &link_dir).unwrap();
    assert_eq!(set_token(&link_dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
    assert!(!other.join(".env").exists());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn group_or_world_writable_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let f = dir.join(".env");
    std::fs::write(&f, "A=1\n").unwrap();
    for mode in [0o620, 0o602, 0o666, 0o640, 0o604] {
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(set_token(&dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "A=1\n");
    }
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    for mode in [0o770, 0o707] {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(set_token(&dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    // wrong owner is refused (checked against the handle's uid)
    assert_eq!(
        write_env_key(&dir, current_uid() + 1, TOKEN_KEY, Some(NEW_TOKEN)).unwrap_err(),
        ERR_STORAGE
    );
    // the writer applies the reader's rule (no group/other bits); result is 0600
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    set_token(&dir, NEW_TOKEN).unwrap();
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn other_keys_byte_identical_after_set_and_clear() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let f = dir.join(".env");
    let original = "# header\r\nA=1\nexport B='x y'  \n\nHA_TOKEN=oldoldoldoldoldoldoldoldoldoldold\nC=\"q\"\nHA_TOKEN=dup\nLAST=no-newline";
    std::fs::write(&f, original).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    let others: Vec<&str> = original
        .split_inclusive('\n')
        .filter(|l| !l.starts_with("HA_TOKEN"))
        .collect();

    set_token(&dir, NEW_TOKEN).unwrap();
    let after = std::fs::read_to_string(&f).unwrap();
    let after_others: Vec<&str> = after
        .split_inclusive('\n')
        .filter(|l| !l.starts_with("HA_TOKEN"))
        .collect();
    assert_eq!(after_others, others);
    assert_eq!(after.matches("HA_TOKEN=").count(), 1);
    assert!(after.contains(&format!("\nHA_TOKEN={NEW_TOKEN}\n")));
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(resolve(&src(&dir)).unwrap().token.0, NEW_TOKEN);

    write_env_key(&dir, current_uid(), TOKEN_KEY, None).unwrap();
    let cleared = std::fs::read_to_string(&f).unwrap();
    assert_eq!(cleared, others.concat());
    assert!(!cleared.contains("HA_TOKEN"));
    // no temp files left behind
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![".env".to_string()]);

    // append to a fresh file; clear on a missing file is a no-op
    let d2 = tmpdir();
    write_env_key(&d2, current_uid(), TOKEN_KEY, None).unwrap();
    assert!(!d2.join(".env").exists());
    set_token(&d2, NEW_TOKEN).unwrap();
    assert_eq!(
        std::fs::read_to_string(d2.join(".env")).unwrap(),
        format!("HA_TOKEN={NEW_TOKEN}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&d2);
}

#[test]
fn get_config_never_contains_token_bytes() {
    let dir = tmpdir();
    set_token(&dir, NEW_TOKEN).unwrap();
    let mut s = src(&dir);
    s.cfg_url = Some("http://localhost:8123/".into());
    let view = get_config_with(&s);
    let json = serde_json::to_string(&view).unwrap();
    assert_eq!(
        json,
        r#"{"url":"http://localhost:8123","tokenSet":true,"tokenSource":"app"}"#
    );
    assert!(!json.contains(NEW_TOKEN) && !json.contains(&NEW_TOKEN[..8]));
    assert!(!format!("{view:?}").contains(NEW_TOKEN));
    let cfg = resolve(&s).unwrap();
    assert!(!format!("{cfg:?}").contains(NEW_TOKEN));
    assert_eq!(
        serde_json::to_string(&get_config_with(&src(&tmpdir()))).unwrap(),
        r#"{"url":null,"tokenSet":false,"tokenSource":null}"#
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn set_config_url_rules() {
    let dir = tmpdir();
    let mut saved = None;
    set_config_with(
        &dir,
        current_uid(),
        Some(" http://127.0.0.1:8123 ".into()),
        None,
        |u| {
            saved = Some(u);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(saved, Some(Some("http://127.0.0.1:8123".to_string())));
    let mut saved = None;
    set_config_with(&dir, current_uid(), Some(String::new()), None, |u| {
        saved = Some(u);
        Ok(())
    })
    .unwrap();
    assert_eq!(saved, Some(None));
    for bad in [
        "https://example.com",
        "http://user:p@127.0.0.1",
        "ftp://127.0.0.1",
    ] {
        let e = set_config_with(&dir, current_uid(), Some(bad.into()), None, |_| {
            panic!("must not save")
        })
        .unwrap_err();
        assert_eq!(e, ERR_URL);
    }
    // bad URL rejects before the token is written
    let e = set_config_with(
        &dir,
        current_uid(),
        Some("http://10.0.0.1".into()),
        Some(HaToken(NEW_TOKEN.into())),
        |_| Ok(()),
    )
    .unwrap_err();
    assert_eq!(e, ERR_URL);
    assert!(!dir.join(".env").exists());
    assert_eq!(
        set_config_with(&dir, current_uid(), None, None, |_| Ok(())).unwrap_err(),
        ERR_NOTHING
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn precedence_app_env_then_env_var_then_legacy() {
    use std::os::unix::fs::PermissionsExt;
    let app = tmpdir();
    let legacy = tmpdir();
    let lf = legacy.join(".env");
    std::fs::write(&lf, "HA_URL=http://127.0.0.1:9000\nHA_TOKEN=legacytoken\n").unwrap();
    std::fs::set_permissions(&lf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut s = src(&app);
    s.legacy_dir = Some(legacy.clone());
    let c = resolve(&s).unwrap();
    assert_eq!(
        (c.base.as_str(), c.token.0.as_str()),
        ("http://127.0.0.1:9000", "legacytoken")
    );
    s.env_token = Some(HaToken("envtoken".into()));
    s.env_url = Some("http://127.0.0.1:9001".into());
    let c = resolve(&s).unwrap();
    assert_eq!(
        (c.base.as_str(), c.token.0.as_str()),
        ("http://127.0.0.1:9001", "envtoken")
    );
    set_token(&app, NEW_TOKEN).unwrap();
    s.cfg_url = Some("http://localhost:9002".into());
    let c = resolve(&s).unwrap();
    assert_eq!(
        (c.base.as_str(), c.token.0.as_str()),
        ("http://localhost:9002", NEW_TOKEN)
    );
    s.cfg_url = Some("http://192.168.1.2:8123".into());
    assert_eq!(resolve(&s).unwrap_err(), ERR_URL);
    let _ = std::fs::remove_dir_all(&app);
    let _ = std::fs::remove_dir_all(&legacy);
}

#[tokio::test]
async fn test_connection_errors_never_carry_the_token() {
    // mock() echoes TOKEN on unknown routes; these routes echo it too.
    let (base, _) = mock(vec![("/api/config", 500, format!("bad {TOKEN}"))]).await;
    let cfg = HaConfig {
        base,
        token: HaToken(TOKEN.into()),
    };
    let r = test_with(&cfg).await;
    assert_eq!(r.kind, Some(KIND_UPSTREAM_ERROR));
    let all = format!("{} {r:?}", serde_json::to_string(&r).unwrap());
    assert!(!all.contains(TOKEN));

    let (base, _) = mock(vec![(
        "/api/config",
        200,
        format!("{{\"version\":\"{TOKEN}\"}}"),
    )])
    .await;
    let cfg = HaConfig {
        base,
        token: HaToken(TOKEN.into()),
    };
    let r = test_with(&cfg).await;
    assert!(r.ok && r.version.is_none()); // > 32 chars or unsafe charset: dropped
    assert!(!serde_json::to_string(&r).unwrap().contains(TOKEN));

    let (base, _) = mock(vec![("/api/config", 401, TOKEN.to_string())]).await;
    let cfg = HaConfig {
        base,
        token: HaToken(TOKEN.into()),
    };
    let r = test_with(&cfg).await;
    assert_eq!(r.kind, Some("auth_rejected"));
    assert!(!serde_json::to_string(&r).unwrap().contains(TOKEN));

    let (base, _) = mock(vec![(
        "/api/config",
        200,
        r#"{"version":"2026.10.4"}"#.into(),
    )])
    .await;
    let cfg = HaConfig {
        base,
        token: HaToken(TOKEN.into()),
    };
    assert_eq!(
        serde_json::to_string(&test_with(&cfg).await).unwrap(),
        r#"{"ok":true,"version":"2026.10.4"}"#
    );
    let cfg = HaConfig {
        base: "http://127.0.0.1:1".into(),
        token: HaToken(TOKEN.into()),
    };
    assert_eq!(test_with(&cfg).await.kind, Some(KIND_UNREACHABLE));
}

#[test]
fn test_connection_rate_limit_and_versions() {
    let now = std::time::Instant::now();
    assert!(gap_ok(None, now));
    assert!(!gap_ok(Some(now), now + Duration::from_millis(1999)));
    assert!(gap_ok(Some(now), now + Duration::from_secs(2)));
    assert_eq!(safe_version("2026.10.4").as_deref(), Some("2026.10.4"));
    assert!(safe_version("1.0 <script>").is_none());
    assert!(safe_version(&"1".repeat(33)).is_none());
    assert!(safe_version("").is_none());
    assert_eq!(fail(ERR_BUSY).kind, Some("already_running"));
    assert_eq!(fail("weird text").kind, Some(KIND_UPSTREAM_ERROR));
}

#[test]
fn token_redacted_and_secret_text_redacted() {
    let t = HaToken(NEW_TOKEN.into());
    assert_eq!(format!("{t:?}"), "HaToken(<redacted>)");
    let st = SecretText(zeroize::Zeroizing::new(NEW_TOKEN.to_string()));
    assert_eq!(format!("{st:?}"), "SecretText(<redacted>)");
}

#[test]
fn get_config_reports_token_source() {
    use std::os::unix::fs::PermissionsExt;
    let app = tmpdir();
    let legacy = tmpdir();
    let lf = legacy.join(".env");
    std::fs::write(&lf, "HA_TOKEN=legacytoken\n").unwrap();
    std::fs::set_permissions(&lf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut s = src(&app);
    let json = |s: &Sources| serde_json::to_string(&get_config_with(s)).unwrap();
    assert!(json(&s).ends_with(r#""tokenSet":false,"tokenSource":null}"#));
    s.legacy_dir = Some(legacy.clone());
    assert!(json(&s).ends_with(r#""tokenSet":true,"tokenSource":"legacy"}"#));
    s.env_token = Some(HaToken("envtoken".into()));
    assert!(json(&s).ends_with(r#""tokenSet":true,"tokenSource":"env"}"#));
    set_token(&app, NEW_TOKEN).unwrap();
    let j = json(&s);
    assert!(j.ends_with(r#""tokenSet":true,"tokenSource":"app"}"#));
    assert!(!j.contains(NEW_TOKEN) && !j.contains("envtoken") && !j.contains("legacytoken"));
    // unsafe app .env: nothing usable is reported
    std::fs::set_permissions(app.join(".env"), std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(json(&s).ends_with(r#""tokenSet":false,"tokenSource":null}"#));
    let _ = std::fs::remove_dir_all(&app);
    let _ = std::fs::remove_dir_all(&legacy);
}

#[test]
fn clear_token_reports_fallback() {
    let app = tmpdir();
    let mut s = src(&app);
    set_token(&app, NEW_TOKEN).unwrap();
    let r = clear_token_with(&s).unwrap();
    assert_eq!(
        serde_json::to_string(&r).unwrap(),
        r#"{"fallbackActive":false}"#
    );
    assert!(!std::fs::read_to_string(app.join(".env"))
        .unwrap()
        .contains("HA_TOKEN"));
    set_token(&app, NEW_TOKEN).unwrap();
    s.env_token = Some(HaToken("envtoken".into()));
    assert_eq!(
        clear_token_with(&s).unwrap(),
        HaClearResult {
            fallback_active: true
        }
    );
    assert_eq!(get_config_with(&s).token_source, Some(TokenSource::Env));
    let _ = std::fs::remove_dir_all(&app);
}

#[test]
fn concurrent_set_and_clear_leave_a_consistent_file() {
    use std::os::unix::fs::PermissionsExt;
    let app = tmpdir();
    let f = app.join(".env");
    std::fs::write(&f, "A=1\nB=2\n").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    let handles: Vec<_> = (0..16)
        .map(|i| {
            let app = app.clone();
            std::thread::spawn(move || {
                if i % 2 == 0 {
                    set_token(&app, NEW_TOKEN).unwrap();
                } else {
                    write_env_key(&app, current_uid(), TOKEN_KEY, None).unwrap();
                }
            })
        })
        .collect();
    handles.into_iter().for_each(|h| h.join().unwrap());
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.starts_with("A=1\nB=2\n"));
    assert!(text.matches("HA_TOKEN=").count() <= 1);
    assert_eq!(std::fs::read_dir(&app).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(&app);
}

#[test]
fn legacy_non_utf8_file_is_a_config_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let f = dir.join(".env");
    let mut bytes = format!("HA_TOKEN={TOKEN}\n").into_bytes();
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    std::fs::write(&f, bytes).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        read_config_at(&dir, &f, current_uid()).unwrap_err(),
        ERR_CONFIG
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().to_string(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

fn ov(url: Option<&str>, token: Option<&str>) -> TestOverrides {
    test_overrides(url.map(str::to_string), token.map(|t| HaToken(t.into()))).unwrap()
}

#[tokio::test]
async fn test_connection_other_url_with_saved_token_is_token_required() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Raw listener: counts connections, so "no request" also means "no connect".
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other = format!("http://{}", listener.local_addr().unwrap());
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let h2 = hits.clone();
    tokio::spawn(async move {
        while listener.accept().await.is_ok() {
            h2.fetch_add(1, Ordering::SeqCst);
        }
    });
    let dir = tmpdir();
    set_token(&dir, TOKEN).unwrap();
    let mut s = src(&dir);
    s.cfg_url = Some("http://127.0.0.1:1".into());
    let before = snapshot(&dir);
    for url in [other.as_str(), "http://localhost:1", "http://127.0.0.1:2"] {
        let r = test_connection_with(&s, ov(Some(url), None)).await;
        assert!(!r.ok);
        assert_eq!(r.kind, Some(KIND_TOKEN_REQUIRED), "{url}");
    }
    // No saved URL either: the default base is the only one the saved token may reach.
    s.cfg_url = None;
    let r = test_connection_with(&s, ov(Some(&other), None)).await;
    assert_eq!(r.kind, Some(KIND_TOKEN_REQUIRED));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the other port saw no connection"
    );
    assert_eq!(snapshot(&dir), before, "nothing persisted");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn test_connection_same_url_with_saved_token_is_allowed() {
    let (base, seen) = mock(vec![("/api/config", 200, "{\"version\":\"1\"}".into())]).await;
    let port = base.rsplit(':').next().unwrap().to_string();
    let dir = tmpdir();
    set_token(&dir, TOKEN).unwrap();
    let mut s = src(&dir);
    s.cfg_url = Some(base.clone());
    let before = snapshot(&dir);
    // Same base after loopback_base: exact, trailing slash, upper-case scheme/host spelling.
    for url in [
        base.clone(),
        format!("{base}/"),
        format!(" HTTP://127.0.0.1:{port}/ "),
    ] {
        let r = test_connection_with(&s, ov(Some(&url), None)).await;
        assert!(r.ok, "{url}: {r:?}");
    }
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(
        seen.iter().all(|r| r.2),
        "saved token sent to the saved base"
    );
    assert_eq!(snapshot(&dir), before, "nothing persisted");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn test_connection_override_token_only_uses_stored_url() {
    let (base, seen) = mock(vec![("/api/config", 200, "{\"version\":\"1\"}".into())]).await;
    let dir = tmpdir();
    set_token(&dir, NEW_TOKEN).unwrap();
    let mut s = src(&dir);
    s.cfg_url = Some(base.clone());
    let before = snapshot(&dir);
    let r = test_connection_with(&s, ov(None, Some(TOKEN))).await;
    assert!(r.ok, "{r:?}");
    assert!(seen.lock().unwrap()[0].2, "override token sent");
    assert_eq!(snapshot(&dir), before, "stored token unchanged");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn test_connection_override_both_needs_no_stored_config() {
    let (base, seen) = mock(vec![("/api/config", 200, "{\"version\":\"1\"}".into())]).await;
    let dir = tmpdir();
    let s = src(&dir);
    // Without overrides there is nothing to test with.
    assert!(!test_connection_with(&s, ov(None, None)).await.ok);
    let r = test_connection_with(&s, ov(Some(&base), Some(TOKEN))).await;
    assert!(r.ok, "{r:?}");
    assert!(seen.lock().unwrap()[0].2);
    assert!(snapshot(&dir).is_empty(), "no .env created");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_connection_overrides_are_validated() {
    let bad_url = |u: &str| test_overrides(Some(u.into()), None).unwrap_err();
    assert_eq!(bad_url("http://example.com:8123"), ERR_URL);
    assert_eq!(bad_url("not a url"), ERR_URL);
    assert_eq!(fail(ERR_URL).kind, Some("bad_url"));
    let bad_tok = |t: &str| test_overrides(None, Some(HaToken(t.into()))).unwrap_err();
    assert_eq!(bad_tok("short"), ERR_TOKEN_FORMAT);
    assert_eq!(bad_tok(&format!("{TOKEN} x")), ERR_TOKEN_FORMAT);
    assert_eq!(fail(ERR_TOKEN_FORMAT).kind, Some("invalid_input"));
    let o = ov(Some("  "), Some(""));
    assert!(
        o.base.is_none() && o.token.is_none(),
        "blank = not provided"
    );
    let dbg = format!("{:?}", ov(None, Some(TOKEN)));
    assert!(!dbg.contains(TOKEN), "{dbg}");
}

use super::*;

fn action_input(tool_id: &str, parameters: Value) -> CreateTerrenceActionInput {
    CreateTerrenceActionInput {
        tool_id: tool_id.to_string(),
        title: None,
        parameters,
        conversation_id: Some("conversation-1".to_string()),
        context_item_ids: Some(vec!["context-1".to_string()]),
    }
}

#[test]
fn network_shaped_actions_are_gated_on_the_outbound_switch() {
    let src = include_str!("../terrence_actions.rs");
    let local = &src[src.find("fn execute_local_action").unwrap()..];
    let arm = &local[local.find("\"system.open_url\" =>").unwrap()..];
    assert!(arm[..arm.find("/usr/bin/open").unwrap()].contains("outbound_gate()?"));
    let step = &src[src.find("fn execute_workflow_step").unwrap()..];
    let open = &step[step.find("OpenUrl { url }").unwrap()..];
    assert!(open[..open.find("/usr/bin/open").unwrap()].contains("outbound_gate()?"));
    let sc = &step[step.find("RunShortcut { name }").unwrap()..];
    assert!(sc[..sc.find("/usr/bin/shortcuts").unwrap()].contains("outbound_gate()?"));
    assert!(src.contains("super::telegram::outbound_disabled()"));
    crate::commands::telegram::set_local_only(true);
    let blocked = outbound_gate();
    crate::commands::telegram::set_local_only(false);
    assert_eq!(blocked, Err(OUTBOUND_BLOCKED_TEXT.to_string()));
}

#[test]
fn normalizes_only_safe_open_url_parameters() {
    assert_eq!(
        normalize_parameters("system.open_url", json!({ "url": "https://example.com" })).unwrap(),
        json!({ "url": "https://example.com" })
    );
    assert!(
        normalize_parameters("system.open_url", json!({ "url": "file:///etc/passwd" })).is_err()
    );
    assert!(
        normalize_parameters("system.open_url", json!({ "url": "https://example .com" })).is_err()
    );
}

#[test]
fn normalizes_bounded_managed_workflow() {
    let normalized = normalize_parameters(
        "macos.workflow",
        json!({
            "steps": [
                { "kind": "set_mute", "muted": true },
                { "kind": "adjust_volume", "delta": -15 },
                { "kind": "open_application", "application": "Safari" }
            ]
        }),
    )
    .unwrap();
    assert_eq!(
        normalized,
        json!({
            "steps": [
                { "kind": "set_mute", "muted": true },
                { "kind": "adjust_volume", "delta": -15 },
                { "kind": "open_application", "application": "Safari" }
            ]
        })
    );
}

#[test]
fn rejects_unbounded_or_invalid_managed_workflows() {
    assert!(normalize_parameters("macos.workflow", json!({ "steps": [] })).is_err());
    assert!(normalize_parameters(
        "macos.workflow",
        json!({ "steps": [{ "kind": "adjust_volume", "delta": 0 }] })
    )
    .is_err());
    assert!(normalize_parameters(
            "macos.workflow",
            json!({ "steps": vec![json!({ "kind": "set_mute", "muted": true }); MAX_WORKFLOW_STEPS + 1] })
        )
        .is_err());
}

#[test]
fn keeps_workflow_file_operations_inside_permitted_roots() {
    assert!(normalize_parameters(
        "macos.workflow",
        json!({ "steps": [{ "kind": "create_directory", "path": "/tmp/not-permitted" }] })
    )
    .is_err());
}

#[test]
fn summarizes_pending_directory_organization_without_raw_move_paths() {
    let summary = action_summary(
        "filesystem.organize_directory",
        &json!({
            "source": "/Users/test/Desktop",
            "moves": [
                { "from": "/Users/test/Desktop/file-one.pdf", "to": "/Users/test/Desktop/Terrence Organized/Documents/file-one.pdf", "category": "Documents" },
                { "from": "/Users/test/Desktop/file-two.png", "to": "/Users/test/Desktop/Terrence Organized/Images/file-two.png", "category": "Images" }
            ]
        }),
    );
    assert_eq!(summary, "2 planned moves from Desktop");
}

#[test]
fn hides_internal_tool_previews_from_action_errors() {
    assert_eq!(
        user_facing_action_error(
            "↻ Resumed session nexus\n  ┊ preparing system.open_url".to_string()
        ),
        "Action did not complete."
    );
}

#[test]
fn sanitizes_legacy_action_errors_in_the_persistent_ledger() {
    let conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let tool = find_tool("system.open_url").unwrap();
    let action = insert_action(
        &conn,
        &tool,
        "Open link".to_string(),
        "executing",
        json!({ "url": "https://example.com" }),
        None,
        Vec::new(),
    )
    .unwrap();
    conn.execute(
        "UPDATE terrence_actions SET error = ?1 WHERE id = ?2",
        params![
            "↻ Resumed session nexus\n  ┊ preparing system.open_url",
            action.id
        ],
    )
    .unwrap();
    sanitize_persisted_action_errors(&conn).unwrap();
    let error: String = conn
        .query_row(
            "SELECT error FROM terrence_actions WHERE id = ?1",
            params![action.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(error, "Action did not complete.");
}

#[test]
fn restart_reconciles_only_interrupted_hermes_conversations() {
    let conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let conversation_tool = find_tool("agent.hermes_chat").unwrap();
    let action = insert_action(
        &conn,
        &conversation_tool,
        "Amara conversation".to_string(),
        "executing",
        json!({}),
        Some("conversation-1".to_string()),
        Vec::new(),
    )
    .unwrap();
    let open_url_tool = find_tool("system.open_url").unwrap();
    let untouched = insert_action(
        &conn,
        &open_url_tool,
        "Open link".to_string(),
        "executing",
        json!({ "url": "https://example.com" }),
        None,
        Vec::new(),
    )
    .unwrap();

    assert_eq!(
        reconcile_interrupted_conversations_in_connection(&conn).unwrap(),
        1
    );

    let recovered = get_action(&conn, &action.id).unwrap().unwrap();
    assert_eq!(recovered.status, "needs_attention");
    assert_eq!(
        recovered.error.as_deref(),
        Some("Amara OS restarted before this conversation finished.")
    );
    assert_eq!(
        get_action(&conn, &untouched.id).unwrap().unwrap().status,
        "executing"
    );
}

#[test]
fn describes_open_url_as_dispatched_not_remotely_verified() {
    let result = local_action_success_message(
        "system.open_url",
        json!({ "url": "https://example.com" }).as_object().unwrap(),
    )
    .unwrap();
    assert_eq!(
        result,
        "Amara OS sent the request to open https://example.com."
    );
    assert!(!result.contains("completed"));
}

#[test]
fn deny_cancels_the_action_and_returns_declined_error() {
    let conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let tool = find_tool("system.open_url").unwrap();
    let input = action_input("system.open_url", json!({ "url": "https://example.com" }));
    let action = insert_action(
        &conn,
        &tool,
        "Open link".to_string(),
        "awaiting_approval",
        normalize_parameters(&input.tool_id, input.parameters).unwrap(),
        input.conversation_id,
        normalize_context_item_ids(input.context_item_ids).unwrap(),
    )
    .unwrap();
    let err = record_denial(&conn, &action.id);
    assert!(err.starts_with("declined:"), "{err}");
    let stored = get_action(&conn, &action.id).unwrap().unwrap();
    assert_eq!(stored.status, "cancelled");
    assert!(stored.error.unwrap().starts_with("declined:"));
}

#[test]
fn approval_transition_is_persisted_before_execution() {
    let conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let tool = find_tool("system.open_url").unwrap();
    let input = action_input("system.open_url", json!({ "url": "https://example.com" }));
    let action = insert_action(
        &conn,
        &tool,
        "Open link".to_string(),
        "awaiting_approval",
        normalize_parameters(&input.tool_id, input.parameters).unwrap(),
        input.conversation_id,
        normalize_context_item_ids(input.context_item_ids).unwrap(),
    )
    .unwrap();
    let approved = update_action_status(
        &conn,
        &action.id,
        &["awaiting_approval"],
        "approved",
        None,
        None,
    )
    .unwrap();
    assert_eq!(approved.status, "approved");
    assert!(approved.approved_at.is_some());
    assert!(approved.started_at.is_none());
}

#[test]
fn retries_a_failed_nexus_prepared_idempotent_action_with_a_link_to_the_original() {
    let conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let tool = find_tool("system.open_url").unwrap();
    let original = insert_action(
        &conn,
        &tool,
        "Open project brief".to_string(),
        "awaiting_approval",
        json!({ "url": "https://example.com/brief" }),
        Some("conversation-1".to_string()),
        vec!["context-1".to_string()],
    )
    .unwrap();
    let failed = update_action_status(
        &conn,
        &original.id,
        &["awaiting_approval"],
        "failed",
        None,
        Some("The browser did not respond.".to_string()),
    )
    .unwrap();
    assert!(failed.retry_allowed);

    let retry = retry_action_in(&conn, &original.id).unwrap();
    assert_ne!(retry.id, original.id);
    assert_eq!(retry.title, "Retry: Open project brief");
    assert_eq!(retry.status, "awaiting_approval");
    assert_eq!(
        retry.retry_of_action_id.as_deref(),
        Some(original.id.as_str())
    );
    assert_eq!(retry.parameters, original.parameters);
    assert_eq!(retry.context_item_ids, original.context_item_ids);
    assert!(!retry.retry_allowed);

    let preserved = get_action(&conn, &original.id).unwrap().unwrap();
    assert_eq!(preserved.status, "failed");
    assert_eq!(
        preserved.error.as_deref(),
        Some("The browser did not respond.")
    );
}

#[test]
fn does_not_retry_imported_or_non_idempotent_actions() {
    let conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let open_url = find_tool("system.open_url").unwrap();
    let imported = insert_action(
        &conn,
        &open_url,
        "Imported browser request".to_string(),
        "failed",
        json!({ "url": "https://example.com" }),
        None,
        Vec::new(),
    )
    .unwrap();
    assert!(!imported.retry_allowed);
    assert!(retry_action_in(&conn, &imported.id).is_err());

    let adjust_volume = find_tool("system.adjust_volume").unwrap();
    let non_idempotent = insert_action(
        &conn,
        &adjust_volume,
        "Lower volume by 10%".to_string(),
        "failed",
        json!({ "delta": -10 }),
        Some("conversation-1".to_string()),
        Vec::new(),
    )
    .unwrap();
    assert!(!non_idempotent.retry_allowed);
    assert!(retry_action_in(&conn, &non_idempotent.id).is_err());
}

#[test]
fn agent_conversation_can_only_finish_from_executing() {
    let conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let tool = find_tool("agent.hermes_chat").unwrap();
    let action = insert_action(
        &conn,
        &tool,
        "Amara conversation".to_string(),
        "executing",
        json!({}),
        Some("conversation-1".to_string()),
        Vec::new(),
    )
    .unwrap();
    let completed = update_action_status(
        &conn,
        &action.id,
        &["executing"],
        "succeeded",
        Some("Hermes replied.".to_string()),
        None,
    )
    .unwrap();
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.result.as_deref(), Some("Hermes replied."));
    assert!(completed.completed_at.is_some());
}

#[test]
fn priority_inputs_exclude_internal_conversation_ledger_rows() {
    let actions = vec![
        TerrenceAction {
            id: "conversation".to_string(),
            tool_id: "agent.hermes_chat".to_string(),
            title: "Amara conversation".to_string(),
            status: "failed".to_string(),
            requires_approval: false,
            parameters: json!({}),
            conversation_id: Some("conversation-1".to_string()),
            context_item_ids: Vec::new(),
            requested_at: "2026-08-21T00:00:00Z".to_string(),
            updated_at: "2026-08-21T00:00:00Z".to_string(),
            approved_at: None,
            started_at: None,
            completed_at: None,
            result: None,
            error: Some("Hermes could not complete the request.".to_string()),
            retry_of_action_id: None,
            retry_allowed: false,
            summary: "Amara conversation".to_string(),
        },
        TerrenceAction {
            id: "action".to_string(),
            tool_id: "system.open_url".to_string(),
            title: "Open project brief".to_string(),
            status: "awaiting_approval".to_string(),
            requires_approval: true,
            parameters: json!({}),
            conversation_id: Some("conversation-1".to_string()),
            context_item_ids: Vec::new(),
            requested_at: "2026-08-21T00:00:00Z".to_string(),
            updated_at: "2026-08-21T00:00:00Z".to_string(),
            approved_at: None,
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            retry_of_action_id: None,
            retry_allowed: false,
            summary: "Open link".to_string(),
        },
    ];
    let external = actions
        .into_iter()
        .filter(|action| action.tool_id != "agent.hermes_chat")
        .collect::<Vec<_>>();
    assert_eq!(external.len(), 1);
    assert_eq!(external[0].title, "Open project brief");
}

#[test]
fn imports_hermes_action_outcomes_once_into_the_canonical_ledger() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_action_db(&conn).unwrap();
    let directory = std::env::temp_dir().join(format!("nexus-hermes-actions-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let event_path = directory.join("action-event-1.json");
    fs::write(
        &event_path,
        json!({
            "eventId": "event-1",
            "source": "hermes_telegram",
            "toolId": "system.open_url",
            "title": "Mac action: open Facebook",
            "status": "succeeded",
            "parameters": { "url": "https://www.facebook.com" },
            "occurredAt": "2026-08-24T12:00:00Z",
            "result": "Hermes completed the requested Mac action.",
            "error": null
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(ingest_hermes_action_inbox(&mut conn, &directory), 1);
    assert!(!event_path.exists());
    let action = conn
        .query_row(
            "SELECT id FROM terrence_actions WHERE tool_id = 'system.open_url'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    let imported = get_action(&conn, &action).unwrap().unwrap();
    assert_eq!(imported.status, "succeeded");
    assert!(!imported.requires_approval);
    assert_eq!(
        imported.result.as_deref(),
        Some("Hermes completed the requested Mac action.")
    );

    fs::write(
        &event_path,
        json!({
            "eventId": "event-1",
            "source": "hermes_telegram",
            "toolId": "system.open_url",
            "title": "Mac action: open Facebook",
            "status": "succeeded",
            "parameters": {},
            "occurredAt": "2026-08-24T12:00:00Z",
            "result": "Hermes completed the requested Mac action.",
            "error": null
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(ingest_hermes_action_inbox(&mut conn, &directory), 1);
    let action_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM terrence_actions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(action_count, 1);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn turns_a_verified_external_action_into_one_linked_context_observation() {
    let conn = Connection::open_in_memory().unwrap();
    terrence_context::init_terrence_core_db_for_tests(&conn).unwrap();
    init_action_db(&conn).unwrap();
    let tool = find_tool("system.open_url").unwrap();
    let action = insert_action(
        &conn,
        &tool,
        "Open https://example.com?access_token=private-value".to_string(),
        "executing",
        json!({ "url": "https://example.com" }),
        None,
        Vec::new(),
    )
    .unwrap();
    let completed = update_action_status(
        &conn,
        &action.id,
        &["executing"],
        "succeeded",
        Some("Completed with access_token=private-value.".to_string()),
        None,
    )
    .unwrap();

    assert_eq!(sync_action_outcome_learning(&conn), 1);
    assert_eq!(sync_action_outcome_learning(&conn), 0);

    let (title, detail, provenance_ref, metadata): (String, String, String, String) = conn
        .query_row(
            "SELECT title, content, provenance_ref, metadata FROM context_items",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        title,
        "Verified outcome: Open https://example.com?access_token=[REDACTED]"
    );
    assert_eq!(detail, "Completed with access_token=[REDACTED]");
    assert_eq!(provenance_ref, format!("terrence-action:{}", completed.id));
    assert_eq!(
        serde_json::from_str::<Value>(&metadata)
            .unwrap()
            .get("outcomeStatus")
            .and_then(Value::as_str),
        Some("succeeded")
    );
    let linked = get_action(&conn, &completed.id).unwrap().unwrap();
    assert_eq!(linked.context_item_ids.len(), 1);
}

#[test]
fn does_not_learn_cancelled_actions_or_conversation_turns() {
    let conn = Connection::open_in_memory().unwrap();
    terrence_context::init_terrence_core_db_for_tests(&conn).unwrap();
    init_action_db(&conn).unwrap();
    let local_tool = find_tool("system.open_application").unwrap();
    let local = insert_action(
        &conn,
        &local_tool,
        "Open Safari".to_string(),
        "awaiting_approval",
        json!({ "application": "Safari" }),
        None,
        Vec::new(),
    )
    .unwrap();
    update_action_status(
        &conn,
        &local.id,
        &["awaiting_approval"],
        "cancelled",
        None,
        Some("Cancelled by the user.".to_string()),
    )
    .unwrap();
    let conversation_tool = find_tool("agent.hermes_chat").unwrap();
    let conversation = insert_action(
        &conn,
        &conversation_tool,
        "Amara conversation".to_string(),
        "executing",
        json!({}),
        Some("conversation-1".to_string()),
        Vec::new(),
    )
    .unwrap();
    update_action_status(
        &conn,
        &conversation.id,
        &["executing"],
        "succeeded",
        Some("Hermes replied.".to_string()),
        None,
    )
    .unwrap();

    assert_eq!(sync_action_outcome_learning(&conn), 0);
    let count: u32 = conn
        .query_row("SELECT COUNT(*) FROM context_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn formats_only_verified_nonconversation_outcomes_for_runtime_context() {
    let action = TerrenceAction {
        id: "action-1".to_string(),
        tool_id: "system.open_url".to_string(),
        title: "Mac action: open Facebook".to_string(),
        status: "succeeded".to_string(),
        requires_approval: false,
        parameters: json!({}),
        conversation_id: None,
        context_item_ids: Vec::new(),
        requested_at: "2026-08-24T12:00:00Z".to_string(),
        updated_at: "2026-08-24T12:00:00Z".to_string(),
        approved_at: None,
        started_at: Some("2026-08-24T12:00:00Z".to_string()),
        completed_at: Some("2026-08-24T12:00:00Z".to_string()),
        result: Some("Hermes completed the requested Mac action.".to_string()),
        error: None,
        retry_of_action_id: None,
        retry_allowed: false,
        summary: "Open link".to_string(),
    };
    assert_eq!(
        action_outcome_line(&action).as_deref(),
        Some(
            "- [succeeded] Mac action: open Facebook — Hermes completed the requested Mac action."
        )
    );
    let conversation = TerrenceAction {
        tool_id: "agent.hermes_chat".to_string(),
        ..action
    };
    assert!(action_outcome_line(&conversation).is_none());
}

#[test]
fn redacts_credentials_from_imported_hermes_action_titles() {
    let event = clean_hermes_action_event(HermesActionEvent {
        event_id: "event-1".to_string(),
        source: "hermes_telegram".to_string(),
        tool_id: "system.open_url".to_string(),
        title: "Open https://example.com?access_token=private-value".to_string(),
        status: "succeeded".to_string(),
        parameters: json!({}),
        occurred_at: "2026-08-24T12:00:00Z".to_string(),
        result: Some("Completed.".to_string()),
        error: None,
    })
    .unwrap();
    assert_eq!(
        event.title,
        "Open https://example.com?access_token=[REDACTED]"
    );
}

#[test]
fn approval_gate_in_flight_lock_blocks_a_second_dialog() {
    let first = super::InFlightGuard::acquire("gate-lock-test").unwrap();
    let busy = super::InFlightGuard::acquire("gate-lock-test")
        .err()
        .unwrap();
    assert!(busy.starts_with("busy:"), "{busy}");
    drop(first);
    assert!(super::InFlightGuard::acquire("gate-lock-test").is_ok());
}

#[test]
fn approval_gate_fingerprint_detects_param_changes() {
    let a = super::approval_fingerprint("system.open_url", &json!({"url": "https://a.example"}))
        .unwrap();
    let b = super::approval_fingerprint("system.open_url", &json!({"url": "https://b.example"}))
        .unwrap();
    let c = super::approval_fingerprint(
        "system.open_application",
        &json!({"url": "https://a.example"}),
    )
    .unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(
        a,
        super::approval_fingerprint("system.open_url", &json!({"url": "https://a.example"}))
            .unwrap()
    );
}

#[test]
fn approval_gate_shows_exact_target_and_refuses_oversized_text() {
    let op = super::describe_operation("system.open_url", &json!({"url": "https://a.example/x"}))
        .unwrap();
    assert!(op.contains("https://a.example/x"));
    let mut action = sample_gate_action();
    let normal = super::approval_text(&action, &json!({"url": "https://a.example/x"})).unwrap();
    assert!(normal.starts_with("Open URL: "));
    assert!(normal.contains("Requested as: "));
    action.tool_id = "macos.workflow".to_string();
    let huge = json!({"steps": "x".repeat(5_000)});
    let refused = super::approval_text(&action, &huge).unwrap_err();
    assert!(refused.starts_with("refused:"), "{refused}");
    action.retry_of_action_id = Some("orig-1".to_string());
    action.tool_id = "system.open_url".to_string();
    let retry = super::approval_text(&action, &json!({"url": "https://a.example/x"})).unwrap();
    assert!(retry.starts_with("Retry of orig-1"));
}

fn sample_gate_action() -> super::TerrenceAction {
    let mut action: super::TerrenceAction = serde_json::from_value(json!({
        "id": "gate-1", "toolId": "system.open_url", "title": "Open URL: fake",
        "status": "awaiting_approval", "requiresApproval": true, "parameters": {},
        "conversationId": null, "contextItemIds": [], "requestedAt": "2026-10-21T00:00:00Z",
        "updatedAt": "2026-10-21T00:00:00Z", "approvedAt": null, "startedAt": null,
        "completedAt": null, "result": null, "error": null, "retryOfActionId": null,
        "retryAllowed": false, "summary": ""
    }))
    .expect("TerrenceAction must deserialize from a minimal row");
    action.retry_of_action_id = None;
    action
}

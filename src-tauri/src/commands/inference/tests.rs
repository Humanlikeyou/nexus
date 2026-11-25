//! Acceptance tests for the shared inference layer (decision 0051). All offline: a scripted
//! transport stands in for the router, so these run with no network and no Ollama.

use super::tools::{
    authorize, run_with_tools, ToolEffect, ToolEvent, ToolExecutor, ToolGrant, ToolSpec,
};
use super::*;
use crate::commands::llm_router::{local_deep_headers, local_tier_headers, ClaudeGate};
use std::cell::RefCell;
use std::collections::VecDeque;

const BASE: &str = "http://127.0.0.1:8787";

type Seen = (String, Vec<(&'static str, String)>, Value);

enum Step {
    Reply(u16, Value),
    Down,
    Hang,
}

/// Plays scripted replies in order and records every request.
struct Script {
    steps: RefCell<VecDeque<Step>>,
    seen: RefCell<Vec<Seen>>,
}

impl Script {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: RefCell::new(steps.into()),
            seen: RefCell::new(vec![]),
        }
    }
    fn calls(&self) -> usize {
        self.seen.borrow().len()
    }
    fn body(&self, i: usize) -> Value {
        self.seen.borrow()[i].2.clone()
    }
    fn header(&self, i: usize, k: &str) -> Option<String> {
        self.seen.borrow()[i]
            .1
            .iter()
            .find(|(h, _)| *h == k)
            .map(|(_, v)| v.clone())
    }
}

impl RouterTransport for Script {
    async fn post(
        &self,
        url: &str,
        headers: &[(&'static str, String)],
        body: &Value,
        _t: u64,
    ) -> Result<(u16, Value), String> {
        self.seen
            .borrow_mut()
            .push((url.to_string(), headers.to_vec(), body.clone()));
        let step = self.steps.borrow_mut().pop_front();
        match step {
            Some(Step::Reply(s, v)) => Ok((s, v)),
            Some(Step::Down) | None => Err("connection refused".into()),
            Some(Step::Hang) => {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Err("never".into())
            }
        }
    }
}

fn ok(tier: &str, content: &str) -> Step {
    Step::Reply(
        200,
        json!({"choices": [{"message": {"role": "assistant", "content": content}}],
               "amara": {"tier": tier, "reason": "header_fast"},
               "usage": {"prompt_tokens": 10, "completion_tokens": 4}}),
    )
}

fn tool_reply(name: &str, args: &str) -> Step {
    Step::Reply(
        200,
        json!({"choices": [{"message": {"role": "assistant", "content": "",
               "tool_calls": [{"id": "c1", "type": "function", "function": {"name": name, "arguments": args}}]}}],
               "amara": {"tier": "fast", "reason": "header_fast"},
               "usage": {"prompt_tokens": 20, "completion_tokens": 6}}),
    )
}

fn gate_open() -> ClaudeGate {
    ClaudeGate {
        allow_claude: true,
        outbound_disabled: false,
        local_mode: false,
        over_daily_cap: false,
    }
}

fn open_inputs() -> PolicyInputs {
    PolicyInputs {
        gate: gate_open(),
        local_only_switch: false,
    }
}

fn closed_inputs() -> PolicyInputs {
    PolicyInputs {
        gate: ClaudeGate {
            allow_claude: false,
            ..gate_open()
        },
        local_only_switch: false,
    }
}

fn req(class: ModelClass) -> InferenceRequest {
    let mut r = InferenceRequest::new("test", class);
    r.messages = vec![Message::user("hi")];
    r.limits.retries = 1;
    r
}

fn day_schema() -> Value {
    json!({"type": "object",
           "properties": {"mood": {"type": "string", "enum": ["good", "ok", "bad"]},
                          "score": {"type": "integer", "minimum": 0, "maximum": 10}},
           "required": ["mood", "score"], "additionalProperties": false})
}

fn take_usage() -> Vec<llm_router::LlmUsageRecord> {
    TEST_USAGE.with(|u| std::mem::take(&mut *u.borrow_mut()))
}

// ── Provider policy ──────────────────────────────────────────────────────────

#[tokio::test]
async fn local_classes_send_exactly_the_local_headers_even_with_the_gate_open() {
    for (class, want) in [
        (ModelClass::Fast, local_tier_headers()),
        (ModelClass::Deep, local_deep_headers()),
    ] {
        for policy in [ProviderPolicy::LocalOnly, ProviderPolicy::CloudIfAuthorized] {
            let t = Script::new(vec![ok("fast", "hello")]);
            let mut r = req(class);
            r.policy = policy;
            infer_with(&t, BASE, "P", &r, &open_inputs()).await.unwrap();
            assert_eq!(t.seen.borrow()[0].1, want);
            assert_eq!(t.header(0, "X-Amara-Allow-Claude").as_deref(), Some("0"));
            assert_eq!(t.seen.borrow()[0].0, format!("{BASE}/v1/chat/completions"));
        }
    }
}

#[tokio::test]
async fn a_forced_cloud_class_cannot_pass_the_policy_or_the_gate() {
    // LocalOnly policy: denied, nothing sent.
    let t = Script::new(vec![ok("claude", "x")]);
    let e = infer_with(&t, BASE, "P", &req(ModelClass::Cloud), &open_inputs())
        .await
        .unwrap_err();
    assert_eq!(e, InferenceError::PolicyDenied("local_only"));
    assert_eq!(t.calls(), 0);

    // CloudIfAuthorized, but each gate check failing: denied, nothing sent.
    type Close = fn(&mut PolicyInputs);
    let cases: [(Close, &str); 5] = [
        (|i| i.gate.outbound_disabled = true, "outbound_disabled"),
        (|i| i.gate.local_mode = true, "local_mode"),
        (|i| i.gate.allow_claude = false, "claude_not_allowed"),
        (|i| i.gate.over_daily_cap = true, "daily_cap"),
        (|i| i.local_only_switch = true, "local_only_switch"),
    ];
    for (close, why) in cases {
        let mut inputs = open_inputs();
        close(&mut inputs);
        let t = Script::new(vec![ok("claude", "x")]);
        let mut r = req(ModelClass::Cloud);
        r.policy = ProviderPolicy::CloudIfAuthorized;
        let e = infer_with(&t, BASE, "P", &r, &inputs).await.unwrap_err();
        assert_eq!(e, InferenceError::PolicyDenied(why));
        assert_eq!(t.calls(), 0, "{why}");
    }

    // All open: the only path that ever sends Allow-Claude 1.
    let t = Script::new(vec![ok("claude", "from cloud")]);
    let mut r = req(ModelClass::Cloud);
    r.policy = ProviderPolicy::CloudIfAuthorized;
    let o = infer_with(&t, BASE, "P", &r, &open_inputs()).await.unwrap();
    assert_eq!(o.tier, Tier::Claude);
    assert_eq!(t.header(0, "X-Amara-Allow-Claude").as_deref(), Some("1"));
    assert_eq!(t.header(0, "X-Amara-Route").as_deref(), Some("claude"));
    take_usage();
}

#[tokio::test]
async fn a_cloud_reply_to_a_local_request_is_refused() {
    let t = Script::new(vec![ok("claude", "leaked")]);
    let e = infer_with(&t, BASE, "P", &req(ModelClass::Fast), &open_inputs())
        .await
        .unwrap_err();
    assert_eq!(e, InferenceError::UnexpectedTier);
    assert_eq!(e.user_text(), llm_router::ROUTER_FAILED);
    take_usage();
}

#[tokio::test]
async fn only_a_loopback_router_is_ever_called() {
    for base in [
        "http://example.com:8787",
        "https://api.anthropic.com",
        "http://192.168.1.5:8787",
    ] {
        let t = Script::new(vec![ok("fast", "x")]);
        let e = infer_with(&t, base, "P", &req(ModelClass::Fast), &closed_inputs())
            .await
            .unwrap_err();
        assert_eq!(e, InferenceError::NotLoopback);
        assert_eq!(t.calls(), 0);
    }
}

#[tokio::test]
async fn a_local_failure_is_reported_and_never_retried_on_the_cloud() {
    let t = Script::new(vec![Step::Down, Step::Down, Step::Down]);
    let mut r = req(ModelClass::Fast);
    r.policy = ProviderPolicy::CloudIfAuthorized;
    r.limits.retries = 2;
    let e = infer_with(&t, BASE, "P", &r, &open_inputs())
        .await
        .unwrap_err();
    assert_eq!(e, InferenceError::RouterUnreachable);
    assert_eq!(e.user_text(), llm_router::ROUTER_DOWN);
    assert_eq!(t.calls(), 3);
    for i in 0..3 {
        assert_eq!(t.header(i, "X-Amara-Allow-Claude").as_deref(), Some("0"));
        assert_eq!(t.header(i, "X-Amara-Route").as_deref(), Some("fast"));
    }
    // 503 (Ollama down) keeps its own honest text.
    let t = Script::new(vec![Step::Reply(503, json!({}))]);
    let mut r = req(ModelClass::Fast);
    r.limits.retries = 0;
    let e = infer_with(&t, BASE, "P", &r, &closed_inputs())
        .await
        .unwrap_err();
    assert_eq!(e.user_text(), llm_router::OLLAMA_DOWN);
}

// ── Structured output ────────────────────────────────────────────────────────

#[tokio::test]
async fn invalid_structured_output_is_retried_with_a_correction_then_accepted() {
    let t = Script::new(vec![
        ok("fast", r#"{"mood": "neutral", "score": 5}"#),
        ok("fast", "```json\n{\"mood\": \"ok\", \"score\": 5}\n```"),
    ]);
    let mut r = req(ModelClass::Fast);
    r.output = OutputSpec::Json {
        name: "day".into(),
        schema: day_schema(),
    };
    let o = infer_with(&t, BASE, "P", &r, &closed_inputs())
        .await
        .unwrap();
    assert_eq!(o.json, Some(json!({"mood": "ok", "score": 5})));
    assert_eq!(o.attempts, 2);
    // Both attempts' tokens count.
    assert_eq!(
        o.usage,
        Usage {
            prompt_tokens: 20,
            completion_tokens: 8
        }
    );
    // The runtime was asked for the schema, and the retry carried a correction (path + rule only).
    let b0 = t.body(0);
    assert_eq!(b0["response_format"]["type"], "json_schema");
    assert_eq!(b0["response_format"]["json_schema"]["schema"], day_schema());
    let msgs = t.body(1)["messages"].as_array().unwrap().clone();
    let last = msgs.last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(last.contains("$.mood: not one of the allowed values"));
    assert!(!last.contains("neutral"));
    take_usage();
}

#[tokio::test]
async fn structured_output_that_stays_invalid_is_rejected() {
    let t = Script::new(vec![
        ok("fast", "not json"),
        ok("fast", r#"{"mood": "ok"}"#),
    ]);
    let mut r = req(ModelClass::Fast);
    r.output = OutputSpec::Json {
        name: "day".into(),
        schema: day_schema(),
    };
    let e = infer_with(&t, BASE, "P", &r, &closed_inputs())
        .await
        .unwrap_err();
    assert_eq!(e, InferenceError::InvalidOutput("$.score: required".into()));
    assert_eq!(t.calls(), 2);
    // Failed attempts still used tokens: recorded, not lost.
    let u = take_usage();
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].prompt_tokens, 20);
    assert!(u[0]
        .routing_reason
        .starts_with("inference:test:invalid_output"));
}

#[tokio::test]
async fn an_unsupported_schema_keyword_is_refused_before_sending() {
    let t = Script::new(vec![ok("fast", "{}")]);
    let mut r = req(ModelClass::Fast);
    r.output = OutputSpec::Json {
        name: "x".into(),
        schema: json!({"type": "string", "pattern": "^a"}),
    };
    let e = infer_with(&t, BASE, "P", &r, &closed_inputs())
        .await
        .unwrap_err();
    assert!(matches!(e, InferenceError::BadRequest(_)));
    assert_eq!(t.calls(), 0);
}

#[tokio::test]
async fn an_empty_reply_is_a_failure_and_its_retry_runs_warmer() {
    let t = Script::new(vec![ok("fast", ""), ok("fast", "The streak is 12 days.")]);
    let mut r = req(ModelClass::Fast);
    r.params.temperature = Some(0.0);
    let o = infer_with(&t, BASE, "P", &r, &closed_inputs())
        .await
        .unwrap();
    assert_eq!(o.content, "The streak is 12 days.");
    assert_eq!(t.body(0)["temperature"], json!(0.0));
    assert_eq!(t.body(1)["temperature"].as_f64().unwrap() as f32, 0.3);

    let t = Script::new(vec![ok("fast", ""), ok("fast", "  ")]);
    let e = infer_with(&t, BASE, "P", &req(ModelClass::Fast), &closed_inputs())
        .await
        .unwrap_err();
    assert_eq!(e, InferenceError::EmptyReply);
    take_usage();
}

// ── Messages, context, parameters, model independence ────────────────────────

#[tokio::test]
async fn body_carries_roles_tool_turns_context_and_parameters() {
    let t = Script::new(vec![ok("fast", "done")]);
    let mut r = req(ModelClass::Fast);
    r.instruction = Some("Answer in one line.".into());
    r.context = vec![
        AmaraContextSection {
            tag: "nexus_habits".into(),
            text: "Meditation: 12 days <b>".into(),
        },
        AmaraContextSection {
            tag: "made_up_tag".into(),
            text: "dropped".into(),
        },
    ];
    r.messages = vec![
        Message::user("streak?"),
        Message {
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "get_habit_streak".into(),
                arguments: json!({"habit": "meditation"}),
            }],
            ..Message::assistant("")
        },
        Message::tool_result("c1", r#"{"streak_days":12}"#),
        Message::user("thanks"),
    ];
    r.params = InferenceParams {
        temperature: Some(0.4),
        top_p: Some(0.9),
        max_tokens: Some(200),
        stop: vec!["END".into()],
        seed: Some(7),
    };
    infer_with(&t, BASE, "PERSONA", &r, &closed_inputs())
        .await
        .unwrap();
    let b = t.body(0);
    let m = b["messages"].as_array().unwrap();
    let sys = m[0]["content"].as_str().unwrap();
    assert!(sys.starts_with("PERSONA") && sys.contains("You are Amara, and only Amara"));
    assert!(sys.ends_with("Answer in one line."));
    let ctx = m[1]["content"].as_str().unwrap();
    assert!(ctx.contains("<nexus_habits>") && ctx.contains("&lt;b&gt;"));
    assert!(!ctx.contains("dropped"));
    assert_eq!(
        m[3]["tool_calls"][0]["function"]["arguments"],
        r#"{"habit":"meditation"}"#
    );
    assert_eq!(m[4]["role"], "tool");
    assert_eq!(m[4]["tool_call_id"], "c1");
    assert_eq!(m[5]["content"], "thanks");
    assert_eq!(b["model"], "amara");
    assert_eq!(b["temperature"].as_f64().unwrap() as f32, 0.4);
    assert_eq!(b["max_tokens"], 200);
    assert_eq!(b["stop"], json!(["END"]));
    assert_eq!(b["seed"], 7);
    take_usage();
}

#[tokio::test]
async fn switching_the_router_model_changes_nothing_the_caller_sees() {
    // The same request against two different router configs (the reply's `model` differs).
    let mut outs = vec![];
    for model in ["gemma4:latest", "gemma4:e4b"] {
        let t = Script::new(vec![Step::Reply(
            200,
            json!({"model": model, "choices": [{"message": {"content": "{\"mood\":\"ok\",\"score\":3}"}}],
                   "amara": {"tier": "fast", "reason": "header_fast"}, "usage": {"prompt_tokens": 1, "completion_tokens": 1}}),
        )]);
        let mut r = req(ModelClass::Fast);
        r.output = OutputSpec::Json {
            name: "day".into(),
            schema: day_schema(),
        };
        let o = infer_with(&t, BASE, "P", &r, &closed_inputs())
            .await
            .unwrap();
        let body = t.body(0).to_string();
        assert!(!body.contains("gemma") && !body.contains("qwen"));
        let leaks = serde_json::to_string(&o).unwrap().contains("gemma");
        outs.push((o.json, o.tier, leaks));
    }
    assert_eq!(outs[0], outs[1]);
    assert!(!outs[0].2);
    take_usage();
}

// ── Timeouts and cancellation ────────────────────────────────────────────────

#[tokio::test]
async fn a_hung_router_times_out_and_a_cancel_stops_the_call() {
    let t = Script::new(vec![Step::Hang]);
    let mut r = req(ModelClass::Fast);
    r.limits = Limits {
        timeout: Duration::from_millis(50),
        retries: 3,
    };
    let e = infer_with(&t, BASE, "P", &r, &closed_inputs())
        .await
        .unwrap_err();
    assert_eq!(e, InferenceError::Timeout);
    assert_eq!(t.calls(), 1);

    let t = Script::new(vec![Step::Hang]);
    let cancel = CancelToken::default();
    let mut r = req(ModelClass::Fast);
    r.cancel = Some(cancel.clone());
    let c2 = cancel.clone();
    let inputs = closed_inputs();
    let (res, _) = tokio::join!(infer_with(&t, BASE, "P", &r, &inputs), async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        c2.cancel();
    });
    assert_eq!(res.unwrap_err(), InferenceError::Cancelled);

    // Already cancelled: nothing is sent.
    let t = Script::new(vec![ok("fast", "x")]);
    let e = infer_with(&t, BASE, "P", &r, &closed_inputs())
        .await
        .unwrap_err();
    assert_eq!(e, InferenceError::Cancelled);
    assert_eq!(t.calls(), 0);
}

// ── Usage ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn usage_is_recorded_once_per_call_with_tier_tokens_and_local_cost_zero() {
    take_usage();
    let t = Script::new(vec![ok("deep", "answer")]);
    let o = infer_with(&t, BASE, "P", &req(ModelClass::Deep), &closed_inputs())
        .await
        .unwrap();
    assert_eq!(o.cost_usd, 0.0);
    let u = take_usage();
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].tier, Tier::Deep);
    assert_eq!((u[0].prompt_tokens, u[0].completion_tokens), (10, 4));
    assert_eq!(u[0].estimated_cost_usd, 0.0);
    assert_eq!(u[0].routing_reason, "inference:test:ok; header_fast");

    // The cloud tier is priced with the same table chat uses.
    let t = Script::new(vec![ok("claude", "answer")]);
    let mut r = req(ModelClass::Cloud);
    r.policy = ProviderPolicy::CloudIfAuthorized;
    let o = infer_with(&t, BASE, "P", &r, &open_inputs()).await.unwrap();
    let want = llm_router::estimate_cost("anthropic", "claude-haiku-4-5-20251001", 10, 4);
    assert!(o.cost_usd > 0.0 && (o.cost_usd - want).abs() < 1e-12);
    assert_eq!(take_usage()[0].estimated_cost_usd, o.cost_usd);

    // A call that never reached a model records nothing.
    let t = Script::new(vec![Step::Down]);
    let mut r = req(ModelClass::Fast);
    r.limits.retries = 0;
    let _ = infer_with(&t, BASE, "P", &r, &closed_inputs()).await;
    assert!(take_usage().is_empty());
}

// ── Tools ────────────────────────────────────────────────────────────────────

fn streak_tool() -> ToolSpec {
    ToolSpec {
        name: "get_habit_streak".into(),
        description: "Days in a row a habit was done.".into(),
        parameters: json!({"type": "object", "properties": {"habit": {"type": "string"}},
                           "required": ["habit"], "additionalProperties": false}),
        effect: ToolEffect::ReadOnly,
    }
}

fn delete_tool() -> ToolSpec {
    ToolSpec {
        name: "delete_goal".into(),
        description: "Deletes a goal.".into(),
        parameters: json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        effect: ToolEffect::StateChanging,
    }
}

#[derive(Default)]
struct Exec {
    ran: RefCell<Vec<String>>,
}

impl ToolExecutor for Exec {
    async fn execute(&self, name: &str, _args: &Value) -> Result<Value, String> {
        self.ran.borrow_mut().push(name.to_string());
        Ok(json!({"streak_days": 12}))
    }
}

#[test]
fn authorization_refuses_every_unauthorized_call() {
    let offered = vec![streak_tool(), delete_tool()];
    let grant = ToolGrant::read_only(&["get_habit_streak", "delete_goal"]);
    let call = |name: &str, args: Value| ToolCall {
        id: "c".into(),
        name: name.into(),
        arguments: args,
    };
    assert!(authorize(
        &call("get_habit_streak", json!({"habit": "m"})),
        &offered,
        &grant
    )
    .is_ok());
    let cases = [
        (call("terminal", json!({})), "not_offered"),
        (
            call("delete_goal", json!({"id": "g1"})),
            "state_change_needs_approval",
        ),
        (call("get_habit_streak", json!({})), "invalid_arguments"),
        (
            call("get_habit_streak", json!({"habit": 1})),
            "invalid_arguments",
        ),
        (
            call("get_habit_streak", json!({"habit": "m", "x": 1})),
            "invalid_arguments",
        ),
    ];
    for (c, why) in cases {
        assert_eq!(authorize(&c, &offered, &grant).unwrap_err(), why);
    }
    let narrow = ToolGrant::read_only(&[]);
    assert_eq!(
        authorize(
            &call("get_habit_streak", json!({"habit": "m"})),
            &offered,
            &narrow
        )
        .unwrap_err(),
        "not_granted"
    );
}

#[tokio::test]
async fn tool_loop_runs_only_authorized_calls_and_feeds_results_back() {
    let t = Script::new(vec![
        tool_reply("get_habit_streak", r#"{"habit":"meditation"}"#),
        ok("fast", "12 days in a row."),
    ]);
    let mut r = req(ModelClass::Fast);
    r.tools = vec![streak_tool(), delete_tool()];
    let exec = Exec::default();
    let grant = ToolGrant::read_only(&["get_habit_streak"]);
    let (o, events) = run_with_tools(&t, BASE, "P", &r, &closed_inputs(), &grant, &exec, 4)
        .await
        .unwrap();
    assert_eq!(o.content, "12 days in a row.");
    assert_eq!(*exec.ran.borrow(), vec!["get_habit_streak".to_string()]);
    assert_eq!(
        events,
        vec![ToolEvent::Executed {
            name: "get_habit_streak".into()
        }]
    );
    // Only granted tools were offered to the model.
    let offered: Vec<String> = t.body(0)["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["function"]["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(offered, vec!["get_habit_streak"]);
    // Offered tools add the tool-use line (without it, Gemma under Amara's guard never calls one).
    assert!(t.body(0)["messages"][0]["content"]
        .as_str()
        .unwrap()
        .ends_with(TOOL_USE_HINT));
    let body1 = t.body(1);
    let last = body1["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["role"], "tool");
    assert_eq!(last["tool_call_id"], "c1");
    assert_eq!(last["content"], r#"{"streak_days":12}"#);
    take_usage();
}

#[tokio::test]
async fn unauthorized_tool_proposals_never_execute() {
    for (name, args) in [
        ("delete_goal", r#"{"id":"g1"}"#),
        ("terminal", r#"{"command":"rm -rf ~"}"#),
        ("get_habit_streak", r#"{"habit":42}"#),
    ] {
        let t = Script::new(vec![tool_reply(name, args), ok("fast", "I can't do that.")]);
        let mut r = req(ModelClass::Fast);
        r.tools = vec![streak_tool(), delete_tool()];
        let exec = Exec::default();
        let grant = ToolGrant {
            allowed: ["get_habit_streak", "delete_goal"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            allow_state_changing: false,
        };
        let (o, events) = run_with_tools(&t, BASE, "P", &r, &closed_inputs(), &grant, &exec, 4)
            .await
            .unwrap();
        assert!(exec.ran.borrow().is_empty(), "{name} ran");
        assert!(matches!(events[0], ToolEvent::Denied { .. }), "{name}");
        assert_eq!(o.content, "I can't do that.");
        let m = t.body(1)["messages"].as_array().unwrap().clone();
        assert_eq!(m.last().unwrap()["content"], r#"{"error":"not permitted"}"#);
    }
    take_usage();
}

#[tokio::test]
async fn a_model_that_keeps_calling_tools_hits_the_step_limit() {
    let t = Script::new(vec![
        tool_reply("get_habit_streak", r#"{"habit":"a"}"#),
        tool_reply("get_habit_streak", r#"{"habit":"b"}"#),
    ]);
    let mut r = req(ModelClass::Fast);
    r.tools = vec![streak_tool()];
    let exec = Exec::default();
    let e = run_with_tools(
        &t,
        BASE,
        "P",
        &r,
        &closed_inputs(),
        &ToolGrant::read_only(&["get_habit_streak"]),
        &exec,
        2,
    )
    .await
    .unwrap_err();
    assert_eq!(e, InferenceError::ToolLoopLimit);
    take_usage();
}

// ── Capabilities ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn capabilities_are_read_from_the_router_and_unknown_on_an_old_router() {
    let t = Script::new(vec![Step::Reply(
        200,
        json!({"tiers": {
            "fast": {"available": true, "local": true, "tools": true, "json_schema": true, "thinking": true,
                     "vision": true, "context_length": 131072, "num_ctx": 65536},
            "deep": {"available": false, "local": true, "tools": false, "json_schema": false, "thinking": false,
                     "vision": false, "context_length": null, "num_ctx": 32768},
            "claude": {"available": false, "local": false, "tools": false, "json_schema": false, "thinking": false,
                       "vision": false, "context_length": null, "num_ctx": null}}}),
    )]);
    let c = capabilities_with(&t, BASE).await.unwrap().unwrap();
    assert!(c.for_class(ModelClass::Fast).tools);
    assert_eq!(c.fast.context_length, Some(131072));
    assert!(!c.for_class(ModelClass::Cloud).available);
    assert_eq!(t.seen.borrow()[0].0, format!("{BASE}/amara/capabilities"));

    let t = Script::new(vec![Step::Reply(404, json!({}))]);
    assert_eq!(capabilities_with(&t, BASE).await.unwrap(), None);
    let t = Script::new(vec![]);
    assert!(capabilities_with(&t, "http://example.com").await.is_err());
}

// ── Source pins: what must stay true as call sites migrate ───────────────────

#[test]
fn the_layer_has_no_model_names_no_direct_provider_and_no_cloud_fallback() {
    let src = include_str!("mod.rs");
    let prod = &src[..src.find("#[cfg(test)]").unwrap()];
    let low = prod.to_lowercase();
    for w in [
        "gemma4:",
        "qwen",
        "api.anthropic",
        "11434",
        "x-api-key",
        "std::process",
    ] {
        assert!(!low.contains(w), "{w}");
    }
    // The only place that ever sends Allow-Claude 1 is the authorized cloud branch.
    assert_eq!(
        prod.matches(r#"("X-Amara-Allow-Claude", "1".to_string())"#)
            .count(),
        1
    );
}

// ── Live check against the real loopback router (opt-in) ─────────────────────
//
// `AMARA_LIVE_ROUTER=1 cargo test --lib inference::tests::live -- --ignored --nocapture`
// Needs the Amara Router on 127.0.0.1:8787 and Ollama with the fast model. Local tiers only:
// the gate is closed, so nothing can leave the Mac.

fn live() -> bool {
    std::env::var("AMARA_LIVE_ROUTER").is_ok_and(|v| v == "1")
}

#[tokio::test]
#[ignore]
async fn live_text_structured_and_tool_round_on_the_fast_tier() {
    if !live() {
        return;
    }
    let t = llm_router::loopback_transport(Some(llm_router::DEEP_BODY_CAP));
    let persona = "You are Amara, a concise assistant.";
    let inputs = closed_inputs();

    let mut r = req(ModelClass::Fast);
    r.messages = vec![Message::user("Say hello in five words.")];
    let o = infer_with(&t, BASE, persona, &r, &inputs).await.unwrap();
    assert_eq!(o.tier, Tier::Light);
    assert!(!o.content.is_empty());
    eprintln!(
        "live text: attempts={} latency_ms={} tokens={:?}",
        o.attempts, o.latency_ms, o.usage
    );

    let mut r = req(ModelClass::Fast);
    r.limits.retries = 2;
    r.messages = vec![Message::user(
        "I slept badly but trained well. Rate my day.",
    )];
    r.output = OutputSpec::Json {
        name: "day".into(),
        schema: day_schema(),
    };
    let o = infer_with(&t, BASE, persona, &r, &inputs).await.unwrap();
    assert!(schema::validate(&day_schema(), o.json.as_ref().unwrap()).is_ok());
    eprintln!(
        "live json: attempts={} latency_ms={}",
        o.attempts, o.latency_ms
    );

    let mut r = req(ModelClass::Fast);
    r.limits.retries = 2;
    r.params.temperature = Some(0.0);
    r.messages = vec![Message::user(
        "How many days in a row have I done my meditation habit?",
    )];
    r.tools = vec![streak_tool(), delete_tool()];
    let exec = Exec::default();
    let (o, events) = run_with_tools(
        &t,
        BASE,
        persona,
        &r,
        &inputs,
        &ToolGrant::read_only(&["get_habit_streak"]),
        &exec,
        4,
    )
    .await
    .unwrap();
    eprintln!(
        "live tools: ran={:?} events={events:?} content_len={} has_12={}",
        exec.ran.borrow(),
        o.content.len(),
        o.content.contains("12")
    );
    assert_eq!(*exec.ran.borrow(), vec!["get_habit_streak".to_string()]);
    assert!(
        o.content.contains("12"),
        "answer should use the tool result"
    );

    let caps = capabilities_with(&t, BASE).await.unwrap();
    eprintln!("live capabilities: {caps:?}");
    take_usage();
}

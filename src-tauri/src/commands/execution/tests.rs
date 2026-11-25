//! Controller and ledger tests (decision 0052). They run the real controller against a real SQLite
//! ledger in a temp folder. The operations below are TEST-ONLY stand-ins that write real files or a
//! real SQLite database, so every test checks the destination state and the ledger history, not
//! just a return value. The "external service" is an in-memory mock, used only here.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("nexus-exec-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn spec(action: &'static str, class: ActionClass) -> OperationSpec {
    OperationSpec {
        action_id: action,
        tool_id: "test.tool",
        target_system: "test_fs",
        class,
        risk: Risk::Low,
        scope: DataScope::None,
        timeout: Duration::from_secs(5),
        retry: RetryPolicy::Never,
        personal_fields: &["diary"],
    }
}

/// Writes `content` to `path`; read-back reads the file.
struct WriteFile {
    spec: OperationSpec,
    runs: AtomicUsize,
    /// Return success without writing (a lying tool).
    lie: bool,
    /// Sleep before writing.
    delay: Duration,
}

impl WriteFile {
    fn new(action: &'static str, class: ActionClass) -> Self {
        Self {
            spec: spec(action, class),
            runs: AtomicUsize::new(0),
            lie: false,
            delay: Duration::ZERO,
        }
    }
}

impl Operation for WriteFile {
    fn spec(&self) -> &OperationSpec {
        &self.spec
    }
    fn validate(&self, p: &Value) -> Result<(), String> {
        p["path"]
            .as_str()
            .map(|_| ())
            .ok_or_else(|| "path required".into())
    }
    fn target_ref(&self, p: &Value) -> Option<String> {
        p["path"].as_str().map(str::to_string)
    }
    fn expected_state(&self, p: &Value) -> Option<Value> {
        Some(json!({"content": p["content"]}))
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.delay);
        if self.lie {
            return Ok(json!({"ok": true}));
        }
        std::fs::write(
            p["path"].as_str().unwrap(),
            p["content"].as_str().unwrap_or(""),
        )
        .map_err(|e| OpError::Failed {
            message: e.to_string(),
            submitted: true,
        })?;
        Ok(json!({"ok": true, "api_key": "sk-should-never-be-stored-aaaaaaaaaaaa"}))
    }
    fn read_back(&self, p: &Value) -> Option<Result<Value, String>> {
        // A missing file is a reading (the change isn't there), not a failure to read.
        Some(
            match std::fs::read_to_string(p["path"].as_str().unwrap_or("")) {
                Ok(c) => Ok(json!({"content": c})),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({"content": null})),
                Err(e) => Err(e.to_string()),
            },
        )
    }
}

/// Fails "before sending" `fail_first` times, then writes. Safe to retry.
struct Flaky {
    spec: OperationSpec,
    fail_first: usize,
    runs: AtomicUsize,
}

impl Operation for Flaky {
    fn spec(&self) -> &OperationSpec {
        &self.spec
    }
    fn expected_state(&self, p: &Value) -> Option<Value> {
        Some(json!({"content": p["content"]}))
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_first {
            return Err(OpError::Failed {
                message: "connection refused".into(),
                submitted: false,
            });
        }
        std::fs::write(p["path"].as_str().unwrap(), p["content"].as_str().unwrap()).unwrap();
        Ok(json!({}))
    }
    fn read_back(&self, p: &Value) -> Option<Result<Value, String>> {
        Some(
            std::fs::read_to_string(p["path"].as_str().unwrap())
                .map(|c| json!({"content": c}))
                .map_err(|e| e.to_string()),
        )
    }
}

/// Three file steps; fails at `fail_at`. Compensation deletes the completed ones.
struct MultiStep {
    spec: OperationSpec,
}

impl Operation for MultiStep {
    fn spec(&self) -> &OperationSpec {
        &self.spec
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        let dir = PathBuf::from(p["dir"].as_str().unwrap());
        let fail_at = p["fail_at"].as_u64().unwrap_or(99) as usize;
        let mut done = vec![];
        for (i, step) in ["a", "b", "c"].iter().enumerate() {
            if i == fail_at {
                return Err(OpError::Partial {
                    message: format!("step {step} failed"),
                    completed: done,
                });
            }
            std::fs::write(dir.join(step), step).unwrap();
            done.push(step.to_string());
        }
        Ok(json!({"steps": done}))
    }
    fn compensate(&self, p: &Value, completed: &[String]) -> Option<Result<(), String>> {
        let dir = PathBuf::from(p["dir"].as_str().unwrap());
        Some(
            completed
                .iter()
                .try_for_each(|s| std::fs::remove_file(dir.join(s)))
                .map_err(|e| e.to_string()),
        )
    }
}

/// Two records in one local SQLite transaction: all or nothing.
struct TwoRecords {
    spec: OperationSpec,
}

impl Operation for TwoRecords {
    fn spec(&self) -> &OperationSpec {
        &self.spec
    }
    fn expected_state(&self, _p: &Value) -> Option<Value> {
        Some(json!({"rows": 2}))
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        let mut conn = rusqlite::Connection::open(p["db"].as_str().unwrap()).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS r (v TEXT)")
            .unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute("INSERT INTO r VALUES ('one')", []).unwrap();
        if p["fail_second"].as_bool() == Some(true) {
            // Dropping the transaction rolls back the first insert.
            return Err(OpError::Failed {
                message: "second record rejected".into(),
                submitted: false,
            });
        }
        tx.execute("INSERT INTO r VALUES ('two')", []).unwrap();
        tx.commit().map_err(|e| OpError::Failed {
            message: e.to_string(),
            submitted: false,
        })?;
        Ok(json!({}))
    }
    fn read_back(&self, p: &Value) -> Option<Result<Value, String>> {
        let conn = rusqlite::Connection::open(p["db"].as_str().unwrap()).ok()?;
        Some(
            conn.query_row("SELECT COUNT(*) FROM r", [], |r| r.get::<_, i64>(0))
                .map(|n| json!({"rows": n}))
                .map_err(|e| e.to_string()),
        )
    }
}

/// TEST-ONLY mock of an external service (no network): a consequential, approval-gated operation.
#[derive(Default)]
struct MockExternal {
    published: Mutex<Vec<String>>,
}

struct Publish {
    spec: OperationSpec,
    svc: Arc<MockExternal>,
}

impl Operation for Publish {
    fn spec(&self) -> &OperationSpec {
        &self.spec
    }
    fn expected_state(&self, p: &Value) -> Option<Value> {
        Some(json!({"published": true, "title": p["title"]}))
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        self.svc
            .published
            .lock()
            .unwrap()
            .push(p["title"].as_str().unwrap().to_string());
        Ok(json!({"id": "post-1"}))
    }
    fn read_back(&self, p: &Value) -> Option<Result<Value, String>> {
        let t = p["title"].as_str().unwrap_or("").to_string();
        let found = self.svc.published.lock().unwrap().contains(&t);
        Some(Ok(json!({"published": found, "title": t})))
    }
}

struct Rig {
    tmp: Tmp,
    c: Controller,
    write: Arc<WriteFile>,
    svc: Arc<MockExternal>,
}

fn rig_with(tag: &str, tweak: impl FnOnce(&mut Registry)) -> Rig {
    let tmp = Tmp::new(tag);
    let write = Arc::new(WriteFile::new(
        "test.write_file",
        ActionClass::ReversibleLocal,
    ));
    let svc = Arc::new(MockExternal::default());
    let mut ops: Registry = HashMap::new();
    ops.insert("test.write_file", write.clone());
    ops.insert(
        "test.publish",
        Arc::new(Publish {
            spec: spec("test.publish", ActionClass::ConsequentialExternal),
            svc: svc.clone(),
        }),
    );
    let mut read_only = WriteFile::new("test.read_only", ActionClass::ReadOnly);
    read_only.spec.action_id = "test.read_only";
    ops.insert("test.read_only", Arc::new(read_only));
    tweak(&mut ops);
    let c = Controller::new(Ledger::at(tmp.path("execution.db")), ops);
    Rig { tmp, c, write, svc }
}

fn rig(tag: &str) -> Rig {
    rig_with(tag, |_| {})
}

fn req(action: &str, key: &str, origin: Origin, params: Value) -> ExecRequest {
    ExecRequest {
        action_id: action.into(),
        workflow_id: "wf-1".into(),
        idempotency_key: key.into(),
        origin,
        params,
        approval_id: None,
    }
}

fn statuses(c: &Controller, id: &str) -> Vec<&'static str> {
    c.ledger
        .history(id)
        .unwrap()
        .iter()
        .map(|t| t.to_status.as_str())
        .collect()
}

// ── Success and verification ─────────────────────────────────────────────────

#[test]
fn successful_change_is_verified_by_independent_read_back() {
    let r = rig("ok");
    let path = r.tmp.path("note.txt");
    let out =
        r.c.submit(&req(
            "test.write_file",
            "k1",
            Origin::User,
            json!({"path": path, "content": "hello"}),
        ))
        .unwrap();
    let rec = out.record();
    assert_eq!(rec.status, ExecStatus::Verified);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    assert_eq!(rec.verification_method.as_deref(), Some("read_back"));
    assert_eq!(rec.verification_result.as_deref(), Some("match"));
    assert_eq!(
        statuses(&r.c, &rec.execution_id),
        [
            "REQUESTED",
            "AUTHORIZED",
            "ATTEMPTED",
            "EXECUTED",
            "VERIFIED"
        ]
    );
    // The response was captured, redacted.
    let resp = rec.response.as_deref().unwrap();
    assert!(resp.contains("\"ok\":true") && !resp.contains("sk-should"));
    assert_eq!(rec.attempts, 1);
    assert_eq!(rec.target_ref.as_deref(), Some(path.to_str().unwrap()));
}

#[test]
fn a_tool_that_reports_success_without_changing_anything_fails_verification() {
    let r = rig_with("lie", |ops| {
        let mut w = WriteFile::new("test.write_file", ActionClass::ReversibleLocal);
        w.lie = true;
        ops.insert("test.write_file", Arc::new(w));
    });
    let path = r.tmp.path("never.txt");
    let rec =
        r.c.submit(&req(
            "test.write_file",
            "k1",
            Origin::User,
            json!({"path": path, "content": "x"}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::Failed);
    assert!(!path.exists());
    assert_eq!(rec.verification_method.as_deref(), Some("read_back"));
    assert_eq!(
        statuses(&r.c, &rec.execution_id),
        ["REQUESTED", "AUTHORIZED", "ATTEMPTED", "EXECUTED", "FAILED"]
    );
    // What the read-back actually saw is recorded, not a made-up result.
    assert_eq!(
        rec.verification_result.as_deref(),
        Some("mismatch: actual {\"content\":null}")
    );
}

#[test]
fn mismatched_destination_state_is_a_verification_failure() {
    let r = rig("mismatch");
    let path = r.tmp.path("n.txt");
    std::fs::write(&path, "other").unwrap();
    let lie = Arc::new({
        let mut w = WriteFile::new("test.write_file", ActionClass::ReversibleLocal);
        w.lie = true;
        w
    });
    let mut ops: Registry = HashMap::new();
    ops.insert("test.write_file", lie);
    let c = Controller::new(Ledger::at(r.tmp.path("execution2.db")), ops);
    let rec = c
        .submit(&req(
            "test.write_file",
            "k1",
            Origin::User,
            json!({"path": path, "content": "want"}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::Failed);
    assert!(rec.verification_result.unwrap().starts_with("mismatch"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "other");
}

#[test]
fn an_operation_without_read_back_is_recorded_unverified_not_verified() {
    struct NoReadBack(OperationSpec);
    impl Operation for NoReadBack {
        fn spec(&self) -> &OperationSpec {
            &self.0
        }
        fn execute(&self, _p: &Value) -> Result<Value, OpError> {
            Ok(json!({"sent": true}))
        }
    }
    let r = rig_with("norb", |ops| {
        ops.insert(
            "test.no_read_back",
            Arc::new(NoReadBack(spec(
                "test.no_read_back",
                ActionClass::ReversibleLocal,
            ))),
        );
    });
    let rec =
        r.c.submit(&req("test.no_read_back", "k1", Origin::User, json!({})))
            .unwrap()
            .record()
            .clone();
    assert_eq!(rec.status, ExecStatus::Unverified);
    assert_eq!(rec.verification_result.as_deref(), Some("no_read_back"));
}

// ── Failures, timeouts, retries, partial ────────────────────────────────────

#[test]
fn a_failed_write_is_failed_with_the_error_and_nothing_written() {
    let r = rig("failwrite");
    let blocker = r.tmp.path("not-a-dir");
    std::fs::write(&blocker, "file").unwrap();
    let path = blocker.join("x.txt");
    let rec =
        r.c.submit(&req(
            "test.write_file",
            "k1",
            Origin::User,
            json!({"path": path, "content": "x"}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::Failed);
    assert!(rec.error.is_some());
    assert!(!path.exists());
    assert_eq!(
        statuses(&r.c, &rec.execution_id),
        ["REQUESTED", "AUTHORIZED", "ATTEMPTED", "FAILED"]
    );
}

#[test]
fn a_timeout_is_never_resubmitted_and_is_reconciled_later_by_read_back() {
    let slow = Arc::new({
        let mut w = WriteFile::new("test.write_file", ActionClass::ReversibleLocal);
        w.delay = Duration::from_millis(400);
        w.spec.timeout = Duration::from_millis(50);
        w.spec.retry = RetryPolicy::SafeIdempotent { max_attempts: 3 };
        w
    });
    let s2 = slow.clone();
    let r = rig_with("timeout", move |ops| {
        ops.insert("test.write_file", s2);
    });
    let path = r.tmp.path("late.txt");
    let params = json!({"path": path, "content": "late"});
    let rec =
        r.c.submit(&req("test.write_file", "k1", Origin::User, params.clone()))
            .unwrap()
            .record()
            .clone();
    assert_eq!(rec.status, ExecStatus::Unverified);
    assert_eq!(rec.verification_result.as_deref(), Some("outcome_unknown"));
    assert!(rec.recovery.as_deref().unwrap().contains("not resubmitted"));
    // Retries were allowed, but a timeout is never retried blindly.
    assert_eq!(slow.runs.load(Ordering::SeqCst), 1);
    // The slow write lands later; reconciliation reads it back and settles the state.
    std::thread::sleep(Duration::from_millis(600));
    let rec = r.c.reconcile(&rec.execution_id, &params).unwrap();
    assert_eq!(rec.status, ExecStatus::Verified);
    assert_eq!(slow.runs.load(Ordering::SeqCst), 1);
    assert_eq!(
        statuses(&r.c, &rec.execution_id),
        [
            "REQUESTED",
            "AUTHORIZED",
            "ATTEMPTED",
            "UNVERIFIED",
            "VERIFIED"
        ]
    );
}

#[test]
fn a_failure_before_sending_is_retried_safely_and_each_attempt_is_in_the_history() {
    let flaky = Arc::new(Flaky {
        spec: OperationSpec {
            retry: RetryPolicy::SafeIdempotent { max_attempts: 3 },
            ..spec("test.flaky", ActionClass::ReversibleLocal)
        },
        fail_first: 2,
        runs: AtomicUsize::new(0),
    });
    let f2 = flaky.clone();
    let r = rig_with("flaky", move |ops| {
        ops.insert("test.flaky", f2);
    });
    let path = r.tmp.path("f.txt");
    let rec =
        r.c.submit(&req(
            "test.flaky",
            "k1",
            Origin::User,
            json!({"path": path, "content": "ok"}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::Verified);
    assert_eq!(rec.attempts, 3);
    assert_eq!(flaky.runs.load(Ordering::SeqCst), 3);
    assert_eq!(
        statuses(&r.c, &rec.execution_id),
        [
            "REQUESTED",
            "AUTHORIZED",
            "ATTEMPTED",
            "ATTEMPTED",
            "ATTEMPTED",
            "EXECUTED",
            "VERIFIED"
        ]
    );
}

#[test]
fn a_partial_multi_step_run_is_partial_with_steps_and_compensation_recorded() {
    let r = rig_with("partial", |ops| {
        ops.insert(
            "test.multi",
            Arc::new(MultiStep {
                spec: spec("test.multi", ActionClass::ReversibleLocal),
            }),
        );
    });
    let dir = r.tmp.path("steps");
    std::fs::create_dir_all(&dir).unwrap();
    let rec =
        r.c.submit(&req(
            "test.multi",
            "k1",
            Origin::User,
            json!({"dir": dir, "fail_at": 2}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::Partial);
    let recovery: Value = serde_json::from_str(rec.recovery.as_deref().unwrap()).unwrap();
    assert_eq!(recovery["completed_steps"], json!(["a", "b"]));
    assert_eq!(recovery["compensation"], "compensated");
    // Compensation really undid the completed steps.
    assert!(!dir.join("a").exists() && !dir.join("b").exists() && !dir.join("c").exists());
}

#[test]
fn a_local_multi_record_change_is_all_or_nothing() {
    let r = rig_with("tx", |ops| {
        ops.insert(
            "test.two_records",
            Arc::new(TwoRecords {
                spec: spec("test.two_records", ActionClass::ReversibleLocal),
            }),
        );
    });
    let db = r.tmp.path("records.db");
    let rec =
        r.c.submit(&req(
            "test.two_records",
            "k1",
            Origin::User,
            json!({"db": db, "fail_second": true}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::Failed);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM r", [], |x| x.get(0))
        .unwrap();
    assert_eq!(n, 0, "the first record must have been rolled back");
    let rec =
        r.c.submit(&req(
            "test.two_records",
            "k2",
            Origin::User,
            json!({"db": db}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::Verified);
}

// ── Duplicates and concurrency ──────────────────────────────────────────────

#[test]
fn a_duplicate_request_never_executes_twice() {
    let r = rig("dup");
    let path = r.tmp.path("d.txt");
    let q = req(
        "test.write_file",
        "same-key",
        Origin::User,
        json!({"path": path, "content": "1"}),
    );
    let first = r.c.submit(&q).unwrap();
    let second = r.c.submit(&q).unwrap();
    assert!(matches!(first, Submitted::Ran(_)));
    assert!(matches!(second, Submitted::Duplicate(_)));
    assert_eq!(second.record().execution_id, first.record().execution_id);
    assert_eq!(r.write.runs.load(Ordering::SeqCst), 1);
    assert_eq!(r.c.ledger.recent(10).unwrap().len(), 1);
}

#[test]
fn concurrent_submissions_of_one_request_execute_once() {
    let r = rig("race");
    let path = r.tmp.path("race.txt");
    let q = req(
        "test.write_file",
        "race-key",
        Origin::User,
        json!({"path": path, "content": "x"}),
    );
    let results: Vec<Submitted> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..8)
            .map(|_| s.spawn(|| r.c.submit(&q).unwrap()))
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        results
            .iter()
            .filter(|x| matches!(x, Submitted::Ran(_)))
            .count(),
        1
    );
    assert_eq!(r.write.runs.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_transitions_of_one_execution_have_exactly_one_winner() {
    let r = rig("cas");
    let rec =
        r.c.ledger
            .insert(&NewExecution {
                workflow_id: "w",
                idempotency_key: "k",
                origin: "user",
                action_id: "test.write_file",
                tool_id: "t",
                target_system: "s",
                target_ref: None,
                action_class: "reversible_local",
                risk: "low",
                data_scope: "none",
                params_redacted: "{}",
                params_fingerprint: "f",
                expected_state: None,
            })
            .unwrap();
    let wins: usize = std::thread::scope(|s| {
        let hs: Vec<_> = [
            ExecStatus::Authorized,
            ExecStatus::Cancelled,
            ExecStatus::NotAuthorized,
        ]
        .into_iter()
        .cycle()
        .take(9)
        .map(|to| {
            let c = &r.c;
            let id = rec.execution_id.clone();
            s.spawn(move || {
                c.ledger
                    .transition(
                        &id,
                        ExecStatus::Requested,
                        to,
                        &Fields::default(),
                        &json!({}),
                    )
                    .is_ok()
            })
        })
        .collect();
        hs.into_iter().map(|h| h.join().unwrap() as usize).sum()
    });
    assert_eq!(wins, 1);
    assert_eq!(statuses(&r.c, &rec.execution_id).len(), 2);
}

#[test]
fn illegal_transitions_are_refused_and_history_is_append_only() {
    let r = rig("legal");
    let path = r.tmp.path("l.txt");
    let rec =
        r.c.submit(&req(
            "test.write_file",
            "k",
            Origin::User,
            json!({"path": path, "content": "x"}),
        ))
        .unwrap()
        .record()
        .clone();
    let e =
        r.c.ledger
            .transition(
                &rec.execution_id,
                ExecStatus::Verified,
                ExecStatus::Attempted,
                &Fields::default(),
                &json!({}),
            )
            .unwrap_err();
    assert!(matches!(e, LedgerError::IllegalTransition(..)));
    let conn = r.c.ledger.open().unwrap();
    assert!(conn
        .execute("UPDATE execution_transitions SET to_status = 'FAILED'", [])
        .is_err());
    assert!(conn
        .execute("DELETE FROM execution_transitions", [])
        .is_err());
    assert_eq!(statuses(&r.c, &rec.execution_id).last(), Some(&"VERIFIED"));
}

// ── Authorization and approvals ─────────────────────────────────────────────

#[test]
fn a_model_cannot_change_state_without_a_user_approval() {
    let r = rig("unauth");
    let path = r.tmp.path("u.txt");
    let rec =
        r.c.submit(&req(
            "test.write_file",
            "k1",
            Origin::Model,
            json!({"path": path, "content": "x"}),
        ))
        .unwrap()
        .record()
        .clone();
    assert_eq!(rec.status, ExecStatus::NotAuthorized);
    assert_eq!(rec.error.as_deref(), Some("approval_required"));
    assert!(!path.exists());
    assert_eq!(r.write.runs.load(Ordering::SeqCst), 0);
    assert_eq!(
        statuses(&r.c, &rec.execution_id),
        ["REQUESTED", "NOT_AUTHORIZED"]
    );
    // A made-up approval id doesn't help.
    let mut q = req(
        "test.write_file",
        "k2",
        Origin::Model,
        json!({"path": path, "content": "x"}),
    );
    q.approval_id = Some("forged".into());
    assert_eq!(
        r.c.submit(&q).unwrap().record().error.as_deref(),
        Some("approval_missing")
    );
    // Read-only needs no approval; consequential external needs one even from the user.
    let ro =
        r.c.submit(&req(
            "test.read_only",
            "k3",
            Origin::Model,
            json!({"path": r.tmp.path("ro.txt"), "content": "r"}),
        ))
        .unwrap();
    assert_eq!(ro.record().status, ExecStatus::Verified);
    let pub_ =
        r.c.submit(&req(
            "test.publish",
            "k4",
            Origin::User,
            json!({"title": "Hello"}),
        ))
        .unwrap();
    assert_eq!(pub_.record().status, ExecStatus::NotAuthorized);
    assert!(r.svc.published.lock().unwrap().is_empty());
}

#[test]
fn an_approved_external_operation_runs_once_and_is_verified() {
    let r = rig("approved");
    let params = json!({"title": "Launch"});
    let aid =
        r.c.grant_user_approval("test.publish", &params, Duration::from_secs(60))
            .unwrap();
    let mut q = req("test.publish", "k1", Origin::Model, params.clone());
    q.approval_id = Some(aid.clone());
    let rec = r.c.submit(&q).unwrap().record().clone();
    assert_eq!(rec.status, ExecStatus::Verified);
    assert_eq!(rec.approval_id.as_deref(), Some(aid.as_str()));
    assert_eq!(*r.svc.published.lock().unwrap(), vec!["Launch".to_string()]);
    // The approval is single-use.
    let mut again = req("test.publish", "k2", Origin::Model, params);
    again.approval_id = Some(aid);
    let rec2 = r.c.submit(&again).unwrap().record().clone();
    assert_eq!(rec2.status, ExecStatus::NotAuthorized);
    assert_eq!(rec2.error.as_deref(), Some("approval_already_used"));
    assert_eq!(r.svc.published.lock().unwrap().len(), 1);
}

fn far_future() -> DateTime<Utc> {
    Utc::now() + ChronoDuration::days(1)
}

#[test]
fn an_expired_approval_does_not_authorize() {
    let r = rig("expired");
    let params = json!({"title": "Old"});
    let aid =
        r.c.grant_user_approval("test.publish", &params, Duration::from_secs(60))
            .unwrap();
    let later = Controller::new(Ledger::at(r.c.ledger.path().to_path_buf()), {
        let mut ops: Registry = HashMap::new();
        ops.insert(
            "test.publish",
            Arc::new(Publish {
                spec: spec("test.publish", ActionClass::ConsequentialExternal),
                svc: r.svc.clone(),
            }),
        );
        ops
    })
    .with_clock(far_future);
    let mut q = req("test.publish", "k1", Origin::Model, params);
    q.approval_id = Some(aid);
    let rec = later.submit(&q).unwrap().record().clone();
    assert_eq!(rec.status, ExecStatus::NotAuthorized);
    assert_eq!(rec.error.as_deref(), Some("approval_expired"));
    assert!(r.svc.published.lock().unwrap().is_empty());
}

#[test]
fn changed_parameters_after_approval_need_a_new_approval() {
    let r = rig("changed");
    let aid =
        r.c.grant_user_approval(
            "test.publish",
            &json!({"title": "Approved title"}),
            Duration::from_secs(60),
        )
        .unwrap();
    let mut q = req(
        "test.publish",
        "k1",
        Origin::Model,
        json!({"title": "Different title"}),
    );
    q.approval_id = Some(aid.clone());
    let rec = r.c.submit(&q).unwrap().record().clone();
    assert_eq!(rec.status, ExecStatus::NotAuthorized);
    assert_eq!(
        rec.error.as_deref(),
        Some("parameters_changed_after_approval")
    );
    assert!(r.svc.published.lock().unwrap().is_empty());
    // Key order doesn't count as a change: the fingerprint is canonical.
    assert_eq!(
        fingerprint("a", &json!({"x": 1, "y": 2})),
        fingerprint("a", &json!({"y": 2, "x": 1}))
    );
    // An approval for another action doesn't transfer.
    let mut q2 = req(
        "test.write_file",
        "k2",
        Origin::Model,
        json!({"path": r.tmp.path("z"), "content": "z"}),
    );
    q2.approval_id = Some(aid);
    assert_eq!(
        r.c.submit(&q2).unwrap().record().error.as_deref(),
        Some("approval_other_action")
    );
}

#[test]
fn high_risk_needs_approval_even_when_the_user_started_it() {
    let mut s = spec("x", ActionClass::ReversibleLocal);
    s.risk = Risk::High;
    assert_eq!(
        required_authorization(&s, Origin::User),
        Requirement::UserApproval
    );
    let s = spec("x", ActionClass::Destructive);
    assert_eq!(
        required_authorization(&s, Origin::User),
        Requirement::UserApproval
    );
    let s = spec("x", ActionClass::ReversibleLocal);
    assert_eq!(required_authorization(&s, Origin::User), Requirement::None);
    assert_eq!(
        required_authorization(&s, Origin::Scheduler),
        Requirement::UserApproval
    );
}

// ── Restart and recovery ────────────────────────────────────────────────────

/// Leaves an execution in `status`, as a crash would.
fn crashed_at(c: &Controller, key: &str, params: &Value, status: ExecStatus) -> String {
    let rec = c
        .ledger
        .insert(&NewExecution {
            workflow_id: "w",
            idempotency_key: key,
            origin: "model",
            action_id: "test.write_file",
            tool_id: "test.tool",
            target_system: "test_fs",
            target_ref: None,
            action_class: "reversible_local",
            risk: "low",
            data_scope: "none",
            params_redacted: &params.to_string(),
            params_fingerprint: &fingerprint("test.write_file", params),
            expected_state: Some(&json!({"content": params["content"]}).to_string()),
        })
        .unwrap();
    let id = rec.execution_id;
    let path = [
        ExecStatus::Authorized,
        ExecStatus::Attempted,
        ExecStatus::Executed,
    ];
    let mut cur = ExecStatus::Requested;
    for s in path {
        if cur == status {
            break;
        }
        c.ledger
            .transition(&id, cur, s, &Fields::default(), &json!({}))
            .unwrap();
        cur = s;
    }
    id
}

#[test]
fn restart_during_execution_is_recovered_without_re_running_anything() {
    let r = rig("restart");
    // Crashed mid-attempt, but the write had landed.
    let landed = json!({"path": r.tmp.path("landed.txt"), "content": "yes"});
    std::fs::write(r.tmp.path("landed.txt"), "yes").unwrap();
    let a = crashed_at(&r.c, "a", &landed, ExecStatus::Attempted);
    // Crashed mid-attempt, write never landed.
    let lost = json!({"path": r.tmp.path("lost.txt"), "content": "no"});
    let b = crashed_at(&r.c, "b", &lost, ExecStatus::Attempted);
    // Authorized but never attempted.
    let never = json!({"path": r.tmp.path("never.txt"), "content": "n"});
    let c_ = crashed_at(&r.c, "c", &never, ExecStatus::Authorized);
    // Executed but not yet verified.
    let exec = json!({"path": r.tmp.path("exec.txt"), "content": "e"});
    std::fs::write(r.tmp.path("exec.txt"), "e").unwrap();
    let d = crashed_at(&r.c, "d", &exec, ExecStatus::Executed);

    // A fresh controller over the same ledger file = the app after restart.
    let after = rig_with("unused", |_| {});
    let restarted = Controller::new(Ledger::at(r.c.ledger.path().to_path_buf()), {
        let mut ops: Registry = HashMap::new();
        ops.insert("test.write_file", after.write.clone());
        ops
    });
    assert_eq!(restarted.recover_after_restart().unwrap(), 4);
    let get = |id: &str| restarted.ledger.get(id).unwrap().unwrap();
    assert_eq!(get(&a).status, ExecStatus::Verified);
    assert!(get(&a)
        .recovery
        .unwrap()
        .contains("restarted during the attempt"));
    assert_eq!(get(&b).status, ExecStatus::Unverified);
    assert_eq!(get(&c_).status, ExecStatus::Cancelled);
    assert_eq!(get(&d).status, ExecStatus::Verified);
    // Nothing was executed again, and nothing new appeared on disk.
    assert_eq!(after.write.runs.load(Ordering::SeqCst), 0);
    assert!(!r.tmp.path("lost.txt").exists() && !r.tmp.path("never.txt").exists());
    // Recovery is idempotent.
    assert_eq!(restarted.recover_after_restart().unwrap(), 0);
}

#[test]
fn an_uncertain_execution_reconciles_once_the_destination_shows_the_change() {
    let r = rig("reconcile");
    let p = json!({"path": r.tmp.path("r.txt"), "content": "v"});
    let id = crashed_at(&r.c, "k", &p, ExecStatus::Attempted);
    r.c.recover_after_restart().unwrap();
    assert_eq!(
        r.c.ledger.get(&id).unwrap().unwrap().status,
        ExecStatus::Unverified
    );
    // Still absent: stays UNVERIFIED (never invented).
    assert_eq!(
        r.c.reconcile(&id, &p).unwrap().status,
        ExecStatus::Unverified
    );
    std::fs::write(r.tmp.path("r.txt"), "v").unwrap();
    assert_eq!(r.c.reconcile(&id, &p).unwrap().status, ExecStatus::Verified);
    assert_eq!(r.write.runs.load(Ordering::SeqCst), 0);
}

// ── Redaction, storage, schema ──────────────────────────────────────────────

#[test]
fn secrets_and_personal_fields_never_reach_the_ledger_file() {
    let r = rig("redact");
    let path = r.tmp.path("p.txt");
    r.c.submit(&req(
        "test.write_file",
        "k",
        Origin::User,
        json!({"path": path, "content": "x", "api_key": "sk-live-abcdefghijklmnopqrstuv", "diary": "very private"}),
    ))
    .unwrap();
    drop(r.c.ledger.open().unwrap());
    let mut raw = Vec::new();
    for f in ["execution.db", "execution.db-wal"] {
        if let Ok(b) = std::fs::read(r.tmp.path(f)) {
            raw.extend(b);
        }
    }
    let text = String::from_utf8_lossy(&raw);
    assert!(!text.contains("sk-live-abcdefghij"));
    assert!(!text.contains("very private"));
    assert!(!text.contains("sk-should-never"));
}

#[test]
fn a_large_response_is_stored_by_reference() {
    struct Big(OperationSpec);
    impl Operation for Big {
        fn spec(&self) -> &OperationSpec {
            &self.0
        }
        fn execute(&self, _p: &Value) -> Result<Value, OpError> {
            let m: serde_json::Map<String, Value> = (0..3000)
                .map(|i| (format!("field_{i}"), json!("value")))
                .collect();
            Ok(Value::Object(m))
        }
    }
    let r = rig_with("big", |ops| {
        ops.insert(
            "test.big",
            Arc::new(Big(spec("test.big", ActionClass::ReadOnly))),
        );
    });
    let rec =
        r.c.submit(&req("test.big", "k", Origin::Model, json!({})))
            .unwrap()
            .record()
            .clone();
    assert!(rec.response.is_none());
    let reference = rec.response_ref.unwrap();
    assert!(r.tmp.0.join(&reference).is_file());
}

#[test]
fn a_ledger_that_cannot_be_written_fails_loudly_and_runs_nothing() {
    let r = rig("nostore");
    let blocker = r.tmp.path("blocker");
    std::fs::write(&blocker, "file").unwrap();
    let w = Arc::new(WriteFile::new(
        "test.write_file",
        ActionClass::ReversibleLocal,
    ));
    let mut ops: Registry = HashMap::new();
    ops.insert("test.write_file", w.clone());
    let c = Controller::new(Ledger::at(blocker.join("execution.db")), ops);
    let e = c
        .submit(&req(
            "test.write_file",
            "k",
            Origin::User,
            json!({"path": r.tmp.path("x"), "content": "x"}),
        ))
        .unwrap_err();
    assert!(matches!(e, ControllerError::Ledger(_)));
    assert_eq!(w.runs.load(Ordering::SeqCst), 0);
    assert!(!r.tmp.path("x").exists());
}

#[test]
fn schema_version_is_recorded_and_a_newer_file_is_refused() {
    let r = rig("schema");
    let conn = r.c.ledger.open().unwrap();
    let v: String = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |x| x.get(0),
        )
        .unwrap();
    assert_eq!(v, ledger::SCHEMA_VERSION.to_string());
    conn.execute(
        "UPDATE schema_meta SET value = '99' WHERE key='schema_version'",
        [],
    )
    .unwrap();
    drop(conn);
    assert!(r.c.ledger.open().is_err());
}

#[test]
fn unknown_actions_and_invalid_parameters_are_refused_before_any_record() {
    let r = rig("unknown");
    assert!(matches!(
        r.c.submit(&req(
            "shell.run",
            "k",
            Origin::Model,
            json!({"cmd": "rm -rf /"})
        )),
        Err(ControllerError::UnknownAction(_))
    ));
    assert!(matches!(
        r.c.submit(&req("test.write_file", "k2", Origin::User, json!({}))),
        Err(ControllerError::InvalidParams(_))
    ));
    assert!(r.c.ledger.recent(10).unwrap().is_empty());
}

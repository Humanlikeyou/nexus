//! The execution controller (decision 0052): one Rust-side path for state-changing operations that
//! Amara initiates.
//!
//! A caller (the model-action path, a scheduler job, a future Life/Enterprise/Workout/NEUROSENSE
//! module) submits an [`ExecRequest`] naming a registered [`Operation`]. The controller owns:
//! authorization and approval checks, the state machine, invocation with a timeout, response
//! capture, independent verification by read-back, retries, recovery after restart, and the
//! structured ledger ([`ledger`]). The model can propose; it can never grant an approval: approvals
//! are created only by [`Controller::grant_user_approval`], which is called from native-dialog
//! flows in Rust and has no IPC command.
//!
//! Operations are a closed registry of Rust implementations. There is no generic shell, filesystem
//! or network operation, so nothing here widens what a model can reach.
//!
//! Direct user workflows (a click in the app) keep their own paths; they may use the controller
//! with [`Origin::User`], where reversible local changes need no extra approval.

pub(crate) mod ledger;
pub(crate) mod redact;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub(crate) use ledger::{ExecStatus, ExecutionRecord, Fields, Ledger, LedgerError, NewExecution};

// ─── Policy vocabulary ────────────────────────────────────────────────────────

/// The interface vocabulary for every module. Variants without a registered operation yet (read-only,
/// destructive) are part of the contract and are exercised by the tests.
#[allow(dead_code)]
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    ReadOnly,
    /// Writes an inactive draft (a skill version awaiting review). No effect until a separate,
    /// approved activation.
    Staging,
    ReversibleLocal,
    ConsequentialExternal,
    Destructive,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
}

/// Business and Neural are for the Enterprise and NEUROSENSE modules (no operations yet).
#[allow(dead_code)]
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataScope {
    None,
    Personal,
    Business,
    Neural,
    System,
}

/// Who started the request. `Model` covers anything a model proposed, wherever it came from.
/// `Scheduler` is for background jobs once they move onto the controller.
#[allow(dead_code)]
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    User,
    Model,
    Scheduler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryPolicy {
    Never,
    /// Retry only failures that certainly did not reach the target (`submitted: false`).
    SafeIdempotent {
        max_attempts: u8,
    },
}

#[derive(Clone, Debug)]
pub struct OperationSpec {
    /// What is done (`system.set_volume`, `skill.store`).
    pub action_id: &'static str,
    /// Which implementation does it.
    pub tool_id: &'static str,
    pub target_system: &'static str,
    pub class: ActionClass,
    pub risk: Risk,
    pub scope: DataScope,
    pub timeout: Duration,
    pub retry: RetryPolicy,
    /// Parameter names whose values are personal data and never stored.
    pub personal_fields: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    None,
    UserApproval,
}

/// The one permission table. High risk, consequential external and destructive operations always
/// need the user's explicit approval; reversible local changes need it unless the user started them.
pub(crate) fn required_authorization(spec: &OperationSpec, origin: Origin) -> Requirement {
    if spec.risk == Risk::High {
        return Requirement::UserApproval;
    }
    match spec.class {
        ActionClass::ReadOnly | ActionClass::Staging => Requirement::None,
        ActionClass::ReversibleLocal if origin == Origin::User => Requirement::None,
        ActionClass::ReversibleLocal => Requirement::UserApproval,
        ActionClass::ConsequentialExternal | ActionClass::Destructive => Requirement::UserApproval,
    }
}

// ─── Operations ───────────────────────────────────────────────────────────────

/// `Partial` is for multi-step operations (none registered yet; the tests cover it).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum OpError {
    /// Failed. `submitted` says whether the target may have received the request.
    Failed { message: String, submitted: bool },
    /// Some steps ran. `completed` names them, in order.
    Partial {
        message: String,
        completed: Vec<String>,
    },
}

/// One kind of state change. Implementations are plain Rust: no shell or network passthrough.
pub trait Operation: Send + Sync {
    fn spec(&self) -> &OperationSpec;
    fn validate(&self, _params: &Value) -> Result<(), String> {
        Ok(())
    }
    fn target_ref(&self, _params: &Value) -> Option<String> {
        None
    }
    /// What the destination should look like afterwards, for verification.
    fn expected_state(&self, _params: &Value) -> Option<Value> {
        None
    }
    fn execute(&self, params: &Value) -> Result<Value, OpError>;
    /// Reads the destination independently of `execute`. `None` = no read-back for this operation.
    fn read_back(&self, _params: &Value) -> Option<Result<Value, String>> {
        None
    }
    fn matches(&self, expected: &Value, actual: &Value) -> bool {
        expected == actual
    }
    /// Undo for completed steps of a partial run. `None` = no compensation exists.
    fn compensate(&self, _params: &Value, _completed: &[String]) -> Option<Result<(), String>> {
        None
    }
}

pub(crate) type Registry = HashMap<&'static str, Arc<dyn Operation>>;

// ─── Requests and results ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ExecRequest {
    pub action_id: String,
    /// Groups related executions (a conversation turn, a job run).
    pub workflow_id: String,
    /// Same key = same request: never executed twice.
    pub idempotency_key: String,
    pub origin: Origin,
    pub params: Value,
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Submitted {
    /// This call ran the request to the state in the record.
    Ran(ExecutionRecord),
    /// The key was seen before. Nothing ran; this is the earlier execution.
    Duplicate(ExecutionRecord),
}

impl Submitted {
    pub fn record(&self) -> &ExecutionRecord {
        match self {
            Self::Ran(r) | Self::Duplicate(r) => r,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ControllerError {
    UnknownAction(String),
    InvalidParams(String),
    Ledger(String),
}

impl std::fmt::Display for ControllerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAction(a) => write!(f, "no registered operation for {a}"),
            Self::InvalidParams(e) => write!(f, "invalid parameters: {e}"),
            Self::Ledger(e) => write!(f, "{e}"),
        }
    }
}

impl From<LedgerError> for ControllerError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e.to_string())
    }
}

/// Canonical JSON (object keys sorted) so equal parameters always hash the same.
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let mut m = serde_json::Map::new();
            for k in keys {
                m.insert(k.clone(), canonical(&o[k]));
            }
            Value::Object(m)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        x => x.clone(),
    }
}

/// The fingerprint an approval is bound to: action plus exact (canonical) parameters.
pub(crate) fn fingerprint(action_id: &str, params: &Value) -> String {
    let mut h = Sha256::new();
    h.update(action_id.as_bytes());
    h.update([0u8]);
    h.update(canonical(params).to_string().as_bytes());
    hex::encode(h.finalize())
}

/// A response larger than this goes to a file next to the ledger and the row keeps a reference.
const MAX_INLINE_RESPONSE: usize = 16 * 1024;

// ─── Controller ───────────────────────────────────────────────────────────────

pub(crate) struct Controller {
    pub(crate) ledger: Ledger,
    ops: Registry,
    /// Injected clock (tests move time to expire approvals).
    now: fn() -> DateTime<Utc>,
}

impl Controller {
    pub(crate) fn new(ledger: Ledger, ops: Registry) -> Self {
        Self {
            ledger,
            ops,
            now: Utc::now,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_clock(mut self, now: fn() -> DateTime<Utc>) -> Self {
        self.now = now;
        self
    }

    fn op(&self, action_id: &str) -> Result<Arc<dyn Operation>, ControllerError> {
        self.ops
            .get(action_id)
            .cloned()
            .ok_or_else(|| ControllerError::UnknownAction(action_id.to_string()))
    }

    /// Called only from Rust user-approval flows (a native dialog the user answered). Not exposed
    /// over IPC, so neither the webview nor a model can mint one.
    pub(crate) fn grant_user_approval(
        &self,
        action_id: &str,
        params: &Value,
        ttl: Duration,
    ) -> Result<String, ControllerError> {
        let expires = (self.now)()
            + ChronoDuration::from_std(ttl).unwrap_or_else(|_| ChronoDuration::seconds(60));
        Ok(self.ledger.grant_approval(
            action_id,
            &fingerprint(action_id, params),
            "user",
            expires,
        )?)
    }

    /// Runs one request to a resting state (VERIFIED, UNVERIFIED, FAILED, PARTIAL, NOT_AUTHORIZED).
    /// Blocking: call it from `spawn_blocking` in async code.
    pub(crate) fn submit(&self, req: &ExecRequest) -> Result<Submitted, ControllerError> {
        let op = self.op(&req.action_id)?;
        let spec = op.spec().clone();
        if let Some(existing) = self.ledger.by_idempotency_key(&req.idempotency_key)? {
            return Ok(Submitted::Duplicate(existing));
        }
        op.validate(&req.params)
            .map_err(|e| ControllerError::InvalidParams(redact::text(&e)))?;

        let fp = fingerprint(&req.action_id, &req.params);
        let params_red = redact::value(&req.params, spec.personal_fields).to_string();
        let expected = op.expected_state(&req.params);
        let expected_red = expected
            .as_ref()
            .map(|e| redact::value(e, spec.personal_fields).to_string());
        let target_ref = op.target_ref(&req.params).map(|t| redact::text(&t));
        let inserted = self.ledger.insert(&NewExecution {
            workflow_id: &req.workflow_id,
            idempotency_key: &req.idempotency_key,
            origin: origin_str(req.origin),
            action_id: &req.action_id,
            tool_id: spec.tool_id,
            target_system: spec.target_system,
            target_ref: target_ref.as_deref(),
            action_class: class_str(spec.class),
            risk: risk_str(spec.risk),
            data_scope: scope_str(spec.scope),
            params_redacted: &params_red,
            params_fingerprint: &fp,
            expected_state: expected_red.as_deref(),
        });
        let rec = match inserted {
            Ok(r) => r,
            // Lost an insert race on the same key: the other request owns it.
            Err(e) => match self.ledger.by_idempotency_key(&req.idempotency_key)? {
                Some(existing) => return Ok(Submitted::Duplicate(existing)),
                None => return Err(e.into()),
            },
        };
        let id = rec.execution_id.clone();

        // Authorization.
        if let Err(why) = self.authorize(&spec, req, &fp, &id)? {
            let r = self.ledger.transition(
                &id,
                ExecStatus::Requested,
                ExecStatus::NotAuthorized,
                &Fields {
                    error: Some(why.to_string()),
                    ..Default::default()
                },
                &json!({"reason": why}),
            )?;
            log_rest(&r);
            return Ok(Submitted::Ran(r));
        }
        self.ledger.transition(
            &id,
            ExecStatus::Requested,
            ExecStatus::Authorized,
            &Fields {
                approval_id: req.approval_id.clone(),
                ..Default::default()
            },
            &json!({"requirement": format!("{:?}", required_authorization(&spec, req.origin))}),
        )?;
        let r = self.attempt_and_verify(
            &id,
            &op,
            &req.params,
            expected.as_ref(),
            ExecStatus::Authorized,
        )?;
        log_rest(&r);
        Ok(Submitted::Ran(r))
    }

    fn authorize(
        &self,
        spec: &OperationSpec,
        req: &ExecRequest,
        fp: &str,
        execution_id: &str,
    ) -> Result<Result<(), &'static str>, ControllerError> {
        match required_authorization(spec, req.origin) {
            Requirement::None => Ok(Ok(())),
            Requirement::UserApproval => {
                let Some(aid) = req.approval_id.as_deref() else {
                    return Ok(Err("approval_required"));
                };
                Ok(self.ledger.consume_approval(
                    aid,
                    &req.action_id,
                    fp,
                    execution_id,
                    (self.now)(),
                )?)
            }
        }
    }

    /// From AUTHORIZED: attempt (with safe retries), capture the response, then verify.
    fn attempt_and_verify(
        &self,
        id: &str,
        op: &Arc<dyn Operation>,
        params: &Value,
        expected: Option<&Value>,
        from: ExecStatus,
    ) -> Result<ExecutionRecord, ControllerError> {
        let spec = op.spec().clone();
        let max = match spec.retry {
            RetryPolicy::Never => 1,
            RetryPolicy::SafeIdempotent { max_attempts } => max_attempts.max(1),
        };
        let mut state = from;
        for attempt in 1..=max {
            self.ledger.transition(
                id,
                state,
                ExecStatus::Attempted,
                &Fields {
                    bump_attempts: true,
                    ..Default::default()
                },
                &json!({"attempt": attempt, "timeout_ms": spec.timeout.as_millis() as u64}),
            )?;
            state = ExecStatus::Attempted;
            match run_with_timeout(Arc::clone(op), params.clone(), spec.timeout) {
                Outcome::Done(Ok(resp)) => {
                    let (inline, reference) =
                        self.capture(id, &redact::value(&resp, spec.personal_fields))?;
                    self.ledger.transition(
                        id,
                        ExecStatus::Attempted,
                        ExecStatus::Executed,
                        &Fields {
                            response: inline,
                            response_ref: reference,
                            ..Default::default()
                        },
                        &json!({"attempt": attempt}),
                    )?;
                    return self.verify(
                        id,
                        op.as_ref(),
                        params,
                        expected,
                        ExecStatus::Executed,
                        None,
                    );
                }
                Outcome::Done(Err(OpError::Failed { message, submitted })) => {
                    let msg = redact::text(&message);
                    let retry_safe = !submitted && attempt < max;
                    if retry_safe {
                        continue;
                    }
                    return Ok(self.ledger.transition(
                        id,
                        ExecStatus::Attempted,
                        ExecStatus::Failed,
                        &Fields {
                            error: Some(msg.clone()),
                            ..Default::default()
                        },
                        &json!({"attempt": attempt, "submitted": submitted}),
                    )?);
                }
                Outcome::Done(Err(OpError::Partial { message, completed })) => {
                    let compensation = match op.compensate(params, &completed) {
                        None => "none_available".to_string(),
                        Some(Ok(())) => "compensated".to_string(),
                        Some(Err(e)) => format!("compensation_failed: {}", redact::text(&e)),
                    };
                    let recovery =
                        json!({"completed_steps": completed, "compensation": compensation});
                    return Ok(self.ledger.transition(
                        id,
                        ExecStatus::Attempted,
                        ExecStatus::Partial,
                        &Fields {
                            error: Some(redact::text(&message)),
                            recovery: Some(recovery.to_string()),
                            ..Default::default()
                        },
                        &recovery,
                    )?);
                }
                Outcome::TimedOut => {
                    // The request may have landed. Never resubmit blindly: reconcile by read-back.
                    return self.reconcile_uncertain(
                        id,
                        op.as_ref(),
                        params,
                        expected,
                        "timed out after submission",
                    );
                }
            }
        }
        unreachable!("the loop always returns")
    }

    /// The outcome is unknown (timeout, restart). Read the destination; if that settles it, record
    /// it; otherwise leave UNVERIFIED with the reason. Never executes again.
    fn reconcile_uncertain(
        &self,
        id: &str,
        op: &dyn Operation,
        params: &Value,
        expected: Option<&Value>,
        why: &str,
    ) -> Result<ExecutionRecord, ControllerError> {
        if let (Some(exp), Some(Ok(actual))) = (expected, op.read_back(params)) {
            if op.matches(exp, &actual) {
                self.ledger.transition(
                    id,
                    ExecStatus::Attempted,
                    ExecStatus::Executed,
                    &Fields {
                        recovery: Some(format!("{why}; read-back shows it was applied")),
                        ..Default::default()
                    },
                    &json!({"reconciled": true, "why": why}),
                )?;
                return self.verify(id, op, params, expected, ExecStatus::Executed, Some(actual));
            }
        }
        Ok(self.ledger.transition(
            id,
            ExecStatus::Attempted,
            ExecStatus::Unverified,
            &Fields {
                verification_method: Some(if op.read_back(params).is_some() {
                    "read_back".into()
                } else {
                    "none".into()
                }),
                verification_result: Some("outcome_unknown".into()),
                recovery: Some(format!(
                    "{why}; not resubmitted; reconcile before any retry"
                )),
                ..Default::default()
            },
            &json!({"reconciled": false, "why": why}),
        )?)
    }

    /// Independent verification. A successful `execute` is not proof.
    fn verify(
        &self,
        id: &str,
        op: &dyn Operation,
        params: &Value,
        expected: Option<&Value>,
        from: ExecStatus,
        already_read: Option<Value>,
    ) -> Result<ExecutionRecord, ControllerError> {
        let personal = op.spec().personal_fields;
        let read = already_read.map(Ok).or_else(|| op.read_back(params));
        let (to, method, result, err) = match (expected, read) {
            (_, None) => (
                ExecStatus::Unverified,
                "none",
                "no_read_back".to_string(),
                None,
            ),
            (None, Some(_)) => (
                ExecStatus::Unverified,
                "read_back",
                "no_expected_state".to_string(),
                None,
            ),
            (Some(_), Some(Err(e))) => (
                ExecStatus::Unverified,
                "read_back",
                "read_back_failed".to_string(),
                Some(redact::text(&e)),
            ),
            (Some(exp), Some(Ok(actual))) if op.matches(exp, &actual) => {
                (ExecStatus::Verified, "read_back", "match".to_string(), None)
            }
            (Some(_), Some(Ok(actual))) => (
                ExecStatus::Failed,
                "read_back",
                format!("mismatch: actual {}", redact::value(&actual, personal)),
                Some("The destination does not show the expected change.".to_string()),
            ),
        };
        Ok(self.ledger.transition(
            id,
            from,
            to,
            &Fields {
                verification_method: Some(method.into()),
                verification_result: Some(redact::text(&result)),
                error: err,
                ..Default::default()
            },
            &json!({"verification": method, "result": redact::text(&result)}),
        )?)
    }

    fn capture(
        &self,
        id: &str,
        resp: &Value,
    ) -> Result<(Option<String>, Option<String>), ControllerError> {
        let text = resp.to_string();
        if text.len() <= MAX_INLINE_RESPONSE {
            return Ok((Some(text), None));
        }
        let dir = self.responses_dir();
        crate::app_paths::create_private_dir_all(&dir)
            .map_err(|e| ControllerError::Ledger(e.to_string()))?;
        let path = dir.join(format!("{id}.json"));
        crate::app_paths::write_private(&path, &text)
            .map_err(|e| ControllerError::Ledger(e.to_string()))?;
        Ok((None, Some(format!("execution_responses/{id}.json"))))
    }

    fn responses_dir(&self) -> PathBuf {
        self.ledger
            .path()
            .parent()
            .map(|p| p.join("execution_responses"))
            .unwrap_or_else(|| PathBuf::from("execution_responses"))
    }

    /// After a restart: nothing in flight is resumed or re-run. Requests never attempted are
    /// cancelled; attempts with an unknown outcome are reconciled by read-back or left UNVERIFIED;
    /// executed-but-unverified ones are verified now. Returns how many executions were touched.
    pub(crate) fn recover_after_restart(&self) -> Result<usize, ControllerError> {
        let open = self.ledger.in_status(&[
            ExecStatus::Requested,
            ExecStatus::Authorized,
            ExecStatus::Attempted,
            ExecStatus::Executed,
        ])?;
        let mut n = 0;
        for rec in open {
            let detail = json!({"recovery": "after_restart"});
            let result = match rec.status {
                ExecStatus::Requested | ExecStatus::Authorized => self
                    .ledger
                    .transition(
                        &rec.execution_id,
                        rec.status,
                        ExecStatus::Cancelled,
                        &Fields {
                            recovery: Some("app restarted before the attempt; not executed".into()),
                            ..Default::default()
                        },
                        &detail,
                    )
                    .map_err(ControllerError::from),
                ExecStatus::Attempted | ExecStatus::Executed => {
                    let op = self.ops.get(rec.action_id.as_str()).cloned();
                    // Only the redacted parameters survive a restart, so an operation whose
                    // read-back needs a redacted value reads back nothing useful and stays
                    // UNVERIFIED. That is the honest result, not a failure of recovery.
                    let stored: Value =
                        serde_json::from_str(&rec.params_redacted).unwrap_or(Value::Null);
                    let expected: Option<Value> = rec
                        .expected_state
                        .as_deref()
                        .and_then(|e| serde_json::from_str(e).ok());
                    match op {
                        Some(op) if rec.status == ExecStatus::Attempted => self
                            .reconcile_uncertain(
                                &rec.execution_id,
                                op.as_ref(),
                                &stored,
                                expected.as_ref(),
                                "app restarted during the attempt",
                            ),
                        Some(op) => self.verify(
                            &rec.execution_id,
                            op.as_ref(),
                            &stored,
                            expected.as_ref(),
                            ExecStatus::Executed,
                            None,
                        ),
                        None => self
                            .ledger
                            .transition(
                                &rec.execution_id,
                                rec.status,
                                ExecStatus::Unverified,
                                &Fields {
                                    verification_result: Some(
                                        "operation_no_longer_registered".into(),
                                    ),
                                    recovery: Some(
                                        "app restarted; operation unknown to this build".into(),
                                    ),
                                    ..Default::default()
                                },
                                &detail,
                            )
                            .map_err(ControllerError::from),
                    }
                }
                _ => continue,
            };
            match result {
                Ok(_) => n += 1,
                // Someone else moved it meanwhile: fine, it's no longer ours to recover.
                Err(ControllerError::Ledger(e)) if e.contains("changed underneath") => {}
                Err(e) => return Err(e),
            }
        }
        Ok(n)
    }

    /// Later reconciliation of an UNVERIFIED or PARTIAL execution: read-back only, never a re-run.
    /// No production caller yet (it needs the unredacted parameters the caller still holds).
    #[allow(dead_code)]
    pub(crate) fn reconcile(
        &self,
        id: &str,
        params: &Value,
    ) -> Result<ExecutionRecord, ControllerError> {
        let rec = self
            .ledger
            .get(id)?
            .ok_or_else(|| ControllerError::Ledger("execution not found".into()))?;
        if !matches!(rec.status, ExecStatus::Unverified | ExecStatus::Partial) {
            return Ok(rec);
        }
        let op = self.op(&rec.action_id)?;
        let expected = op.expected_state(params);
        match (expected, op.read_back(params)) {
            (Some(exp), Some(Ok(actual))) if op.matches(&exp, &actual) => {
                Ok(self.ledger.transition(
                    id,
                    rec.status,
                    ExecStatus::Verified,
                    &Fields {
                        verification_method: Some("read_back".into()),
                        verification_result: Some("match (reconciled)".into()),
                        ..Default::default()
                    },
                    &json!({"reconciled": true}),
                )?)
            }
            _ => Ok(rec),
        }
    }
}

// ─── Live wiring ──────────────────────────────────────────────────────────────

/// Every registered operation. A closed list: adding one is a code change with its own tests.
pub(crate) fn registry() -> Registry {
    let mut ops: Registry = HashMap::new();
    for op in crate::commands::terrence_actions::local_operations()
        .into_iter()
        .chain(crate::commands::skills_lifecycle::operations())
    {
        ops.insert(op.spec().action_id, op);
    }
    ops
}

pub(crate) fn live_controller() -> Controller {
    Controller::new(Ledger::at(ledger::default_path()), registry())
}

/// At startup: recover the ledger, then settle local actions left `executing`. Errors are logged;
/// they never stop the app from starting.
pub(crate) fn recover_on_startup() {
    let c = live_controller();
    match c.recover_after_restart() {
        Ok(n) if n > 0 => eprintln!("[execution] recovered {n} interrupted execution(s)"),
        Ok(_) => {}
        Err(e) => eprintln!("[execution] recovery failed: {e}"),
    }
    match crate::commands::terrence_actions::reconcile_interrupted_local_actions(&c) {
        Ok(n) if n > 0 => eprintln!("[execution] settled {n} interrupted Amara action(s)"),
        Ok(_) => {}
        Err(e) => eprintln!("[execution] action reconcile failed: {e}"),
    }
}

// ─── Read-only queries for the app ────────────────────────────────────────────

/// The most recent executions, newest first (current state only).
#[tauri::command]
#[specta::specta]
pub fn list_executions(limit: Option<u32>) -> Result<Vec<ExecutionRecord>, String> {
    Ledger::at(ledger::default_path())
        .recent(limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

/// Every state transition of one execution, in order.
#[tauri::command]
#[specta::specta]
pub fn execution_history(execution_id: String) -> Result<Vec<ledger::Transition>, String> {
    Ledger::at(ledger::default_path())
        .history(&execution_id)
        .map_err(|e| e.to_string())
}

enum Outcome {
    Done(Result<Value, OpError>),
    TimedOut,
}

/// Runs the operation on its own thread with a hard deadline. A thread that outlives the deadline
/// is left to finish (Rust can't kill it); its late result is dropped and the outcome is treated as
/// unknown, so the caller reconciles instead of resubmitting.
fn run_with_timeout(op: Arc<dyn Operation>, params: Value, timeout: Duration) -> Outcome {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(op.execute(&params));
    });
    match rx.recv_timeout(timeout) {
        Ok(r) => Outcome::Done(r),
        Err(_) => Outcome::TimedOut,
    }
}

fn log_rest(r: &ExecutionRecord) {
    // Fixed labels only: no parameters or responses.
    eprintln!(
        "[execution] action={} status={} attempts={} verification={}",
        r.action_id,
        r.status.as_str(),
        r.attempts,
        r.verification_result.as_deref().unwrap_or("-")
    );
}

fn origin_str(o: Origin) -> &'static str {
    match o {
        Origin::User => "user",
        Origin::Model => "model",
        Origin::Scheduler => "scheduler",
    }
}
fn class_str(c: ActionClass) -> &'static str {
    match c {
        ActionClass::ReadOnly => "read_only",
        ActionClass::Staging => "staging",
        ActionClass::ReversibleLocal => "reversible_local",
        ActionClass::ConsequentialExternal => "consequential_external",
        ActionClass::Destructive => "destructive",
    }
}
fn risk_str(r: Risk) -> &'static str {
    match r {
        Risk::Low => "low",
        Risk::Medium => "medium",
        Risk::High => "high",
    }
}
fn scope_str(s: DataScope) -> &'static str {
    match s {
        DataScope::None => "none",
        DataScope::Personal => "personal",
        DataScope::Business => "business",
        DataScope::Neural => "neural",
        DataScope::System => "system",
    }
}

#[cfg(test)]
mod tests;

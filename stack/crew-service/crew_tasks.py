"""R13-C: the CrewTask contract, the PLAN..DOCUMENT state machine, tier batching, the retry cap and
the verification runner. Pure logic (no crewai, no DB driver import): runners, the store and the
approver are injected, so the whole loop is unit-testable.

Rules enforced here (design doc 4, 5, 10):
- `succeeded` needs evidence for every acceptance criterion. A model saying "tests passed" is a
  string, not Evidence: evidence is only built by this module from a verification result, a
  recorded review or an approval id.
- Verification runs ONLY the commands listed on the task, only if they match a fixed allowlist
  (no shell, argv only, inside the container, scrubbed env). Anything else is refused.
- retry_limit automatic FIX cycles (default 2), then `escalated` and a retry_limit approval.
"""

from __future__ import annotations

import json
import os
import re
import shlex
import signal
import subprocess
import sys
import tempfile
import time
import uuid
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable, Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from crew_safety import redact_block, redact_one_line

SCHEMA_ID = "amara.crew.task.v1"
TASK_STATUSES = ("pending", "running", "awaiting_approval", "succeeded", "failed", "cancelled", "escalated")
TERMINAL = frozenset({"succeeded", "failed", "cancelled"})
TIERS = ("fast", "deep")
TASK_ROLES = ("planner", "researcher", "analyst", "writer", "executor", "reviewer")
VERIFY_TYPES = ("tests", "review", "approval")
MAX_COMMANDS = 5
MAX_TASKS = 20
MAX_RETRY_LIMIT = 5
MAX_TEXT = 1000
UUID_RE = re.compile(r"^[0-9a-fA-F-]{36}$")


# ── Evidence ────────────────────────────────────────────────────────────────
class Evidence(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: str
    kind: Literal["test_output", "review", "approval"]
    criteria: Optional[List[int]] = None  # None = every criterion of the task
    command: Optional[str] = None
    exit_code: Optional[int] = None
    output_tail: Optional[str] = Field(default=None, max_length=4100)
    duration_ms: Optional[int] = None
    reviewer_role: Optional[str] = None
    source_id: Optional[str] = None  # review: id of the test_output it rests on
    notes: Optional[str] = Field(default=None, max_length=600)
    approval_id: Optional[str] = None
    recorded_at: float = 0.0


def evidence_valid(ev: Evidence, task: "CrewTask", resolve: Optional[Callable[[str], Optional[Evidence]]] = None) -> bool:
    """Structural validity of one item for `task`. resolve(source_id) finds the evidence a review
    rests on (own or an input task's); without it only this task's own evidence is searched."""
    if ev.kind == "test_output":
        return (ev.exit_code == 0 and bool(ev.command) and ev.command in task.verification.commands
                and ev.output_tail is not None)
    if ev.kind == "review":
        if not ev.reviewer_role or ev.reviewer_role == task.role:  # no reviewing your own work
            return False
        src = None
        for cand in (task.evidence or []):
            if cand.id == ev.source_id:
                src = cand
        if src is None and resolve is not None and ev.source_id:
            src = resolve(ev.source_id)
        return bool(src and src.kind == "test_output" and src.exit_code == 0 and src.command)
    if ev.kind == "approval":
        return bool(ev.approval_id and UUID_RE.match(ev.approval_id))
    return False


def evidence_gaps(task: "CrewTask", resolve: Optional[Callable[[str], Optional[Evidence]]] = None) -> List[int]:
    """Indices of acceptance criteria with no valid evidence."""
    covered: set = set()
    for ev in task.evidence or []:
        if evidence_valid(ev, task, resolve):
            covered |= set(range(len(task.acceptance_criteria))) if ev.criteria is None else set(ev.criteria)
    return [i for i in range(len(task.acceptance_criteria)) if i not in covered]


# ── Task contract ───────────────────────────────────────────────────────────
class Verification(BaseModel):
    model_config = ConfigDict(extra="forbid")
    type: Literal["tests", "review", "approval"]
    commands: List[str] = []
    evidence_required: bool = True

    @field_validator("commands")
    @classmethod
    def _commands(cls, v: List[str]) -> List[str]:
        if len(v) > MAX_COMMANDS:
            raise ValueError(f"at most {MAX_COMMANDS} verification commands")
        if any(not isinstance(c, str) or not c.strip() or len(c) > 300 for c in v):
            raise ValueError("verification commands must be short non-empty strings")
        return [c.strip() for c in v]

    @model_validator(mode="after")
    def _shape(self):
        if self.type == "tests" and not self.commands:
            raise ValueError("a tests verification needs at least one command")
        if self.commands and not self.evidence_required:
            raise ValueError("evidence_required must be true when commands can check the task")
        return self


class TokenCount(BaseModel):
    prompt: int = 0
    completion: int = 0


class CrewTask(BaseModel):
    model_config = ConfigDict(extra="forbid", populate_by_name=True, validate_assignment=False)
    schema_id: Literal["amara.crew.task.v1"] = Field(default=SCHEMA_ID, alias="$id")
    task_id: str = Field(default_factory=lambda: uuid.uuid4().hex, min_length=1, max_length=64)
    run_id: str = Field(min_length=1, max_length=64)
    parent_task_id: Optional[str] = None
    goal: Optional[str] = Field(default=None, max_length=MAX_TEXT)
    role: str
    tier: Literal["fast", "deep"]
    objective: str = Field(min_length=1, max_length=MAX_TEXT)
    deliverables: List[str]
    acceptance_criteria: List[str]
    verification: Verification
    context_summary: Optional[str] = Field(default=None, max_length=MAX_TEXT)
    inputs: List[str] = []  # task_ids whose results this task needs (readiness)
    status: Literal["pending", "running", "awaiting_approval", "succeeded", "failed", "cancelled", "escalated"] = "pending"
    retry_count: int = Field(default=0, ge=0)
    retry_limit: int = Field(default=2, ge=0, le=MAX_RETRY_LIMIT)
    started_at: Optional[float] = None
    finished_at: Optional[float] = None
    tokens: TokenCount = TokenCount()
    requests: int = 0
    error: Optional[str] = Field(default=None, max_length=400)
    evidence: Optional[List[Evidence]] = None

    @field_validator("role")
    @classmethod
    def _role(cls, v: str) -> str:
        if v not in TASK_ROLES:
            raise ValueError(f"unknown role {v!r}; allowed: {list(TASK_ROLES)}")
        return v

    @field_validator("deliverables", "acceptance_criteria")
    @classmethod
    def _non_empty(cls, v: List[str]) -> List[str]:
        if not v or any(not isinstance(s, str) or not s.strip() for s in v):
            raise ValueError("must be a non-empty list of non-empty strings")
        if len(v) > 20 or any(len(s) > 400 for s in v):
            raise ValueError("too many items or an item over 400 chars")
        return [s.strip() for s in v]

    @model_validator(mode="after")
    def _succeeded_needs_evidence(self):
        if self.status == "succeeded" and evidence_gaps(self):
            raise ValueError("a task cannot be succeeded without evidence for every acceptance criterion")
        return self

    def doc(self) -> dict:
        return self.model_dump(by_alias=True, exclude_none=False)


# ── Task state machine ──────────────────────────────────────────────────────
class Phase(str, Enum):
    PLAN = "PLAN"
    DELEGATE = "DELEGATE"
    EXECUTE = "EXECUTE"
    TEST = "TEST"
    REVIEW = "REVIEW"
    FIX = "FIX"
    VERIFY = "VERIFY"
    DOCUMENT = "DOCUMENT"


ALLOWED_TRANSITIONS: Dict[str, frozenset] = {
    "pending": frozenset({"running", "cancelled"}),
    "running": frozenset({"awaiting_approval", "succeeded", "failed", "escalated", "cancelled"}),
    "awaiting_approval": frozenset({"running", "cancelled", "failed"}),
    "escalated": frozenset({"running", "cancelled"}),  # running only after an approved retry_limit request
    "failed": frozenset(),
    "succeeded": frozenset(),
    "cancelled": frozenset(),
}


class TransitionError(ValueError):
    pass


class EvidenceError(ValueError):
    pass


def transition(task: CrewTask, new: str, now: Callable[[], float] = time.time) -> CrewTask:
    """Move a task to `new` or raise. `succeeded` is only reachable through mark_succeeded."""
    if new not in ALLOWED_TRANSITIONS.get(task.status, frozenset()):
        raise TransitionError(f"{task.status} -> {new} is not allowed")
    if new == "succeeded":
        raise TransitionError("use mark_succeeded: success needs evidence")
    if new == "running" and task.started_at is None:
        task.started_at = now()
    task.status = new
    if new in TERMINAL:
        task.finished_at = now()
    return task


def mark_succeeded(task: CrewTask, resolve: Optional[Callable[[str], Optional[Evidence]]] = None,
                   now: Callable[[], float] = time.time) -> CrewTask:
    gaps = evidence_gaps(task, resolve)
    if gaps:
        raise EvidenceError(f"no evidence for acceptance criteria {gaps}")
    if "succeeded" not in ALLOWED_TRANSITIONS.get(task.status, frozenset()):
        raise TransitionError(f"{task.status} -> succeeded is not allowed")
    task.status = "succeeded"
    task.finished_at = now()
    task.error = None
    return task


# ── Verification runner ─────────────────────────────────────────────────────
VERIFY_TIMEOUT_S = 120.0
VERIFY_OUTPUT_CHARS = 4000


class VerifyRefused(ValueError):
    """The command is not on the allowlist (or we are not in the container). Never executed."""


class NeedsApproval(VerifyRefused):
    """The command is valid but this deployment does not execute interpreters (the container has
    network and no sandbox). The task waits for Taisun's approval instead of running anything."""


_META = re.compile(r"[;&|<>`$\n\r(){}*?!~\\\[\]#]")
_SAFE_FLAGS = frozenset({"-q", "-v", "-vv", "-x", "--tb=short", "--tb=line", "--tb=no"})
_MAXFAIL = re.compile(r"^--maxfail=\d{1,3}$")
_EXPR = re.compile(r"^[A-Za-z0-9_ ]{1,80}$")
_PYTEST_TARGET = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_./-]*\.py(::[A-Za-z0-9_]+)*$")
_UNITTEST_TARGET = re.compile(r"^[A-Za-z_][A-Za-z0-9_.]*$")


def in_container() -> bool:
    return os.environ.get("CREW_CONTAINER") == "1" or os.path.exists("/.dockerenv")


def parse_verify_command(command: str) -> List[str]:
    """argv for an allowlisted command, or VerifyRefused. The fixed forms are:
         pytest [flags] [file.py[::name]...]        python -m pytest [...]
         python -m unittest [flags] [module[.Class[.test]]...]
    flags: -q -v -vv -x --tb=short|line|no --maxfail=N -k "words" and -p no:cacheprovider."""
    if _META.search(command):
        raise VerifyRefused("command contains shell syntax")
    try:
        toks = shlex.split(command)
    except ValueError:
        raise VerifyRefused("command could not be parsed") from None
    if not toks:
        raise VerifyRefused("empty command")
    if toks[0] == "pytest":
        mode, rest = "pytest", toks[1:]
    elif toks[0] in ("python", "python3") and toks[1:3] in (["-m", "pytest"], ["-m", "unittest"]):
        mode, rest = toks[2], toks[3:]
    else:
        raise VerifyRefused("command is not on the verification allowlist")
    target_re = _PYTEST_TARGET if mode == "pytest" else _UNITTEST_TARGET
    out: List[str] = []
    i = 0
    while i < len(rest):
        t = rest[i]
        if t in _SAFE_FLAGS or _MAXFAIL.match(t):
            out.append(t)
        elif t == "-k" and i + 1 < len(rest) and _EXPR.match(rest[i + 1]):
            out += [t, rest[i + 1]]
            i += 1
        elif t == "-p" and rest[i + 1:i + 2] == ["no:cacheprovider"] and mode == "pytest":
            out += [t, "no:cacheprovider"]
            i += 1
        elif not t.startswith("-") and target_re.match(t) and ".." not in t.split("/") and not t.startswith("/"):
            out.append(t)
        else:
            raise VerifyRefused("argument is not allowed")
        i += 1
    return [sys.executable, "-m", mode, *out]


@dataclass
class VerifyResult:
    command: str
    exit_code: Optional[int]
    output_tail: str
    duration_ms: int
    timed_out: bool = False

    @property
    def ok(self) -> bool:
        return self.exit_code == 0 and not self.timed_out


def verify_exec_allowed() -> bool:
    """Executing verification commands needs BOTH switches, set by an operator who has run the
    container without network (`--network none` or an equivalent policy). Neither is set in the
    image or compose: by default nothing is executed and every verification waits for Taisun."""
    return os.environ.get("CREW_VERIFY_EXEC") == "1" and os.environ.get("CREW_VERIFY_NO_NETWORK") == "1"


class VerificationRunner:
    """Validates commands against the fixed allowlist and, only when exec_enabled, runs them.
    exec_enabled defaults to verify_exec_allowed() (False in production), so the default runner can
    check a plan but never starts a process: run() raises NeedsApproval."""

    def __init__(self, cwd: Optional[str] = None, timeout_s: float = VERIFY_TIMEOUT_S,
                 require_container: bool = True, exec_enabled: Optional[bool] = None):
        self.cwd = cwd or os.environ.get("CREW_VERIFY_CWD", "/app")
        self.timeout_s, self.require_container = timeout_s, require_container
        self.exec_enabled = verify_exec_allowed() if exec_enabled is None else exec_enabled

    def check(self, command: str) -> List[str]:
        if self.require_container and not in_container():
            raise VerifyRefused("verification runs only inside the container")
        return parse_verify_command(command)

    def run(self, command: str) -> VerifyResult:
        argv = self.check(command)  # raises before anything starts
        if not self.exec_enabled:
            raise NeedsApproval("verification commands are not executed here; needs Taisun")
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": tempfile.gettempdir(),
               "PYTHONDONTWRITEBYTECODE": "1", "LANG": "C.UTF-8"}  # no CREW_*, DB or router secrets
        t0 = time.monotonic()
        proc = subprocess.Popen(argv, cwd=self.cwd, env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
        timed_out = False
        try:
            out, _ = proc.communicate(timeout=self.timeout_s)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                pass
            out, _ = proc.communicate()
        text = out.decode("utf-8", "replace") if out else ""
        return VerifyResult(command, None if timed_out else proc.returncode,
                            redact_block(text, VERIFY_OUTPUT_CHARS), int((time.monotonic() - t0) * 1000), timed_out)


# ── Persistence ─────────────────────────────────────────────────────────────
TASK_SCHEMA = """
CREATE TABLE IF NOT EXISTS crew_tasks (
    task_id    TEXT PRIMARY KEY,
    run_id     TEXT NOT NULL,
    status     TEXT NOT NULL,
    role       TEXT NOT NULL,
    tier       TEXT NOT NULL,
    doc        JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS crew_tasks_run_idx ON crew_tasks (run_id, updated_at);
"""


class InMemoryTaskStore:
    def __init__(self):
        self.docs: Dict[str, dict] = {}
        self.saves: List[tuple] = []

    def save(self, task: CrewTask) -> None:
        self.docs[task.task_id] = json.loads(json.dumps(task.doc()))
        self.saves.append((task.task_id, task.status, task.retry_count))

    def load_run(self, run_id: str) -> List[CrewTask]:
        return [CrewTask.model_validate(d) for d in self.docs.values() if d["run_id"] == run_id]


class PgTaskStore:
    """connect() returns a psycopg connection (autocommit). Docs round-trip through CrewTask, so a
    stored `succeeded` without evidence fails validation on load."""

    def __init__(self, connect: Callable[[], Any]):
        self._connect = connect

    def save(self, task: CrewTask) -> None:
        with self._connect() as c:
            c.execute(
                "INSERT INTO crew_tasks (task_id, run_id, status, role, tier, doc) VALUES (%s,%s,%s,%s,%s,%s::jsonb) "
                "ON CONFLICT (task_id) DO UPDATE SET status=EXCLUDED.status, doc=EXCLUDED.doc, updated_at=now()",
                (task.task_id, task.run_id, task.status, task.role, task.tier, json.dumps(task.doc())))

    def load_run(self, run_id: str) -> List[CrewTask]:
        with self._connect() as c:
            rows = c.execute("SELECT doc FROM crew_tasks WHERE run_id=%s ORDER BY updated_at, task_id", (run_id,)).fetchall()
        return [CrewTask.model_validate(r[0] if isinstance(r[0], dict) else json.loads(r[0])) for r in rows]


# ── Plan validation and tier batching ───────────────────────────────────────
class PlanError(ValueError):
    pass


def validate_plan(tasks: List[CrewTask], verifier: Optional[VerificationRunner] = None) -> None:
    """The orchestrator rejects a plan with missing acceptance criteria or verification (the model
    already guarantees both), unknown or cyclic inputs, or a command the runner would refuse."""
    if not tasks:
        raise PlanError("the plan has no tasks")
    if len(tasks) > MAX_TASKS:
        raise PlanError(f"the plan has more than {MAX_TASKS} tasks")
    ids = [t.task_id for t in tasks]
    if len(set(ids)) != len(ids):
        raise PlanError("duplicate task ids")
    by_id = {t.task_id: t for t in tasks}
    for t in tasks:
        for dep in t.inputs:
            if dep not in by_id or dep == t.task_id:
                raise PlanError(f"task {t.task_id} has an unknown input")
        if verifier is not None:
            for cmd in t.verification.commands:
                try:
                    verifier.check(cmd)
                except VerifyRefused as e:
                    raise PlanError(f"task {t.task_id}: verification command refused ({e})") from None
    seen: set = set()

    def visit(tid: str, stack: tuple) -> None:
        if tid in stack:
            raise PlanError("the plan has a dependency cycle")
        if tid in seen:
            return
        for dep in by_id[tid].inputs:
            visit(dep, stack + (tid,))
        seen.add(tid)

    for tid in ids:
        visit(tid, ())


def ready_tasks(tasks: List[CrewTask]) -> List[CrewTask]:
    done = {t.task_id for t in tasks if t.status == "succeeded"}
    return [t for t in tasks if t.status == "pending" and all(d in done for d in t.inputs)]


def pick_next(ready: List[CrewTask], last_tier: Optional[str]) -> Optional[CrewTask]:
    """Batch by tier: stay on the tier just used while any ready task wants it (one model swap per
    batch, not per task); start on fast. Within a tier, plan order."""
    if not ready:
        return None
    for tier in ([last_tier] if last_tier else []) + ["fast", "deep"]:
        for t in ready:
            if t.tier == tier:
                return t
    return ready[0]


# ── The loop ────────────────────────────────────────────────────────────────
class TierUnavailable(RuntimeError):
    """The router answered 503 / deep_unavailable for this tier."""


@dataclass
class ExecResult:
    output: str = ""
    prompt_tokens: int = 0
    completion_tokens: int = 0
    requests: int = 0


@dataclass
class ReviewResult:
    passed: bool
    reviewer_role: str
    notes: str = ""


class Runners:
    """What the loop needs from the outside. The crewai-backed implementation lives with the child
    process; tests pass fakes."""

    def plan(self, goal: str, run_id: str) -> List[CrewTask]:
        raise NotImplementedError

    def execute(self, task: CrewTask, feedback: Optional[str]) -> ExecResult:
        raise NotImplementedError

    def review(self, task: CrewTask, output: str) -> ReviewResult:
        raise NotImplementedError

    def request_approval(self, task: CrewTask) -> Optional[str]:
        """Ask a human to sign off on this task. Returns the approval id if (and only if) approved."""
        raise NotImplementedError


@dataclass
class LoopResult:
    status: str  # completed | escalated | failed | cancelled
    tasks: List[CrewTask]
    log: List[tuple] = field(default_factory=list)  # (task_id | None, Phase, tier | None)
    history: List[tuple] = field(default_factory=list)  # (task_id, status) in order


class TaskLoop:
    def __init__(self, runners: Runners, store, verifier: Optional[VerificationRunner] = None, approver=None,
                 sleep: Callable[[float], None] = time.sleep, now: Callable[[], float] = time.time,
                 tier_wait_s: float = 10.0):
        self.runners, self.store, self.verifier = runners, store, verifier or VerificationRunner()
        self.approver, self.sleep, self.now, self.tier_wait_s = approver, sleep, now, tier_wait_s
        self.result = LoopResult("failed", [])

    # -- helpers
    def _log(self, task: Optional[CrewTask], phase: Phase) -> None:
        self.result.log.append((task.task_id if task else None, phase, task.tier if task else None))

    def _move(self, task: CrewTask, status: str) -> None:
        transition(task, status, self.now)
        self.result.history.append((task.task_id, status))
        self.store.save(task)

    def _resolve(self, tasks: List[CrewTask]) -> Callable[[str], Optional[Evidence]]:
        def find(eid: str) -> Optional[Evidence]:
            for t in tasks:
                for ev in t.evidence or []:
                    if ev.id == eid:
                        return ev
            return None
        return find

    def _add(self, task: CrewTask, ev: Evidence) -> None:
        task.evidence = (task.evidence or []) + [ev]

    # -- one attempt: returns None on success of EXECUTE+TEST+REVIEW, else a failure message
    def _attempt(self, task: CrewTask, tasks: List[CrewTask], feedback: Optional[str]) -> Optional[str]:
        self._log(task, Phase.EXECUTE)
        try:
            res = self._execute_with_tier_retry(task, feedback)
        except TierUnavailable:
            raise
        except Exception as e:
            return f"execution failed ({type(e).__name__})"
        task.tokens.prompt += res.prompt_tokens
        task.tokens.completion += res.completion_tokens
        task.requests += res.requests
        task.evidence = None  # evidence belongs to the attempt that produced it
        test_ids: List[str] = []
        if task.verification.commands:
            self._log(task, Phase.TEST)
            for cmd in task.verification.commands:
                try:
                    r = self.verifier.run(cmd)  # other VerifyRefused propagate: not a FIX matter
                except NeedsApproval:
                    task.evidence = None  # nothing ran: no test evidence; a human decides (VERIFY)
                    return None
                if not r.ok:
                    why = "timed out" if r.timed_out else f"exit {r.exit_code}"
                    return redact_one_line(f"verification failed: {cmd} ({why}): " + r.output_tail[-200:], 300)
                ev = Evidence(id=uuid.uuid4().hex, kind="test_output", command=cmd, exit_code=r.exit_code,
                              output_tail=r.output_tail, duration_ms=r.duration_ms, recorded_at=self.now())
                self._add(task, ev)
                test_ids.append(ev.id)
        if task.verification.type == "review":
            self._log(task, Phase.REVIEW)
            rr = self.runners.review(task, res.output)
            if not rr.passed:
                return redact_one_line("review rejected: " + rr.notes, 300)
            src = test_ids[0] if test_ids else self._input_test_evidence(task, tasks)
            if src is not None:
                self._add(task, Evidence(id=uuid.uuid4().hex, kind="review", reviewer_role=rr.reviewer_role,
                                         source_id=src, notes=redact_one_line(rr.notes, 600), recorded_at=self.now()))
        return None

    def _input_test_evidence(self, task: CrewTask, tasks: List[CrewTask]) -> Optional[str]:
        by_id = {t.task_id: t for t in tasks}
        for dep in task.inputs:
            for ev in (by_id[dep].evidence or []):
                if ev.kind == "test_output" and ev.exit_code == 0:
                    return ev.id
        return None

    def _execute_with_tier_retry(self, task: CrewTask, feedback: Optional[str]) -> ExecResult:
        try:
            return self.runners.execute(task, feedback)
        except TierUnavailable:
            self.sleep(self.tier_wait_s)  # model loading: wait once, same tier, never swap in a fast answer
            return self.runners.execute(task, feedback)

    def _ask_human(self, task: CrewTask) -> Optional[str]:
        self._log(task, Phase.VERIFY)
        self._move(task, "awaiting_approval")
        aid = self.runners.request_approval(task)
        if aid is None or not UUID_RE.match(str(aid)):
            self._move(task, "cancelled")
            return None
        self._move(task, "running")
        return aid

    def _run_task(self, task: CrewTask, tasks: List[CrewTask]) -> bool:
        self._log(task, Phase.DELEGATE)
        self._move(task, "running")
        feedback: Optional[str] = None
        while True:
            try:
                failure = self._attempt(task, tasks, feedback)
            except (TierUnavailable, VerifyRefused) as e:
                task.error = redact_one_line(
                    "deep tier unavailable" if isinstance(e, TierUnavailable) else f"verification blocked: {e}", 300)
                self._move(task, "failed")  # honest failure: cannot be fixed by retrying
                return False
            if failure is None:
                self._log(task, Phase.VERIFY)
                if task.verification.type == "approval" or evidence_gaps(task, self._resolve(tasks)):
                    aid = self._ask_human(task)
                    if aid is None:
                        return False
                    self._add(task, Evidence(id=uuid.uuid4().hex, kind="approval", approval_id=aid, recorded_at=self.now()))
                try:
                    mark_succeeded(task, self._resolve(tasks), self.now)
                except EvidenceError:
                    failure = "no evidence"
                else:
                    self.result.history.append((task.task_id, "succeeded"))
                    self.store.save(task)
                    return True
            task.error = redact_one_line(failure, 300)
            if task.retry_count < task.retry_limit:
                task.retry_count += 1
                self._log(task, Phase.FIX)
                self.store.save(task)
                feedback = task.error
                continue
            # Out of automatic retries: escalate to the Crews queue (phase D's retry_limit request).
            self._move(task, "escalated")
            if self.approver is None:
                return False
            if not self.approver.request("retry_limit", [f"{task.role} task {task.task_id[:8]}"]):
                self._move(task, "cancelled")
                return False
            self._move(task, "running")  # an approval buys exactly one more attempt
            task.retry_count += 1
            feedback = task.error

    def run(self, goal: Optional[str] = None, run_id: str = "run", tasks: Optional[List[CrewTask]] = None) -> LoopResult:
        res = self.result
        self._log(None, Phase.PLAN)
        try:
            tasks = tasks if tasks is not None else self.runners.plan(goal or "", run_id)
            validate_plan(tasks, self.verifier)
        except (PlanError, ValueError) as e:
            res.status = "failed"
            res.tasks = tasks or []
            res.history.append((None, f"plan rejected: {redact_one_line(str(e), 200)}"))
            return res
        res.tasks = tasks
        for t in tasks:
            self.store.save(t)
        last_tier: Optional[str] = None
        while True:
            nxt = pick_next(ready_tasks(tasks), last_tier)
            if nxt is None:
                break
            last_tier = nxt.tier
            if not self._run_task(nxt, tasks):
                res.status = {"escalated": "escalated", "cancelled": "cancelled"}.get(nxt.status, "failed")
                break  # no later task starts after a failure, escalation or denial
        if all(t.status == "succeeded" for t in tasks):
            res.status = "completed"
        self._log(None, Phase.DOCUMENT)
        return res

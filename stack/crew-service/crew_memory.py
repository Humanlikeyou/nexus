"""R13-E: run-end memory summaries and controlled skill proposals.

Summaries: a small, size-capped record (objective, outcome, evidence pointer, open issues) built at
the end of a run and handed to a pluggable writer. The default writer is a NO-OP; the only other
one appends JSON lines to a local file in a mapped app-data directory. There is no Letta writer:
decision 0007 did not approve that path, and `select_writer("letta")` refuses.

Skill proposals: data in `<CREW_SKILLS_DIR>/<skill_id>/vNNNN.json` plus an append-only
`audit.log`. They are never applied here. Each one raises a Crews "Needs you" approval (kind skill).
Approving records the decision only. SOUL.md, security rules, tool allowlists and agent
permissions are protected targets: a proposal naming one is flagged and can never be marked
applicable without an approved action whose id matches the stored request.
"""

from __future__ import annotations

import json
import os
import re
import time
from typing import Any, Callable, Dict, List, Optional, Protocol

from crew_safety import redact_one_line

SUMMARY_MAX_CHARS = 2000
OBJECTIVE_CHARS = 200
OUTCOME_CHARS = 300
ISSUE_CHARS = 160
MAX_ISSUES = 5
MAX_POINTERS = 10
SKILL_ID_RE = re.compile(r"^[a-z0-9][a-z0-9_-]{0,47}$")
UUID_RE = re.compile(r"^[0-9a-fA-F-]{36}$")
MAX_SKILL_BODY_CHARS = 4000

# Targets a proposal must never change by itself (design doc 7 and 10.9).
PROTECTED_MARKERS = ("soul.md", "soul", "allowlist", "allowed_tools", "crew_allowed_tools", "read_only_tools",
                     "blocked_kinds", "permission", "security", "approval_kinds", "verify_allowlist",
                     "tool_registry", "sandbox", "grant", "token", "secret")


def _line(text: Any, n: int) -> str:
    return redact_one_line(text, n)


def build_run_summary(run: Dict[str, Any], tasks: Optional[List[Any]] = None,
                      open_issues: Optional[List[str]] = None, now: Callable[[], float] = time.time) -> Dict[str, Any]:
    """run: {id, status, task|goal, finished_at, result?}. Never copies prompts, transcripts or
    tool output: the objective is a redacted, capped one-liner; the outcome is the status plus a
    fixed-shape note; evidence is a list of pointers (ids), not content."""
    status = str(run.get("status") or "unknown")
    objective = _line(run.get("goal") or run.get("task") or "", OBJECTIVE_CHARS)
    pointers = [f"run:{str(run.get('id', ''))[:36]}"]
    issues = [_line(i, ISSUE_CHARS) for i in (open_issues or []) if i]
    done = failed = 0
    for t in tasks or []:
        tid = str(getattr(t, "task_id", ""))[:16]
        if getattr(t, "status", "") == "succeeded":
            done += 1
            for ev in getattr(t, "evidence", None) or []:
                if ev.kind == "approval":
                    pointers.append(f"approval:{ev.approval_id}")
                else:
                    pointers.append(f"{ev.kind}:{tid}:{ev.id[:12]}")
        else:
            failed += 1
            issues.append(_line(f"task {tid} {getattr(t, 'status', '?')}", ISSUE_CHARS))
    if status not in ("completed", "succeeded") and not issues:
        issues.append(_line(f"run ended as {status}", ISSUE_CHARS))
    outcome = f"{status}" + (f"; {done} task(s) succeeded, {failed} not" if tasks else "")
    summary: Dict[str, Any] = {
        "run_id": str(run.get("id", ""))[:36], "objective": objective, "outcome": _line(outcome, OUTCOME_CHARS),
        "evidence": pointers[:MAX_POINTERS], "open_issues": issues[:MAX_ISSUES],
        "finished_at": run.get("finished_at") or None, "written_at": int(now()),
    }
    while len(json.dumps(summary, ensure_ascii=False)) > SUMMARY_MAX_CHARS and (summary["open_issues"] or len(summary["evidence"]) > 1):
        if summary["open_issues"]:
            summary["open_issues"].pop()
        else:
            summary["evidence"].pop()
    if len(json.dumps(summary, ensure_ascii=False)) > SUMMARY_MAX_CHARS:
        summary["objective"] = summary["objective"][:60]
    return summary


class SummaryWriter(Protocol):
    def write(self, summary: Dict[str, Any]) -> None: ...


class NoopWriter:
    """Default: the summary goes nowhere."""

    def write(self, summary: Dict[str, Any]) -> None:
        return None


class LocalFileWriter:
    """Appends one JSON line per run to <dir>/run-summaries.jsonl (0600). The dir is mapped from
    app data by a TAISUN step; nothing here ever talks to the network."""

    def __init__(self, directory: str):
        self.path = os.path.join(directory, "run-summaries.jsonl")
        self._dir = directory

    def write(self, summary: Dict[str, Any]) -> None:
        os.makedirs(self._dir, exist_ok=True)
        fd = os.open(self.path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
        with os.fdopen(fd, "a", encoding="utf-8") as f:
            f.write(json.dumps(summary, ensure_ascii=False) + "\n")


def select_writer(kind: str = "", directory: str = "") -> SummaryWriter:
    kind = (kind or "").strip().lower()
    if kind in ("", "none", "noop"):
        return NoopWriter()
    if kind == "file":
        if not directory:
            return NoopWriter()  # no mapped dir: fail closed to the no-op
        return LocalFileWriter(directory)
    raise ValueError(f"memory writer {kind!r} is not available (Letta writes are not approved: decision 0007)")


def write_summary_safely(writer: SummaryWriter, summary: Dict[str, Any]) -> bool:
    """A writer failure never affects the run. Returns whether the write succeeded."""
    try:
        writer.write(summary)
        return True
    except Exception:
        return False


# ── Skill proposals ─────────────────────────────────────────────────────────
class SkillError(ValueError):
    pass


def touches_protected(change: Any) -> bool:
    """True if a proposal's change names a protected target (checked over keys and string values)."""
    blob = json.dumps(change, ensure_ascii=False).lower()
    return any(m in blob for m in PROTECTED_MARKERS)


class SkillStore:
    def __init__(self, root: str, now: Callable[[], float] = time.time):
        if not root:
            raise SkillError("skills directory is not configured")
        self.root, self.now = root, now

    def _dir(self, skill_id: str) -> str:
        if not SKILL_ID_RE.match(skill_id or ""):
            raise SkillError("invalid skill id")
        return os.path.join(self.root, skill_id)

    def _audit(self, event: str, **fields: Any) -> None:
        os.makedirs(self.root, exist_ok=True)
        rec = {"at": int(self.now()), "event": event, **fields}
        fd = os.open(os.path.join(self.root, "audit.log"), os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
        with os.fdopen(fd, "a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")

    def _path(self, skill_id: str, version: int) -> str:
        return os.path.join(self._dir(skill_id), f"v{version:04d}.json")

    def versions(self, skill_id: str) -> List[int]:
        d = self._dir(skill_id)
        if not os.path.isdir(d):
            return []
        return sorted(int(m.group(1)) for n in os.listdir(d) if (m := re.fullmatch(r"v(\d{4})\.json", n)))

    def get(self, skill_id: str, version: int) -> Dict[str, Any]:
        try:
            with open(self._path(skill_id, version), encoding="utf-8") as f:
                return json.load(f)
        except FileNotFoundError:
            raise SkillError("no such proposal") from None

    def _write(self, rec: Dict[str, Any]) -> None:
        os.makedirs(self._dir(rec["skill_id"]), exist_ok=True)
        path = self._path(rec["skill_id"], rec["version"])
        tmp = path + ".tmp"
        fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(rec, f, ensure_ascii=False, indent=1)
        os.replace(tmp, path)

    def propose(self, skill_id: str, title: str, change: Dict[str, Any], run_id: str,
                request_approval: Callable[[str, List[str]], str]) -> Dict[str, Any]:
        """Store a new version as `proposed` and raise a Crews approval (kind skill). Never applies.
        request_approval(kind, items) returns the approval id; if it fails the proposal is not kept."""
        if not isinstance(change, dict) or not change:
            raise SkillError("a proposal needs a change object")
        body = json.dumps(change, ensure_ascii=False)
        if len(body) > MAX_SKILL_BODY_CHARS:
            raise SkillError("proposal is too large")
        if redact_one_line(body, len(body) + 1) != " ".join(body.split()):
            raise SkillError("proposal looks like it contains a secret")
        version = (self.versions(skill_id) or [0])[-1] + 1
        protected = touches_protected(change) or touches_protected(title)
        items = [f"{skill_id} v{version}"] + (["touches a protected target"] if protected else [])
        approval_id = request_approval("skill", items)
        if not approval_id or not UUID_RE.match(str(approval_id)):
            raise SkillError("could not raise the approval")
        rec = {"skill_id": skill_id, "version": version, "title": _line(title, 120), "change": change,
               "run_id": str(run_id)[:36], "protected": protected, "state": "proposed",
               "approval_id": approval_id, "proposed_at": int(self.now()), "decided_at": None, "decided_by": None}
        self._write(rec)
        self._audit("proposed", skill_id=skill_id, version=version, approval_id=approval_id, protected=protected)
        return rec

    def record_decision(self, approval_id: str, approved: bool, by: str = "taisun") -> Optional[Dict[str, Any]]:
        """Record the human decision for the proposal that raised `approval_id`. Records only: no
        skill is applied. Returns the updated record, or None if no proposal matches."""
        if not os.path.isdir(self.root):
            return None
        for sid in sorted(os.listdir(self.root)):
            if not SKILL_ID_RE.match(sid) or not os.path.isdir(os.path.join(self.root, sid)):
                continue
            for v in self.versions(sid):
                rec = self.get(sid, v)
                if rec.get("approval_id") == approval_id and rec.get("state") == "proposed":
                    rec.update(state="approved" if approved else "denied", decided_at=int(self.now()), decided_by=by)
                    self._write(rec)
                    self._audit("decision", skill_id=sid, version=v, approval_id=approval_id, approved=bool(approved), by=by)
                    return rec
        return None

    def can_apply(self, skill_id: str, version: int, approval_id: str) -> bool:
        """The gate a future apply step must pass: the stored decision is approved and the caller
        presents the same approval id. Applying itself is out of scope."""
        try:
            rec = self.get(skill_id, version)
        except SkillError:
            return False
        return rec.get("state") == "approved" and bool(approval_id) and rec.get("approval_id") == approval_id

    def list(self) -> List[Dict[str, Any]]:
        out: List[Dict[str, Any]] = []
        if not os.path.isdir(self.root):
            return out
        for sid in sorted(os.listdir(self.root)):
            if SKILL_ID_RE.match(sid) and os.path.isdir(os.path.join(self.root, sid)):
                out += [{k: r[k] for k in ("skill_id", "version", "title", "state", "protected", "approval_id")}
                        for r in (self.get(sid, v) for v in self.versions(sid))]
        return out

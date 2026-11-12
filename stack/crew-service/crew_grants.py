"""1110i: the per-crew Files / terminal switch, enforced here (decision 0007 item 8).

Default OFF and fail closed: no grant row means no fs_read / fs_write / shell. Only Taisun
creates a grant (`actor == "taisun"`, reached through the token-gated endpoints the app calls);
a crew can only ask, which raises a Crews approval. A grant is scoped to a sandbox directory
(CREW_SANDBOX_ROOT/<crew>, mounted by a TAISUN step; unset = the tools do not exist), lasts
"this run only" by default, never more than 24 h, and is re-checked on every call and every
0.5 s while a command runs, so an expiry or a revoke stops a running shell without a restart.
Even with a grant, fs_write and shell still need the batch approval (main.gate_tool_call).
Always blocked, grant or not: network/outbound, git push, sudo, spending.
"""

from __future__ import annotations

import json
import os
import re
import shlex
import signal
import subprocess
import tempfile
import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional

from crew_safety import redact_block, redact_one_line

GRANT_TOOLS = ("fs_read", "fs_write", "shell")
DURATIONS = ("run", "1h", "24h")
DEFAULT_DURATION = "run"  # decision 0007: this run only
MAX_GRANT_S = 24 * 3600
REQUEST_EXPIRY_S = 3600
SHELL_TIMEOUT_S = 60.0
SHELL_OUTPUT_CHARS = 4000
FS_READ_CHARS = 8000
FS_WRITE_MAX_BYTES = 64 * 1024
WATCH_POLL_S = 0.5
ACTOR = "taisun"
CREW_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9 _.\-]{0,63}$")
AUDIT_KINDS = ("grant", "revoke", "confirm", "shell", "fs_write", "blocked", "asked", "info")


class GrantError(ValueError):
    pass


def slug(crew: str) -> str:
    return re.sub(r"^-|-$", "", re.sub(r"[^a-z0-9]+", "-", crew.lower())) or "crew"


def within(path: str, root: str) -> bool:
    root = os.path.realpath(root)
    path = os.path.realpath(path)
    return path == root or path.startswith(root.rstrip(os.sep) + os.sep)


def resolve_scope(root: str, crew: str, requested: Optional[str] = None) -> str:
    """The crew's folder: <root>/<slug>[/sub]. A requested scope must stay inside it."""
    if not root:
        raise GrantError("the sandbox is not configured")
    base = os.path.join(root, slug(crew))
    if requested:
        rel = requested
        marker = f"crew-sandbox/{slug(crew)}"
        if marker in requested:
            rel = requested.split(marker, 1)[1]
        elif os.path.isabs(requested):
            rel = os.path.relpath(requested, base)
        rel = rel.lstrip("/")
        cand = os.path.join(base, rel) if rel and rel != "." else base
        if ".." in cand.split(os.sep) or not within(cand, base):
            raise GrantError("scope must be inside the crew's sandbox folder")
        return cand
    return base


# ── Records and stores ──────────────────────────────────────────────────────
@dataclass
class Grant:
    crew: str
    tools: tuple
    scope_path: str
    duration: str
    granted_by: str
    granted_at: float
    expires_at: Optional[float] = None
    revoked_at: Optional[float] = None
    revoked_by: Optional[str] = None
    reason: Optional[str] = None
    request_id: Optional[str] = None
    run_id: Optional[str] = None
    enabled: bool = True

    def active(self, now: float, run_id: Optional[str] = None) -> bool:
        """enabled, not revoked, not expired; a "this run only" grant is active only for the run
        it was claimed by."""
        if not self.enabled or self.revoked_at is not None:
            return False
        if self.expires_at is not None and now >= self.expires_at:
            return False
        if self.duration == "run":
            return run_id is not None and self.run_id == run_id
        return True


class MemoryGrantStore:
    def __init__(self):
        self.rows: List[Grant] = []

    def latest(self, crew: str) -> Optional[Grant]:
        mine = [g for g in self.rows if g.crew == crew]
        return mine[-1] if mine else None

    def add(self, g: Grant) -> None:
        self.rows.append(g)

    def revoke(self, crew: str, by: str, reason: str, now: float) -> bool:
        g = self.latest(crew)
        if g is None or g.revoked_at is not None:
            return False
        g.revoked_at, g.revoked_by, g.reason = now, by, reason
        return True

    def claim_run(self, crew: str, run_id: str, now: float) -> bool:
        g = self.latest(crew)
        if (g and g.duration == "run" and g.run_id is None and g.revoked_at is None
                and (g.expires_at is None or now < g.expires_at)):
            g.run_id = run_id
            return True
        return False

    def end_run(self, run_id: str, now: float) -> List[str]:
        out = []
        for g in self.rows:
            if g.duration == "run" and g.run_id == run_id and g.revoked_at is None:
                g.revoked_at, g.revoked_by, g.reason = now, "crew-service", "run ended"
                out.append(g.crew)
        return out

    def expire_due(self, now: float) -> List[str]:
        out = []
        for g in self.rows:
            if g.revoked_at is None and g.expires_at is not None and now >= g.expires_at:
                g.revoked_at, g.revoked_by, g.reason = now, "crew-service", "expired"
                out.append(g.crew)
        return out


GRANT_SCHEMA = """
CREATE TABLE IF NOT EXISTS crew_tool_grants (
    id         BIGSERIAL PRIMARY KEY,
    crew_id    TEXT NOT NULL,
    enabled    BOOLEAN NOT NULL DEFAULT true,
    tools      TEXT[] NOT NULL,
    scope_path TEXT NOT NULL,
    duration   TEXT NOT NULL CHECK (duration IN ('run','1h','24h')),
    granted_by TEXT NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoked_by TEXT,
    reason     TEXT,
    request_id TEXT,
    run_id     TEXT
);
CREATE INDEX IF NOT EXISTS crew_tool_grants_crew_idx ON crew_tool_grants (crew_id, id DESC);
CREATE TABLE IF NOT EXISTS crew_tool_audit (
    id          BIGSERIAL PRIMARY KEY,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor       TEXT NOT NULL,
    crew_id     TEXT NOT NULL,
    run_id      TEXT,
    kind        TEXT NOT NULL CHECK (kind IN ('grant','revoke','confirm','shell','fs_write','blocked','asked','info')),
    action      TEXT NOT NULL,
    target      TEXT,
    exit_code   INTEGER,
    duration_ms INTEGER,
    checkpoint  TEXT
);
CREATE INDEX IF NOT EXISTS crew_tool_audit_crew_idx ON crew_tool_audit (crew_id, id DESC);
"""

_GCOLS = ("crew_id, enabled, tools, scope_path, duration, granted_by, EXTRACT(EPOCH FROM granted_at), "
          "EXTRACT(EPOCH FROM expires_at), EXTRACT(EPOCH FROM revoked_at), revoked_by, reason, request_id, run_id")


def _f(v):
    return None if v is None else float(v)


class PgGrantStore:
    def __init__(self, connect: Callable[[], Any]):
        self._c = connect

    @staticmethod
    def _row(r) -> Grant:
        return Grant(crew=r[0], enabled=r[1], tools=tuple(r[2]), scope_path=r[3], duration=r[4], granted_by=r[5],
                     granted_at=float(r[6]), expires_at=_f(r[7]), revoked_at=_f(r[8]), revoked_by=r[9],
                     reason=r[10], request_id=r[11], run_id=r[12])

    def latest(self, crew: str) -> Optional[Grant]:
        with self._c() as c:
            r = c.execute(f"SELECT {_GCOLS} FROM crew_tool_grants WHERE crew_id=%s ORDER BY id DESC LIMIT 1", (crew,)).fetchone()
        return self._row(r) if r else None

    def add(self, g: Grant) -> None:
        with self._c() as c:
            c.execute("INSERT INTO crew_tool_grants (crew_id, enabled, tools, scope_path, duration, granted_by, "
                      "expires_at, reason, request_id, run_id) VALUES (%s,%s,%s,%s,%s,%s,to_timestamp(%s),%s,%s,%s)",
                      (g.crew, g.enabled, list(g.tools), g.scope_path, g.duration, g.granted_by, g.expires_at,
                       g.reason, g.request_id, g.run_id))

    def revoke(self, crew: str, by: str, reason: str, now: float) -> bool:
        with self._c() as c:
            n = c.execute("UPDATE crew_tool_grants SET revoked_at=now(), revoked_by=%s, reason=%s "
                          "WHERE crew_id=%s AND revoked_at IS NULL", (by, reason, crew)).rowcount
        return n > 0

    def claim_run(self, crew: str, run_id: str, now: float) -> bool:
        with self._c() as c:
            n = c.execute("UPDATE crew_tool_grants SET run_id=%s WHERE crew_id=%s AND duration='run' AND run_id IS NULL "
                          "AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())", (run_id, crew)).rowcount
        return n > 0

    def end_run(self, run_id: str, now: float) -> List[str]:
        with self._c() as c:
            rows = c.execute("UPDATE crew_tool_grants SET revoked_at=now(), revoked_by='crew-service', reason='run ended' "
                             "WHERE duration='run' AND run_id=%s AND revoked_at IS NULL RETURNING crew_id", (run_id,)).fetchall()
        return [r[0] for r in rows]

    def expire_due(self, now: float) -> List[str]:
        with self._c() as c:
            rows = c.execute("UPDATE crew_tool_grants SET revoked_at=now(), revoked_by='crew-service', reason='expired' "
                             "WHERE revoked_at IS NULL AND expires_at IS NOT NULL AND expires_at <= now() RETURNING crew_id").fetchall()
        return [r[0] for r in rows]


@dataclass
class AuditRow:
    id: int
    at: float
    actor: str
    crew: str
    run_id: Optional[str]
    kind: str
    action: str
    target: Optional[str] = None
    exit_code: Optional[int] = None
    duration_ms: Optional[int] = None
    checkpoint: Optional[str] = None


class MemoryAudit:
    def __init__(self):
        self.rows: List[AuditRow] = []

    def add(self, row: Dict[str, Any]) -> None:
        self.rows.append(AuditRow(id=len(self.rows) + 1, **row))

    def list(self, crew: str, limit: int = 50, before: Optional[int] = None) -> List[AuditRow]:
        mine = [r for r in self.rows if r.crew == crew and (before is None or r.id < before)]
        return list(reversed(mine))[:limit]


class PgAudit:
    def __init__(self, connect: Callable[[], Any]):
        self._c = connect

    def add(self, row: Dict[str, Any]) -> None:
        with self._c() as c:
            c.execute("INSERT INTO crew_tool_audit (at, actor, crew_id, run_id, kind, action, target, exit_code, duration_ms, checkpoint) "
                      "VALUES (to_timestamp(%s),%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                      (row["at"], row["actor"], row["crew"], row.get("run_id"), row["kind"], row["action"],
                       row.get("target"), row.get("exit_code"), row.get("duration_ms"), row.get("checkpoint")))

    def list(self, crew: str, limit: int = 50, before: Optional[int] = None) -> List[AuditRow]:
        sql = ("SELECT id, EXTRACT(EPOCH FROM at), actor, crew_id, run_id, kind, action, target, exit_code, duration_ms, checkpoint "
               "FROM crew_tool_audit WHERE crew_id=%s")
        params: list = [crew]
        if before is not None:
            sql += " AND id < %s"
            params.append(before)
        sql += " ORDER BY id DESC LIMIT %s"
        params.append(limit)
        with self._c() as c:
            rows = c.execute(sql, tuple(params)).fetchall()
        return [AuditRow(r[0], float(r[1]), r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10]) for r in rows]


# ── Service (parent side): set / revoke / state ─────────────────────────────
class GrantService:
    def __init__(self, store, audit, root: str, base_allowed: Callable[[], frozenset] = lambda: frozenset(),
                 now: Callable[[], float] = time.time):
        self.store, self.audit, self.root, self.base_allowed, self.now = store, audit, root, base_allowed, now

    def log(self, actor: str, crew: str, kind: str, action: str, **kw) -> None:
        try:
            self.audit.add(dict(at=self.now(), actor=actor, crew=crew, run_id=kw.pop("run_id", None), kind=kind,
                                action=redact_one_line(action, 200), target=kw.pop("target", None), **kw))
        except Exception:  # an audit failure must never open a door; callers stay fail-closed
            pass

    def set(self, crew: str, tools: List[str], duration: str, actor: str, scope_path: Optional[str] = None,
            request_id: Optional[str] = None) -> Dict[str, Any]:
        if actor != ACTOR:
            self.log(str(actor)[:32], crew, "blocked", "refused: only Taisun can turn files and terminal on")
            raise PermissionError("only Taisun can create a tool grant")
        if not CREW_RE.match(crew or ""):
            raise GrantError("invalid crew name")
        if not tools or any(t not in GRANT_TOOLS for t in tools):
            raise GrantError(f"tools must be a non-empty subset of {list(GRANT_TOOLS)}")
        if duration not in DURATIONS:
            raise GrantError(f"duration must be one of {list(DURATIONS)}")
        if not self.root or not os.path.isdir(self.root):
            raise GrantError("the sandbox folder is not mounted; nothing was turned on")
        scope = resolve_scope(self.root, crew, scope_path)
        os.makedirs(scope, exist_ok=True)
        if not within(scope, os.path.join(self.root, slug(crew))):  # symlink escape after creation
            raise GrantError("scope must be inside the crew's sandbox folder")
        now = self.now()
        expires = None
        if duration == "1h":
            expires = now + 3600
        elif duration == "24h":
            expires = now + MAX_GRANT_S
        elif duration == "run":
            expires = now + MAX_GRANT_S  # an unused run grant lapses at the cap
        if self.store.revoke(crew, ACTOR, "replaced", now):
            self.log(ACTOR, crew, "revoke", "previous grant replaced")
        g = Grant(crew=crew, tools=tuple(sorted(set(tools))), scope_path=scope, duration=duration, granted_by=ACTOR,
                  granted_at=now, expires_at=expires, request_id=request_id)
        self.store.add(g)
        self.log(ACTOR, crew, "grant", f"turned on {', '.join(g.tools)} for {duration}", target=scope)
        state = self.state(crew)
        self.log("crew-service", crew, "confirm", "enforcing allowlist", target=", ".join(state["enforced_allowlist"]))
        return state

    def revoke(self, crew: str, actor: str = ACTOR) -> Dict[str, Any]:
        if self.store.revoke(crew, actor, "turned off", self.now()):
            self.log(actor, crew, "revoke", "turned off files and terminal; any running command is stopped")
        return self.state(crew)

    def active(self, crew: str, run_id: Optional[str] = None) -> Optional[Grant]:
        g = self.store.latest(crew)
        return g if g and g.active(self.now(), run_id) else None

    def state(self, crew: str) -> Dict[str, Any]:
        g = self.store.latest(crew)
        now = self.now()
        live = bool(g and g.enabled and g.revoked_at is None and (g.expires_at is None or now < g.expires_at))
        tools = sorted(g.tools) if g and live else []
        return {"enabled": live, "tools": tools, "scope_path": g.scope_path if g else None,
                "duration": g.duration if g else None, "granted_at": g.granted_at if g else None,
                "expires_at": g.expires_at if g and live and g.duration != "run" else None,
                "granted_by": g.granted_by if g else None,
                "enforced_allowlist": sorted(set(self.base_allowed()) | set(tools))}

    def sweep(self) -> int:
        n = 0
        for crew in self.store.expire_due(self.now()):
            self.log("crew-service", crew, "revoke", "grant expired")
            n += 1
        return n


# ── Shell command policy ────────────────────────────────────────────────────
class Blocked(Exception):
    def __init__(self, category: str, why: str):
        super().__init__(why)
        self.category, self.why = category, why


_SHELL_META = re.compile(r"[;&|<>`$(){}\n\r\\!~#]")
_SUDO = frozenset({"sudo", "su", "doas", "pkexec"})
_NETWORK = frozenset({"curl", "wget", "nc", "ncat", "netcat", "ssh", "scp", "sftp", "rsync", "ftp", "telnet", "ping",
                      "dig", "nslookup", "nmap", "pip", "pip3", "npm", "npx", "yarn", "pnpm", "brew", "apt", "apt-get",
                      "docker", "kubectl", "mail", "sendmail"})
_GIT_OK = frozenset({"status", "diff", "log"})  # read-only only: no commit, add, branch, init, show
_GIT_NET = frozenset({"fetch", "pull", "clone", "remote", "send-email", "submodule", "ls-remote"})
# Plain file utilities only: none of these can exec another program or open a socket. Interpreters
# and exec-capable tools (python, pytest, sh, env, xargs, find -exec, awk, sed...) are NOT here: this
# container has network and no seccomp/unshare sandbox, so an interpreter would be unrestricted
# network access (decision 0007 item 8). git is limited to status/diff/log (see parse_shell).
SHELL_ALLOW = frozenset({"ls", "cat", "head", "tail", "wc", "grep", "sort", "uniq", "diff", "echo", "pwd", "mkdir",
                         "touch", "cp", "mv", "rm", "date", "git", "true", "false"})
_INTERPRETERS = frozenset({"python", "python3", "pytest", "py.test", "node", "nodejs", "deno", "bun", "sh", "bash", "zsh",
                           "dash", "ksh", "fish", "env", "xargs", "find", "awk", "gawk", "sed", "perl", "ruby", "php", "lua",
                           "tclsh", "expect", "make", "cmake", "gcc", "cc", "go", "cargo", "rustc", "java", "busybox",
                           "timeout", "nohup", "nice", "watch", "strace", "tee", "dd", "tar", "unzip", "zip", "gzip", "vi",
                           "vim", "nano", "less", "more", "man", "script", "eval", "exec", "source", "command", "time"})

# Per-program option grammar. Anything not listed is refused (fail closed). `bool` = letters of
# boolean short flags (clusters allowed only from these), `long` = boolean long flags, `val` = flags
# that take a value, with the value's kind: path (must resolve inside the folder), text (never used
# as a path) or num. `pos` = kind of positional arguments.
_SPEC: Dict[str, Dict[str, Any]] = {
    "ls": dict(bool="lahR1AFtrSd", long=set(), val={}, pos="path"),
    "cat": dict(bool="nbsET", long=set(), val={}, pos="path"),
    "head": dict(bool="q", long=set(), val={"-n": "num", "-c": "num"}, pos="path"),
    "tail": dict(bool="q", long=set(), val={"-n": "num", "-c": "num"}, pos="path"),
    "wc": dict(bool="lwcm", long=set(), val={}, pos="path"),
    "grep": dict(bool="ivnclErFwxqHhs", long=set(), val={"-e": "text", "-m": "num", "-A": "num", "-B": "num", "-C": "num"}, pos="path", first_text=True),
    "sort": dict(bool="nrufbc", long=set(), val={"-o": "path"}, pos="path"),
    "uniq": dict(bool="cdui", long=set(), val={}, pos="path"),
    "diff": dict(bool="uqrNi", long={"--brief"}, val={}, pos="path"),
    "echo": dict(bool="n", long=set(), val={}, pos="text"),
    "pwd": dict(bool="LP", long=set(), val={}, pos=None),
    "mkdir": dict(bool="pv", long=set(), val={}, pos="path"),
    "touch": dict(bool="c", long=set(), val={}, pos="path"),
    "cp": dict(bool="rRpnvfa", long=set(), val={"-t": "path", "--target-directory": "path"}, pos="path"),
    "mv": dict(bool="nvf", long=set(), val={"-t": "path", "--target-directory": "path"}, pos="path"),
    "rm": dict(bool="rRfvd", long=set(), val={}, pos="path"),
    "date": dict(bool="u", long=set(), val={}, pos="datefmt"),
    "true": dict(bool="", long=set(), val={}, pos=None),
    "false": dict(bool="", long=set(), val={}, pos=None),
    "git:status": dict(bool="sb", long={"--short", "--branch"}, val={}, pos="path"),
    "git:diff": dict(bool="", long={"--stat", "--cached", "--name-only", "--staged"}, val={}, pos="path"),
    "git:log": dict(bool="", long={"--oneline", "--stat"}, val={"-n": "num"}, pos="path"),
}
_NUM = re.compile(r"^-?\d{1,6}$")
_PYTEST_TARGET = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_./-]*\.py(::[A-Za-z0-9_]+)*$")
_MODPATH = re.compile(r"^[A-Za-z_][A-Za-z0-9_.]*$")
_MAX_TEXT = 200


def _check_path(val: str, scope: str) -> None:
    """Every path-like value, wherever it came from: no NUL, no `..`, and after resolving symlinks
    it must be inside the folder."""
    if not val or "\x00" in val:
        raise Blocked("path", "empty or invalid path")
    base = val.split("::", 1)[0]
    parts = base.replace("\\", "/").split("/")
    if ".." in parts:
        raise Blocked("path", "path leaves the folder")
    if ".git" in parts:
        raise Blocked("path", ".git is off limits")
    cand = base if os.path.isabs(base) else os.path.join(scope, base)
    if not within(cand, scope):
        raise Blocked("path", "path is outside the folder")
    rel = os.path.relpath(os.path.realpath(cand), os.path.realpath(scope))
    if ".git" in rel.split(os.sep):  # a symlink into .git
        raise Blocked("path", ".git is off limits")


def _check_value(kind: str, val: str, scope: str, spec: Dict[str, Any]) -> None:
    if len(val) > _MAX_TEXT:
        raise Blocked("syntax", "argument too long")
    if kind == "path":
        _check_path(val, scope)
    elif kind == "num":
        if not _NUM.match(val):
            raise Blocked("syntax", "expected a number")
    elif kind == "text":
        if "\x00" in val:
            raise Blocked("syntax", "invalid text")
    elif kind == "target":
        if not _PYTEST_TARGET.match(val):
            raise Blocked("not_allowed", "pytest target must be a .py file")
        _check_path(val, scope)
    elif kind == "modpath":
        if "/" in val or val.endswith(".py"):
            _check_path(val, scope)
        elif not _MODPATH.match(val):
            raise Blocked("not_allowed", "bad module name")
    elif kind == "datefmt":
        if not val.startswith("+"):
            raise Blocked("not_allowed", "date takes only a +format")
    else:
        raise Blocked("not_allowed", "this command takes no positional arguments")


def _parse_args(spec: Dict[str, Any], args: List[str], scope: str) -> None:
    """Walk the arguments against `spec`; every option value and positional is validated."""
    i, n, dd = 0, len(args), False
    saw_pattern = bool(not spec.get("first_text"))
    def take_value(flag: str, attached: Optional[str]) -> None:
        nonlocal i
        kind = spec["val"][flag]
        if attached is None:
            i += 1
            if i >= n:
                raise Blocked("syntax", f"{flag} needs a value")
            attached = args[i]
        _check_value(kind, attached, scope, spec)
        if flag == "-e":
            nonlocal saw_pattern
            saw_pattern = True
    while i < n:
        t = args[i]
        if dd or not t.startswith("-") or t == "-":
            kind = spec["pos"]
            if not saw_pattern:  # grep: the first positional is the pattern, not a path
                saw_pattern = True
                _check_value("text", t, scope, spec)
            else:
                _check_value(kind or "", t, scope, spec)
        elif t == "--":
            dd = True
        elif t.startswith("--"):
            name, eq, val = t.partition("=")
            if t in spec["long"]:
                pass
            elif name in spec["val"]:
                take_value(name, val if eq else None)
            else:
                raise Blocked("not_allowed", "option is not allowed")
        elif t in spec["long"]:
            pass
        else:
            letters = t[1:]
            for idx, ch in enumerate(letters):
                if ch in spec["bool"]:
                    continue
                if "-" + ch in spec["val"]:
                    rest = letters[idx + 1:]
                    take_value("-" + ch, rest if rest else None)
                    break
                raise Blocked("not_allowed", "option is not allowed")
        i += 1


def parse_shell(command: str, scope: str) -> List[str]:
    """argv for an allowed command inside `scope`, or Blocked(category, why). No shell is ever
    used: pipes, redirects, substitution and chaining are refused, not interpreted. Options come
    from an explicit per-program grammar; every option value and every positional must resolve
    (symlinks and `..` included) inside the folder, and anything unparsed is refused."""
    if not command or not command.strip() or len(command) > 500:
        raise Blocked("syntax", "empty or too long")
    if _SHELL_META.search(command):
        raise Blocked("syntax", "shell syntax is not allowed")
    try:
        toks = shlex.split(command)
    except ValueError:
        raise Blocked("syntax", "could not parse") from None
    if not toks:
        raise Blocked("syntax", "empty command")
    if "/" in toks[0]:
        raise Blocked("not_allowed", "run programs by name, not by path")
    prog = toks[0]
    if prog in _SUDO:
        raise Blocked("sudo", "sudo is never allowed")
    if prog in _NETWORK:
        raise Blocked("network", "network and installers are never allowed")
    if prog in _INTERPRETERS:
        raise Blocked("not_allowed", "interpreters and exec-capable tools are never allowed")
    args = toks[1:]
    if prog == "git":
        sub = next((t for t in args if not t.startswith("-")), "")
        if sub == "push":
            raise Blocked("git_push", "git push is never allowed")
        if sub in _GIT_NET:
            raise Blocked("network", "git network commands are never allowed")
        if args[:1] and args[0].startswith("-"):
            raise Blocked("not_allowed", "git global options are not allowed")
        if sub not in _GIT_OK:
            raise Blocked("not_allowed", "only git status, diff and log are allowed")
        _parse_args(_SPEC["git:" + sub], args[1:], scope)
        return ["git", *GIT_HARDENING, sub, *args[1:]]
    if prog not in SHELL_ALLOW or prog not in _SPEC:
        raise Blocked("not_allowed", "program is not on the terminal allowlist")
    _parse_args(_SPEC[prog], args, scope)
    if prog in ("rm", "mv"):  # the folder itself (and its checkpoint) can never be removed or moved
        for t in args:
            if not t.startswith("-") and os.path.realpath(t if os.path.isabs(t) else os.path.join(scope, t)) == os.path.realpath(scope):
                raise Blocked("path", "the sandbox folder itself cannot be removed or moved")
    return toks


# No hooks, no fsmonitor, no pager, no external diff or attribute files, no network protocols.
GIT_HARDENING = ["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=", "-c", "core.pager=cat",
                 "-c", "protocol.allow=never", "-c", "diff.external=", "-c", "core.attributesFile=/dev/null"]


_GIT_ENV = {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_TERMINAL_PROMPT": "0", "GIT_AUTHOR_NAME": "crew", "GIT_AUTHOR_EMAIL": "crew@localhost",
            "GIT_COMMITTER_NAME": "crew", "GIT_COMMITTER_EMAIL": "crew@localhost"}


class CheckpointError(RuntimeError):
    pass


def scrub_env(extra: Optional[Dict[str, str]] = None) -> Dict[str, str]:
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": tempfile.gettempdir(), "LANG": "C.UTF-8",
           "PYTHONDONTWRITEBYTECODE": "1"}
    env.update(extra or {})
    return env


def git_checkpoint(scope: str) -> str:
    """Commit the folder's current state and return the short hash. Raises CheckpointError."""
    def git(*args: str) -> str:
        p = subprocess.run(["git", "-c", "commit.gpgsign=false", *GIT_HARDENING, *args], cwd=scope,
                           env=scrub_env(_GIT_ENV), capture_output=True, text=True, timeout=30)
        if p.returncode != 0:
            raise CheckpointError("git failed")
        return p.stdout.strip()
    try:
        if not os.path.isdir(os.path.join(scope, ".git")):
            git("init", "-q")
        git("add", "-A")
        git("commit", "-q", "--allow-empty", "-m", "crew checkpoint")
        return git("rev-parse", "--short", "HEAD")
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        raise CheckpointError("git is unavailable") from None


# ── Child side: the tools ───────────────────────────────────────────────────
@dataclass
class ToolContext:
    crew: str
    run_id: Optional[str]
    service: GrantService
    checkpoint: Callable[[str], str] = git_checkpoint
    checkpointed: bool = False
    poll_s: float = WATCH_POLL_S
    shell_timeout_s: float = SHELL_TIMEOUT_S
    _lock: threading.Lock = field(default_factory=threading.Lock)


_CTX: Optional[ToolContext] = None


def set_context(ctx: Optional[ToolContext]) -> None:
    global _CTX
    _CTX = ctx


def _need(tool: str, ctx: Optional[ToolContext] = None) -> tuple:
    """(ctx, grant) if the tool is currently granted, else raises Blocked. Fails closed."""
    ctx = ctx or _CTX
    if ctx is None:
        raise Blocked("not_granted", "files and terminal are off")
    try:
        g = ctx.service.active(ctx.crew, ctx.run_id)
    except Exception:
        raise Blocked("not_granted", "could not confirm the grant") from None
    if g is None or tool not in g.tools:
        raise Blocked("not_granted", "files and terminal are off for this crew")
    return ctx, g


def _safe_path(scope: str, rel: str, must_exist: bool = False) -> str:
    rel = (rel or "").strip()
    if not rel or "\x00" in rel:
        raise Blocked("path", "empty path")
    if ".." in rel.replace("\\", "/").split("/"):
        raise Blocked("path", "path leaves the folder")
    full = rel if os.path.isabs(rel) else os.path.join(scope, rel)
    if not within(full, scope):
        raise Blocked("path", "path is outside the folder")
    if ".git" in os.path.relpath(os.path.realpath(full), os.path.realpath(scope)).split(os.sep) or ".git" in rel.replace("\\", "/").split("/"):
        raise Blocked("path", ".git is off limits")
    if must_exist and not os.path.exists(full):
        raise Blocked("path", "no such file")
    return full


def _blocked_text(b: Blocked) -> str:
    return f"Tool error: not allowed ({b.category}): {b.why}"


def _audit_blocked(ctx: ToolContext, tool: str, arg: str, b: Blocked, scope: str = "") -> None:
    target = redact_one_line(str(arg).replace(scope, "") if scope else str(arg), 120)
    ctx.service.log(f"crew:{tool}", ctx.crew, "blocked", f"{tool} blocked: {b.category}", target=target, run_id=ctx.run_id)


def _checkpoint_once(ctx: ToolContext, scope: str) -> Optional[str]:
    with ctx._lock:
        if ctx.checkpointed:
            return None
        h = ctx.checkpoint(scope)
        ctx.checkpointed = True
        return h


def precheck(tool: str) -> Callable[[str], Optional[str]]:
    """Run before the approval request: refuse a tool that is not granted, so nobody is asked to
    approve something that cannot run."""
    def check(arg: str) -> Optional[str]:
        try:
            _need(tool)
        except Blocked as b:
            if _CTX is not None:
                _audit_blocked(_CTX, tool, "", b)
            return _blocked_text(b)
        return None
    return check


def fs_read(arg: str) -> str:
    ctx = _CTX
    try:
        ctx, g = _need("fs_read")
        p = _safe_path(g.scope_path, arg, must_exist=True)
        if not os.path.isfile(p):
            raise Blocked("path", "not a file")
        with open(p, "rb") as f:
            data = f.read(FS_READ_CHARS + 1)
    except Blocked as b:
        if ctx is not None:
            _audit_blocked(ctx, "fs_read", arg, b)
        return _blocked_text(b)
    return redact_block(data.decode("utf-8", "replace"), FS_READ_CHARS, keep="head")


def fs_write(arg: str) -> str:
    """arg: JSON {"path": relative path, "content": text}. Writes atomically inside the folder
    after a git checkpoint (once per run)."""
    ctx = _CTX
    try:
        ctx, g = _need("fs_write")
        try:
            doc = json.loads(arg)
            rel, content = doc["path"], doc["content"]
            if not isinstance(rel, str) or not isinstance(content, str):
                raise TypeError
        except (ValueError, KeyError, TypeError):
            raise Blocked("syntax", 'expected JSON {"path": ..., "content": ...}') from None
        if len(content.encode("utf-8")) > FS_WRITE_MAX_BYTES:
            raise Blocked("size", "content is too large")
        p = _safe_path(g.scope_path, rel)
        if ".git" in os.path.relpath(p, g.scope_path).split(os.sep):
            raise Blocked("path", ".git is off limits")
        if os.path.islink(p):
            raise Blocked("path", "refusing to write through a symlink")
    except Blocked as b:
        if ctx is not None:
            _audit_blocked(ctx, "fs_write", arg if not arg.lstrip().startswith("{") else "", b)
        return _blocked_text(b)
    try:
        h = _checkpoint_once(ctx, g.scope_path)
    except CheckpointError:
        _audit_blocked(ctx, "fs_write", rel, Blocked("checkpoint", "no git checkpoint; write refused"))
        return "Tool error: write refused: could not take a git checkpoint first"
    t0 = time.monotonic()
    os.makedirs(os.path.dirname(p), exist_ok=True)
    tmp = p + ".crew-tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(content)
    os.replace(tmp, p)
    rel_p = os.path.relpath(p, g.scope_path)
    ctx.service.log(f"crew:{ctx.crew}", ctx.crew, "fs_write", "wrote a file", target=rel_p, run_id=ctx.run_id,
                    duration_ms=int((time.monotonic() - t0) * 1000), checkpoint=h)
    return f"wrote {rel_p} ({len(content)} chars)"


def shell(arg: str) -> str:
    ctx = _CTX
    scope = ""
    try:
        ctx, g = _need("shell")
        scope = g.scope_path
        argv = parse_shell(arg, scope)
    except Blocked as b:
        if ctx is not None:
            _audit_blocked(ctx, "shell", arg, b, scope)
        return _blocked_text(b)
    try:
        h = _checkpoint_once(ctx, scope)
    except CheckpointError:
        _audit_blocked(ctx, "shell", arg, Blocked("checkpoint", "no git checkpoint"), scope)
        return "Tool error: command refused: could not take a git checkpoint first"
    t0 = time.monotonic()
    proc = subprocess.Popen(argv, cwd=scope, env=scrub_env(_GIT_ENV if argv[0] == "git" else None), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, start_new_session=True)
    why = ""
    box: Dict[str, bytes] = {}
    reader = threading.Thread(target=lambda: box.__setitem__("out", proc.stdout.read(1_000_000)), daemon=True)
    reader.start()
    while proc.poll() is None:
        if time.monotonic() - t0 > ctx.shell_timeout_s:
            why = "timed out"
        else:
            try:
                _need("shell", ctx)
            except Blocked:
                why = "grant ended"
        if why:
            _kill(proc)
            break
        time.sleep(ctx.poll_s)
    reader.join(2)
    code = proc.returncode
    out = redact_block((box.get("out") or b"").decode("utf-8", "replace"), SHELL_OUTPUT_CHARS)
    ctx.service.log(f"crew:{ctx.crew}", ctx.crew, "shell", f"ran a command{' (' + why + ')' if why else ''}",
                    target=redact_one_line(arg.replace(scope, ""), 120), run_id=ctx.run_id, exit_code=code,
                    duration_ms=int((time.monotonic() - t0) * 1000), checkpoint=h)
    if why:
        return f"Tool error: command stopped ({why})"
    return f"exit {code}\n{out}"


def _kill(proc: subprocess.Popen, grace_s: float = 2.0) -> None:
    for sig, wait in ((signal.SIGTERM, grace_s), (signal.SIGKILL, 2.0)):
        try:
            os.killpg(proc.pid, sig)
        except (ProcessLookupError, PermissionError):
            return
        try:
            proc.wait(wait)
            return
        except subprocess.TimeoutExpired:
            continue


TOOL_SPECS = {
    "fs_read": ("Read a text file inside this crew's sandbox folder. Input: a relative path.", fs_read, "write"),
    "fs_write": ('Write a text file inside the sandbox folder. Input: JSON {"path": "...", "content": "..."}.', fs_write, "write"),
    "shell": ("Run one file-utility command (ls, cat, head, tail, wc, grep, sort, uniq, diff, echo, mkdir, touch, cp, mv, rm, date, git status/diff/log) inside the sandbox folder. No pipes, redirects, interpreters or network. Input: the command.", shell, "write"),
}

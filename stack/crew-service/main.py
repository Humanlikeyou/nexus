"""Terrance crew-service — Stage 5E.

FastAPI wrapper around CrewAI. Contract matches src-tauri/src/commands/crew.rs:
  GET  /health                 (unauthenticated, liveness only)
  GET  /crews/list             -> [CrewInfo]
  POST /crews/start            {name, task, agents?} -> CrewInfo
  GET  /crews/status?id=       -> CrewInfo
  GET  /crews/runs/{id}/steps  -> {steps, handoffs}
  POST /crews/stop             {id} -> {"id", "status", "stopped": bool}
  GET  /crews/approvals?state= pending|decided -> [ApprovalOut]
  POST /crews/approvals/{id}/decide {approve} -> ApprovalOut (409 if already decided)
  POST /crews/retry            {id} -> CrewInfo (new run, retry_of = id)

Every /crews/* call needs header X-Crew-Token == $CREW_SERVICE_TOKEN.
LLM: every model call goes through the Amara Router (OpenAI /v1, CREW_ROUTER_URL). Each agent
role has a tier (fast|deep) pinned by model alias (amara-fast / amara-deep) and an
X-Amara-Route header. Never Ollama directly. There is no cloud path in this service (MiniMax was
removed); a Claude tier, if one is ever added, must also be off while NEXUS_OUTBOUND_DISABLED is set.
Runs persist to Postgres table crew_runs (created at startup).
Each crew runs in its own process so /crews/stop can really cancel it.
Tools: an agent gets a tool only if it is implemented in TOOL_REGISTRY and named in
CREW_ALLOWED_TOOLS (unset = no tools). Each call has a timeout and an output cap.
"""

from __future__ import annotations

import hmac
import logging
import multiprocessing as mp
import os
import re
import signal
import threading
import time
import urllib.parse
import uuid
from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import TimeoutError as FutureTimeout
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Callable, Dict, List, Optional

import psycopg
from fastapi import Depends, FastAPI, Header, HTTPException, Query
from pydantic import BaseModel, Field, StrictBool, field_validator

log = logging.getLogger("crew-service")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


# ── Config (env) ────────────────────────────────────────────────────────────
CREW_SERVICE_TOKEN = _env("CREW_SERVICE_TOKEN")
MAX_TASK_CHARS = int(_env("CREW_MAX_TASK_CHARS", "4000"))
MAX_CONCURRENT = int(_env("CREW_MAX_CONCURRENT", "1"))  # one crew = one in-flight LLM generation
RUN_TIMEOUT_S = int(_env("CREW_RUN_TIMEOUT_SECONDS", "900"))
# Automatic retries of a failed attempt before a person must decide (kind retry_limit).
AUTO_RETRIES = int(_env("CREW_AUTO_RETRIES", "2"))
RETRY_BACKOFF_S = float(_env("CREW_RETRY_BACKOFF_SECONDS", "2"))
# A crew run starts only once the deep model is not resident (a deep chat turn is using it).
# 0 turns the wait off. The wait is capped so a crew can't starve.
DEEP_WAIT_MAX_S = float(_env("CREW_DEEP_WAIT_MAX_SECONDS", "300"))
DEEP_WAIT_POLL_S = 5.0
# How long a child waits for a human decision. A request also extends the run's deadline by this.
APPROVAL_TIMEOUT_S = int(_env("CREW_APPROVAL_TIMEOUT_SECONDS", "1800"))

# OLLAMA_BASE_URL / CREW_OLLAMA_MODEL are no longer read here: the router owns model residency.
CREW_ROUTER_URL = _env("CREW_ROUTER_URL", "http://host.docker.internal:8787/v1")
CREW_ROUTER_KEY = _env("CREW_ROUTER_KEY")  # optional bearer; the loopback router needs none today
NEXUS_OUTBOUND_DISABLED = _env("NEXUS_OUTBOUND_DISABLED")
ROUTER_TIERS = ("fast", "deep")
OLLAMA_PORT = 11 * 1000 + 434  # refused as a router port (see router_llm_kwargs)

PG = dict(
    host=_env("CREW_DB_HOST", "postgres"),
    port=int(_env("CREW_DB_PORT", "5432")),
    dbname=_env("CREW_DB_NAME", "terrance"),
    user=_env("POSTGRES_USER"),
    password=_env("POSTGRES_PASSWORD"),
    connect_timeout=5,
)

# ── Allowlists ──────────────────────────────────────────────────────────────
# Crew names: optional explicit allowlist via env; always a safe charset.
CREW_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9 _.\-]{0,63}$")
ALLOWED_CREW_NAMES = {n.strip() for n in _env("CREW_ALLOWED_NAMES").split(",") if n.strip()}

# ── Tool registry ───────────────────────────────────────────────────────────
# A tool must be both implemented here (TOOL_REGISTRY) and named in CREW_ALLOWED_TOOLS
# (comma-separated) before any agent receives it. Empty or unset allows none: fail closed.
DEFAULT_TOOL_TIMEOUT_S = 15.0
DEFAULT_TOOL_OUTPUT_CHARS = 4000


@dataclass(frozen=True)
class CrewTool:
    name: str
    description: str
    fn: Callable[[str], str]
    timeout_s: float = DEFAULT_TOOL_TIMEOUT_S
    max_output_chars: int = DEFAULT_TOOL_OUTPUT_CHARS
    # What a human is asked when this tool is called (ignored for READ_ONLY_TOOLS). Kinds in
    # BLOCKED_KINDS are never approvable: the call is refused and no request is made.
    approval_kind: str = "write"


# Tools that run without asking. Anything else that reaches an agent (registered + allowlisted)
# blocks on an approval request first: an unknown or non-read-only tool fails closed.
READ_ONLY_TOOLS = frozenset({"kb_query", "memory_query"})


TOOL_REGISTRY: Dict[str, CrewTool] = {}


def register_tool(tool: CrewTool, registry: Optional[Dict[str, CrewTool]] = None) -> None:
    reg = TOOL_REGISTRY if registry is None else registry
    if tool.name in reg:
        raise ValueError(f"tool {tool.name} is already registered")
    reg[tool.name] = tool


def parse_allowed_tools(raw: str, registry: Dict[str, CrewTool]) -> frozenset:
    """Names from CREW_ALLOWED_TOOLS that are implemented. Unknown names are dropped (logged)."""
    names = {n.strip() for n in raw.split(",") if n.strip()}
    unknown = sorted(names - registry.keys())
    if unknown:
        log.warning("CREW_ALLOWED_TOOLS names unknown tools %s; ignoring them", unknown)
    return frozenset(names & registry.keys())


# Tools implemented in this file that register only when their config is present (the Ingress
# tools need CREW_INGRESS_SECRET). An agent may declare these even when they're unregistered.
OPTIONAL_TOOLS = frozenset({"kb_query", "memory_query"})


def tools_for(agent_key: str, allowed: Optional[frozenset] = None,
              registry: Optional[Dict[str, CrewTool]] = None,
              optional: frozenset = OPTIONAL_TOOLS) -> List[CrewTool]:
    """The tools an agent may receive: its declared tools that are registered and allowlisted.
    A declared tool that is neither registered nor a known optional tool is a bug and raises."""
    allowed = ALLOWED_TOOLS if allowed is None else allowed
    registry = TOOL_REGISTRY if registry is None else registry
    out = []
    for name in AGENT_CATALOG[agent_key]["tools"]:
        if name not in registry:
            if name in optional:
                continue
            raise ValueError(f"agent {agent_key} declares unknown tool {name}")
        if name in allowed:
            out.append(registry[name])
    return out


def run_tool(tool: CrewTool, arg: str) -> str:
    """Call a tool with its timeout and output cap. Failures come back as a tool error string
    (sanitized), never an exception, so one bad tool call can't crash the crew."""
    pool = ThreadPoolExecutor(max_workers=1, thread_name_prefix=f"tool-{tool.name}")
    try:
        out = pool.submit(tool.fn, arg).result(timeout=tool.timeout_s)
    except FutureTimeout:
        return f"Tool error: {tool.name} timed out after {tool.timeout_s:g}s"
    except Exception as e:
        return f"Tool error: {tool.name}: {sanitize_error(e)}"
    finally:
        pool.shutdown(wait=False, cancel_futures=True)
    out = "" if out is None else str(out)
    if len(out) > tool.max_output_chars:
        out = out[:tool.max_output_chars] + "…[truncated]"
    return out


# Approvable kinds. A run asks ONCE for everything its write tools may do (a batch with a list of
# items); one approve covers the batch, a deny is final (task cancelled).
APPROVAL_KINDS = ("write", "destructive", "skill", "retry_limit", "claude_tier")
# Never approvable, whoever asks: network/outbound, spending, git push, sudo. No request is created.
BLOCKED_KINDS = frozenset({"outbound", "spend", "network", "git_push", "sudo"})
# Risk is decided here from the kind, never taken from the child; a batch takes its riskiest item.
APPROVAL_RISK = {"write": "Medium", "destructive": "High", "skill": "Medium",
                 "retry_limit": "Low", "claude_tier": "Medium"}
_RISK_ORDER = ("destructive", "skill", "claude_tier", "write", "retry_limit")
APPROVAL_REASON = {
    "write": "the run wants to change something", "destructive": "the run wants to do something destructive",
    "skill": "it changes a skill or instruction", "retry_limit": "the task failed after its automatic retries",
    "claude_tier": "it asks for the Claude tier",
}
APPROVAL_TITLE = {
    "write": "Allow this run to make changes", "destructive": "Allow this run to make destructive changes",
    "skill": "Allow this run to change a skill", "retry_limit": "Retry after repeated failures",
    "claude_tier": "Allow this run to use the Claude tier",
}
MAX_APPROVAL_ITEMS = 20
APPROVAL_DENIED = "Tool error: approval was denied"
TOOL_BLOCKED = "Tool error: this action is not allowed"


def outbound_disabled() -> bool:
    return bool(NEXUS_OUTBOUND_DISABLED.strip())


def tool_blocked(tool: CrewTool) -> bool:
    """Never approvable. The Claude tier is also off while the outbound kill switch is set."""
    return tool.approval_kind in BLOCKED_KINDS or (tool.approval_kind == "claude_tier" and outbound_disabled())


def riskiest_kind(kinds) -> str:
    kinds = [k for k in kinds if k in APPROVAL_KINDS]
    return min(kinds, key=_RISK_ORDER.index) if kinds else "write"


def write_candidates(tools) -> Dict[str, str]:
    """name -> kind of the tools in a run that need approval and may ever be approved."""
    return {t.name: t.approval_kind for t in tools
            if t.name not in READ_ONLY_TOOLS and not tool_blocked(t)}


class ChildApprover:
    """Child side of the approval pipe. A request goes to the parent and BLOCKS the calling tool
    until the parent answers on `resp`. Per run the first write call asks once, listing every write
    action the run could take (`candidates`); an approve covers all of them for the rest of the run.
    Deny, timeout or a closed pipe abort the run (fail closed): the parent marks it cancelled.
    Messages carry a kind and tool names only, never prompt text or tool arguments."""

    def __init__(self, send: Callable[[dict], None], resp, timeout_s: float = APPROVAL_TIMEOUT_S,
                 abort: Optional[Callable[[], None]] = None, candidates: Optional[Dict[str, str]] = None):
        self._send, self._resp, self._timeout = send, resp, timeout_s
        self._abort = abort or self._exit
        self._candidates = dict(candidates or {})
        self.approved: set = set()
        self._lock = threading.Lock()  # one outstanding request per run
        self._ask_lock = threading.Lock()  # one prompt for parallel first writes

    @staticmethod
    def _exit() -> None:
        os._exit(0)  # the final message was already sent; nothing else may run after a denial

    def request(self, kind: str, items: List[str]) -> bool:
        if kind not in APPROVAL_KINDS:
            kind = "write"
        with self._lock:
            self._send({"k": "approval", "kind": kind, "items": [str(i)[:MAX_LABEL_CHARS] for i in items][:MAX_APPROVAL_ITEMS]})
            why = "approval timed out"
            try:
                if self._resp.poll(self._timeout):
                    m = self._resp.recv()
                    if isinstance(m, dict) and m.get("k") == "decision" and m.get("approve") is True:
                        return True
                    why = "approval denied"
            except (EOFError, OSError):
                why = "approval channel closed"
            try:
                self._send({"k": "final", "status": "cancelled", "result": why, "llm": None})
            finally:
                self._abort()
            return False

    def allow_tool(self, tool: CrewTool) -> bool:
        """True if this tool is covered by an approved batch; otherwise ask once for the batch."""
        with self._lock:
            if tool.name in self.approved:
                return True
        # One request at a time: a second parallel first-write waits here and then re-checks, so
        # the human sees one prompt for the batch, not two.
        with self._ask_lock:
            with self._lock:
                if tool.name in self.approved:
                    return True
            batch = dict(self._candidates)
            batch[tool.name] = tool.approval_kind
            if not self.request(riskiest_kind(batch.values()), sorted(batch)):
                return False
            with self._lock:
                self.approved |= set(batch)
            return True


def gate_tool_call(tool: CrewTool, approver: Optional[ChildApprover]) -> Optional[str]:
    """None if the call may run; otherwise the tool-error string to return instead. Read-only tools
    pass. Blocked kinds are refused outright. Everything else needs an approver and a yes (once per
    run, for the batch); no approver means no."""
    if tool.name in READ_ONLY_TOOLS:
        return None
    if tool_blocked(tool):
        return TOOL_BLOCKED
    if approver is None or not approver.allow_tool(tool):
        return APPROVAL_DENIED
    return None


def run_with_retries(attempt: Callable[[], object], approver: Optional[ChildApprover],
                     max_retries: Optional[int] = None, backoff_s: Optional[float] = None,
                     sleep: Callable[[float], None] = time.sleep):
    """Run attempt(); on failure retry automatically up to max_retries times, then ask a person
    (kind retry_limit). An approve buys exactly one more attempt; a deny cancels the run.
    Returns (True, result) or (False, sanitized error)."""
    max_retries = AUTO_RETRIES if max_retries is None else max_retries
    backoff_s = RETRY_BACKOFF_S if backoff_s is None else backoff_s
    failures, last = 0, "no attempt ran"
    while True:
        try:
            return True, attempt()
        except Exception as e:
            last = sanitize_error(e)
            failures += 1
        if failures <= max_retries:
            sleep(backoff_s * failures)
            continue
        if approver is None or not approver.request("retry_limit", ["Retry the failed task"]):
            return False, last
        failures = max_retries  # one more attempt, then ask again


def router_health_url(url: str = "") -> str:
    base = check_router_url(url or CREW_ROUTER_URL)
    return (base[:-3] if base.endswith("/v1") else base) + "/health"


def deep_resident(url: str = "", opener=None) -> Optional[bool]:
    """Is the deep model resident, per the router's GET /health (`loaded.deep`)? None if unknown.
    The router exposes residency, not an in-flight flag: deep is manual/on-demand, so a resident deep
    model is the best signal that a deep chat turn is using the GPU. Same local host as the LLM calls."""
    import json
    import urllib.request
    try:
        with (opener or urllib.request.urlopen)(router_health_url(url), timeout=3) as resp:
            doc = json.loads(resp.read(64 * 1024))
        deep = (doc.get("loaded") or {}).get("deep")
        return deep if isinstance(deep, bool) else None
    except Exception:
        return None


def wait_for_deep_idle(max_wait_s: Optional[float] = None, poll_s: Optional[float] = None,
                       probe: Callable[[], Optional[bool]] = deep_resident,
                       sleep: Callable[[float], None] = time.sleep, now: Callable[[], float] = time.monotonic) -> float:
    """Hold a crew run while the deep model is resident, up to max_wait_s. Unknown (router down or
    no field) does not wait: the run's own LLM calls report that. Returns seconds waited."""
    max_wait_s = DEEP_WAIT_MAX_S if max_wait_s is None else max_wait_s
    poll_s = DEEP_WAIT_POLL_S if poll_s is None else poll_s
    t0 = now()
    while max_wait_s > 0 and probe() is True and now() - t0 < max_wait_s:
        sleep(poll_s)
    return now() - t0


def _to_crewai_tool(tool: CrewTool, recorder=None, agent: str = "crew", approver=None):
    """Wrap a CrewTool as a CrewAI tool (imported lazily: only the crew child needs crewai).
    With a recorder, each call is timed as a tool step; a "Tool error:" result (already
    sanitized by run_tool) marks it failed. The step never carries the query or the output."""
    from crewai.tools import BaseTool

    class _Tool(BaseTool):
        name: str = tool.name
        description: str = tool.description

        def _run(self, query: str) -> str:
            blocked = gate_tool_call(tool, approver)
            if blocked is not None:
                return blocked
            t0 = time.time()
            out = run_tool(tool, query)
            if recorder is not None:
                failed = out.startswith("Tool error:")
                recorder.step("tool", agent, step_label("tool", agent, tool.name), t0, time.time(),
                              "failed" if failed else "ok",
                              sanitize_text(out[len("Tool error:"):].strip()) if failed else None)
            return out

    return _Tool()


# ── Ingress tools (read-only) ───────────────────────────────────────────────
# kb_query and memory_query call the n8n Ingress's crew webhook, which accepts only those two
# event types for this key (anything else is 403). The key is sent as X-Nexus-Secret on these
# calls only and is scrubbed from every output and error.
CREW_INGRESS_SECRET = _env("CREW_INGRESS_SECRET")
CREW_INGRESS_URL = _env("CREW_INGRESS_URL", "http://n8n:5678/webhook/terrance/ingress-crew")
INGRESS_TIMEOUT_S = 15.0
INGRESS_OUTPUT_CHARS = 4000
MAX_QUERY_CHARS = 2000


class IngressError(RuntimeError):
    """A failed Ingress call. Its message is fixed text plus an HTTP status, never the body."""


def _scrub(text: str, secret: str) -> str:
    return text.replace(secret, "[redacted]") if secret else text


def ingress_call(event_type: str, payload: dict, *, url: str, secret: str,
                 timeout: float = INGRESS_TIMEOUT_S, opener=None) -> dict:
    """POST one event to the Ingress and return its `result`. Raises IngressError."""
    import json
    import urllib.error
    import urllib.request

    if not url.startswith(("http://", "https://")):
        raise IngressError("ingress URL must be http(s)")
    body = json.dumps({"trace_id": f"crew-{uuid.uuid4().hex[:12]}",
                       "event": {"type": event_type, "payload": payload}}).encode()
    req = urllib.request.Request(url, data=body, method="POST", headers={
        "Content-Type": "application/json", "X-Nexus-Secret": secret})
    open_ = opener or urllib.request.urlopen
    try:
        with open_(req, timeout=timeout) as resp:
            raw = resp.read(256 * 1024)
    except urllib.error.HTTPError as e:
        raise IngressError(f"{event_type}: ingress returned HTTP {e.code}") from None
    except (urllib.error.URLError, OSError) as e:
        reason = getattr(e, "reason", e)
        raise IngressError(f"{event_type}: ingress unreachable ({type(reason).__name__})") from None
    try:
        doc = json.loads(raw)
    except ValueError:
        raise IngressError(f"{event_type}: ingress returned invalid JSON") from None
    if not isinstance(doc, dict) or doc.get("ok") is not True:
        err = doc.get("error") if isinstance(doc, dict) else None
        msg = err.get("message") if isinstance(err, dict) else None
        raise IngressError(f"{event_type}: {sanitize_text(str(msg)) if msg else 'request failed'}")
    return doc.get("result") or {}


def _ingress_tool(fn: Callable[[str], str], secret: str) -> Callable[[str], str]:
    """Scrub the key from a tool's output and from any error it raises."""
    def run(arg: str) -> str:
        try:
            return _scrub(fn(arg), secret)
        except IngressError as e:
            raise IngressError(_scrub(str(e), secret)) from None
    return run


def make_ingress_tools(secret: str, url: str, opener=None) -> List[CrewTool]:
    import json

    def kb_query(query: str) -> str:
        q = str(query or "").strip()[:MAX_QUERY_CHARS]
        if not q:
            raise IngressError("kb.query: empty query")
        result = ingress_call("kb.query", {"query": q}, url=url, secret=secret, opener=opener)
        return json.dumps(result, ensure_ascii=False)

    def memory_query(label: str) -> str:
        result = ingress_call("memory.query", {}, url=url, secret=secret, opener=opener)
        blocks = result.get("blocks") or []
        want = str(label or "").strip().lower()
        if want:
            blocks = [b for b in blocks if str(b.get("label", "")).lower() == want]
        return json.dumps([{"label": b.get("label"), "value": b.get("value")} for b in blocks],
                          ensure_ascii=False)

    return [
        CrewTool("kb_query", "Search the local knowledge base (Verba). Input: a search query.",
                 _ingress_tool(kb_query, secret), INGRESS_TIMEOUT_S, INGRESS_OUTPUT_CHARS),
        CrewTool("memory_query", "Read Amara's core memory blocks (read-only). Input: a block "
                 "label such as 'human' or 'persona', or empty for all.",
                 _ingress_tool(memory_query, secret), INGRESS_TIMEOUT_S, INGRESS_OUTPUT_CHARS),
    ]


def register_ingress_tools(registry: Dict[str, CrewTool], secret: str, url: str, opener=None) -> bool:
    """Register kb_query and memory_query when the crew key is set; otherwise register neither."""
    if not secret:
        log.warning("CREW_INGRESS_SECRET not set; kb_query and memory_query are not registered")
        return False
    for tool in make_ingress_tools(secret, url, opener):
        register_tool(tool, registry)
    return True


register_ingress_tools(TOOL_REGISTRY, CREW_INGRESS_SECRET, CREW_INGRESS_URL)
ALLOWED_TOOLS = parse_allowed_tools(_env("CREW_ALLOWED_TOOLS"), TOOL_REGISTRY)


AGENT_CATALOG: Dict[str, dict] = {
    "researcher": dict(
        role="Terrance Research Assistant",
        goal="Investigate the task thoroughly and report verified findings.",
        backstory="A precise, efficient researcher in the Terrance ecosystem.",
        tools=["kb_query", "memory_query"],
        tier="fast",
    ),
    "analyst": dict(
        role="Terrance Analyst",
        goal="Turn findings into clear analysis, trade-offs and recommendations.",
        backstory="A careful analyst who separates evidence from opinion.",
        tools=[],
        tier="deep",
    ),
    "writer": dict(
        role="Terrance Writer",
        goal="Produce a concise, well-structured final write-up.",
        backstory="A clear writer who keeps output short and actionable.",
        tools=[],
        tier="fast",
    ),
}
DEFAULT_AGENTS = ["researcher"]
for _key in AGENT_CATALOG:
    tools_for(_key)  # raises at import if an agent declares an unimplemented tool


# ── Models ──────────────────────────────────────────────────────────────────
class CrewInfo(BaseModel):
    id: str
    name: str
    status: str
    agents: List[str]
    started_at: Optional[str] = None
    task: Optional[str] = None
    result: Optional[str] = None
    llm: Optional[str] = None
    finished_at: Optional[str] = None
    tokens: Optional[int] = None      # prompt + completion, from the LLM responses' usage
    requests: Optional[int] = None    # LLM calls
    cost_usd: Optional[float] = None  # 0 for the local router; None when unknown (real cost only if a Claude tier is ever used)
    retry_of: Optional[str] = None    # the run this one retries, if any


class CrewStepOut(BaseModel):
    id: int
    agent: str
    kind: str
    label: str
    start_ms: int
    end_ms: int
    status: str
    error: Optional[str] = None


class CrewHandoffOut(BaseModel):
    model_config = {"populate_by_name": True}
    from_: str = Field(alias="from")
    to: str
    status: str


class CrewRunSteps(BaseModel):
    steps: List[CrewStepOut] = []
    handoffs: List[CrewHandoffOut] = []


class CrewStartRequest(BaseModel):
    name: str = Field(min_length=1, max_length=64)
    task: str = Field(min_length=1)
    agents: Optional[List[str]] = None
    retry_of: Optional[str] = None

    @field_validator("retry_of")
    @classmethod
    def _retry_of(cls, v: Optional[str]) -> Optional[str]:
        if v is not None and not re.fullmatch(r"[0-9a-fA-F-]{36}", v):
            raise ValueError("retry_of must be a run id")
        return v

    @field_validator("name")
    @classmethod
    def _name(cls, v: str) -> str:
        v = v.strip()
        if not CREW_NAME_RE.match(v):
            raise ValueError("crew name must be 1-64 chars of letters, digits, space, _ . -")
        if ALLOWED_CREW_NAMES and v not in ALLOWED_CREW_NAMES:
            raise ValueError(f"crew name not allowed; allowed: {sorted(ALLOWED_CREW_NAMES)}")
        return v

    @field_validator("task")
    @classmethod
    def _task(cls, v: str) -> str:
        v = v.strip()
        if not v:
            raise ValueError("task must not be empty")
        if len(v) > MAX_TASK_CHARS:
            raise ValueError(f"task exceeds {MAX_TASK_CHARS} characters")
        return v

    @field_validator("agents")
    @classmethod
    def _agents(cls, v: Optional[List[str]]) -> Optional[List[str]]:
        if v is None:
            return None
        if not v:
            raise ValueError("agents, if given, must not be empty")
        unknown = [a for a in v if a not in AGENT_CATALOG]
        if unknown:
            raise ValueError(f"unknown agents {unknown}; allowed: {sorted(AGENT_CATALOG)}")
        if len(set(v)) != len(v):
            raise ValueError("agents must not repeat")
        return v


class CrewStopRequest(BaseModel):
    id: str


class ApprovalOut(BaseModel):
    id: str
    run_id: str
    crew: str
    agents: List[str]
    kind: str
    title: str
    items: List[str] = []
    reason: str
    risk: str
    requested_at: str
    decided_at: Optional[str] = None
    approved: Optional[bool] = None
    decided_by: Optional[str] = None


class DecideRequest(BaseModel):
    # Strict: only a real JSON true/false. "yes" or 1 is a 422, never an approval.
    approve: StrictBool


class RetryRequest(BaseModel):
    id: str


# ── Persistence ─────────────────────────────────────────────────────────────
SCHEMA = """
CREATE TABLE IF NOT EXISTS crew_runs (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    status      TEXT NOT NULL,
    agents      TEXT[] NOT NULL,
    task        TEXT NOT NULL,
    result      TEXT,
    llm         TEXT,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS crew_runs_started_idx ON crew_runs (started_at DESC);
ALTER TABLE crew_runs ADD COLUMN IF NOT EXISTS tokens BIGINT;
ALTER TABLE crew_runs ADD COLUMN IF NOT EXISTS requests INTEGER;
CREATE TABLE IF NOT EXISTS crew_steps (
    id       BIGSERIAL PRIMARY KEY,
    run_id   TEXT NOT NULL,
    agent    TEXT NOT NULL,
    kind     TEXT NOT NULL CHECK (kind IN ('task','tool','llm')),
    label    TEXT NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms   INTEGER NOT NULL,
    status   TEXT NOT NULL CHECK (status IN ('ok','failed','skipped')),
    error    TEXT
);
CREATE INDEX IF NOT EXISTS crew_steps_run_idx ON crew_steps (run_id, start_ms);
CREATE TABLE IF NOT EXISTS crew_handoffs (
    id         BIGSERIAL PRIMARY KEY,
    run_id     TEXT NOT NULL,
    from_agent TEXT NOT NULL,
    to_agent   TEXT NOT NULL,
    status     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS crew_handoffs_run_idx ON crew_handoffs (run_id, id);
ALTER TABLE crew_runs ADD COLUMN IF NOT EXISTS retry_of TEXT;
CREATE TABLE IF NOT EXISTS crew_approvals (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('write','destructive','skill','retry_limit','claude_tier')),
    title        TEXT NOT NULL,
    items        TEXT[] NOT NULL DEFAULT '{}',
    reason       TEXT NOT NULL,
    risk         TEXT NOT NULL CHECK (risk IN ('Low','Medium','High')),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at   TIMESTAMPTZ,
    approved     BOOLEAN,
    decided_by   TEXT
);
CREATE INDEX IF NOT EXISTS crew_approvals_run_idx ON crew_approvals (run_id);
CREATE INDEX IF NOT EXISTS crew_approvals_pending_idx ON crew_approvals (decided_at, requested_at);
"""


def db():
    return psycopg.connect(**PG, autocommit=True)


def _row_to_info(r) -> CrewInfo:
    return CrewInfo(
        id=r[0], name=r[1], status=r[2], agents=list(r[3]),
        started_at=r[4].strftime("%Y-%m-%dT%H:%M:%SZ") if r[4] else None,
        task=r[5], result=safe_result(r[2], r[6]),
        llm=r[7] if len(r) > 7 else None,
        finished_at=r[8].strftime("%Y-%m-%dT%H:%M:%SZ") if len(r) > 8 and r[8] else None,
        tokens=r[9] if len(r) > 9 else None,
        requests=r[10] if len(r) > 10 else None,
        cost_usd=run_cost_usd(r[7] if len(r) > 7 else None),
        retry_of=r[11] if len(r) > 11 else None,
    )


def run_cost_usd(llm: Optional[str]) -> Optional[float]:
    """0 for runs on the local router; None (unknown) for anything else."""
    if not llm or not all(p.startswith("amara-") for p in llm.split(",")):
        return None
    return 0.0


_SELECT = ("SELECT id, name, status, agents, started_at AT TIME ZONE 'UTC', task, result, llm, "
           "finished_at AT TIME ZONE 'UTC', tokens, requests, retry_of FROM crew_runs")


def db_get(crew_id: str) -> Optional[CrewInfo]:
    with db() as c:
        r = c.execute(_SELECT + " WHERE id = %s", (crew_id,)).fetchone()
    return _row_to_info(r) if r else None


def db_finish(crew_id: str, status: str, result: Optional[str], llm: Optional[str] = None) -> None:
    # Never overwrite a terminal state (e.g. a stop that already landed).
    with db() as c:
        c.execute(
            "UPDATE crew_runs SET status=%s, result=%s, llm=COALESCE(%s, llm), finished_at=now() "
            "WHERE id=%s AND status IN ('running','awaiting_approval')",
            (status, safe_result(status, result), llm, crew_id),
        )


LIVE_STATUSES = ("running", "awaiting_approval")


# ── Approvals ───────────────────────────────────────────────────────────────
_APPROVAL_SELECT = (
    "SELECT a.id, a.run_id, r.name, r.agents, a.kind, a.title, a.items, a.reason, a.risk, "
    "a.requested_at AT TIME ZONE 'UTC', a.decided_at AT TIME ZONE 'UTC', a.approved, a.decided_by "
    "FROM crew_approvals a JOIN crew_runs r ON r.id = a.run_id")
APPROVAL_ID_RE = re.compile(r"^[0-9a-fA-F-]{36}$")


def _iso(t) -> Optional[str]:
    return t.strftime("%Y-%m-%dT%H:%M:%SZ") if t else None


def _row_to_approval(r) -> ApprovalOut:
    return ApprovalOut(id=r[0], run_id=r[1], crew=r[2], agents=list(r[3]), kind=r[4],
                       title=sanitize_text(r[5]), items=[sanitize_text(i) for i in (r[6] or [])],
                       reason=sanitize_text(r[7]), risk=r[8],
                       requested_at=_iso(r[9]) or "", decided_at=_iso(r[10]), approved=r[11],
                       decided_by=r[12])


def db_create_approval(crew_id: str, m: dict) -> str:
    """Store a child's batch request and park the run as awaiting_approval. kind is allowlisted;
    title, reason and risk come from the kind (never the child); items are sanitized and capped."""
    kind = m.get("kind") if m.get("kind") in APPROVAL_KINDS else "write"
    raw = m.get("items") if isinstance(m.get("items"), list) else []
    items = [t for t in (sanitize_text(i)[:MAX_LABEL_CHARS] for i in raw[:MAX_APPROVAL_ITEMS]) if t]
    approval_id = str(uuid.uuid4())
    with db() as c:
        c.execute("INSERT INTO crew_approvals (id, run_id, kind, title, items, reason, risk) "
                  "VALUES (%s,%s,%s,%s,%s,%s,%s)",
                  (approval_id, crew_id, kind, APPROVAL_TITLE[kind], items, APPROVAL_REASON[kind],
                   APPROVAL_RISK[kind]))
        c.execute("UPDATE crew_runs SET status='awaiting_approval' WHERE id=%s AND status='running'",
                  (crew_id,))
    return approval_id


def db_list_approvals(state: str, limit: int = 50) -> List[ApprovalOut]:
    if state == "pending":
        sql, params = _APPROVAL_SELECT + " WHERE a.decided_at IS NULL ORDER BY a.requested_at", ()
    else:
        sql, params = (_APPROVAL_SELECT + " WHERE a.decided_at IS NOT NULL "
                       "ORDER BY a.decided_at DESC LIMIT %s"), (limit,)
    with db() as c:
        rows = c.execute(sql, params).fetchall()
    return [_row_to_approval(r) for r in rows]


def db_decide_approval(approval_id: str, approve: bool, by: str = "user") -> Optional[tuple]:
    """Atomically record a decision. Returns (run_id,) if this call decided it, None if it was
    already decided (the caller answers 409) or does not exist (see db_approval_exists)."""
    with db() as c:
        r = c.execute("UPDATE crew_approvals SET decided_at=now(), approved=%s, decided_by=%s "
                      "WHERE id=%s AND decided_at IS NULL RETURNING run_id",
                      (approve, by, approval_id)).fetchone()
    return (r[0],) if r else None


def db_approval_exists(approval_id: str) -> bool:
    with db() as c:
        return c.execute("SELECT 1 FROM crew_approvals WHERE id=%s", (approval_id,)).fetchone() is not None


def db_get_approval(approval_id: str) -> Optional[ApprovalOut]:
    with db() as c:
        r = c.execute(_APPROVAL_SELECT + " WHERE a.id=%s", (approval_id,)).fetchone()
    return _row_to_approval(r) if r else None


def db_expire_approvals(crew_id: str, by: str = "system") -> None:
    """A run that ended (stopped, crashed, timed out, restarted) can't use a pending request."""
    with db() as c:
        c.execute("UPDATE crew_approvals SET decided_at=now(), approved=false, decided_by=%s "
                  "WHERE run_id=%s AND decided_at IS NULL", (by, crew_id))


def db_resume_run(crew_id: str) -> None:
    with db() as c:
        c.execute("UPDATE crew_runs SET status='running' WHERE id=%s AND status='awaiting_approval'",
                  (crew_id,))


STEP_KINDS = ("task", "tool", "llm")
STEP_STATUSES = ("ok", "failed", "skipped")
MAX_LABEL_CHARS = 80
MAX_STEPS_PER_RUN = 500


def db_set_totals(crew_id: str, tokens: int, requests: int) -> None:
    with db() as c:
        c.execute("UPDATE crew_runs SET tokens=%s, requests=%s WHERE id=%s",
                  (max(0, int(tokens)), max(0, int(requests)), crew_id))


def db_add_step(crew_id: str, m: dict) -> None:
    """Store one step message from the child. Re-validated here: kind/status are allowlisted,
    the label is capped and the error text is sanitized, whatever the child sent."""
    kind = m.get("kind") if m.get("kind") in STEP_KINDS else "task"
    status = m.get("status") if m.get("status") in STEP_STATUSES else "failed"
    start = max(0, int(m.get("start_ms") or 0))
    end = max(start, int(m.get("end_ms") or 0))
    err = sanitize_text(m.get("error")) if m.get("error") else None
    if err and kind != "tool":  # only tool errors (our own sanitized strings) keep message text
        err = fixed_failure("LLM call failed" if kind == "llm" else "Task failed",
                            re.sub(r"^.*?\((\w+)\)$", r"\1", err) if re.search(r"\(\w+\)$", err) else None)
    with db() as c:
        c.execute(
            "INSERT INTO crew_steps (run_id, agent, kind, label, start_ms, end_ms, status, error) "
            "SELECT %s,%s,%s,%s,%s,%s,%s,%s WHERE (SELECT count(*) FROM crew_steps WHERE run_id=%s) < %s",
            (crew_id, str(m.get("agent") or "crew")[:MAX_LABEL_CHARS], kind,
             str(m.get("label") or kind)[:MAX_LABEL_CHARS], start, end, status, err,
             crew_id, MAX_STEPS_PER_RUN))


MAX_HANDOFFS_PER_RUN = 50


def db_add_handoff(crew_id: str, m: dict) -> None:
    status = m.get("status") if m.get("status") in STEP_STATUSES else "ok"
    frm, to = str(m.get("from") or ""), str(m.get("to") or "")
    known = set(AGENT_CATALOG) | set(TOOL_REGISTRY)  # tool names only if registered
    if frm not in known or to not in known:
        return
    with db() as c:
        c.execute(
            "INSERT INTO crew_handoffs (run_id, from_agent, to_agent, status) "
            "SELECT %s,%s,%s,%s WHERE (SELECT count(*) FROM crew_handoffs WHERE run_id=%s) < %s",
            (crew_id, frm, to, status, crew_id, MAX_HANDOFFS_PER_RUN))


def db_run_steps(crew_id: str) -> CrewRunSteps:
    with db() as c:
        steps = c.execute(
            "SELECT id, agent, kind, label, start_ms, end_ms, status, error FROM crew_steps "
            "WHERE run_id=%s ORDER BY start_ms, id", (crew_id,)).fetchall()
        hands = c.execute("SELECT from_agent, to_agent, status FROM crew_handoffs "
                          "WHERE run_id=%s ORDER BY id", (crew_id,)).fetchall()
    return CrewRunSteps(
        steps=[CrewStepOut(id=r[0], agent=r[1], kind=r[2], label=r[3], start_ms=r[4], end_ms=r[5],
                           status=r[6], error=sanitize_text(r[7]) if r[7] else None) for r in steps],
        handoffs=[CrewHandoffOut(**{"from": r[0]}, to=r[1], status=r[2]) for r in hands])


# ── Error sanitizing ────────────────────────────────────────────────────────
_SECRETISH = re.compile(r"(?i)(bearer\s+\S+|(?:api[_-]?key|token|password|secret)\s*[=:]\s*\S+|[A-Za-z0-9_\-]{32,})")
_URL_CREDS = re.compile(r"(\w+://)[^/\s:@]+:[^/\s@]+@")
# Filesystem paths (/a/b, ~/a, ./a/b). URL paths are kept: they follow host:port, not a boundary.
_SEG = r"/[^\s'\"/:,;()\[\]<>]+"
_PATH = re.compile(rf"(?<![\w.:/])(?:(?:~|\.{{1,2}})(?:{_SEG})+|(?:{_SEG}){{2,}})/?")
MAX_ERR_CHARS = 300


def sanitize_text(text: Optional[str]) -> str:
    """First line only, stack traces dropped, URL creds/paths/secrets redacted, capped."""
    msg = str(text).splitlines()[0] if text else ""
    if msg.lstrip().startswith(("Traceback", 'File "')):  # an embedded stack trace
        msg = ""
    msg = _URL_CREDS.sub(r"\1***@", msg)
    msg = _PATH.sub("[path]", msg)
    msg = _SECRETISH.sub("[redacted]", msg)
    msg = "".join(ch for ch in msg if ch.isprintable())
    if len(msg) > MAX_ERR_CHARS:
        msg = msg[:MAX_ERR_CHARS] + "…"
    return msg


def sanitize_error(exc: BaseException) -> str:
    """Exception type + a short, redacted message. Never the raw upstream body."""
    msg = sanitize_text(str(exc))
    return f"{type(exc).__name__}: {msg}" if msg else type(exc).__name__


def safe_result(status: Optional[str], result: Optional[str]) -> Optional[str]:
    """Error results (failed/interrupted) are re-sanitized on write and read, so rows stored
    before sanitizing existed never reach clients raw. Completed output is left as is."""
    if result is None or status not in ("failed", "interrupted"):
        return result
    return sanitize_text(result) or "error"


# ── Crew execution (child process) ──────────────────────────────────────────
def router_model(tier: str) -> str:
    if tier not in ROUTER_TIERS:
        raise ValueError(f"unknown router tier {tier!r}")
    return f"amara-{tier}"


ROUTER_HOSTS = {"host.docker.internal", "localhost", "127.0.0.1", "::1"}


def check_router_url(url: str) -> str:
    """The router URL must be a plain http(s) URL to a local host with an explicit port that is
    not Ollama's. Parsed, not pattern-matched, so a zero-padded port, ?x or #a can't slip by,
    and no new outbound host can be configured (hard rule 3)."""
    url = url.strip().rstrip("/")
    u = urllib.parse.urlsplit(url)
    if u.scheme not in ("http", "https"):
        raise ValueError("CREW_ROUTER_URL must be http(s)")
    if u.username or u.password or u.query or u.fragment or "\\" in url:
        raise ValueError("CREW_ROUTER_URL must have no userinfo, query or fragment")
    if (u.hostname or "").lower() not in ROUTER_HOSTS:
        raise ValueError("CREW_ROUTER_URL must point at a local host")
    try:
        port = u.port
    except ValueError:
        raise ValueError("CREW_ROUTER_URL has a bad port") from None
    if port is None:
        raise ValueError("CREW_ROUTER_URL needs an explicit port")
    if port == OLLAMA_PORT:
        raise ValueError("CREW_ROUTER_URL must be the Amara Router, not Ollama")
    return url


def router_llm_kwargs(tier: str, url: str = "", key: str = "") -> dict:
    """LLM() kwargs for one tier against the router. The tier is pinned twice: by model alias and
    by X-Amara-Route. No keep_alive / num_ctx: the router manages residency. Refuses Ollama."""
    url = check_router_url(url or CREW_ROUTER_URL)
    key = key or CREW_ROUTER_KEY
    return dict(
        model=router_model(tier), custom_openai=True, base_url=url,
        api_key=key or "router-no-key",  # the SDK needs a non-empty value; loopback ignores it
        default_headers={"X-Amara-Route": tier},
    )


def planned_llm_label(agent_keys: List[str]) -> str:
    """Router model names for a run's agents, e.g. 'amara-deep,amara-fast'."""
    return ",".join(sorted({router_model(AGENT_CATALOG[k]["tier"]) for k in agent_keys}))


def _build_llms(agent_keys: List[str]):
    """Attempts, in order: [(label, {agent_key: LLM})]. The Amara Router only: there is no cloud
    path in the crew service."""
    from crewai import LLM

    return [(planned_llm_label(agent_keys), {
        k: LLM(**router_llm_kwargs(AGENT_CATALOG[k]["tier"])) for k in agent_keys})]


def step_label(kind: str, agent: str, tool: Optional[str] = None) -> str:
    """Short fixed-shape labels, built from role ids and tool names only: never prompt text,
    completions or tool output."""
    if kind == "tool":
        return f"tool: {tool}"
    return f"{agent}: task" if kind == "task" else f"llm: {agent}"


class RunRecorder:
    """Collects steps, handoffs and token totals inside the crew child and sends them to the
    parent as dict messages over the pipe. Pure (no crewai import), thread-safe, and tolerant of
    crewai's async event handlers delivering an end before its start: a step is emitted once both
    ends are known. Times are epoch seconds; messages carry ms since the recorder was created."""

    def __init__(self, send: Callable[[dict], None], agent_keys: List[str], now=time.time):
        self._send, self._order, self._now = send, list(agent_keys), now
        self._t0 = now()
        self._lock = threading.Lock()
        self._open: Dict[str, dict] = {}
        self._started: set = set()
        self._handed: set = set()
        self.prompt_tokens = self.completion_tokens = self.requests = self.usage_missing = 0

    def _ms(self, t: Optional[float]) -> int:
        return max(0, int(((self._now() if t is None else t) - self._t0) * 1000))

    def _emit_step(self, e: dict) -> None:
        self._send({"k": "step", "agent": e["agent"], "kind": e["kind"], "label": e["label"],
                    "start_ms": self._ms(e["start"]), "end_ms": self._ms(e["end"]),
                    "status": e["status"], "error": e.get("error")})

    def mark(self, key: str, kind: str, agent: str, label: str, *, start: Optional[float] = None,
             end: Optional[float] = None, status: Optional[str] = None,
             error: Optional[str] = None) -> None:
        with self._lock:
            e = self._open.setdefault(key, dict(kind=kind, agent=agent, label=label))
            if start is not None:
                e["start"] = start
            if end is not None:
                e.update(end=end, status=status or "ok", error=error)
            if "start" in e and "end" in e:
                del self._open[key]
                self._emit_step(e)

    def step(self, kind: str, agent: str, label: str, start: float, end: float, status: str,
             error: Optional[str] = None) -> None:
        with self._lock:
            self._emit_step(dict(kind=kind, agent=agent, label=label, start=start, end=end,
                                 status=status, error=error))

    def task_started(self, agent: str, t: Optional[float] = None) -> None:
        """The planned order is fixed, so task N starting means task N-1's output feeds it."""
        with self._lock:
            self._started.add(agent)
            i = self._order.index(agent) if agent in self._order else 0
            pair = (self._order[i - 1], agent) if i > 0 else None
            if pair and pair not in self._handed:
                self._handed.add(pair)
                self._send({"k": "handoff", "from": pair[0], "to": pair[1], "status": "ok"})

    def llm_done(self, usage: Optional[dict]) -> None:
        with self._lock:
            self.requests += 1
            if isinstance(usage, dict) and usage:
                self.prompt_tokens += int(usage.get("prompt_tokens", usage.get("input_tokens", 0)) or 0)
                self.completion_tokens += int(usage.get("completion_tokens", usage.get("output_tokens", 0)) or 0)
            else:
                self.usage_missing += 1  # reported, never estimated
            self._send({"k": "usage", "tokens": self.prompt_tokens + self.completion_tokens,
                        "requests": self.requests, "usage_missing": self.usage_missing})

    def close(self, failed: bool) -> None:
        """Flush steps whose end never arrived and, on failure, mark tasks that never started."""
        with self._lock:
            now = self._now()
            for e in self._open.values():
                if "start" in e:
                    e.update(end=now, status="failed" if failed else "skipped")
                    self._emit_step(e)
            self._open.clear()
            if failed:
                for agent in self._order:
                    if agent not in self._started:
                        self._emit_step(dict(kind="task", agent=agent, label=step_label("task", agent),
                                             start=now, end=now, status="skipped"))
                        self._started.add(agent)


def fixed_failure(text: str, exc_type=None) -> str:
    """Fixed text plus an exception type name only. The message body is dropped: it can echo
    prompt or completion fragments or short secrets."""
    name = exc_type if isinstance(exc_type, str) else getattr(exc_type, "__name__", None)
    name = name if isinstance(name, str) and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.]{0,63}", name) else None
    return f"{text} ({name})" if name else text


def _attach_listeners(rec: RunRecorder, role_to_key: Dict[str, str]):
    """Wire crewai's event bus to the recorder (task and LLM events; tool steps are timed in
    _to_crewai_tool). Returns a detach function. Handlers only read ids, roles and timings, never
    message or response text."""
    from crewai.events import crewai_event_bus as bus
    from crewai.events.types.llm_events import LLMCallCompletedEvent, LLMCallFailedEvent, LLMCallStartedEvent
    from crewai.events.types.task_events import TaskCompletedEvent, TaskFailedEvent, TaskStartedEvent

    def agent_of(event) -> str:
        for obj in (getattr(event, "from_agent", None), getattr(getattr(event, "task", None), "agent", None),
                    getattr(getattr(event, "from_task", None), "agent", None)):
            key = role_to_key.get(getattr(obj, "role", None) or "")
            if key:
                return key
        return role_to_key.get(getattr(event, "agent_role", None) or "", "crew")

    def task_key(event) -> str:
        return "task:" + str(getattr(getattr(event, "task", None), "id", None) or id(event))

    def on_task_start(_src, ev):
        a = agent_of(ev)
        rec.task_started(a)
        rec.mark(task_key(ev), "task", a, step_label("task", a), start=ev.timestamp.timestamp())

    def on_task_done(_src, ev):
        a = agent_of(ev)
        rec.mark(task_key(ev), "task", a, step_label("task", a), end=ev.timestamp.timestamp(), status="ok")

    def on_task_failed(_src, ev):
        a = agent_of(ev)
        rec.mark(task_key(ev), "task", a, step_label("task", a), end=ev.timestamp.timestamp(),
                 status="failed", error=fixed_failure("Task failed", getattr(ev, "error_type", None)))

    def on_llm_start(_src, ev):
        a = agent_of(ev)
        rec.mark("llm:" + ev.call_id, "llm", a, step_label("llm", a), start=ev.timestamp.timestamp())

    def on_llm_done(_src, ev):
        a = agent_of(ev)
        rec.mark("llm:" + ev.call_id, "llm", a, step_label("llm", a), end=ev.timestamp.timestamp(), status="ok")
        rec.llm_done(getattr(ev, "usage", None))

    def on_llm_failed(_src, ev):
        a = agent_of(ev)
        rec.mark("llm:" + ev.call_id, "llm", a, step_label("llm", a), end=ev.timestamp.timestamp(),
                 status="failed", error=fixed_failure("LLM call failed"))

    pairs = [(TaskStartedEvent, on_task_start), (TaskCompletedEvent, on_task_done),
             (TaskFailedEvent, on_task_failed), (LLMCallStartedEvent, on_llm_start),
             (LLMCallCompletedEvent, on_llm_done), (LLMCallFailedEvent, on_llm_failed)]
    for ev_type, h in pairs:
        bus.on(ev_type)(h)

    def detach() -> None:
        bus.flush(timeout=10)  # let in-flight handlers land before the final flush
        for ev_type, h in pairs:
            bus.off(ev_type, h)

    return detach


def _run_crew_child(agent_keys: List[str], task_description: str, conn, resp_conn=None) -> None:
    """Runs in a separate process. Streams step/handoff/usage dicts, then one final dict."""
    # Own process group, so stop/timeout can kill the crew and anything it spawned.
    os.setsid()
    send_lock = threading.Lock()

    def send(msg: dict) -> None:
        with send_lock:
            conn.send(msg)

    rec = RunRecorder(send, agent_keys)
    run_tools = [t for k in agent_keys for t in tools_for(k)]
    approver = (ChildApprover(send, resp_conn, candidates=write_candidates(run_tools))
                if resp_conn is not None else None)
    role_to_key = {AGENT_CATALOG[k]["role"]: k for k in AGENT_CATALOG}
    detach = None
    final = None
    try:
        from crewai import Agent, Crew, Process, Task

        detach = _attach_listeners(rec, role_to_key)
        waited = wait_for_deep_idle()  # a deep chat turn goes first
        if waited >= 1:
            log.info("crew run waited %.0fs for the deep model to go idle", waited)
        last_err = "no LLM configured"
        for label, llms in _build_llms(agent_keys):
            def attempt(label=label, llms=llms):
                agents = [
                    Agent(role=AGENT_CATALOG[k]["role"], goal=AGENT_CATALOG[k]["goal"],
                          backstory=AGENT_CATALOG[k]["backstory"],
                          tools=[_to_crewai_tool(t, rec, k, approver) for t in tools_for(k)],
                          llm=llms[k], allow_delegation=False, verbose=False)
                    for k in agent_keys
                ]
                tasks = [Task(description=task_description, agent=agents[0],
                              expected_output="A detailed summary of the findings.")]
                for a in agents[1:]:
                    tasks.append(Task(
                        description=f"Build on the previous output for: {task_description}",
                        agent=a, expected_output="An improved, concise result.",
                        context=[tasks[-1]],
                    ))
                return Crew(agents=agents, tasks=tasks, process=Process.sequential,
                            verbose=False).kickoff()

            ok, res = run_with_retries(attempt, approver)
            if ok:
                final = ("completed", str(res), label, res)
                break
            last_err = f"{label}: {res}"
        if final is None:
            final = ("failed", last_err, None, None)
    except Exception as e:
        final = ("failed", sanitize_error(e), None, None)
    try:
        if detach:
            detach()
        status, result, label, out = final
        rec.close(failed=status != "completed")
        tokens, requests = rec.prompt_tokens + rec.completion_tokens, rec.requests
        usage = getattr(out, "token_usage", None)  # crewai's own total over the same responses
        if usage is not None and (rec.usage_missing or tokens == 0):
            tokens = max(tokens, int(getattr(usage, "total_tokens", 0) or 0))
            requests = max(requests, int(getattr(usage, "successful_requests", 0) or 0))
        if rec.usage_missing:
            log.warning("%d LLM call(s) returned no usage; token total may be low", rec.usage_missing)
        send({"k": "final", "status": status, "result": result, "llm": label,
              "tokens": tokens, "requests": requests, "usage_missing": rec.usage_missing})
    except Exception as e:
        try:
            conn.send({"k": "final", "status": "failed", "result": sanitize_error(e), "llm": None})
        except Exception:
            pass
    finally:
        conn.close()


_ctx = mp.get_context("spawn")
_procs: Dict[str, mp.Process] = {}
_resp_pipes: Dict[str, object] = {}  # parent -> child decision pipes, by run id
_lock = threading.Lock()


def kill_group(proc, grace_s: float = 5.0) -> None:
    """SIGTERM the run's whole process group, then SIGKILL whatever is left."""
    if proc.pid is None:
        return
    try:
        pgid = os.getpgid(proc.pid)
    except ProcessLookupError:
        return
    if pgid == os.getpgid(0):  # setsid not applied yet: never signal our own group
        proc.kill()
        # setsid() may have landed after the check; the group id is then the child's pid
        # (still unreaped here, so it can't have been reused).
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
        proc.join(grace_s)
        return
    for sig, wait in ((signal.SIGTERM, grace_s), (signal.SIGKILL, 2.0)):
        try:
            os.killpg(pgid, sig)
        except ProcessLookupError:
            break
        proc.join(wait)
        try:
            os.killpg(pgid, 0)
        except ProcessLookupError:
            break  # group fully gone


def _watch(crew_id: str, proc, parent_conn, resp_conn=None) -> None:
    """Drain the child's pipe while it runs (steps are written as they arrive), then settle the
    run. Totals are written on every path, so stopped and timed-out runs keep what they used."""
    deadline_box = [time.monotonic() + RUN_TIMEOUT_S]
    final: Optional[dict] = None
    totals = {"tokens": 0, "requests": 0, "missing": 0}
    timed_out = False

    def handle(msg) -> None:
        nonlocal final
        if not isinstance(msg, dict):
            return
        try:
            k = msg.get("k")
            if k == "step":
                db_add_step(crew_id, msg)
            elif k == "handoff":
                db_add_handoff(crew_id, msg)
            elif k == "approval":
                db_create_approval(crew_id, msg)
                # The human may take a while; the wait must not eat the run's own budget.
                deadline_box[0] += APPROVAL_TIMEOUT_S
            elif k in ("usage", "final"):
                totals["tokens"] = int(msg.get("tokens", totals["tokens"]) or 0)
                totals["requests"] = int(msg.get("requests", totals["requests"]) or 0)
                totals["missing"] = int(msg.get("usage_missing", totals["missing"]) or 0)
                if k == "final":
                    final = msg
        except Exception as e:  # a DB hiccup must not stop the run's settlement
            log.warning("run %s: dropped a %s message: %s", crew_id, msg.get("k"), sanitize_error(e))

    while final is None:
        remain = deadline_box[0] - time.monotonic()
        if remain <= 0:
            timed_out = True
            break
        try:
            if parent_conn.poll(min(remain, 0.5)):
                handle(parent_conn.recv())
        except (EOFError, OSError):
            break  # the child closed its end: it exited
    if final is None and not timed_out:
        try:
            while parent_conn.poll(0):
                handle(parent_conn.recv())
        except (EOFError, OSError):
            pass

    if final is not None:
        proc.join(5)
        db_finish(crew_id, final.get("status", "failed"), final.get("result"), final.get("llm"))
    elif timed_out and proc.is_alive():
        db_finish(crew_id, "failed", f"TimeoutError: run exceeded {RUN_TIMEOUT_S}s")
        kill_group(proc)
    else:
        proc.join(5)
        # Exited without a result: killed by /crews/stop or crashed.
        db_finish(crew_id, "failed", f"process exited with code {proc.exitcode}")
    try:
        # No usage ever reported (or some calls lacked it): leave NULL so the UI stays unwired.
        if totals["requests"] > 0 and totals["missing"] == 0:
            db_set_totals(crew_id, totals["tokens"], totals["requests"])
    except Exception as e:
        log.warning("run %s: could not store totals: %s", crew_id, sanitize_error(e))
    try:
        db_expire_approvals(crew_id)
    except Exception as e:
        log.warning("run %s: could not expire approvals: %s", crew_id, sanitize_error(e))
    with _lock:
        _procs.pop(crew_id, None)
        pipe = _resp_pipes.pop(crew_id, None)
    for c in (pipe, resp_conn):
        try:
            if c is not None:
                c.close()
        except OSError:
            pass


def deliver_decision(crew_id: str, approve: bool) -> None:
    """Act on a recorded decision. Approve resumes the blocked child (run back to running); deny
    cancels the run and kills the child, so nothing after the denied call can start."""
    with _lock:
        proc, pipe = _procs.get(crew_id), _resp_pipes.get(crew_id)
    if approve:
        db_resume_run(crew_id)
        try:
            if pipe is not None:
                pipe.send({"k": "decision", "approve": True})
        except (OSError, ValueError):
            log.warning("run %s: could not deliver the approval to the child", crew_id)
    else:
        db_finish(crew_id, "cancelled", "approval denied")
        if proc is not None and proc.is_alive():
            kill_group(proc)


# ── App ─────────────────────────────────────────────────────────────────────
@asynccontextmanager
async def lifespan(_app: FastAPI):
    if not CREW_SERVICE_TOKEN:
        raise RuntimeError("CREW_SERVICE_TOKEN is not set; refusing to start")
    with db() as c:
        c.execute(SCHEMA)
        n = c.execute(
            "UPDATE crew_runs SET status='interrupted', finished_at=now() "
            "WHERE status IN ('running','awaiting_approval')"
        ).rowcount
        c.execute("UPDATE crew_approvals SET decided_at=now(), approved=false, decided_by='system' "
                  "WHERE decided_at IS NULL")
    if n:
        log.info("marked %d orphaned runs as interrupted", n)
    check_router_url(CREW_ROUTER_URL)  # fail at startup, not on the first run
    log.info("crew LLM: Amara Router %s (no cloud path); max %d concurrent", CREW_ROUTER_URL, MAX_CONCURRENT)
    log.info("crew tools allowed: %s", ", ".join(sorted(ALLOWED_TOOLS)) or "none")
    yield
    with _lock:
        for p in _procs.values():
            kill_group(p)


app = FastAPI(title="Terrance Crew Service", lifespan=lifespan)


def require_token(x_crew_token: str = Header(default="")) -> None:
    if not x_crew_token or not hmac.compare_digest(
            x_crew_token.encode("utf-8"), CREW_SERVICE_TOKEN.encode("utf-8")):
        raise HTTPException(status_code=401, detail="invalid or missing X-Crew-Token")


@app.get("/health")
async def health():
    return {"status": "ok"}


@app.get("/crews/list", response_model=List[CrewInfo], dependencies=[Depends(require_token)])
def list_crews(limit: int = Query(default=50, ge=1, le=500)):
    with db() as c:
        rows = c.execute(_SELECT + " ORDER BY started_at DESC LIMIT %s", (limit,)).fetchall()
    return [_row_to_info(r) for r in rows]


def _launch(name: str, task: str, agents: List[str], retry_of: Optional[str] = None) -> CrewInfo:
    with _lock:
        if sum(p.is_alive() for p in _procs.values()) >= MAX_CONCURRENT:
            raise HTTPException(status_code=429, detail=f"max {MAX_CONCURRENT} concurrent crews")
        crew_id = str(uuid.uuid4())
        with db() as c:
            c.execute(
                "INSERT INTO crew_runs (id, name, status, agents, task, llm, retry_of) "
                "VALUES (%s,%s,'running',%s,%s,%s,%s)",
                (crew_id, name, agents, task, planned_llm_label(agents), retry_of),
            )
        parent_conn, child_conn = _ctx.Pipe(duplex=False)
        resp_recv, resp_send = _ctx.Pipe(duplex=False)  # parent -> child: approval decisions
        proc = _ctx.Process(target=_run_crew_child, args=(agents, task, child_conn, resp_recv), daemon=True)
        proc.start()
        child_conn.close()
        resp_recv.close()
        _procs[crew_id] = proc
        _resp_pipes[crew_id] = resp_send
    threading.Thread(target=_watch, args=(crew_id, proc, parent_conn), daemon=True).start()
    return db_get(crew_id)


@app.post("/crews/start", response_model=CrewInfo, dependencies=[Depends(require_token)])
def start_crew(req: CrewStartRequest):
    if req.retry_of and not db_get(req.retry_of):
        raise HTTPException(status_code=404, detail="Crew execution not found")
    return _launch(req.name, req.task, req.agents or DEFAULT_AGENTS, req.retry_of)


@app.post("/crews/retry", response_model=CrewInfo, dependencies=[Depends(require_token)])
def retry_crew(req: RetryRequest):
    """Clone a finished run's name, task and agents into a new run linked by retry_of."""
    if not RUN_ID_RE.match(req.id):
        raise HTTPException(status_code=404, detail="Crew execution not found")
    old = db_get(req.id)
    if not old or not old.task:
        raise HTTPException(status_code=404, detail="Crew execution not found")
    if old.status in LIVE_STATUSES:
        raise HTTPException(status_code=409, detail="Crew execution is still active")
    try:
        again = CrewStartRequest(name=old.name, task=old.task, agents=old.agents, retry_of=old.id)
    except ValueError:
        raise HTTPException(status_code=422, detail="This run can no longer be retried") from None
    return _launch(again.name, again.task, again.agents or DEFAULT_AGENTS, old.id)


@app.get("/crews/status", response_model=CrewInfo, dependencies=[Depends(require_token)])
def get_status(id: str):
    info = db_get(id)
    if not info:
        raise HTTPException(status_code=404, detail="Crew execution not found")
    return info


RUN_ID_RE = re.compile(r"^[0-9a-fA-F-]{36}$")


@app.get("/crews/runs/{run_id}/steps", response_model=CrewRunSteps, response_model_by_alias=True,
         dependencies=[Depends(require_token)])
def get_run_steps(run_id: str):
    if not RUN_ID_RE.match(run_id):
        raise HTTPException(status_code=404, detail="Crew execution not found")
    if not db_get(run_id):
        raise HTTPException(status_code=404, detail="Crew execution not found")
    return db_run_steps(run_id)


@app.get("/crews/approvals", response_model=List[ApprovalOut], dependencies=[Depends(require_token)])
def list_approvals(state: str = Query(default="pending", pattern="^(pending|decided)$"),
                   limit: int = Query(default=50, ge=1, le=200)):
    return db_list_approvals(state, limit)


@app.post("/crews/approvals/{approval_id}/decide", response_model=ApprovalOut,
          dependencies=[Depends(require_token)])
def decide_approval(approval_id: str, req: DecideRequest):
    """The only way a request is approved. Idempotent by refusal: a second decision is 409."""
    if not APPROVAL_ID_RE.match(approval_id):
        raise HTTPException(status_code=404, detail="Approval not found")
    got = db_decide_approval(approval_id, req.approve)
    if got is None:
        if db_approval_exists(approval_id):
            raise HTTPException(status_code=409, detail="Already decided")
        raise HTTPException(status_code=404, detail="Approval not found")
    deliver_decision(got[0], req.approve)
    return db_get_approval(approval_id)


@app.post("/crews/stop", dependencies=[Depends(require_token)])
def stop_crew(req: CrewStopRequest):
    info = db_get(req.id)
    if not info:
        raise HTTPException(status_code=404, detail="Crew execution not found")
    if info.status not in LIVE_STATUSES:
        return {"id": req.id, "status": info.status, "stopped": False}
    with _lock:
        proc = _procs.get(req.id)
    # Mark stopped first so the watcher can't overwrite it with "failed".
    db_finish(req.id, "stopped", "stopped by user")
    db_expire_approvals(req.id, "system")
    if proc and proc.is_alive():
        kill_group(proc)  # also kills a child that is blocked waiting for an approval
    return {"id": req.id, "status": "stopped", "stopped": True}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host=_env("CREW_SERVICE_HOST", "127.0.0.1"),
                port=int(_env("CREW_SERVICE_PORT", "8400")))

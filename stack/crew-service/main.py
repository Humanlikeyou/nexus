"""Terrance crew-service — Stage 5E.

FastAPI wrapper around CrewAI. Contract matches src-tauri/src/commands/crew.rs:
  GET  /health                 (unauthenticated, liveness only)
  GET  /crews/list             -> [CrewInfo]
  POST /crews/start            {name, task, agents?} -> CrewInfo
  GET  /crews/status?id=       -> CrewInfo
  POST /crews/stop             {id} -> {"id", "status", "stopped": bool}

Every /crews/* call needs header X-Crew-Token == $CREW_SERVICE_TOKEN.
LLM: every model call goes through the Amara Router (OpenAI /v1, CREW_ROUTER_URL). Each agent
role has a tier (fast|deep) pinned by model alias (amara-fast / amara-deep) and an
X-Amara-Route header. Never Ollama directly. The MiniMax path (Anthropic-messages API) is
fail-closed: used only if CREW_ALLOW_CLOUD=1, NEXUS_OUTBOUND_DISABLED is unset and a key is set.
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
from pydantic import BaseModel, Field, field_validator

log = logging.getLogger("crew-service")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


# ── Config (env) ────────────────────────────────────────────────────────────
CREW_SERVICE_TOKEN = _env("CREW_SERVICE_TOKEN")
MAX_TASK_CHARS = int(_env("CREW_MAX_TASK_CHARS", "4000"))
MAX_CONCURRENT = int(_env("CREW_MAX_CONCURRENT", "2"))
RUN_TIMEOUT_S = int(_env("CREW_RUN_TIMEOUT_SECONDS", "900"))

MINIMAX_API_KEY = _env("MINIMAX_API_KEY")
MINIMAX_BASE_URL = _env("MINIMAX_BASE_URL", "https://api.minimax.io/anthropic")
MINIMAX_MODEL = _env("MINIMAX_MODEL", "MiniMax-M2.5")
# OLLAMA_BASE_URL / CREW_OLLAMA_MODEL are no longer read here: the router owns model residency.
CREW_ROUTER_URL = _env("CREW_ROUTER_URL", "http://host.docker.internal:8787/v1")
CREW_ROUTER_KEY = _env("CREW_ROUTER_KEY")  # optional bearer; the loopback router needs none today
CREW_ALLOW_CLOUD = _env("CREW_ALLOW_CLOUD")
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


def _to_crewai_tool(tool: CrewTool):
    """Wrap a CrewTool as a CrewAI tool (imported lazily: only the crew child needs crewai)."""
    from crewai.tools import BaseTool

    class _Tool(BaseTool):
        name: str = tool.name
        description: str = tool.description

        def _run(self, query: str) -> str:
            return run_tool(tool, query)

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


class CrewStartRequest(BaseModel):
    name: str = Field(min_length=1, max_length=64)
    task: str = Field(min_length=1)
    agents: Optional[List[str]] = None

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
"""


def db():
    return psycopg.connect(**PG, autocommit=True)


def _row_to_info(r) -> CrewInfo:
    return CrewInfo(
        id=r[0], name=r[1], status=r[2], agents=list(r[3]),
        started_at=r[4].strftime("%Y-%m-%dT%H:%M:%SZ") if r[4] else None,
        task=r[5], result=safe_result(r[2], r[6]),
        llm=r[7] if len(r) > 7 else None,
    )


_SELECT = "SELECT id, name, status, agents, started_at AT TIME ZONE 'UTC', task, result, llm FROM crew_runs"


def db_get(crew_id: str) -> Optional[CrewInfo]:
    with db() as c:
        r = c.execute(_SELECT + " WHERE id = %s", (crew_id,)).fetchone()
    return _row_to_info(r) if r else None


def db_finish(crew_id: str, status: str, result: Optional[str], llm: Optional[str] = None) -> None:
    # Never overwrite a terminal state (e.g. a stop that already landed).
    with db() as c:
        c.execute(
            "UPDATE crew_runs SET status=%s, result=%s, llm=COALESCE(%s, llm), finished_at=now() "
            "WHERE id=%s AND status='running'",
            (status, safe_result(status, result), llm, crew_id),
        )


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


def cloud_allowed(allow: Optional[str] = None, outbound_disabled: Optional[str] = None,
                  key: Optional[str] = None) -> bool:
    """Fail-closed MiniMax gate: CREW_ALLOW_CLOUD=1 AND NEXUS_OUTBOUND_DISABLED unset AND a key."""
    allow = CREW_ALLOW_CLOUD if allow is None else allow
    off = NEXUS_OUTBOUND_DISABLED if outbound_disabled is None else outbound_disabled
    key = MINIMAX_API_KEY if key is None else key
    return allow.strip() == "1" and not off.strip() and bool(key.strip())


def planned_llm_label(agent_keys: List[str]) -> str:
    """Router model names for a run's agents, e.g. 'amara-deep,amara-fast'."""
    return ",".join(sorted({router_model(AGENT_CATALOG[k]["tier"]) for k in agent_keys}))


def _build_llms(agent_keys: List[str]):
    """Attempts, in order: [(label, {agent_key: LLM})]. The router first; MiniMax only if gated."""
    from crewai import LLM

    attempts = [(planned_llm_label(agent_keys), {
        k: LLM(**router_llm_kwargs(AGENT_CATALOG[k]["tier"])) for k in agent_keys})]
    if cloud_allowed():
        cloud = LLM(model=MINIMAX_MODEL, provider="anthropic",
                    base_url=MINIMAX_BASE_URL, api_key=MINIMAX_API_KEY)
        attempts.append(("minimax/" + MINIMAX_MODEL, {k: cloud for k in agent_keys}))
    return attempts


def _run_crew_child(agent_keys: List[str], task_description: str, conn) -> None:
    """Runs in a separate process. Sends (status, result, llm_label) back."""
    # Own process group, so stop/timeout can kill the crew and anything it spawned.
    os.setsid()
    try:
        from crewai import Agent, Crew, Process, Task

        last_err = "no LLM configured"
        for label, llms in _build_llms(agent_keys):
            try:
                agents = [
                    Agent(role=AGENT_CATALOG[k]["role"], goal=AGENT_CATALOG[k]["goal"],
                          backstory=AGENT_CATALOG[k]["backstory"],
                          tools=[_to_crewai_tool(t) for t in tools_for(k)],
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
                out = Crew(agents=agents, tasks=tasks, process=Process.sequential,
                           verbose=False).kickoff()
                conn.send(("completed", str(out), label))
                return
            except Exception as e:  # try next LLM
                last_err = f"{label}: {sanitize_error(e)}"
        conn.send(("failed", last_err, None))
    except Exception as e:
        conn.send(("failed", sanitize_error(e), None))
    finally:
        conn.close()


_ctx = mp.get_context("spawn")
_procs: Dict[str, mp.Process] = {}
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


def _watch(crew_id: str, proc, parent_conn) -> None:
    proc.join(RUN_TIMEOUT_S)
    if proc.is_alive():
        db_finish(crew_id, "failed", f"TimeoutError: run exceeded {RUN_TIMEOUT_S}s")
        kill_group(proc)
    elif parent_conn.poll():
        status, result, llm = parent_conn.recv()
        db_finish(crew_id, status, result, llm)
    else:
        # Exited without a result: killed by /crews/stop or crashed.
        db_finish(crew_id, "failed", f"process exited with code {proc.exitcode}")
    with _lock:
        _procs.pop(crew_id, None)


# ── App ─────────────────────────────────────────────────────────────────────
@asynccontextmanager
async def lifespan(_app: FastAPI):
    if not CREW_SERVICE_TOKEN:
        raise RuntimeError("CREW_SERVICE_TOKEN is not set; refusing to start")
    with db() as c:
        c.execute(SCHEMA)
        n = c.execute(
            "UPDATE crew_runs SET status='interrupted', finished_at=now() WHERE status='running'"
        ).rowcount
    if n:
        log.info("marked %d orphaned runs as interrupted", n)
    check_router_url(CREW_ROUTER_URL)  # fail at startup, not on the first run
    log.info("crew LLM: Amara Router %s; cloud path %s", CREW_ROUTER_URL,
             "enabled" if cloud_allowed() else "off")
    log.info("crew tools allowed: %s", ", ".join(sorted(ALLOWED_TOOLS)) or "none")
    yield
    with _lock:
        for p in _procs.values():
            kill_group(p)


app = FastAPI(title="Terrance Crew Service", lifespan=lifespan)


def require_token(x_crew_token: str = Header(default="")) -> None:
    if not x_crew_token or not hmac.compare_digest(x_crew_token, CREW_SERVICE_TOKEN):
        raise HTTPException(status_code=401, detail="invalid or missing X-Crew-Token")


@app.get("/health")
async def health():
    return {"status": "ok"}


@app.get("/crews/list", response_model=List[CrewInfo], dependencies=[Depends(require_token)])
def list_crews(limit: int = Query(default=50, ge=1, le=500)):
    with db() as c:
        rows = c.execute(_SELECT + " ORDER BY started_at DESC LIMIT %s", (limit,)).fetchall()
    return [_row_to_info(r) for r in rows]


@app.post("/crews/start", response_model=CrewInfo, dependencies=[Depends(require_token)])
def start_crew(req: CrewStartRequest):
    agents = req.agents or DEFAULT_AGENTS
    with _lock:
        if sum(p.is_alive() for p in _procs.values()) >= MAX_CONCURRENT:
            raise HTTPException(status_code=429, detail=f"max {MAX_CONCURRENT} concurrent crews")
        crew_id = str(uuid.uuid4())
        with db() as c:
            c.execute(
                "INSERT INTO crew_runs (id, name, status, agents, task, llm) "
                "VALUES (%s,%s,'running',%s,%s,%s)",
                (crew_id, req.name, agents, req.task, planned_llm_label(agents)),
            )
        parent_conn, child_conn = _ctx.Pipe(duplex=False)
        proc = _ctx.Process(target=_run_crew_child, args=(agents, req.task, child_conn), daemon=True)
        proc.start()
        child_conn.close()
        _procs[crew_id] = proc
    threading.Thread(target=_watch, args=(crew_id, proc, parent_conn), daemon=True).start()
    return db_get(crew_id)


@app.get("/crews/status", response_model=CrewInfo, dependencies=[Depends(require_token)])
def get_status(id: str):
    info = db_get(id)
    if not info:
        raise HTTPException(status_code=404, detail="Crew execution not found")
    return info


@app.post("/crews/stop", dependencies=[Depends(require_token)])
def stop_crew(req: CrewStopRequest):
    info = db_get(req.id)
    if not info:
        raise HTTPException(status_code=404, detail="Crew execution not found")
    if info.status != "running":
        return {"id": req.id, "status": info.status, "stopped": False}
    with _lock:
        proc = _procs.get(req.id)
    # Mark stopped first so the watcher can't overwrite it with "failed".
    db_finish(req.id, "stopped", "stopped by user")
    if proc and proc.is_alive():
        kill_group(proc)
    return {"id": req.id, "status": "stopped", "stopped": True}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host=_env("CREW_SERVICE_HOST", "127.0.0.1"),
                port=int(_env("CREW_SERVICE_PORT", "8400")))

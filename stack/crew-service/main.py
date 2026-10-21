"""Terrance crew-service — Stage 5E.

FastAPI wrapper around CrewAI. Contract matches src-tauri/src/commands/crew.rs:
  GET  /health                 (unauthenticated, liveness only)
  GET  /crews/list             -> [CrewInfo]
  POST /crews/start            {name, task, agents?} -> CrewInfo
  GET  /crews/status?id=       -> CrewInfo
  POST /crews/stop             {id} -> {"id", "status", "stopped": bool}

Every /crews/* call needs header X-Crew-Token == $CREW_SERVICE_TOKEN.
LLM: local Ollama gemma4 primary. MiniMax-M2.5 (Anthropic-messages API, same endpoint
OpenClaw uses) is an optional fallback, enabled only when MINIMAX_API_KEY is non-empty.
Runs persist to Postgres table crew_runs (created at startup).
Each crew runs in its own process so /crews/stop can really cancel it.
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
import uuid
from contextlib import asynccontextmanager
from typing import Dict, List, Optional

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
OLLAMA_BASE_URL = _env("OLLAMA_BASE_URL", "http://host.docker.internal:11434")
CREW_OLLAMA_MODEL = _env("CREW_OLLAMA_MODEL", "gemma4")

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

# Tools: none are enabled in Phase 1. Add names here AND wire them in _build_tools.
ALLOWED_TOOLS: Dict[str, str] = {}

AGENT_CATALOG: Dict[str, dict] = {
    "researcher": dict(
        role="Terrance Research Assistant",
        goal="Investigate the task thoroughly and report verified findings.",
        backstory="A precise, efficient researcher in the Terrance ecosystem.",
        tools=[],
    ),
    "analyst": dict(
        role="Terrance Analyst",
        goal="Turn findings into clear analysis, trade-offs and recommendations.",
        backstory="A careful analyst who separates evidence from opinion.",
        tools=[],
    ),
    "writer": dict(
        role="Terrance Writer",
        goal="Produce a concise, well-structured final write-up.",
        backstory="A clear writer who keeps output short and actionable.",
        tools=[],
    ),
}
DEFAULT_AGENTS = ["researcher"]
for _key, _spec in AGENT_CATALOG.items():
    for _tool in _spec["tools"]:
        assert _tool in ALLOWED_TOOLS, f"agent {_key} uses non-allowlisted tool {_tool}"


# ── Models ──────────────────────────────────────────────────────────────────
class CrewInfo(BaseModel):
    id: str
    name: str
    status: str
    agents: List[str]
    started_at: Optional[str] = None
    task: Optional[str] = None
    result: Optional[str] = None


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
        task=r[5], result=r[6],
    )


_SELECT = "SELECT id, name, status, agents, started_at AT TIME ZONE 'UTC', task, result FROM crew_runs"


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
            (status, result, llm, crew_id),
        )


# ── Error sanitizing ────────────────────────────────────────────────────────
_SECRETISH = re.compile(r"(?i)(bearer\s+\S+|(?:api[_-]?key|token|password|secret)\s*[=:]\s*\S+|[A-Za-z0-9_\-]{32,})")
_URL_CREDS = re.compile(r"(\w+://)[^/\s:@]+:[^/\s@]+@")
MAX_ERR_CHARS = 300


def sanitize_error(exc: BaseException) -> str:
    """Exception type + a short, redacted message. Never the raw upstream body."""
    msg = str(exc).splitlines()[0] if str(exc) else ""
    msg = _URL_CREDS.sub(r"\1***@", msg)
    msg = _SECRETISH.sub("[redacted]", msg)
    msg = "".join(ch for ch in msg if ch.isprintable())
    if len(msg) > MAX_ERR_CHARS:
        msg = msg[:MAX_ERR_CHARS] + "…"
    return f"{type(exc).__name__}: {msg}" if msg else type(exc).__name__


# ── Crew execution (child process) ──────────────────────────────────────────
def _build_llms():
    from crewai import LLM

    # Local-first: Ollama is primary. MiniMax is optional and only added when a key is set.
    llms = [("ollama/" + CREW_OLLAMA_MODEL, LLM(
        model=CREW_OLLAMA_MODEL, provider="ollama", base_url=OLLAMA_BASE_URL,
    ))]
    if MINIMAX_API_KEY:
        llms.append(("minimax/" + MINIMAX_MODEL, LLM(
            model=MINIMAX_MODEL, provider="anthropic",
            base_url=MINIMAX_BASE_URL, api_key=MINIMAX_API_KEY,
        )))
    return llms


def _run_crew_child(agent_keys: List[str], task_description: str, conn) -> None:
    """Runs in a separate process. Sends (status, result, llm_label) back."""
    # Own process group, so stop/timeout can kill the crew and anything it spawned.
    os.setsid()
    try:
        from crewai import Agent, Crew, Process, Task

        last_err = "no LLM configured"
        for label, llm in _build_llms():
            try:
                agents = [
                    Agent(role=AGENT_CATALOG[k]["role"], goal=AGENT_CATALOG[k]["goal"],
                          backstory=AGENT_CATALOG[k]["backstory"], tools=[],
                          llm=llm, allow_delegation=False, verbose=False)
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
    if not MINIMAX_API_KEY:
        log.info("MINIMAX_API_KEY not set; local Ollama only (%s)", CREW_OLLAMA_MODEL)
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
                "INSERT INTO crew_runs (id, name, status, agents, task) VALUES (%s,%s,'running',%s,%s)",
                (crew_id, req.name, agents, req.task),
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

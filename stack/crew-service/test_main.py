"""crew-service tests: error sanitizing, process-group stop and the tool registry.

Run inside the crew image (it has the deps):  python -m unittest -v test_main
"""

import io
import json
import os
import signal
import subprocess
import threading
import time
import unittest
import urllib.error
from unittest import mock

import main


def _alive(pid: int) -> bool:
    """True if pid exists and is not a zombie."""
    try:
        with open(f"/proc/{pid}/stat") as f:
            return f.read().rsplit(")", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


def _crew_like_child(conn, ignore_term: bool) -> None:
    """Mimics _run_crew_child: own session, then spawns workers that outlive a parent-only kill."""
    os.setsid()
    if ignore_term:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    workers = [
        subprocess.Popen(["sleep", "300"]),
        # A worker that ignores SIGTERM, so only the SIGKILL pass can stop it.
        subprocess.Popen(["sh", "-c", "trap '' TERM; while :; do sleep 1; done"]),
    ]
    conn.send([w.pid for w in workers])
    conn.close()
    time.sleep(300)


def _plain_child() -> None:
    time.sleep(300)


class SanitizeErrorTests(unittest.TestCase):
    def check_clean(self, out: str) -> None:
        self.assertNotIn("/", out.replace("://", ""), out)
        self.assertNotIn("\n", out)

    def test_filesystem_paths_are_redacted(self):
        out = main.sanitize_error(FileNotFoundError(2, "No such file or directory", "/app/data/secret/run.json"))
        self.assertNotIn("/app", out)
        self.assertIn("[path]", out)
        self.assertTrue(out.startswith("FileNotFoundError: "))

    def test_home_and_relative_paths_are_redacted(self):
        for p in ("~/projects/nexus/stack/.env", "./crew/run.log", "../../etc/passwd"):
            out = main.sanitize_error(RuntimeError(f"cannot open {p}"))
            self.assertNotIn(p, out)
            self.check_clean(out)

    def test_tokens_and_bearer_are_redacted(self):
        tok = "sk-" + "A1b2C3d4" * 6
        for text in (f"Authorization: Bearer {tok}", f"api_key={tok}", f"token: {tok}",
                     f"password=hunter2hunter2", f"upstream said {tok}"):
            out = main.sanitize_error(ValueError(text))
            self.assertNotIn(tok, out)
            self.assertNotIn("hunter2hunter2", out)
            self.assertIn("[redacted]", out)

    def test_url_credentials_are_stripped_but_host_kept(self):
        out = main.sanitize_error(ConnectionError("failed: postgresql://terrance:pw123@postgres:5432/db"))
        self.assertNotIn("pw123", out)
        self.assertNotIn("terrance:", out)
        out = main.sanitize_error(ConnectionError("refused: http://host.docker.internal:11434/api/chat"))
        self.assertIn("host.docker.internal:11434/api/chat", out)

    def test_stack_traces_are_dropped(self):
        tb = 'Traceback (most recent call last):\n  File "/app/main.py", line 3, in <module>\nKeyError: x'
        self.assertEqual(main.sanitize_error(RuntimeError(tb)), "RuntimeError")
        out = main.sanitize_error(RuntimeError('boom\n  File "/usr/lib/python3.12/x.py", line 9'))
        self.assertEqual(out, "RuntimeError: boom")
        out = main.sanitize_error(RuntimeError('File "/app/main.py", line 3, in run'))
        self.assertEqual(out, "RuntimeError")

    def test_length_cap_and_unprintables(self):
        out = main.sanitize_error(RuntimeError("x " * 1000))
        self.assertLessEqual(len(out), len("RuntimeError: ") + main.MAX_ERR_CHARS + 1)
        out = main.sanitize_error(RuntimeError("bad\x1b[31mcolor\x00"))
        self.assertNotIn("\x1b", out)
        self.assertNotIn("\x00", out)

    def test_llm_label_prefix_survives(self):
        # _run_crew_child prefixes errors with the LLM label, e.g. "ollama/gemma4: ...".
        self.assertEqual(main.sanitize_error(RuntimeError("ollama/gemma4 timed out")),
                         "RuntimeError: ollama/gemma4 timed out")


class SafeResultTests(unittest.TestCase):
    """Error results are sanitized on write (db_finish) and on read (_row_to_info)."""

    RAW = ('Traceback (most recent call last):\n  File "/app/main.py", line 9\n'
           'ConnectionError: http://user:hunter2@db:5432 at /Users/someone/secret/key.pem')

    def test_failed_and_interrupted_results_are_sanitized(self):
        for status in ("failed", "interrupted"):
            out = main.safe_result(status, "OSError: open /home/app/.config/x.json failed\nmore detail")
            self.assertNotIn("/home/app", out)
            self.assertNotIn("more detail", out)
            self.assertIn("[path]", out)

    def test_legacy_raw_traceback_row_never_leaks(self):
        out = main.safe_result("failed", self.RAW)
        self.assertNotIn("/app/main.py", out)
        self.assertNotIn("hunter2", out)
        self.assertNotIn("/Users/someone", out)
        self.assertEqual(out, "error")  # first line is the traceback header, so nothing survives

    def test_row_to_info_sanitizes_stored_error(self):
        info = main._row_to_info(("id1", "n", "failed", ["a"], None, "t", "boom at /var/lib/crew/run.log\nsecond"))
        self.assertEqual(info.result, "boom at [path]")

    def test_completed_and_missing_results_untouched(self):
        self.assertEqual(main.safe_result("completed", "see /docs/a/b\nline2"), "see /docs/a/b\nline2")
        self.assertIsNone(main.safe_result("failed", None))

    def test_db_finish_stores_sanitized_error(self):
        captured = []

        class Conn:
            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

            def execute(self, _sql, params):
                captured.append(params)

        orig = main.db
        main.db = Conn
        try:
            main.db_finish("id1", "failed", "ValueError: /srv/app/secret.txt\ntrace")
        finally:
            main.db = orig
        self.assertEqual(captured[0][1], "ValueError: [path]")


class KillGroupTests(unittest.TestCase):
    def _start(self, ignore_term: bool):
        parent, child = main._ctx.Pipe(duplex=False)
        proc = main._ctx.Process(target=_crew_like_child, args=(child, ignore_term), daemon=True)
        proc.start()
        child.close()
        self.assertTrue(parent.poll(30), "child never reported its workers")
        workers = parent.recv()
        self.assertTrue(all(_alive(p) for p in workers))
        self.assertNotEqual(os.getpgid(proc.pid), os.getpgid(0))
        return proc, workers

    def _assert_all_gone(self, proc, workers):
        deadline = time.time() + 5
        while time.time() < deadline and any(_alive(p) for p in [proc.pid, *workers]):
            time.sleep(0.1)
        self.assertFalse(proc.is_alive())
        self.assertEqual([p for p in workers if _alive(p)], [], "workers survived the stop")

    def test_stop_kills_whole_group(self):
        proc, workers = self._start(ignore_term=False)
        main.kill_group(proc, grace_s=2.0)
        self._assert_all_gone(proc, workers)

    def test_sigterm_ignoring_group_gets_sigkill(self):
        proc, workers = self._start(ignore_term=True)
        t0 = time.time()
        main.kill_group(proc, grace_s=1.0)
        self._assert_all_gone(proc, workers)
        self.assertLess(time.time() - t0, 10)

    def test_child_in_our_group_is_killed_without_signalling_us(self):
        proc = main._ctx.Process(target=_plain_child, daemon=True)
        proc.start()
        main.kill_group(proc, grace_s=2.0)  # would kill this test process if it signalled our group
        self.assertFalse(proc.is_alive())

    def test_already_exited_process_is_a_noop(self):
        proc = main._ctx.Process(target=_plain_child, daemon=True)
        proc.start()
        proc.kill()
        proc.join(5)
        main.kill_group(proc, grace_s=0.5)
        self.assertFalse(proc.is_alive())


def _tool(name: str, fn=lambda q: f"echo:{q}", **kw) -> main.CrewTool:
    return main.CrewTool(name=name, description=f"{name} test tool", fn=fn, **kw)


class ToolRegistryTests(unittest.TestCase):
    """The allowlist and registry are passed in; no env vars are set."""

    def setUp(self):
        self.reg = {}
        for n in ("echo", "other"):
            main.register_tool(_tool(n), self.reg)
        agents = dict(main.AGENT_CATALOG)
        agents["tester"] = dict(role="r", goal="g", backstory="b", tools=["echo", "other"])
        patcher = mock.patch.object(main, "AGENT_CATALOG", agents)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_default_allowlist_is_empty(self):
        self.assertEqual(main.parse_allowed_tools("", self.reg), frozenset())
        self.assertEqual(main.parse_allowed_tools(" , ,", self.reg), frozenset())
        self.assertEqual(main.ALLOWED_TOOLS, frozenset())  # this image has no CREW_ALLOWED_TOOLS
        self.assertEqual(main.TOOL_REGISTRY, {})

    def test_empty_allowlist_gives_agents_no_tools(self):
        self.assertEqual(main.tools_for("tester", frozenset(), self.reg), [])
        for key in ("researcher", "analyst", "writer"):
            self.assertEqual(main.tools_for(key), [])

    def test_only_allowlisted_tools_reach_the_agent(self):
        allowed = main.parse_allowed_tools("echo", self.reg)
        self.assertEqual([t.name for t in main.tools_for("tester", allowed, self.reg)], ["echo"])
        allowed = main.parse_allowed_tools("echo, other", self.reg)
        self.assertEqual([t.name for t in main.tools_for("tester", allowed, self.reg)], ["echo", "other"])

    def test_unknown_allowlist_names_are_ignored(self):
        with self.assertLogs("crew-service", "WARNING") as logs:
            allowed = main.parse_allowed_tools("echo,shell,http_get", self.reg)
        self.assertEqual(allowed, frozenset({"echo"}))
        self.assertIn("http_get", logs.output[0])

    def test_agent_declaring_an_unknown_tool_is_rejected(self):
        main.AGENT_CATALOG["bad"] = dict(role="r", goal="g", backstory="b", tools=["http_get"])
        with self.assertRaisesRegex(ValueError, "unknown tool http_get"):
            main.tools_for("bad", frozenset({"http_get"}), self.reg)

    def test_duplicate_registration_is_rejected(self):
        with self.assertRaises(ValueError):
            main.register_tool(_tool("echo"), self.reg)

    def test_run_tool_returns_output(self):
        self.assertEqual(main.run_tool(self.reg["echo"], "hi"), "echo:hi")

    def test_timeout_returns_a_tool_error_quickly(self):
        release = threading.Event()
        slow = _tool("slow", fn=lambda q: release.wait(10) and "late", timeout_s=0.2)
        t0 = time.time()
        out = main.run_tool(slow, "x")
        release.set()
        self.assertLess(time.time() - t0, 2)
        self.assertEqual(out, "Tool error: slow timed out after 0.2s")

    def test_default_timeout_and_cap(self):
        t = _tool("d")
        self.assertEqual((t.timeout_s, t.max_output_chars), (15.0, 4000))

    def test_output_is_capped(self):
        big = _tool("big", fn=lambda q: "x" * 10_000, max_output_chars=100)
        out = main.run_tool(big, "")
        self.assertEqual(out, "x" * 100 + "…[truncated]")

    def test_exceptions_become_sanitized_tool_errors(self):
        tok = "sk-" + "Z9y8X7w6" * 6

        def boom(_q):
            raise ConnectionError(f"failed at /srv/app/x.py with token={tok}\nmore")

        out = main.run_tool(_tool("boom", fn=boom), "q")
        self.assertTrue(out.startswith("Tool error: boom: ConnectionError: "), out)
        self.assertNotIn(tok, out)
        self.assertNotIn("/srv/app", out)
        self.assertNotIn("more", out)

    def test_crewai_wrapper_goes_through_run_tool(self):
        big = _tool("kb_query", fn=lambda q: q * 50, max_output_chars=10)  # read-only: no approval
        wrapped = main._to_crewai_tool(big)
        self.assertEqual(wrapped.name, "kb_query")
        self.assertEqual(wrapped._run("ab"), "ababababab…[truncated]")


class _FakeResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


class _FakeIngress:
    """Stands in for urllib.request.urlopen: records requests, returns a canned reply."""

    def __init__(self, reply=None, status=200, exc=None):
        self.reply, self.status, self.exc, self.requests = reply, status, exc, []

    def __call__(self, req, timeout=None):
        self.requests.append((req, timeout))
        if self.exc:
            raise self.exc
        if self.status != 200:
            raise urllib.error.HTTPError(req.full_url, self.status, "x", {}, io.BytesIO(b'{"leak":"body"}'))
        return _FakeResponse(json.dumps(self.reply).encode())


KEY = "crew-" + "Kk9Qz7Lp" * 5  # fake; built at runtime
URL = "http://n8n:5678/webhook/terrance/ingress-crew"


class IngressToolTests(unittest.TestCase):
    """kb_query / memory_query with a mocked HTTP layer (no network, no env vars)."""

    def tools(self, fake):
        reg = {}
        self.assertTrue(main.register_ingress_tools(reg, KEY, URL, opener=fake))
        return reg

    def test_missing_key_registers_neither_tool_and_logs_no_value(self):
        reg = {}
        with self.assertLogs("crew-service", "WARNING") as logs:
            self.assertFalse(main.register_ingress_tools(reg, "", URL, opener=_FakeIngress()))
        self.assertEqual(reg, {})
        self.assertIn("CREW_INGRESS_SECRET not set", logs.output[0])
        # Declared on the researcher but unregistered: silently skipped, not an error.
        self.assertEqual(main.tools_for("researcher", frozenset({"kb_query", "memory_query"}), reg), [])

    def test_registered_tools_have_15s_timeout_and_4000_char_cap(self):
        reg = self.tools(_FakeIngress({"ok": True, "result": {}}))
        self.assertEqual(sorted(reg), ["kb_query", "memory_query"])
        for t in reg.values():
            self.assertEqual((t.timeout_s, t.max_output_chars), (15.0, 4000))

    def test_allowlist_still_gates_the_ingress_tools(self):
        reg = self.tools(_FakeIngress({"ok": True, "result": {}}))
        self.assertEqual(main.tools_for("researcher", frozenset(), reg), [])
        got = main.tools_for("researcher", main.parse_allowed_tools("kb_query", reg), reg)
        self.assertEqual([t.name for t in got], ["kb_query"])
        self.assertEqual(main.tools_for("writer", frozenset(reg), reg), [])

    def test_kb_query_sends_the_crew_key_to_the_ingress_only(self):
        fake = _FakeIngress({"ok": True, "trace_id": "t", "result": {"documents": [{"title": "Nexus"}]}})
        out = main.run_tool(self.tools(fake)["kb_query"], "  what is nexus  ")
        self.assertEqual(json.loads(out), {"documents": [{"title": "Nexus"}]})
        (req, timeout), = fake.requests
        self.assertEqual(req.full_url, URL)
        self.assertEqual(req.get_method(), "POST")
        self.assertEqual(req.get_header("X-nexus-secret"), KEY)
        self.assertEqual(timeout, 15.0)
        sent = json.loads(req.data)
        self.assertEqual(sent["event"], {"type": "kb.query", "payload": {"query": "what is nexus"}})
        self.assertTrue(sent["trace_id"].startswith("crew-"))
        self.assertNotIn(KEY, req.data.decode())

    def test_memory_query_filters_by_label(self):
        blocks = [{"label": "human", "value": "h", "limit": 5}, {"label": "persona", "value": "p"}]
        fake = _FakeIngress({"ok": True, "result": {"blocks": blocks}})
        tool = self.tools(fake)["memory_query"]
        self.assertEqual(json.loads(main.run_tool(tool, "Persona")), [{"label": "persona", "value": "p"}])
        self.assertEqual(len(json.loads(main.run_tool(tool, ""))), 2)
        self.assertEqual(json.loads(fake.requests[0][0].data)["event"], {"type": "memory.query", "payload": {}})

    def test_output_is_capped_at_4000(self):
        fake = _FakeIngress({"ok": True, "result": {"text": "x" * 10_000}})
        out = main.run_tool(self.tools(fake)["kb_query"], "q")
        self.assertEqual(len(out), 4000 + len("…[truncated]"))

    def test_http_403_is_a_tool_error_without_the_body(self):
        out = main.run_tool(self.tools(_FakeIngress(status=403))["kb_query"], "q")
        self.assertEqual(out, "Tool error: kb_query: IngressError: kb.query: ingress returned HTTP 403")
        self.assertNotIn("leak", out)

    def test_ingress_error_payload_is_a_sanitized_tool_error(self):
        fake = _FakeIngress({"ok": False, "error": {"message": "knowledge base not running\nat /srv/x.py"}})
        out = main.run_tool(self.tools(fake)["kb_query"], "q")
        self.assertEqual(out, "Tool error: kb_query: IngressError: kb.query: knowledge base not running")

    def test_unreachable_and_bad_json_are_tool_errors(self):
        out = main.run_tool(self.tools(_FakeIngress(exc=urllib.error.URLError(ConnectionRefusedError())))["memory_query"], "")
        self.assertEqual(out, "Tool error: memory_query: IngressError: memory.query: ingress unreachable (ConnectionRefusedError)")
        out = main.run_tool(self.tools(lambda req, timeout=None: _FakeResponse(b"<html>"))["kb_query"], "q")
        self.assertEqual(out, "Tool error: kb_query: IngressError: kb.query: ingress returned invalid JSON")

    def test_empty_query_never_calls_the_ingress(self):
        fake = _FakeIngress({"ok": True, "result": {}})
        out = main.run_tool(self.tools(fake)["kb_query"], "   ")
        self.assertEqual(out, "Tool error: kb_query: IngressError: kb.query: empty query")
        self.assertEqual(fake.requests, [])

    def test_slow_ingress_times_out_as_a_tool_error(self):
        release = threading.Event()

        def slow(req, timeout=None):
            release.wait(10)
            return _FakeResponse(b'{"ok": true, "result": {}}')

        reg = {}
        main.register_ingress_tools(reg, KEY, URL, opener=slow)
        tool = reg["kb_query"]
        fast = main.CrewTool(tool.name, tool.description, tool.fn, timeout_s=0.2)
        t0 = time.time()
        out = main.run_tool(fast, "q")
        release.set()
        self.assertLess(time.time() - t0, 2)
        self.assertEqual(out, "Tool error: kb_query timed out after 0.2s")

    def test_the_key_never_reaches_output_errors_or_logs(self):
        echo = _FakeIngress({"ok": True, "result": {"echo": f"header was {KEY}"}})
        self.assertNotIn(KEY, main.run_tool(self.tools(echo)["kb_query"], "q"))
        err = _FakeIngress({"ok": False, "error": {"message": f"bad key {KEY}"}})
        with self.assertLogs("crew-service", "DEBUG") as logs:
            main.log.debug("sentinel")
            out = main.run_tool(self.tools(err)["kb_query"], "q")
        self.assertNotIn(KEY, out)
        self.assertNotIn(KEY, "\n".join(logs.output))
        # And a failed run's stored result goes through safe_result as well.
        self.assertNotIn(KEY, main.safe_result("failed", out) or "")

    def test_non_http_url_is_refused(self):
        with self.assertRaisesRegex(main.IngressError, "http"):
            main.ingress_call("kb.query", {}, url="file:///etc/passwd", secret=KEY, opener=_FakeIngress())

    def test_service_reads_no_crew_key_in_this_test_image(self):
        self.assertEqual(main.CREW_INGRESS_SECRET, "")
        self.assertEqual(main.TOOL_REGISTRY, {})


# ── R13-A: model calls go through the Amara Router ──────────────────────────
import http.server
import socketserver


class _MockRouter:
    """A local stand-in for the router: records each request, answers OpenAI chat completions."""

    def __init__(self):
        outer = self
        self.requests = []

        class H(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                n = int(self.headers.get("Content-Length", "0"))
                outer.requests.append({"path": self.path, "headers": dict(self.headers.items()),
                                       "body": json.loads(self.rfile.read(n) or b"{}")})
                body = json.dumps({
                    "id": "c1", "object": "chat.completion", "created": 0,
                    "model": outer.requests[-1]["body"].get("model"),
                    "choices": [{"index": 0, "finish_reason": "stop",
                                 "message": {"role": "assistant", "content": "pong"}}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                }).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *a):
                pass

        self.server = socketserver.TCPServer(("127.0.0.1", 0), H)
        self.port = self.server.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}/v1"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class RouterLLMTests(unittest.TestCase):
    def setUp(self):
        self.router = _MockRouter()
        self.addCleanup(self.router.close)

    def _call(self, tier, key=""):
        from crewai import LLM
        llm = LLM(**main.router_llm_kwargs(tier, url=self.router.url, key=key))
        return llm.call("ping")

    def test_default_url_is_router_on_8787(self):
        self.assertEqual(main.CREW_ROUTER_URL, "http://host.docker.internal:8787/v1")
        kw = main.router_llm_kwargs("fast")
        self.assertEqual(kw["base_url"], "http://host.docker.internal:8787/v1")

    def test_fast_and_deep_pin_alias_and_header(self):
        for tier in ("fast", "deep"):
            self.router.requests.clear()
            self._call(tier)
            self.assertEqual(len(self.router.requests), 1)
            r = self.router.requests[0]
            self.assertEqual(r["path"], "/v1/chat/completions")
            self.assertEqual(r["body"]["model"], f"amara-{tier}")
            self.assertEqual(r["headers"].get("X-Amara-Route"), tier)
            self.assertTrue(r["headers"]["Host"].startswith(f"127.0.0.1:{self.router.port}"))

    def test_no_ollama_residency_fields_sent(self):
        self._call("fast")
        body = self.router.requests[0]["body"]
        for field in ("keep_alive", "num_ctx", "options"):
            self.assertNotIn(field, body)

    def test_optional_bearer_only_when_configured(self):
        self._call("fast", key="")
        self.assertNotIn("sk-", self.router.requests[-1]["headers"].get("Authorization", ""))
        self._call("deep", key="router-bearer-xyz")
        self.assertEqual(self.router.requests[-1]["headers"]["Authorization"], "Bearer router-bearer-xyz")

    def test_ollama_url_is_refused(self):
        for bad in ("http://host.docker.internal:11434", "http://127.0.0.1:11434/v1", "ftp://x/v1",
                    "http://h:011434/v1", "http://127.0.0.1:011434/v1", "http://127.0.0.1:11434?x",
                    "http://127.0.0.1:11434#a", "http://127.0.0.1:11434\\x", "http://127.0.0.1/v1",
                    "http://evil.example:8787/v1", "http://u:p@127.0.0.1:8787/v1", "http://127.0.0.1:8787/v1?a=1"):
            with self.assertRaises(ValueError):
                main.router_llm_kwargs("fast", url=bad)
        with self.assertRaises(ValueError):
            main.router_llm_kwargs("medium")

    def test_source_never_targets_11434(self):
        with open(main.__file__) as f:
            self.assertNotIn("11434", f.read())

    def test_build_llms_uses_router_for_every_agent_with_its_tier(self):
        with mock.patch.object(main, "CREW_ROUTER_URL", self.router.url):
            [(label, llms)] = main._build_llms(["researcher", "analyst"])
        self.assertEqual(label, "amara-deep,amara-fast")
        self.assertEqual(llms["researcher"].model, "amara-fast")
        self.assertEqual(llms["analyst"].model, "amara-deep")
        for llm in llms.values():
            self.assertEqual(llm.base_url, self.router.url)

    def test_ollama_env_no_longer_drives_model_calls(self):
        self.assertFalse(hasattr(main, "OLLAMA_BASE_URL"))
        self.assertFalse(hasattr(main, "CREW_OLLAMA_MODEL"))

    def test_telemetry_stays_disabled(self):
        self.assertEqual(os.environ.get("CREWAI_DISABLE_TELEMETRY"), "true")


class MiniMaxRemovedTests(unittest.TestCase):
    def test_no_cloud_path_remains(self):
        for name in ("cloud_allowed", "MINIMAX_API_KEY", "MINIMAX_BASE_URL", "MINIMAX_MODEL", "CREW_ALLOW_CLOUD"):
            self.assertFalse(hasattr(main, name), name)
        src = open(main.__file__).read().lower()
        self.assertNotIn("minimax/", src)
        self.assertNotIn("api.minimax", src)

    def test_build_llms_is_router_only(self):
        attempts = main._build_llms(["writer"])
        self.assertEqual([a[0] for a in attempts], ["amara-fast"])

    def test_concurrency_defaults_to_one(self):
        self.assertEqual(main.MAX_CONCURRENT, 1)
        compose = open(os.path.join(os.path.dirname(main.__file__), "..", "docker-compose.yml")).read() \
            if os.path.exists(os.path.join(os.path.dirname(main.__file__), "..", "docker-compose.yml")) else None
        if compose:  # not copied into the test image; checked on the host by the executor too
            self.assertIn("CREW_MAX_CONCURRENT:-1}", compose)
            self.assertNotIn("MINIMAX", compose)


class CrewInfoLlmTests(unittest.TestCase):
    def test_row_to_info_carries_router_model(self):
        info = main._row_to_info(("i", "n", "running", ["a"], None, "t", None, "amara-fast"))
        self.assertEqual(info.llm, "amara-fast")
        self.assertIsNone(main._row_to_info(("i", "n", "running", ["a"], None, "t", None)).llm)

    def test_start_records_router_model_not_ollama_or_cloud(self):
        inserts = []

        class Conn:
            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

            def execute(self, sql, params=()):
                inserts.append((sql, params))
                return self

            def fetchone(self):
                return None

        ctx = mock.Mock()
        ctx.Pipe.return_value = (mock.Mock(), mock.Mock())
        with mock.patch.object(main, "db", return_value=Conn()), mock.patch.object(main, "_ctx", ctx), \
                mock.patch.object(main.threading, "Thread"), \
                mock.patch.object(main, "db_get", return_value=main.CrewInfo(
                    id="x", name="n", status="running", agents=["researcher"], llm="amara-fast")):
            main._procs.clear()
            main.start_crew(main.CrewStartRequest(name="n", task="t"))
            main._procs.clear()
        sql, params = inserts[0]
        self.assertIn("llm", sql)
        self.assertEqual(params[-2:], ("amara-fast", None))  # llm, retry_of
        main._resp_pipes.clear()
        self.assertEqual(set(main.planned_llm_label(list(main.AGENT_CATALOG)).split(",")),
                         {"amara-fast", "amara-deep"})


# ── R13-B: steps, handoffs, totals ──────────────────────────────────────────
class _Conn:
    """A fake DB connection that records every statement."""

    def __init__(self, log, rows=None):
        self.log, self.rows = log, rows or []

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def execute(self, sql, params=()):
        self.log.append((sql, params))
        return self

    def fetchall(self):
        return self.rows

    def fetchone(self):
        return None


class _ListConn:
    """Stands in for a multiprocessing Connection in-process: send() appends to a list."""

    def __init__(self):
        self.msgs = []

    def send(self, m):
        self.msgs.append(m)

    def close(self):
        pass


class RunRecorderTests(unittest.TestCase):
    def make(self, order=("researcher", "analyst")):
        out = []
        clock = [1000.0]
        rec = main.RunRecorder(out.append, list(order), now=lambda: clock[0])
        return rec, out, clock

    def test_two_role_run_has_a_timeline_and_one_handoff(self):
        rec, out, _ = self.make()
        rec.task_started("researcher")
        rec.mark("t1", "task", "researcher", "researcher: task", start=1000.5)
        rec.mark("t1", "task", "researcher", "researcher: task", end=1002.0, status="ok")
        rec.task_started("analyst")
        rec.mark("t2", "task", "analyst", "analyst: task", start=1002.1)
        rec.mark("t2", "task", "analyst", "analyst: task", end=1004.0, status="ok")
        steps = [m for m in out if m["k"] == "step"]
        hands = [m for m in out if m["k"] == "handoff"]
        self.assertEqual([(s["label"], s["start_ms"], s["end_ms"], s["status"]) for s in steps],
                         [("researcher: task", 500, 2000, "ok"), ("analyst: task", 2100, 4000, "ok")])
        self.assertEqual(hands, [{"k": "handoff", "from": "researcher", "to": "analyst", "status": "ok"}])
        rec.task_started("analyst")  # a repeat start does not duplicate the handoff
        self.assertEqual(len([m for m in out if m["k"] == "handoff"]), 1)

    def test_end_before_start_still_makes_one_step(self):
        rec, out, _ = self.make()
        rec.mark("c1", "llm", "analyst", "llm: analyst", end=1003.0, status="ok")
        self.assertEqual(out, [])
        rec.mark("c1", "llm", "analyst", "llm: analyst", start=1001.0)
        self.assertEqual([(m["start_ms"], m["end_ms"]) for m in out], [(1000, 3000)])

    def test_usage_totals_and_missing_usage_is_counted_not_estimated(self):
        rec, out, _ = self.make()
        rec.llm_done({"prompt_tokens": 10, "completion_tokens": 5})
        rec.llm_done({"input_tokens": 3, "output_tokens": 2})
        rec.llm_done(None)
        self.assertEqual((rec.prompt_tokens, rec.completion_tokens, rec.requests, rec.usage_missing),
                         (13, 7, 3, 1))
        self.assertEqual(out[-1], {"k": "usage", "tokens": 20, "requests": 3, "usage_missing": 1})

    def test_close_marks_unfinished_failed_and_unstarted_tasks_skipped(self):
        rec, out, clock = self.make()
        rec.task_started("researcher")
        rec.mark("t1", "task", "researcher", "researcher: task", start=1000.0)
        clock[0] = 1005.0
        rec.close(failed=True)
        got = {(m["label"], m["status"]) for m in out if m["k"] == "step"}
        self.assertEqual(got, {("researcher: task", "failed"), ("analyst: task", "skipped")})

    def test_tool_error_is_redacted_and_the_step_has_no_query_or_output(self):
        out = []
        rec = main.RunRecorder(out.append, ["researcher"])

        def boom(arg):
            raise RuntimeError("upstream said api_key=sk-LIVEKEY123456 at /app/data/x.json")

        tool = main._to_crewai_tool(main.CrewTool("kb_query", "d", boom), rec, "researcher")
        res = tool._run("QUERY-TEXT-PRIVATE")
        self.assertTrue(res.startswith("Tool error:"))
        [step] = [m for m in out if m["k"] == "step"]
        self.assertEqual((step["kind"], step["label"], step["status"]), ("tool", "tool: kb_query", "failed"))
        blob = json.dumps(step)
        for bad in ("sk-LIVEKEY", "/app/data", "QUERY-TEXT-PRIVATE"):
            self.assertNotIn(bad, blob)

    def test_ok_tool_call_is_an_ok_step(self):
        out = []
        rec = main.RunRecorder(out.append, ["researcher"])
        tool = main._to_crewai_tool(main.CrewTool("kb_query", "d", lambda a: "OUTPUT-PRIVATE"), rec, "researcher")
        self.assertEqual(tool._run("q"), "OUTPUT-PRIVATE")
        self.assertEqual((out[0]["status"], out[0]["error"]), ("ok", None))
        self.assertNotIn("OUTPUT-PRIVATE", json.dumps(out))


class CrewChildRunTests(unittest.TestCase):
    """A real two-role crew against the mock router, run in-process (setsid patched out)."""

    def test_two_role_run_streams_steps_handoff_and_exact_usage(self):
        router = _MockRouter()
        self.addCleanup(router.close)
        conn = _ListConn()
        prompt = "ZZ-PRIVATE-PROMPT-ZZ"
        with mock.patch.object(main, "CREW_ROUTER_URL", router.url), \
                mock.patch.object(main.os, "setsid"):
            main._run_crew_child(["researcher", "analyst"], prompt, conn)
        final = conn.msgs[-1]
        self.assertEqual((final["k"], final["status"]), ("final", "completed"))
        steps = [m for m in conn.msgs if m["k"] == "step"]
        tasks = [s for s in steps if s["kind"] == "task"]
        llms = [s for s in steps if s["kind"] == "llm"]
        self.assertEqual(sorted(t["label"] for t in tasks), ["analyst: task", "researcher: task"])
        self.assertGreaterEqual(len(llms), 2)
        self.assertTrue(all(s["status"] == "ok" for s in steps))
        self.assertEqual([m for m in conn.msgs if m["k"] == "handoff"],
                         [{"k": "handoff", "from": "researcher", "to": "analyst", "status": "ok"}])
        # The mock router reports 2 tokens per call; the totals are exactly what it reported.
        self.assertEqual(final["requests"], len(router.requests))
        self.assertEqual(final["tokens"], 2 * len(router.requests))
        self.assertNotIn(prompt, json.dumps(conn.msgs).replace(final["result"] or "", ""))
        for s in steps:
            self.assertRegex(s["label"], r"^(researcher|analyst): task$|^llm: (researcher|analyst)$|^tool: \w+$")


class WatchTests(unittest.TestCase):
    class _Proc:
        exitcode = 0
        pid = None

        def __init__(self, alive=False):
            self.alive = alive

        def join(self, t=None):
            pass

        def is_alive(self):
            return self.alive

    def run_watch(self, msgs, proc=None):
        r, w = mp.Pipe(duplex=False)
        for m in msgs:
            w.send(m)
        w.close()
        calls = []
        with mock.patch.object(main, "db_add_step", lambda i, m: calls.append(("step", m["label"]))), \
                mock.patch.object(main, "db_add_handoff", lambda i, m: calls.append(("handoff", m["from"], m["to"]))), \
                mock.patch.object(main, "db_finish", lambda i, st, res, llm=None: calls.append(("finish", st, res, llm))), \
                mock.patch.object(main, "db_set_totals", lambda i, t, q: calls.append(("totals", t, q))), \
                mock.patch.object(main, "db_create_approval", lambda i, m: calls.append(("approval", m["kind"])) or "a1"), \
                mock.patch.object(main, "db_expire_approvals", lambda i, by="system": calls.append(("expire",))):
            main._watch("rid", proc or self._Proc(), r)
        return calls

    def test_steps_are_written_as_they_arrive_then_the_run_settles_with_totals(self):
        calls = self.run_watch([
            {"k": "step", "label": "a: task"}, {"k": "handoff", "from": "a", "to": "b"},
            {"k": "usage", "tokens": 7, "requests": 2},
            {"k": "final", "status": "completed", "result": "ok", "llm": "amara-fast", "tokens": 9, "requests": 3}])
        self.assertEqual(calls, [("step", "a: task"), ("handoff", "a", "b"),
                                 ("finish", "completed", "ok", "amara-fast"), ("totals", 9, 3), ("expire",)])

    def test_child_that_dies_keeps_its_steps_and_totals(self):
        calls = self.run_watch([{"k": "step", "label": "a: task"}, {"k": "usage", "tokens": 4, "requests": 1}])
        self.assertEqual(calls[0], ("step", "a: task"))
        self.assertEqual(calls[1][:2], ("finish", "failed"))
        self.assertEqual(calls[-2], ("totals", 4, 1))


mp = main.mp


class StepPersistenceTests(unittest.TestCase):
    def test_db_add_step_allowlists_sanitizes_and_caps(self):
        log = []
        with mock.patch.object(main, "db", return_value=_Conn(log)):
            main.db_add_step("rid", {"agent": "a", "kind": "evil", "label": "x" * 500, "start_ms": 5,
                                     "end_ms": 1, "status": "weird",
                                     "error": "bad token=abcdef /srv/x/y.json"})
        _sql, p = log[0]
        self.assertEqual((p[2], p[6], p[4], p[5]), ("task", "failed", 5, 5))
        self.assertEqual(len(p[3]), main.MAX_LABEL_CHARS)
        self.assertNotIn("abcdef", p[7])
        self.assertNotIn("/srv", p[7])

    def test_schema_is_idempotent_and_additive(self):
        sql = main.SCHEMA
        self.assertNotRegex(sql, r"(?i)drop |delete |truncate ")
        for stmt in [s for s in sql.split(";") if s.strip()]:
            self.assertRegex(stmt, r"(?i)IF NOT EXISTS")
        for col in ("tokens", "requests"):
            self.assertIn(f"ADD COLUMN IF NOT EXISTS {col}", sql)
        self.assertIn("crew_steps", sql)
        self.assertIn("crew_handoffs", sql)

    def test_info_carries_totals_and_local_cost_is_zero(self):
        import datetime as dt
        fin = dt.datetime(2026, 1, 1, 0, 1, 0)
        row = ("i", "n", "completed", ["a"], fin, "t", "r", "amara-fast", fin, 120, 4)
        info = main._row_to_info(row)
        self.assertEqual((info.tokens, info.requests, info.cost_usd, info.finished_at),
                         (120, 4, 0.0, "2026-01-01T00:01:00Z"))
        old = main._row_to_info(("i", "n", "completed", ["a"], None, "t", "r", "amara-fast", None, None, None))
        self.assertEqual((old.tokens, old.requests, old.finished_at), (None, None, None))
        cloud = main._row_to_info(("i", "n", "completed", ["a"], None, "t", "r", "minimax/M", None, None, None))
        self.assertIsNone(cloud.cost_usd)  # cloud cost is unknown, never invented

    def test_steps_endpoint_shape_and_empty_and_bad_id(self):
        from fastapi import HTTPException
        rid = "11111111-2222-3333-4444-555555555555"
        info = main.CrewInfo(id=rid, name="n", status="completed", agents=["a"])
        rows = [[(1, "researcher", "task", "researcher: task", 0, 10, "ok", None)],
                [("researcher", "analyst", "ok")]]

        class C(_Conn):
            def execute(self, sql, params=()):
                self.rows = rows[0] if "crew_steps" in sql else rows[1]
                return super().execute(sql, params)

        with mock.patch.object(main, "db_get", return_value=info), \
                mock.patch.object(main, "db", return_value=C([])):
            out = main.get_run_steps(rid)
            body = json.loads(out.model_dump_json(by_alias=True))
        self.assertEqual(body["handoffs"], [{"from": "researcher", "to": "analyst", "status": "ok"}])
        self.assertEqual(body["steps"][0]["label"], "researcher: task")
        with mock.patch.object(main, "db_get", return_value=info), \
                mock.patch.object(main, "db", return_value=_Conn([], [])):
            self.assertEqual(main.get_run_steps(rid).steps, [])
        with self.assertRaises(HTTPException) as cm:
            main.get_run_steps("../etc/passwd")
        self.assertEqual(cm.exception.status_code, 404)

    def test_llm_and_task_errors_store_fixed_text_only(self):
        out = []
        rec = main.RunRecorder(out.append, ["researcher"])
        handlers = {}
        fake_bus = mock.Mock()
        fake_bus.on = lambda et: (lambda h: handlers.setdefault(et.__name__, h))
        with mock.patch("crewai.events.crewai_event_bus", fake_bus):
            main._attach_listeners(rec, {main.AGENT_CATALOG["researcher"]["role"]: "researcher"})
        ev = mock.Mock(call_id="c", error="prompt: SECRETPROMPT key=sk-short1", from_agent=None,
                       task=None, from_task=None, agent_role=None)
        import datetime as dt
        ev.timestamp = dt.datetime.now(dt.timezone.utc)
        handlers["LLMCallStartedEvent"](None, ev)
        handlers["LLMCallFailedEvent"](None, ev)
        ev.error_type = ValueError
        handlers["TaskStartedEvent"](None, ev)
        handlers["TaskFailedEvent"](None, ev)
        rec.close(failed=True)
        blob = json.dumps(out)
        self.assertNotIn("SECRETPROMPT", blob)
        self.assertNotIn("sk-short1", blob)
        self.assertIn("LLM call failed", blob)
        self.assertIn("Task failed (ValueError)", blob)
        # the parent also drops message bodies for non-tool kinds
        log = []
        with mock.patch.object(main, "db", return_value=_Conn(log)):
            main.db_add_step("rid", {"kind": "llm", "status": "failed", "error": "echo SECRETPROMPT"})
            main.db_add_step("rid", {"kind": "tool", "status": "failed", "error": "plain tool failure"})
        self.assertEqual(log[0][1][7], "LLM call failed")
        self.assertEqual(log[1][1][7], "plain tool failure")

    def test_handoffs_need_known_roles_and_are_capped(self):
        log = []
        with mock.patch.object(main, "db", return_value=_Conn(log)):
            main.db_add_handoff("rid", {"from": "researcher", "to": "analyst"})
            main.db_add_handoff("rid", {"from": "evil", "to": "analyst"})
            main.db_add_handoff("rid", {"from": "researcher", "to": "unregistered_tool"})
        self.assertEqual(len(log), 1)
        self.assertEqual(log[0][1][-1], main.MAX_HANDOFFS_PER_RUN)

    def test_totals_left_null_without_usage(self):
        for msgs in ([], [{"k": "usage", "tokens": 0, "requests": 0}],
                     [{"k": "usage", "tokens": 4, "requests": 2, "usage_missing": 1}]):
            calls = WatchTests().run_watch(msgs)
            self.assertFalse([c for c in calls if c[0] == "totals"], msgs)

    def test_unknown_llm_has_no_cost(self):
        self.assertIsNone(main.run_cost_usd("weird"))
        self.assertIsNone(main.run_cost_usd(""))
        self.assertEqual(main.run_cost_usd("amara-deep,amara-fast"), 0.0)

    def test_non_ascii_token_is_401_not_500(self):
        from fastapi import HTTPException
        with mock.patch.object(main, "CREW_SERVICE_TOKEN", "good-token"):
            with self.assertRaises(HTTPException) as cm:
                main.require_token("tökén")
            self.assertEqual(cm.exception.status_code, 401)
            self.assertIsNone(main.require_token("good-token"))

    def test_steps_route_requires_the_token(self):
        route = next(r for r in main.app.routes if getattr(r, "path", "") == "/crews/runs/{run_id}/steps")
        self.assertTrue(any(d.call is main.require_token for d in route.dependant.dependencies))


# ── R13-D approvals ─────────────────────────────────────────────────────────
class _Boom(BaseException):
    """Stands in for ChildApprover's os._exit in tests."""


def _write_tool(calls):
    """A test-only tool that is NOT read-only (so it needs approval). Never registered globally."""
    return main.CrewTool("send_note", "test-only write tool", lambda q: calls.append(q) or "sent")


class ApprovalGateTests(unittest.TestCase):
    def make(self, timeout=5.0, candidates=None):
        sent = []
        r, w = mp.Pipe(duplex=False)  # parent -> child

        def abort():
            raise _Boom()
        return main.ChildApprover(sent.append, r, timeout, abort, candidates), sent, w

    def test_read_only_tools_never_ask(self):
        self.assertIsNone(main.gate_tool_call(_tool("kb_query"), None))
        self.assertIsNone(main.gate_tool_call(_tool("memory_query"), None))

    def test_write_tool_without_an_approver_fails_closed(self):
        calls = []
        wrapped = main._to_crewai_tool(_write_tool(calls), None, "researcher", None)
        self.assertEqual(wrapped._run("hi"), main.APPROVAL_DENIED)
        self.assertEqual(calls, [])

    def test_blocked_kinds_are_refused_with_no_request(self):
        for kind in ("outbound", "spend", "network", "git_push", "sudo"):
            approver, sent, w = self.make()
            w.send({"k": "decision", "approve": True})  # even a waiting yes can't help
            tool = main.CrewTool("x_" + kind, "d", lambda q: "ran", approval_kind=kind)
            self.assertEqual(main.gate_tool_call(tool, approver), main.TOOL_BLOCKED)
            self.assertEqual(sent, [])
            self.assertEqual(main.write_candidates([tool]), {})

    def test_claude_tier_is_blocked_while_outbound_is_disabled(self):
        tool = main.CrewTool("ask_claude", "d", lambda q: "ran", approval_kind="claude_tier")
        with mock.patch.object(main, "NEXUS_OUTBOUND_DISABLED", "1"):
            self.assertEqual(main.gate_tool_call(tool, None), main.TOOL_BLOCKED)
        with mock.patch.object(main, "NEXUS_OUTBOUND_DISABLED", ""):
            self.assertEqual(main.gate_tool_call(tool, None), main.APPROVAL_DENIED)  # needs a yes, none given

    def test_request_blocks_until_approved_then_the_batch_is_covered_for_the_run(self):
        a, b = _write_tool([]), main.CrewTool("edit_file", "d", lambda q: "edited", approval_kind="destructive")
        approver, sent, w = self.make(candidates=main.write_candidates([a, b]))
        calls, out = [], []
        wrapped = main._to_crewai_tool(_write_tool(calls), None, "researcher", approver)
        t = threading.Thread(target=lambda: out.append(wrapped._run("hi")))
        t.start()
        for _ in range(100):
            if sent:
                break
            time.sleep(0.02)
        # ONE request lists every write action the run may take; the riskiest kind labels it.
        self.assertEqual(sent, [{"k": "approval", "kind": "destructive", "items": ["edit_file", "send_note"]}])
        time.sleep(0.2)
        self.assertTrue(t.is_alive(), "the call must wait for the human")
        self.assertEqual(calls, [])
        w.send({"k": "decision", "approve": True})
        t.join(5)
        self.assertEqual((out, calls), (["sent"], ["hi"]))
        # Both tools in the batch now run without another request.
        self.assertIsNone(main.gate_tool_call(b, approver))
        self.assertEqual(wrapped._run("again"), "sent")
        self.assertEqual(len(sent), 1)

    def test_deny_cancels_and_the_tool_never_runs(self):
        approver, sent, w = self.make()
        calls = []
        w.send({"k": "decision", "approve": False})
        with self.assertRaises(_Boom):
            main.gate_tool_call(_write_tool(calls), approver)
        self.assertEqual(calls, [])
        self.assertEqual((sent[-1]["status"], sent[-1]["result"]), ("cancelled", "approval denied"))

    def test_timeout_and_closed_pipe_are_denials(self):
        approver, sent, w = self.make(timeout=0.1)
        with self.assertRaises(_Boom):
            approver.request("write", ["x"])
        self.assertEqual(sent[-1]["result"], "approval timed out")
        approver, sent, w = self.make()
        w.close()
        with self.assertRaises(_Boom):
            approver.request("write", ["x"])
        self.assertEqual(sent[-1]["result"], "approval channel closed")

    def test_anything_but_an_explicit_true_is_a_denial(self):
        for bad in ({"k": "decision", "approve": "yes"}, {"k": "decision"}, "approve", {"k": "other", "approve": True}):
            approver, sent, w = self.make()
            w.send(bad)
            with self.assertRaises(_Boom):
                approver.request("write", ["x"])


class RetryLimitTests(unittest.TestCase):
    def make(self):
        return ApprovalGateTests().make()

    def test_succeeds_without_asking(self):
        approver, sent, w = self.make()
        self.assertEqual(main.run_with_retries(lambda: "ok", approver, 2, 0, lambda t: None), (True, "ok"))
        self.assertEqual(sent, [])

    def test_at_most_two_automatic_retries_then_a_needs_you_item(self):
        approver, sent, w = self.make()
        runs = []

        def flaky():
            runs.append(1)
            raise RuntimeError("router 503")
        w.send({"k": "decision", "approve": False})
        with self.assertRaises(_Boom):
            main.run_with_retries(flaky, approver, 2, 0, lambda t: None)
        self.assertEqual(len(runs), 3)  # the first try plus two retries, no more
        self.assertEqual(sent[0], {"k": "approval", "kind": "retry_limit", "items": ["Retry the failed task"]})
        self.assertEqual(sent[-1]["status"], "cancelled")

    def test_approve_buys_exactly_one_more_attempt_then_asks_again(self):
        approver, sent, w = self.make()
        runs = []

        def flaky():
            runs.append(1)
            if len(runs) == 4:
                return "finally"
            raise RuntimeError("x")
        w.send({"k": "decision", "approve": True})
        self.assertEqual(main.run_with_retries(flaky, approver, 2, 0, lambda t: None), (True, "finally"))
        self.assertEqual(len(runs), 4)
        self.assertEqual([m["kind"] for m in sent], ["retry_limit"])
        # a fifth failure would have asked a second time
        approver, sent, w = self.make()
        w.send({"k": "decision", "approve": True})
        w.send({"k": "decision", "approve": False})
        n = []
        with self.assertRaises(_Boom):
            main.run_with_retries(lambda: n.append(1) or (_ for _ in ()).throw(RuntimeError("x")), approver, 2, 0, lambda t: None)
        self.assertEqual(len(n), 4)
        self.assertEqual([m["kind"] for m in sent if m["k"] == "approval"], ["retry_limit", "retry_limit"])

    def test_without_a_pipe_it_fails_with_the_sanitized_error(self):
        ok, err = main.run_with_retries(lambda: (_ for _ in ()).throw(ValueError("/srv/secret/x")), None, 2, 0, lambda t: None)
        self.assertFalse(ok)
        self.assertNotIn("/srv", err)

    def test_backoff_grows_between_retries(self):
        waits = []
        main.run_with_retries(lambda: (_ for _ in ()).throw(RuntimeError("x")), None, 2, 1.5, waits.append)
        self.assertEqual(waits, [1.5, 3.0])


class DeepWaitTests(unittest.TestCase):
    def test_waits_while_deep_is_resident_then_goes(self):
        states = iter([True, True, False])
        sleeps, clock = [], [0.0]

        def sleep(t):
            sleeps.append(t)
            clock[0] += t
        waited = main.wait_for_deep_idle(100, 5, lambda: next(states), sleep, lambda: clock[0])
        self.assertEqual((sleeps, waited), ([5, 5], 10.0))

    def test_unknown_or_idle_does_not_wait_and_the_wait_is_capped(self):
        for state in (None, False):
            self.assertEqual(main.wait_for_deep_idle(100, 5, lambda s=state: s, lambda t: self.fail("slept"), lambda: 0.0), 0.0)
        clock = [0.0]

        def sleep(t):
            clock[0] += t
        self.assertEqual(main.wait_for_deep_idle(20, 5, lambda: True, sleep, lambda: clock[0]), 20.0)  # gives up
        self.assertEqual(main.wait_for_deep_idle(0, 5, lambda: self.fail("probed"), sleep, lambda: 0.0), 0.0)  # off

    def test_residency_is_read_from_router_health_on_the_router_host(self):
        self.assertEqual(main.router_health_url("http://host.docker.internal:8787/v1"), "http://host.docker.internal:8787/health")

        def opener(doc):
            return lambda req, timeout=0: _Resp(doc)
        self.assertIs(main.deep_resident("http://127.0.0.1:8787/v1", opener({"loaded": {"fast": True, "deep": True}})), True)
        self.assertIs(main.deep_resident("http://127.0.0.1:8787/v1", opener({"loaded": {"fast": True, "deep": False}})), False)
        self.assertIsNone(main.deep_resident("http://127.0.0.1:8787/v1", opener({"status": "ok"})))
        self.assertIsNone(main.deep_resident("http://127.0.0.1:8787/v1", lambda req, timeout=0: (_ for _ in ()).throw(OSError("down"))))

    def test_a_child_run_waits_before_building_agents(self):
        conn = _ListConn()
        order = []
        with mock.patch.object(main.os, "setsid"), mock.patch.object(main, "_attach_listeners", lambda *a: (lambda: None)), \
                mock.patch.object(main, "wait_for_deep_idle", lambda: order.append("wait") or 0.0), \
                mock.patch.object(main, "_build_llms", lambda keys: order.append("build") or []):
            main._run_crew_child(["writer"], "task", conn)
        self.assertEqual(order, ["wait", "build"])


class _Resp:
    def __init__(self, doc):
        self._b = json.dumps(doc).encode()

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def read(self, n=-1):
        return self._b


class ApprovalPersistenceTests(unittest.TestCase):
    def test_create_allowlists_kind_and_derives_risk_and_reason(self):
        log = []
        with mock.patch.object(main, "db", return_value=_Conn(log)):
            aid = main.db_create_approval("rid", {"kind": "rm -rf", "title": "attacker", "risk": "Low",
                                                  "reason": "attacker text", "items": ["edit_file", "x\nsecret", ""] + ["t"] * 40})
        ins, upd = log[0], log[1]
        self.assertEqual(ins[1][0], aid)
        kind, title, items, reason, risk = ins[1][2:]
        self.assertEqual((kind, title, reason, risk), ("write", main.APPROVAL_TITLE["write"],
                                                       main.APPROVAL_REASON["write"], "Medium"))
        self.assertEqual(items[:2], ["edit_file", "x"])  # first line only, blanks dropped
        self.assertLessEqual(len(items), main.MAX_APPROVAL_ITEMS)
        self.assertIn("status='awaiting_approval'", upd[0])

    def test_every_kind_has_risk_and_reason(self):
        for k in main.APPROVAL_KINDS:
            self.assertIn(main.APPROVAL_RISK[k], ("Low", "Medium", "High"))
            self.assertTrue(main.APPROVAL_REASON[k] and main.APPROVAL_TITLE[k])
        self.assertFalse(set(main.APPROVAL_KINDS) & main.BLOCKED_KINDS)
        self.assertEqual(main.riskiest_kind(["write", "destructive"]), "destructive")

    def test_finish_and_resume_only_touch_live_runs(self):
        log = []
        with mock.patch.object(main, "db", return_value=_Conn(log)):
            main.db_finish("rid", "cancelled", "approval denied")
            main.db_resume_run("rid")
        self.assertIn("'running','awaiting_approval'", log[0][0])
        self.assertIn("status='awaiting_approval'", log[1][0])


class _Store:
    """In-memory crew_approvals for the endpoint tests."""

    def __init__(self, decided=False):
        self.decided = decided
        self.delivered = []

    def decide(self, aid, approve, by="user"):
        if aid != AID or self.decided:
            return None
        self.decided = True
        return ("11111111-2222-3333-4444-555555555555",)

    def exists(self, aid):
        return aid == AID


AID = "aaaaaaaa-2222-3333-4444-555555555555"


class ApprovalEndpointTests(unittest.TestCase):
    def call(self, store, approve, aid=AID):
        with mock.patch.object(main, "db_decide_approval", store.decide), \
                mock.patch.object(main, "db_approval_exists", store.exists), \
                mock.patch.object(main, "deliver_decision", lambda r, a: store.delivered.append((r, a))), \
                mock.patch.object(main, "db_get_approval", lambda a: "row"):
            return main.decide_approval(aid, main.DecideRequest(approve=approve))

    def test_decide_twice_is_409(self):
        from fastapi import HTTPException
        store = _Store()
        self.assertEqual(self.call(store, True), "row")
        with self.assertRaises(HTTPException) as cm:
            self.call(store, False)
        self.assertEqual(cm.exception.status_code, 409)
        self.assertEqual(store.delivered, [("11111111-2222-3333-4444-555555555555", True)])

    def test_decide_body_is_strict_only_a_real_true_approves(self):
        from pydantic import ValidationError
        for loose in ("yes", 1, "true", None):
            with self.assertRaises(ValidationError):
                main.DecideRequest(approve=loose)
        self.assertIs(main.DecideRequest(approve=True).approve, True)

    def test_parallel_first_writes_ask_once(self):
        asked = []
        gate = threading.Event()
        def send(msg):
            asked.append(msg)
        class Resp:
            def poll(self, t=None):
                gate.wait(2); return True
            def recv(self):
                return {"k": "decision", "approve": True}
        ap = main.ChildApprover(send, Resp(), timeout_s=5, abort=lambda: None,
                                candidates={"tool_a": "write"})
        tool = type("T", (), {"name": "tool_a", "approval_kind": "write"})()
        results = []
        ths = [threading.Thread(target=lambda: results.append(ap.allow_tool(tool))) for _ in range(2)]
        for th in ths: th.start()
        gate.set()
        for th in ths: th.join(5)
        self.assertEqual(results, [True, True])
        self.assertEqual(sum(1 for m in asked if m.get("kind") == "approval_request" or "items" in m), 1, asked)

    def test_bad_or_unknown_id_is_404_and_delivers_nothing(self):
        from fastapi import HTTPException
        store = _Store()
        for aid in ("../x", "bbbbbbbb-2222-3333-4444-555555555555"):
            with self.assertRaises(HTTPException) as cm:
                self.call(store, True, aid)
            self.assertEqual(cm.exception.status_code, 404)
        self.assertEqual(store.delivered, [])

    def test_routes_need_the_token(self):
        for path in ("/crews/approvals", "/crews/approvals/{approval_id}/decide", "/crews/retry"):
            route = next(r for r in main.app.routes if getattr(r, "path", "") == path)
            self.assertTrue(any(d.call is main.require_token for d in route.dependant.dependencies), path)


class DeliverDecisionTests(unittest.TestCase):
    def setUp(self):
        self.ctx = mp.get_context("spawn")

    def live_proc(self):
        proc = self.ctx.Process(target=_plain_child, daemon=True)
        proc.start()
        self.addCleanup(lambda: proc.is_alive() and proc.kill())
        return proc

    def test_approve_resumes_the_run_and_sends_the_decision(self):
        r, w = mp.Pipe(duplex=False)
        main._resp_pipes["rid"] = w
        self.addCleanup(main._resp_pipes.pop, "rid", None)
        with mock.patch.object(main, "db_resume_run") as resume, mock.patch.object(main, "db_finish") as fin:
            main.deliver_decision("rid", True)
        resume.assert_called_once_with("rid")
        fin.assert_not_called()
        self.assertEqual(r.recv(), {"k": "decision", "approve": True})

    def test_deny_cancels_the_run_and_kills_the_waiting_child(self):
        proc = self.live_proc()
        main._procs["rid"] = proc
        self.addCleanup(main._procs.pop, "rid", None)
        with mock.patch.object(main, "db_finish") as fin:
            main.deliver_decision("rid", False)
        fin.assert_called_once_with("rid", "cancelled", "approval denied")
        proc.join(10)
        self.assertFalse(proc.is_alive())

    def test_stop_kills_a_child_waiting_for_approval_and_expires_its_request(self):
        proc = self.live_proc()
        main._procs["rid2"] = proc
        self.addCleanup(main._procs.pop, "rid2", None)
        info = main.CrewInfo(id="rid2", name="n", status="awaiting_approval", agents=["researcher"])
        with mock.patch.object(main, "db_get", return_value=info), mock.patch.object(main, "db_finish") as fin, \
                mock.patch.object(main, "db_expire_approvals") as exp:
            out = main.stop_crew(main.CrewStopRequest(id="rid2"))
        self.assertEqual(out["stopped"], True)
        fin.assert_called_once_with("rid2", "stopped", "stopped by user")
        exp.assert_called_once()
        proc.join(10)
        self.assertFalse(proc.is_alive())


class WatchApprovalTests(unittest.TestCase):
    def test_approval_message_creates_a_request_and_the_run_keeps_going(self):
        calls = WatchTests().run_watch([
            {"k": "approval", "kind": "destructive", "items": ["x"]},
            {"k": "step", "label": "a: task"},
            {"k": "final", "status": "completed", "result": "ok", "llm": "amara-fast"}])
        self.assertEqual(calls[0], ("approval", "destructive"))
        self.assertIn(("finish", "completed", "ok", "amara-fast"), calls)
        self.assertEqual(calls[-1], ("expire",))  # nothing stays pending after the run settles

    def test_cancelled_final_settles_as_cancelled(self):
        calls = WatchTests().run_watch([{"k": "final", "status": "cancelled", "result": "approval denied"}])
        self.assertIn(("finish", "cancelled", "approval denied", None), calls)


class RetryTests(unittest.TestCase):
    ID = "11111111-2222-3333-4444-555555555555"

    def info(self, status="failed", task="do it"):
        return main.CrewInfo(id=self.ID, name="Research", status=status, agents=["researcher", "writer"], task=task)

    def test_retry_clones_name_task_agents_and_links_retry_of(self):
        with mock.patch.object(main, "db_get", return_value=self.info()), \
                mock.patch.object(main, "_launch", return_value="new") as launch:
            self.assertEqual(main.retry_crew(main.RetryRequest(id=self.ID)), "new")
        launch.assert_called_once_with("Research", "do it", ["researcher", "writer"], self.ID)

    def test_retry_refuses_active_unknown_and_bad_ids(self):
        from fastapi import HTTPException
        cases = [(self.info("running"), self.ID, 409), (self.info("awaiting_approval"), self.ID, 409),
                 (None, self.ID, 404), (self.info(), "../x", 404)]
        for found, rid, code in cases:
            with mock.patch.object(main, "db_get", return_value=found), mock.patch.object(main, "_launch") as launch:
                with self.assertRaises(HTTPException) as cm:
                    main.retry_crew(main.RetryRequest(id=rid))
            self.assertEqual(cm.exception.status_code, code)
            launch.assert_not_called()

    def test_start_with_retry_of_links_and_validates(self):
        from fastapi import HTTPException
        req = main.CrewStartRequest(name="n", task="t", retry_of=self.ID)
        with mock.patch.object(main, "db_get", return_value=self.info()), \
                mock.patch.object(main, "_launch", return_value="new") as launch:
            main.start_crew(req)
        launch.assert_called_once_with("n", "t", main.DEFAULT_AGENTS, self.ID)
        with mock.patch.object(main, "db_get", return_value=None), mock.patch.object(main, "_launch"):
            with self.assertRaises(HTTPException):
                main.start_crew(req)
        with self.assertRaises(ValueError):
            main.CrewStartRequest(name="n", task="t", retry_of="../etc")

    def test_row_carries_retry_of(self):
        import datetime
        row = (self.ID, "n", "failed", ["researcher"], datetime.datetime(2026, 1, 1), "t", None, "amara-fast",
               None, None, None, "99999999-2222-3333-4444-555555555555")
        self.assertEqual(main._row_to_info(row).retry_of, "99999999-2222-3333-4444-555555555555")


if __name__ == "__main__":
    unittest.main()

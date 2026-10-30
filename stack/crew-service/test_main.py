"""crew-service tests: error sanitizing, process-group stop and the tool registry.

Run inside the crew image (it has the deps):  python -m unittest -v test_main
"""

import os
import signal
import subprocess
import threading
import time
import unittest
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
        main.AGENT_CATALOG["bad"] = dict(role="r", goal="g", backstory="b", tools=["shell"])
        with self.assertRaisesRegex(ValueError, "unknown tool shell"):
            main.tools_for("bad", frozenset({"shell"}), self.reg)

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
        big = _tool("capped", fn=lambda q: q * 50, max_output_chars=10)
        wrapped = main._to_crewai_tool(big)
        self.assertEqual(wrapped.name, "capped")
        self.assertEqual(wrapped._run("ab"), "ababababab…[truncated]")


if __name__ == "__main__":
    unittest.main()

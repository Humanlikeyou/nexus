"""crew-service tests: error sanitizing and process-group stop.

Run inside the crew image (it has the deps):  python -m unittest -v test_main
"""

import os
import signal
import subprocess
import time
import unittest

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


if __name__ == "__main__":
    unittest.main()

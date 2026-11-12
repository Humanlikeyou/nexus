"""R13-E tests: run summaries through a pluggable writer; skill proposals as data."""

import json
import os
import tempfile
import unittest

import crew_memory as cm
import crew_tasks as ct

SECRET = "sk-" + "Q7w8E9r0" * 6


def done_task():
    t = ct.CrewTask(task_id="T1", run_id="r", role="researcher", tier="fast", objective="o", deliverables=["d"],
                    acceptance_criteria=["c"], verification={"type": "tests", "commands": ["pytest a.py"]})
    t.evidence = [ct.Evidence(id="abcdef0123456789", kind="test_output", command="pytest a.py", exit_code=0,
                              output_tail="SECRET OUTPUT should never be copied")]
    t.status = "succeeded"
    return t


class MockWriter:
    def __init__(self):
        self.got = []

    def write(self, s):
        self.got.append(s)


class SummaryTests(unittest.TestCase):
    def test_summary_is_capped_and_secret_free_and_has_no_content(self):
        run = {"id": "11111111-2222-3333-4444-555555555555", "status": "completed",
               "task": f"Research the thing with token={SECRET} and key {SECRET} " + "x" * 5000,
               "finished_at": "2026-11-11T10:00:00Z", "result": "FULL TRANSCRIPT SHOULD NOT APPEAR"}
        issues = [f"problem {i} with password=hunter2 {SECRET}" + "y" * 400 for i in range(50)]
        s = cm.build_run_summary(run, [done_task()] + [ct.CrewTask(task_id=f"F{i}", run_id="r", role="executor", tier="fast",
              objective="o", deliverables=["d"], acceptance_criteria=["c"], verification={"type": "approval"}) for i in range(30)], issues)
        blob = json.dumps(s)
        self.assertLessEqual(len(blob), cm.SUMMARY_MAX_CHARS)
        self.assertLessEqual(len(s["objective"]), cm.OBJECTIVE_CHARS + 1)
        self.assertLessEqual(len(s["open_issues"]), cm.MAX_ISSUES)
        for bad in (SECRET, "hunter2", "FULL TRANSCRIPT", "SECRET OUTPUT"):
            self.assertNotIn(bad, blob)
        self.assertEqual(set(s), {"run_id", "objective", "outcome", "evidence", "open_issues", "finished_at", "written_at"})
        self.assertIn("test_output:T1:abcdef012345", s["evidence"])
        self.assertTrue(s["outcome"].startswith("completed; 1 task(s) succeeded"))

    def test_failed_run_records_an_open_issue(self):
        s = cm.build_run_summary({"id": "r", "status": "failed", "task": "t"})
        self.assertEqual(s["open_issues"], ["run ended as failed"])

    def test_writers(self):
        self.assertIsInstance(cm.select_writer(), cm.NoopWriter)
        self.assertIsInstance(cm.select_writer("file", ""), cm.NoopWriter)  # no dir: no-op
        with self.assertRaises(ValueError):
            cm.select_writer("letta")
        with self.assertRaises(ValueError):
            cm.select_writer("http")
        cm.NoopWriter().write({"x": 1})

    def test_file_writer_appends_0600_json_lines(self):
        with tempfile.TemporaryDirectory() as d:
            w = cm.select_writer("file", os.path.join(d, "mem"))
            self.assertTrue(cm.write_summary_safely(w, {"a": 1}))
            self.assertTrue(cm.write_summary_safely(w, {"a": 2}))
            lines = open(w.path).read().splitlines()
            self.assertEqual([json.loads(x)["a"] for x in lines], [1, 2])
            self.assertEqual(os.stat(w.path).st_mode & 0o777, 0o600)

    def test_writer_failure_never_raises(self):
        class Bad:
            def write(self, s):
                raise OSError("disk")
        self.assertFalse(cm.write_summary_safely(Bad(), {}))

    def test_mock_writer_receives_the_summary(self):
        w = MockWriter()
        cm.write_summary_safely(w, cm.build_run_summary({"id": "r", "status": "completed", "task": "t"}))
        self.assertEqual(len(w.got), 1)


AID = "11111111-2222-3333-4444-555555555555"


class SkillTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.store = cm.SkillStore(os.path.join(self.tmp.name, "skills"), now=lambda: 1000)
        self.asked = []

    def ask(self, kind, items):
        self.asked.append((kind, items))
        return AID

    def test_proposal_is_versioned_data_raises_a_skill_approval_and_is_never_applied(self):
        a = self.store.propose("summarise-pr", "Summarise PRs", {"steps": ["read diff"]}, "r1", self.ask)
        b = self.store.propose("summarise-pr", "Summarise PRs v2", {"steps": ["read diff", "list risks"]}, "r1", self.ask)
        self.assertEqual((a["version"], b["version"], a["state"]), (1, 2, "proposed"))
        self.assertEqual(self.asked[0], ("skill", ["summarise-pr v1"]))
        self.assertFalse(self.store.can_apply("summarise-pr", 1, AID))
        audit = [json.loads(x) for x in open(os.path.join(self.store.root, "audit.log"))]
        self.assertEqual([e["event"] for e in audit], ["proposed", "proposed"])
        self.assertEqual(os.stat(self.store._path("summarise-pr", 1)).st_mode & 0o777, 0o600)

    def test_approving_records_the_decision_only(self):
        self.store.propose("s1", "t", {"steps": ["a"]}, "r1", self.ask)
        rec = self.store.record_decision(AID, True)
        self.assertEqual((rec["state"], rec["decided_by"]), ("approved", "taisun"))
        self.assertTrue(self.store.can_apply("s1", 1, AID))
        self.assertFalse(self.store.can_apply("s1", 1, "99999999-2222-3333-4444-555555555555"))
        self.assertIsNone(self.store.record_decision(AID, False))  # already decided
        self.assertEqual(json.loads(open(os.path.join(self.store.root, "audit.log")).read().splitlines()[-1])["event"], "decision")

    def test_denied_proposal_can_never_apply(self):
        self.store.propose("s1", "t", {"steps": ["a"]}, "r1", self.ask)
        self.store.record_decision(AID, False)
        self.assertFalse(self.store.can_apply("s1", 1, AID))

    def test_a_proposal_cannot_change_an_allowlist_without_an_approved_action(self):
        import main
        before = (main.ALLOWED_TOOLS, dict(main.TOOL_REGISTRY), main.READ_ONLY_TOOLS, main.BLOCKED_KINDS)
        rec = self.store.propose("evil", "Widen tools", {"allowlist": {"add": ["shell"]}, "CREW_ALLOWED_TOOLS": "shell"}, "r1", self.ask)
        self.assertTrue(rec["protected"])
        self.assertIn("touches a protected target", self.asked[-1][1])
        self.assertFalse(self.store.can_apply("evil", 1, AID))  # no approved action
        self.assertEqual(before, (main.ALLOWED_TOOLS, dict(main.TOOL_REGISTRY), main.READ_ONLY_TOOLS, main.BLOCKED_KINDS))
        self.store.record_decision(AID, True)
        self.assertTrue(self.store.can_apply("evil", 1, AID))  # only an approved action opens the gate
        self.assertFalse(hasattr(self.store, "apply"))  # applying is out of scope

    def test_soul_and_unknown_ids_and_secrets_are_refused(self):
        r = self.store.propose("persona", "Edit SOUL.md", {"text": "x"}, "r1", self.ask)
        self.assertTrue(r["protected"])
        for sid in ("../x", "A", "", "a b"):
            with self.assertRaises(cm.SkillError):
                self.store.propose(sid, "t", {"a": 1}, "r1", self.ask)
        with self.assertRaises(cm.SkillError):
            self.store.propose("ok", "t", {"note": f"key={SECRET}"}, "r1", self.ask)
        with self.assertRaises(cm.SkillError):
            self.store.propose("ok", "t", {}, "r1", self.ask)

    def test_failed_approval_keeps_no_proposal(self):
        with self.assertRaises(cm.SkillError):
            self.store.propose("s1", "t", {"a": 1}, "r1", lambda k, i: "")
        self.assertEqual(self.store.list(), [])

    def test_unconfigured_dir_fails_closed(self):
        with self.assertRaises(cm.SkillError):
            cm.SkillStore("")

    def test_list(self):
        self.store.propose("s1", "t", {"a": 1}, "r1", self.ask)
        self.assertEqual(self.store.list()[0]["state"], "proposed")


if __name__ == "__main__":
    unittest.main()

"""R13-C tests: CrewTask schema, state machine, evidence, verification runner, loop."""

import os
import tempfile
import unittest
from unittest import mock

import crew_tasks as ct


def mk(**over):
    base = dict(run_id="r1", role="researcher", tier="fast", objective="find facts",
                deliverables=["notes"], acceptance_criteria=["facts cited"],
                verification={"type": "tests", "commands": ["pytest test_x.py -q"], "evidence_required": True})
    base.update(over)
    return ct.CrewTask(**base)


def mkev(task, cmd=None, code=0):
    return ct.Evidence(id="e1", kind="test_output", command=cmd or task.verification.commands[0],
                       exit_code=code, output_tail="1 passed")


class SchemaTests(unittest.TestCase):
    def test_design_doc_example_parses_and_round_trips_with_dollar_id(self):
        doc = {"$id": "amara.crew.task.v1", "task_id": "01HZX", "run_id": "01HZY", "parent_task_id": None,
               "goal": "g", "role": "executor", "tier": "fast", "objective": "Record timing",
               "deliverables": ["Migration"], "acceptance_criteria": ["steps returned", "no secrets"],
               "verification": {"type": "tests", "commands": ["pytest test_main.py -k steps"], "evidence_required": True},
               "context_summary": "x", "inputs": [], "status": "pending", "retry_count": 0, "retry_limit": 2,
               "started_at": None, "finished_at": None, "tokens": {"prompt": 0, "completion": 0},
               "requests": 0, "error": None, "evidence": None}
        t = ct.CrewTask.model_validate(doc)
        self.assertEqual(t.doc()["$id"], "amara.crew.task.v1")
        self.assertEqual(ct.CrewTask.model_validate(t.doc()), t)

    def test_required_fields_and_bad_values_are_rejected(self):
        bad = [dict(deliverables=[]), dict(acceptance_criteria=[]), dict(acceptance_criteria=[" "]),
               dict(tier="medium"), dict(role="root"), dict(status="done"), dict(retry_limit=99),
               dict(objective=""), dict(extra_field=1),
               dict(verification={"type": "tests", "commands": []}),
               dict(verification={"type": "review", "commands": ["pytest a.py"], "evidence_required": False})]
        for over in bad:
            with self.assertRaises(ValueError, msg=str(over)):
                mk(**over)
        with self.assertRaises(ValueError):
            ct.CrewTask(run_id="r", role="executor", tier="fast", objective="o")

    def test_succeeded_without_evidence_cannot_be_constructed(self):
        with self.assertRaises(ValueError):
            mk(status="succeeded")
        t = mk()
        ok = mk(status="succeeded", evidence=[mkev(t)])
        self.assertEqual(ok.status, "succeeded")
        with self.assertRaises(ValueError):
            mk(status="succeeded", evidence=[mkev(t, code=1)])


class StateMachineTests(unittest.TestCase):
    def test_transition_table(self):
        t = mk()
        ct.transition(t, "running")
        self.assertIsNotNone(t.started_at)
        with self.assertRaises(ct.TransitionError):
            ct.transition(mk(), "escalated")  # pending -> escalated
        for terminal in ("failed", "cancelled"):
            x = mk()
            ct.transition(x, "running")
            ct.transition(x, terminal)
            self.assertIsNotNone(x.finished_at)
            with self.assertRaises(ct.TransitionError):
                ct.transition(x, "running")

    def test_success_only_through_mark_succeeded_with_evidence(self):
        t = mk()
        ct.transition(t, "running")
        with self.assertRaises(ct.TransitionError):
            ct.transition(t, "succeeded")
        with self.assertRaises(ct.EvidenceError):
            ct.mark_succeeded(t)
        t.evidence = [ct.Evidence(id="x", kind="test_output", command="pytest test_x.py -q", exit_code=1, output_tail="F")]
        with self.assertRaises(ct.EvidenceError):
            ct.mark_succeeded(t)
        t.evidence = [mkev(t)]
        ct.mark_succeeded(t)
        self.assertEqual(t.status, "succeeded")

    def test_a_claim_in_text_is_not_evidence(self):
        t = mk(context_summary="all tests passed", error=None)
        ct.transition(t, "running")
        with self.assertRaises(ct.EvidenceError):
            ct.mark_succeeded(t)

    def test_escalated_returns_to_running_only_via_transition(self):
        t = mk()
        ct.transition(t, "running")
        ct.transition(t, "escalated")
        ct.transition(t, "running")
        self.assertEqual(t.status, "running")

    def test_review_evidence_rules(self):
        t = mk(role="executor", verification={"type": "review", "commands": [], "evidence_required": True})
        src = ct.Evidence(id="s", kind="test_output", command="pytest a.py", exit_code=0, output_tail="ok")
        t.evidence = [src, ct.Evidence(id="r", kind="review", reviewer_role="executor", source_id="s")]
        self.assertEqual(ct.evidence_gaps(t, lambda i: src if i == "s" else None), [0])  # own work
        t.evidence = [ct.Evidence(id="r", kind="review", reviewer_role="reviewer", source_id="s")]
        self.assertEqual(ct.evidence_gaps(t), [0])  # no recorded command result
        self.assertEqual(ct.evidence_gaps(t, lambda i: src if i == "s" else None), [])
        t.evidence = [ct.Evidence(id="a", kind="approval", approval_id="not-a-uuid")]
        self.assertEqual(ct.evidence_gaps(t), [0])
        t.evidence = [ct.Evidence(id="a", kind="approval", approval_id="11111111-2222-3333-4444-555555555555")]
        self.assertEqual(ct.evidence_gaps(t), [])


class VerifyAllowlistTests(unittest.TestCase):
    def test_allowed_forms(self):
        for c in ("pytest", "pytest test_main.py -q", "pytest test_main.py::Cls::test_a -x --tb=short",
                  'pytest -k "steps and tool" -q', "python -m pytest test_a.py --maxfail=2",
                  "python3 -m unittest -v test_main.RouterLLMTests", "pytest -p no:cacheprovider test_a.py"):
            argv = ct.parse_verify_command(c)
            self.assertEqual(argv[1], "-m")
            self.assertIn(argv[2], ("pytest", "unittest"))

    def test_everything_else_is_refused(self):
        for c in ("ls -la", "rm -rf /", "sh -c 'id'", "pytest; id", "pytest && id", "pytest | cat", "pytest $(id)",
                  "pytest `id`", "pytest > out", "python -c 'print(1)'", "python script.py", "python -m pip install x",
                  "pytest /etc/passwd", "pytest ../x.py", "pytest -p evil_plugin", "pytest --rootdir=/",
                  "pytest -c evil.ini", "pytest conftest", "pytest x.py -k 'a;b'", "pytest -k", "curl http://x",
                  "python -m unittest -p x", "npm test", "pytest x.py\nid", "", "   ", "pytest 'unterminated"):
            with self.assertRaises(ct.VerifyRefused, msg=c):
                ct.parse_verify_command(c)

    def test_refused_outside_the_container_and_never_executes(self):
        r = ct.VerificationRunner(require_container=True, exec_enabled=True)
        with mock.patch.object(ct, "in_container", return_value=False), mock.patch.object(ct.subprocess, "Popen") as p:
            with self.assertRaises(ct.VerifyRefused):
                r.run("pytest test_x.py")
        p.assert_not_called()

    def test_default_runner_never_executes_anything_and_needs_taisun(self):
        env = {k: v for k, v in os.environ.items() if k not in ("CREW_VERIFY_EXEC", "CREW_VERIFY_NO_NETWORK")}
        with mock.patch.dict(os.environ, env, clear=True):
            r = ct.VerificationRunner(require_container=False)
            self.assertFalse(r.exec_enabled)
            r.check("pytest test_x.py -q")  # a plan can still be validated
            with mock.patch.object(ct.subprocess, "Popen") as p:
                with self.assertRaises(ct.NeedsApproval):
                    r.run("pytest test_x.py -q")
            p.assert_not_called()

    def test_exec_needs_both_operator_switches(self):
        for env, want in (({}, False), ({"CREW_VERIFY_EXEC": "1"}, False), ({"CREW_VERIFY_NO_NETWORK": "1"}, False),
                          ({"CREW_VERIFY_EXEC": "1", "CREW_VERIFY_NO_NETWORK": "1"}, True)):
            with mock.patch.dict(os.environ, env, clear=True):
                self.assertEqual(ct.verify_exec_allowed(), want)
        self.assertNotIn("CREW_VERIFY_EXEC", open(os.path.join(os.path.dirname(ct.__file__), "Dockerfile")).read() if os.path.exists(os.path.join(os.path.dirname(ct.__file__), "Dockerfile")) else "")

    def test_refused_command_never_spawns(self):
        r = ct.VerificationRunner(require_container=False)
        with mock.patch.object(ct.subprocess, "Popen") as p:
            with self.assertRaises(ct.VerifyRefused):
                r.run("ls")
        p.assert_not_called()

    def run_in(self, files, cmd, **kw):
        with tempfile.TemporaryDirectory() as d:
            for name, body in files.items():
                with open(os.path.join(d, name), "w") as f:
                    f.write(body)
            return ct.VerificationRunner(cwd=d, require_container=False, exec_enabled=True, **kw).run(cmd)

    def test_real_pass_and_fail(self):
        ok = self.run_in({"test_a.py": "def test_a():\n    assert True\n"}, "pytest test_a.py -q -p no:cacheprovider")
        self.assertTrue(ok.ok, ok.output_tail)
        bad = self.run_in({"test_a.py": "def test_a():\n    assert False\n"}, "pytest test_a.py -q -p no:cacheprovider")
        self.assertFalse(bad.ok)
        self.assertEqual(bad.exit_code, 1)
        un = self.run_in({"test_u.py": "import unittest\nclass T(unittest.TestCase):\n    def test_x(self): pass\n"},
                         "python -m unittest test_u")
        self.assertTrue(un.ok, un.output_tail)

    def test_env_is_scrubbed_and_output_redacted(self):
        body = ("import os\ndef test_env():\n    assert 'CREW_SERVICE_TOKEN' not in os.environ\n"
                "    assert 'POSTGRES_PASSWORD' not in os.environ\n"
                "def test_leak():\n    print('token=' + 'a'*40)\n    assert False\n")
        with mock.patch.dict(os.environ, {"CREW_SERVICE_TOKEN": "tok", "POSTGRES_PASSWORD": "pw"}):
            r = self.run_in({"test_e.py": body}, "pytest test_e.py -q -p no:cacheprovider")
        self.assertNotIn("a" * 32, r.output_tail)
        self.assertIn("1 failed, 1 passed", r.output_tail)

    def test_timeout_kills_and_is_not_ok(self):
        r = self.run_in({"test_s.py": "import time\ndef test_s():\n    time.sleep(30)\n"},
                        "pytest test_s.py -q -p no:cacheprovider", timeout_s=1.5)
        self.assertTrue(r.timed_out)
        self.assertFalse(r.ok)


class FakeVerifier(ct.VerificationRunner):
    def __init__(self, results):
        super().__init__(require_container=False)
        self.results, self.ran = list(results), []

    def run(self, command):
        self.ran.append(command)
        code = self.results.pop(0) if self.results else 0
        return ct.VerifyResult(command, code, "1 passed" if code == 0 else "1 failed", 5)


class FakeRunners(ct.Runners):
    def __init__(self, review_ok=True, approve=None, execute_raises=None):
        self.executed, self.feedbacks, self.review_ok = [], [], review_ok
        self.approve, self.execute_raises = approve, list(execute_raises or [])

    def execute(self, task, feedback):
        self.executed.append((task.task_id, task.tier))
        self.feedbacks.append(feedback)
        if self.execute_raises:
            raise self.execute_raises.pop(0)
        return ct.ExecResult("out", 10, 5, 1)

    def review(self, task, output):
        return ct.ReviewResult(self.review_ok, "reviewer", "looks right")

    def request_approval(self, task):
        return self.approve


class Approver:
    def __init__(self, answer):
        self.answer, self.asked = answer, []

    def request(self, kind, items):
        self.asked.append((kind, items))
        return self.answer


def task(tid, tier="fast", role="researcher", inputs=(), cmds=("pytest test_x.py -q",), vtype="tests", **kw):
    return ct.CrewTask(task_id=tid, run_id="r1", role=role, tier=tier, objective=f"do {tid}",
                       deliverables=["d"], acceptance_criteria=["c1", "c2"], inputs=list(inputs),
                       verification={"type": vtype, "commands": list(cmds), "evidence_required": True}, **kw)


class LoopTests(unittest.TestCase):
    def loop(self, runners=None, results=(), approver=None):
        self.store = ct.InMemoryTaskStore()
        self.v = FakeVerifier(results)
        self.runners = runners or FakeRunners()
        return ct.TaskLoop(self.runners, self.store, self.v, approver, sleep=lambda s: None)

    def test_research_on_fast_then_review_on_deep_in_that_order(self):
        research = task("A", "fast", "researcher")
        review = task("B", "deep", "reviewer", inputs=["A"], cmds=("pytest test_review.py -q",))
        res = self.loop().run(tasks=[review, research])  # plan order must not matter
        self.assertEqual(res.status, "completed")
        self.assertEqual(self.runners.executed, [("A", "fast"), ("B", "deep")])
        self.assertEqual([t.status for t in res.tasks], ["succeeded", "succeeded"])
        phases = [p.value for _, p, _ in res.log]
        self.assertEqual(phases[0], "PLAN")
        self.assertEqual(phases[-1], "DOCUMENT")
        for p in ("DELEGATE", "EXECUTE", "TEST", "VERIFY"):
            self.assertIn(p, phases)
        for t in res.tasks:
            self.assertEqual(ct.evidence_gaps(t), [])
        self.assertEqual(self.store.load_run("r1")[0].status, "succeeded")
        self.assertEqual(res.tasks[1].tokens.prompt, 10)  # totals recorded

    def test_batches_by_tier(self):
        ts = [task("D1", "deep"), task("F1", "fast"), task("F2", "fast"), task("D2", "deep"), task("F3", "fast", inputs=["D1"])]
        self.loop().run(tasks=ts)
        self.assertEqual([x[0] for x in self.runners.executed], ["F1", "F2", "D1", "D2", "F3"][:2] + ["D1", "D2", "F3"])
        self.assertEqual([x[1] for x in self.runners.executed], ["fast", "fast", "deep", "deep", "fast"])

    def test_failing_verification_fixes_then_escalates(self):
        t = task("A")
        approver = None
        res = self.loop(results=[1, 1, 1]).run(tasks=[t])
        self.assertEqual(res.status, "escalated")
        self.assertEqual(t.status, "escalated")
        self.assertEqual(t.retry_count, 2)
        self.assertEqual(len(self.runners.executed), 3)  # first try + 2 FIX cycles
        self.assertEqual(sum(1 for _, p, _ in res.log if p == ct.Phase.FIX), 2)
        self.assertIsNone(t.evidence and [e for e in t.evidence if e.exit_code == 0] or None)
        self.assertIn("verification failed", t.error)
        self.assertEqual(self.runners.feedbacks[0], None)
        self.assertIn("verification failed", self.runners.feedbacks[1])
        self.assertEqual(approver, None)

    def test_escalation_asks_the_retry_limit_approval_and_deny_cancels(self):
        t, nxt = task("A"), task("B", inputs=["A"])
        ap = Approver(False)
        res = self.loop(results=[1, 1, 1], approver=ap).run(tasks=[t, nxt])
        self.assertEqual(ap.asked[0][0], "retry_limit")
        self.assertIn(("A", "escalated"), res.history)
        self.assertEqual((t.status, nxt.status, res.status), ("cancelled", "pending", "cancelled"))

    def test_approval_buys_one_more_attempt_then_it_can_succeed(self):
        t = task("A")
        ap = Approver(True)
        res = self.loop(results=[1, 1, 1, 0], approver=ap).run(tasks=[t])
        self.assertEqual(res.status, "completed")
        self.assertEqual(t.status, "succeeded")
        self.assertEqual(len(ap.asked), 1)
        self.assertEqual(len(self.runners.executed), 4)

    def test_fix_can_succeed_inside_the_limit(self):
        t = task("A")
        res = self.loop(results=[1, 0]).run(tasks=[t])
        self.assertEqual((res.status, t.retry_count), ("completed", 1))

    def test_no_evidence_cannot_succeed(self):
        # a review task with no recorded command result and no human approval
        t = task("A", role="executor", cmds=(), vtype="review")
        res = self.loop(runners=FakeRunners(approve=None)).run(tasks=[t])
        self.assertNotEqual(t.status, "succeeded")
        self.assertEqual(t.status, "cancelled")
        self.assertEqual(res.status, "cancelled")

    def test_review_without_command_result_needs_an_approval_id(self):
        t = task("A", role="executor", cmds=(), vtype="review")
        res = self.loop(runners=FakeRunners(approve="11111111-2222-3333-4444-555555555555")).run(tasks=[t])
        self.assertEqual(res.status, "completed")
        self.assertEqual([e.kind for e in t.evidence], ["approval"])

    def test_review_on_top_of_a_recorded_test_run_succeeds(self):
        t = task("A", role="executor", vtype="review")
        res = self.loop().run(tasks=[t])
        self.assertEqual(res.status, "completed")
        self.assertEqual(sorted(e.kind for e in t.evidence), ["review", "test_output"])

    def test_rejected_review_is_a_fix_not_a_success(self):
        t = task("A", role="executor", vtype="review")
        res = self.loop(runners=FakeRunners(review_ok=False)).run(tasks=[t])
        self.assertEqual(res.status, "escalated")
        self.assertIn("review rejected", t.error)

    def test_refused_verification_command_rejects_the_plan(self):
        t = task("A", cmds=("rm -rf /",))
        res = self.loop().run(tasks=[t])
        self.assertEqual(res.status, "failed")
        self.assertEqual(self.runners.executed, [])
        self.assertIn("plan rejected", res.history[-1][1])

    def test_verify_refused_at_run_time_fails_without_fix(self):
        t = task("A")
        lp = self.loop()
        self.v.run = mock.Mock(side_effect=ct.VerifyRefused("verification runs only inside the container"))
        res = lp.run(tasks=[t])
        self.assertEqual((t.status, res.status, t.retry_count), ("failed", "failed", 0))
        self.assertIn("verification blocked", t.error)

    def test_deep_unavailable_waits_once_then_fails_honestly(self):
        t = task("A", "deep", "reviewer")
        sleeps = []
        lp = self.loop(runners=FakeRunners(execute_raises=[ct.TierUnavailable(), ct.TierUnavailable()]))
        lp.sleep = sleeps.append
        res = lp.run(tasks=[t])
        self.assertEqual((t.status, res.status), ("failed", "failed"))
        self.assertEqual(len(sleeps), 1)
        self.assertIn("deep tier unavailable", t.error)
        lp = self.loop(runners=FakeRunners(execute_raises=[ct.TierUnavailable()]))
        self.assertEqual(lp.run(tasks=[task("B", "deep", "reviewer")]).status, "completed")

    def test_when_nothing_may_execute_the_task_waits_for_taisun_and_succeeds_only_on_approval(self):
        aid = "11111111-2222-3333-4444-555555555555"
        t = task("A")
        store = ct.InMemoryTaskStore()
        runners = FakeRunners(approve=aid)
        lp = ct.TaskLoop(runners, store, ct.VerificationRunner(require_container=False, exec_enabled=False), None, sleep=lambda s: None)
        with mock.patch.object(ct.subprocess, "Popen") as p:
            res = lp.run(tasks=[t])
        p.assert_not_called()
        self.assertEqual((res.status, t.status), ("completed", "succeeded"))
        self.assertEqual([e.kind for e in t.evidence], ["approval"])  # never a test_output: nothing ran
        self.assertIn(("A", "awaiting_approval"), res.history)
        # denied: cancelled, never succeeded
        t2 = task("B")
        lp = ct.TaskLoop(FakeRunners(approve=None), ct.InMemoryTaskStore(), ct.VerificationRunner(require_container=False, exec_enabled=False), None)
        res = lp.run(tasks=[t2])
        self.assertEqual((t2.status, res.status), ("cancelled", "cancelled"))
        self.assertIsNone(t2.evidence)

    def test_plan_validation(self):
        v = FakeVerifier([])
        with self.assertRaises(ct.PlanError):
            ct.validate_plan([], v)
        with self.assertRaises(ct.PlanError):
            ct.validate_plan([task("A"), task("A")], v)
        with self.assertRaises(ct.PlanError):
            ct.validate_plan([task("A", inputs=["ghost"])], v)
        with self.assertRaises(ct.PlanError):
            ct.validate_plan([task("A", inputs=["B"]), task("B", inputs=["A"])], v)
        ct.validate_plan([task("A"), task("B", inputs=["A"])], v)

    def test_planner_runner_is_used_when_no_tasks_given(self):
        class P(FakeRunners):
            def plan(self, goal, run_id):
                return [task("A")]
        res = self.loop(runners=P()).run(goal="g", run_id="r1")
        self.assertEqual(res.status, "completed")


class _Cur:
    def __init__(self, rows=()):
        self.rows = rows

    def fetchall(self):
        return self.rows


class _Conn:
    def __init__(self, log, rows=()):
        self.log, self.rows = log, rows

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def execute(self, sql, params=()):
        self.log.append((sql, params))
        return _Cur(self.rows)


class PgStoreTests(unittest.TestCase):
    def test_save_upserts_a_json_doc_and_load_revalidates(self):
        log = []
        t = task("A")
        ct.PgTaskStore(lambda: _Conn(log)).save(t)
        sql, params = log[0]
        self.assertIn("ON CONFLICT (task_id)", sql)
        self.assertEqual(params[0], "A")
        import json
        self.assertEqual(json.loads(params[5])["$id"], "amara.crew.task.v1")
        loaded = ct.PgTaskStore(lambda: _Conn([], rows=[(json.loads(params[5]),)])).load_run("r1")
        self.assertEqual(loaded[0].task_id, "A")
        forged = json.loads(params[5])
        forged["status"] = "succeeded"
        with self.assertRaises(ValueError):
            ct.PgTaskStore(lambda: _Conn([], rows=[(forged,)])).load_run("r1")

    def test_schema_creates_the_table(self):
        self.assertIn("CREATE TABLE IF NOT EXISTS crew_tasks", ct.TASK_SCHEMA)


if __name__ == "__main__":
    unittest.main()

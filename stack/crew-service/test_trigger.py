"""R13-F tests: the signed trigger endpoint. Unauthenticated = 403, fail closed."""

import asyncio
import json
import time
import unittest
from unittest import mock

from fastapi import HTTPException
from starlette.requests import Request

import main

SECRET = "trigger-secret-for-tests-only"
BODY = json.dumps({"name": "n8n research", "task": "summarise the kb"}).encode()


def req(body: bytes) -> Request:
    sent = {"done": False}

    async def receive():
        if sent["done"]:
            return {"type": "http.disconnect"}
        sent["done"] = True
        return {"type": "http.request", "body": body, "more_body": False}

    return Request({"type": "http", "method": "POST", "path": "/crews/trigger", "headers": []}, receive)


def call(body=BODY, ts=None, sig=None, secret=SECRET):
    ts = str(int(time.time())) if ts is None else ts
    if sig is None:
        sig = main.trigger_signature(SECRET, ts, body)
    with mock.patch.object(main, "CREW_TRIGGER_SECRET", secret), \
            mock.patch.object(main, "_launch", return_value=main.CrewInfo(id="i", name="n", status="running", agents=["researcher"])) as launch:
        try:
            out = asyncio.run(main.trigger_crew(req(body), ts, sig))
        except HTTPException as e:
            return e.status_code, launch
        return out, launch


class TriggerTests(unittest.TestCase):
    def setUp(self):
        main._trigger_seen.clear()

    def test_unauthenticated_is_403_and_never_launches(self):
        for kwargs in (dict(ts="", sig=""), dict(sig="sha256=" + "0" * 64), dict(sig="garbage"),
                       dict(ts=str(int(time.time()) - 4000)), dict(ts="12x"), dict(secret="")):
            code, launch = call(**kwargs)
            self.assertEqual(code, 403, kwargs)
            launch.assert_not_called()

    def test_unset_secret_disables_the_endpoint_even_with_a_matching_empty_signature(self):
        ts = str(int(time.time()))
        sig = main.trigger_signature("", ts, BODY)
        code, launch = call(ts=ts, sig=sig, secret="")
        self.assertEqual(code, 403)
        launch.assert_not_called()

    def test_wrong_secret_and_tampered_body_are_403(self):
        ts = str(int(time.time()))
        bad = main.trigger_signature("other-secret", ts, BODY)
        self.assertEqual(call(ts=ts, sig=bad)[0], 403)
        good = main.trigger_signature(SECRET, ts, BODY)
        self.assertEqual(call(body=BODY + b" ", ts=ts, sig=good)[0], 403)

    def test_signed_trigger_starts_a_run(self):
        out, launch = call()
        self.assertEqual(out.id, "i")
        # last arg False: a triggered run never claims or uses a files/terminal grant
        launch.assert_called_once_with("n8n research", "summarise the kb", main.DEFAULT_AGENTS, None, False)

    def test_replay_is_403(self):
        ts = str(int(time.time()))
        sig = main.trigger_signature(SECRET, ts, BODY)
        self.assertEqual(call(ts=ts, sig=sig)[0].id, "i")
        self.assertEqual(call(ts=ts, sig=sig)[0], 403)

    def test_replay_with_whitespace_around_the_signature_is_403(self):
        ts = str(int(time.time()))
        sig = main.trigger_signature(SECRET, ts, BODY)
        self.assertEqual(call(ts=ts, sig=sig)[0].id, "i")
        for variant in (sig + " ", " " + sig, sig + "\t"):
            code, launch = call(ts=ts, sig=variant)
            self.assertEqual(code, 403, repr(variant))
            launch.assert_not_called()

    def test_triggered_run_gets_no_grant_context_and_claims_no_run_grant(self):
        class Conn:
            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

            def execute(self, sql, params=()):
                return self

        ctx = mock.Mock()
        ctx.Pipe.return_value = (mock.Mock(), mock.Mock())
        svc = mock.Mock()
        info = main.CrewInfo(id="x", name="n", status="running", agents=["researcher"])
        for allowed, want_ctx in ((False, None), (True, {"crew": "n", "run_id": "RID"})):
            svc.reset_mock()
            ctx.Process.reset_mock()
            with mock.patch.object(main, "db", return_value=Conn()), mock.patch.object(main, "_ctx", ctx), \
                    mock.patch.object(main, "CREW_SANDBOX_ROOT", "/sandbox"), \
                    mock.patch.object(main, "grant_service", return_value=svc), \
                    mock.patch.object(main.uuid, "uuid4", return_value="RID"), \
                    mock.patch.object(main.threading, "Thread"), mock.patch.object(main, "db_get", return_value=info):
                main._procs.clear()
                main._launch("n", "t", ["executor"], None, allowed)
                main._procs.clear()
                main._resp_pipes.clear()
            self.assertEqual(ctx.Process.call_args.kwargs["kwargs"]["grant_ctx"], want_ctx)
            self.assertEqual(svc.store.claim_run.called, allowed)

    def test_body_is_validated_like_start_and_cannot_link_a_retry(self):
        for body in (b"{}", b"not json", json.dumps({"name": "x", "task": "t", "retry_of": "11111111-2222-3333-4444-555555555555"}).encode(),
                     json.dumps({"name": "bad/name", "task": "t"}).encode(),
                     json.dumps({"name": "x", "task": "t", "agents": ["root"]}).encode(),
                     json.dumps({"name": "x", "task": "t", "extra": 1}).encode()):
            code, launch = call(body=body)
            self.assertEqual(code, 422, body)
            launch.assert_not_called()

    def test_oversized_body_is_403(self):
        self.assertEqual(call(body=b"x" * (main.TRIGGER_MAX_BODY + 1))[0], 403)

    def test_comparison_is_bytes_constant_time(self):
        with mock.patch.object(main.hmac, "compare_digest", wraps=main.hmac.compare_digest) as cd:
            call()
        args = cd.call_args[0]
        self.assertTrue(all(isinstance(a, bytes) for a in args))

    def test_route_uses_the_signature_not_the_crew_token(self):
        route = next(r for r in main.app.routes if getattr(r, "path", "") == "/crews/trigger")
        self.assertFalse(any(d.call is main.require_token for d in route.dependant.dependencies))
        self.assertIn("POST", route.methods)


if __name__ == "__main__":
    unittest.main()

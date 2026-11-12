"""1110i tests: the per-crew Files / terminal grant, enforced fail closed."""

import json
import os
import shutil
import tempfile
import threading
import time
import unittest
from unittest import mock

import crew_grants as cg
import main

SECRET = "sk-" + "Zq8Wx7Ev" * 6
HAVE_GIT = shutil.which("git") is not None


class Clock:
    def __init__(self, t=1_000_000.0):
        self.t = t

    def __call__(self):
        return self.t


class Env:
    """A sandbox root, a service with in-memory stores and a tool context for crew 'Research'."""

    def __init__(self, tc: unittest.TestCase, crew="Research", run_id="run-1"):
        self.tmp = tempfile.TemporaryDirectory()
        tc.addCleanup(self.tmp.cleanup)
        self.root = os.path.realpath(self.tmp.name)
        self.clock = Clock()
        self.store, self.audit = cg.MemoryGrantStore(), cg.MemoryAudit()
        self.svc = cg.GrantService(self.store, self.audit, self.root, lambda: frozenset({"kb_query"}), self.clock)
        self.crew, self.run_id = crew, run_id
        self.ctx = cg.ToolContext(crew=crew, run_id=run_id, service=self.svc, checkpoint=lambda scope: "abc1234", poll_s=0.05)
        cg.set_context(self.ctx)
        tc.addCleanup(cg.set_context, None)

    def grant(self, tools=("fs_read", "fs_write", "shell"), duration="1h"):
        st = self.svc.set(self.crew, list(tools), duration, "taisun")
        if duration == "run":
            self.store.claim_run(self.crew, self.run_id, self.clock())
        return st

    @property
    def scope(self):
        return os.path.join(self.root, cg.slug(self.crew))

    def kinds(self):
        return [r.kind for r in self.audit.rows]


class DefaultOffTests(unittest.TestCase):
    def test_no_grant_rejects_every_tool_even_if_a_role_names_it(self):
        env = Env(self)
        for fn, arg in ((cg.fs_read, "a.txt"), (cg.fs_write, json.dumps({"path": "a", "content": "x"})), (cg.shell, "ls")):
            self.assertIn("(not_granted)", fn(arg))
        for name in cg.GRANT_TOOLS:
            self.assertIn("not_granted", cg.precheck(name)(""))
        self.assertEqual(env.kinds().count("blocked"), 6)
        self.assertFalse(os.path.exists(env.scope))

    def test_no_context_is_off(self):
        cg.set_context(None)
        self.assertIn("(not_granted)", cg.shell("ls"))

    def test_state_is_off_by_default_and_lists_the_read_only_allowlist(self):
        env = Env(self)
        st = env.svc.state("Research")
        self.assertEqual((st["enabled"], st["tools"], st["enforced_allowlist"]), (False, [], ["kb_query"]))

    def test_a_grant_for_one_tool_does_not_open_the_others(self):
        env = Env(self)
        env.grant(tools=("fs_read",))
        self.assertIn("(not_granted)", cg.shell("ls"))
        self.assertNotIn("not_granted", cg.fs_read("nothing.txt"))  # path error, not a grant error

    def test_another_crews_grant_does_not_apply(self):
        env = Env(self)
        env.svc.set("Other", ["shell"], "1h", "taisun")
        self.assertIn("(not_granted)", cg.shell("ls"))


class LifecycleTests(unittest.TestCase):
    def test_expired_grant_is_rejected_without_a_restart(self):
        env = Env(self)
        env.grant(duration="1h")
        self.assertEqual(cg.shell("echo hi").splitlines()[0], "exit 0")
        env.clock.t += 3601
        self.assertIn("(not_granted)", cg.shell("echo hi"))
        self.assertEqual(env.svc.state("Research")["enabled"], False)

    def test_24h_is_the_cap_and_the_sweeper_records_expiry(self):
        env = Env(self)
        st = env.grant(duration="24h")
        self.assertEqual(st["expires_at"] - env.clock.t, 86400)
        env.clock.t += 86401
        self.assertEqual(env.svc.sweep(), 1)
        self.assertEqual(env.audit.rows[-1].action, "grant expired")
        self.assertEqual(env.svc.sweep(), 0)

    def test_there_is_no_until_i_turn_it_off_duration(self):
        env = Env(self)
        for d in ("forever", "until_off", "48h", ""):
            with self.assertRaises(cg.GrantError):
                env.svc.set("Research", ["shell"], d, "taisun")

    def test_run_grant_belongs_to_one_run_and_ends_with_it(self):
        env = Env(self)
        env.svc.set("Research", ["shell"], "run", "taisun")
        self.assertIn("(not_granted)", cg.shell("echo hi"))  # armed, but no run has claimed it
        self.assertTrue(env.store.claim_run("Research", "run-1", env.clock()))
        self.assertEqual(cg.shell("echo hi").splitlines()[0], "exit 0")
        env.ctx.run_id = "run-2"
        self.assertIn("(not_granted)", cg.shell("echo hi"))  # a different run
        env.ctx.run_id = "run-1"
        self.assertEqual(env.store.end_run("run-1", env.clock()), ["Research"])
        self.assertIn("(not_granted)", cg.shell("echo hi"))

    def test_default_duration_is_this_run_only(self):
        self.assertEqual(cg.DEFAULT_DURATION, "run")
        self.assertEqual(main.clean_grant_payload({"tools": ["shell"]})["duration"], "run")
        self.assertEqual(main.clean_grant_payload({"tools": ["shell"], "duration": "forever"})["duration"], "run")

    def test_only_taisun_can_create_a_grant_and_a_crew_attempt_is_audited(self):
        env = Env(self)
        for actor in ("crew:executor", "executor", "", "Taisun", "system"):
            with self.assertRaises(PermissionError):
                env.svc.set("Research", ["shell"], "1h", actor)
        self.assertEqual(env.svc.state("Research")["enabled"], False)
        self.assertEqual(env.kinds().count("blocked"), 5)

    def test_endpoint_refuses_a_non_taisun_actor_with_403(self):
        from fastapi import HTTPException
        env = Env(self)
        with mock.patch.object(main, "_GRANT_SERVICE", env.svc):
            with self.assertRaises(HTTPException) as cm:
                main.set_tool_grant("Research", main.GrantSetRequest(tools=["shell"], granted_by="crew"))
            self.assertEqual(cm.exception.status_code, 403)
            st = main.set_tool_grant("Research", main.GrantSetRequest(tools=["shell"], duration="1h", granted_by="taisun"))
            self.assertEqual(st["enabled"], True)
            with self.assertRaises(HTTPException) as cm:
                main.set_tool_grant("Research", main.GrantSetRequest(tools=["sudo"], granted_by="taisun"))
            self.assertEqual(cm.exception.status_code, 422)
            self.assertEqual(main.revoke_tool_grant("Research")["enabled"], False)

    def test_sandbox_not_mounted_fails_closed(self):
        svc = cg.GrantService(cg.MemoryGrantStore(), cg.MemoryAudit(), "/nonexistent-root-xyz")
        with self.assertRaises(cg.GrantError):
            svc.set("Research", ["shell"], "1h", "taisun")
        self.assertFalse(svc.state("Research")["enabled"])
        with self.assertRaises(cg.GrantError):
            cg.GrantService(cg.MemoryGrantStore(), cg.MemoryAudit(), "").set("Research", ["shell"], "1h", "taisun")

    def test_replacing_and_revoking_are_audited(self):
        env = Env(self)
        env.grant()
        env.grant(tools=("fs_read",))
        env.svc.revoke("Research")
        self.assertEqual(env.kinds(), ["grant", "confirm", "revoke", "grant", "confirm", "revoke"])
        self.assertEqual(env.svc.state("Research")["enabled"], False)

    def test_confirm_reports_the_enforced_allowlist(self):
        env = Env(self)
        st = env.grant(tools=("fs_read", "shell"))
        self.assertEqual(st["enforced_allowlist"], ["fs_read", "kb_query", "shell"])
        self.assertEqual(st["granted_by"], "taisun")


class ScopeTests(unittest.TestCase):
    def test_scope_is_the_crew_folder_and_must_stay_inside(self):
        root = "/crew-sandbox"
        self.assertEqual(cg.resolve_scope(root, "Research"), "/crew-sandbox/research")
        self.assertEqual(cg.resolve_scope(root, "Research", "~/crew-sandbox/research/latest-run"), "/crew-sandbox/research/latest-run")
        for bad in ("../other", "/etc", "/crew-sandbox/other", "~/crew-sandbox/research/../x"):
            with self.assertRaises(cg.GrantError, msg=bad):
                cg.resolve_scope(root, "Research", bad)

    def test_path_escape_and_symlink_escape_are_rejected_and_audited_as_blocked(self):
        env = Env(self)
        env.grant()
        outside = os.path.join(env.root, "outside.txt")
        open(outside, "w").write("top secret")
        os.symlink(outside, os.path.join(env.scope, "link.txt"))
        os.symlink(env.root, os.path.join(env.scope, "linkdir"))
        for arg in ("../outside.txt", "/etc/passwd", "link.txt", "linkdir/outside.txt", "sub/../../outside.txt"):
            out = cg.fs_read(arg)
            self.assertIn("(path)", out, arg)
            self.assertNotIn("top secret", out)
        for rel in ("../x.txt", "link.txt", "linkdir/new.txt", ".git/config"):
            self.assertIn("Tool error", cg.fs_write(json.dumps({"path": rel, "content": "x"})), rel)
        self.assertEqual(open(outside).read(), "top secret")
        self.assertFalse(os.path.exists(os.path.join(env.root, "x.txt")))
        self.assertGreaterEqual(env.kinds().count("blocked"), 8)

    def test_fs_read_inside_works_and_redacts_secrets(self):
        env = Env(self)
        env.grant()
        open(os.path.join(env.scope, "n.txt"), "w").write(f"hello\ntoken={SECRET}\n")
        out = cg.fs_read("n.txt")
        self.assertIn("hello", out)
        self.assertNotIn(SECRET, out)


class WriteTests(unittest.TestCase):
    def test_write_is_atomic_inside_scope_with_a_checkpoint_hash_in_the_audit(self):
        env = Env(self)
        env.grant()
        out = cg.fs_write(json.dumps({"path": "notes/a.txt", "content": "hello"}))
        self.assertEqual(out, "wrote notes/a.txt (5 chars)")
        self.assertEqual(open(os.path.join(env.scope, "notes", "a.txt")).read(), "hello")
        row = [r for r in env.audit.rows if r.kind == "fs_write"][0]
        self.assertEqual((row.checkpoint, row.target), ("abc1234", "notes/a.txt"))
        self.assertNotIn("hello", json.dumps(row.__dict__))  # no content in the audit

    def test_checkpoint_taken_once_per_run_and_failure_refuses_the_write(self):
        env = Env(self)
        env.grant()
        calls = []
        env.ctx.checkpoint = lambda scope: calls.append(scope) or "h1"
        cg.fs_write(json.dumps({"path": "a", "content": "1"}))
        cg.fs_write(json.dumps({"path": "b", "content": "2"}))
        self.assertEqual(len(calls), 1)
        env2 = Env(self, crew="Second", run_id="r2")
        env2.grant()
        def boom(scope):
            raise cg.CheckpointError("no git")
        env2.ctx.checkpoint = boom
        out = cg.fs_write(json.dumps({"path": "a", "content": "1"}))
        self.assertIn("checkpoint", out)
        self.assertFalse(os.path.exists(os.path.join(env2.scope, "a")))

    def test_bad_arguments_and_size_cap(self):
        env = Env(self)
        env.grant()
        self.assertIn("(syntax)", cg.fs_write("not json"))
        self.assertIn("(syntax)", cg.fs_write(json.dumps({"path": 1, "content": "x"})))
        self.assertIn("(size)", cg.fs_write(json.dumps({"path": "a", "content": "x" * (cg.FS_WRITE_MAX_BYTES + 1)})))

    @unittest.skipUnless(HAVE_GIT, "git is in the crew image")
    def test_real_git_checkpoint_commits_prior_state(self):
        d = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, d, True)
        open(os.path.join(d, "f.txt"), "w").write("v1")
        h = cg.git_checkpoint(d)
        self.assertRegex(h, r"^[0-9a-f]{7,}$")
        open(os.path.join(d, "f.txt"), "w").write("v2")
        h2 = cg.git_checkpoint(d)
        self.assertNotEqual(h, h2)

    def test_git_control_files_are_refused_in_any_case_or_unicode_spelling(self):
        env = Env(self)
        env.grant()
        os.makedirs(os.path.join(env.scope, ".git"), exist_ok=True)
        for path in (".GIT/config", ".Git/hooks/pre-commit", "sub/.GiT/config", ".git\u200c/config", "\uff0egit/config",
                     ".gitattributes", ".GITATTRIBUTES", "d/.gitattributes", ".gitmodules", ".git./config"):
            out = cg.fs_write(json.dumps({"path": path, "content": "[filter \"x\"]\n\tclean = touch PWNED"}))
            self.assertTrue(out.startswith("Tool error"), f"{path!r}: {out}")
            with self.assertRaises(cg.Blocked, msg=path):
                cg.parse_shell(f"touch {path}", env.scope)
        self.assertFalse(os.path.exists(os.path.join(env.scope, ".git", "config")))
        self.assertTrue(cg.fs_write(json.dumps({"path": "gitnotes.txt", "content": "ok"})).startswith("wrote"))

    def test_checkpoint_ignores_in_folder_attributes_so_no_filter_driver_runs(self):
        d = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, d, True)
        self.assertIn(f"attr.tree={cg.EMPTY_TREE}", cg.GIT_HARDENING)
        cg.git_checkpoint(d)  # creates .git
        marker = os.path.join(d, "PWNED")
        with open(os.path.join(d, ".git", "config"), "a") as f:  # as if a crew had reached .git
            f.write(f"[filter \"evil\"]\n\tclean = touch {marker}\n\tsmudge = touch {marker}\n")
        open(os.path.join(d, ".gitattributes"), "w").write("* filter=evil\n")
        open(os.path.join(d, "f.txt"), "w").write("v1")
        cg.git_checkpoint(d)
        self.assertFalse(os.path.exists(marker), "a filter driver ran during the checkpoint")

    def test_write_still_needs_the_batch_approval_when_the_grant_is_on(self):
        env = Env(self)
        env.grant()
        tool = main.CrewTool("fs_write", "d", cg.fs_write, approval_kind="write", precheck=cg.precheck("fs_write"))
        self.assertEqual(main.gate_tool_call(tool, None), main.APPROVAL_DENIED)  # no approver: no
        asked = []
        class Ap:
            def allow_tool(self, t):
                asked.append(t.name)
                return True
        self.assertIsNone(main.gate_tool_call(tool, Ap()))
        self.assertEqual(asked, ["fs_write"])
        self.assertIn("fs_write", main.write_candidates([tool]))
        self.assertNotIn("fs_read", main.write_candidates([main.CrewTool("fs_read", "d", cg.fs_read)]))

    def test_precheck_runs_before_any_approval_request(self):
        Env(self)  # no grant
        asked = []
        class Ap:
            def allow_tool(self, t):
                asked.append(t.name)
                return True
        tool = main.CrewTool("shell", "d", cg.shell, precheck=cg.precheck("shell"))
        wrapped = main._to_crewai_tool(tool, None, "executor", Ap())
        self.assertIn("not_granted", wrapped._run("ls"))
        self.assertEqual(asked, [])


class ShellPolicyTests(unittest.TestCase):
    def test_always_blocked_even_with_the_grant_on(self):
        env = Env(self)
        env.grant()
        cases = {"git push origin main": "git_push", "git push": "git_push", "sudo ls": "sudo", "su root": "sudo",
                 "curl http://example.com": "network", "wget x": "network", "ssh host": "network", "pip install x": "network",
                 "nc -l 1": "network", "git fetch": "network", "git clone x": "network", "git remote add x y": "network",
                 "python -c 'import os'": "not_allowed", "python script.py": "not_allowed", "python -m pip install x": "not_allowed",
                 "ls | cat": "syntax", "ls; id": "syntax", "ls && id": "syntax", "echo $(id)": "syntax", "echo `id`": "syntax",
                 "cat > f": "syntax", "ls /etc": "path", "cat ../x": "path", "rm -rf /": "path", "cp a /tmp/b": "path", "cp -t../.. a": "path", "sort -o/etc/passwd a": "path", "ls -z": "not_allowed",
                 "bash -c id": "not_allowed", "sh": "not_allowed", "find . -exec id {}": "syntax", "find . -ok id": "not_allowed",
                 "git -c core.sshCommand=x status": "not_allowed", "env": "not_allowed", "": "syntax", "   ": "syntax"}
        for cmd, cat in cases.items():
            out = cg.shell(cmd)
            self.assertIn(f"({cat})", out, f"{cmd!r}: {out}")
        self.assertNotIn("shell", env.kinds())  # nothing ran
        self.assertEqual(env.kinds().count("blocked"), len(cases))

    def test_option_values_cannot_smuggle_a_path_out_of_the_folder(self):
        env = Env(self)
        env.grant()
        outside = os.path.join(env.root, "outside.txt")
        open(outside, "w").write("top secret")
        os.symlink(outside, os.path.join(env.scope, "link.txt"))
        os.symlink(env.root, os.path.join(env.scope, "linkdir"))
        open(os.path.join(env.scope, "a.txt"), "w").write("a")
        blocked = ["sort -o/etc/passwd a.txt", "sort -o /etc/passwd a.txt", "sort -o../../x a.txt", "cp -t../.. a.txt",
                   "cp -t ../.. a.txt", "cp -t/tmp a.txt", "cp --target-directory=/tmp a.txt", "cp --target-directory ../ a.txt",
                   "mv -t../ a.txt", "sort --output=/tmp/x a.txt", "sort --output /tmp/x a.txt", "ls -- /etc",
                   "ls -- ../outside.txt", "cat -- /etc/passwd", "rm -rf -- /tmp/x", "cat link.txt", "ls linkdir", "ls -R linkdir",
                   "cat linkdir/outside.txt", "rm link.txt", "cp link.txt b.txt", "cp a.txt linkdir/b.txt", "sort -o link.txt a.txt",
                   "grep -e x /etc/passwd", "grep x -r ..", "head -n 3 ../outside.txt", "diff a.txt /etc/hosts",
                   "git diff -- /etc/passwd", "git log -n 3 -- ../x",
                   "git -C /etc status", "mkdir -p ../x", "touch /tmp/x"]
        for cmd in blocked:
            out = cg.shell(cmd)
            self.assertTrue(out.startswith("Tool error: not allowed"), f"{cmd!r}: {out}")
        self.assertEqual(open(outside).read(), "top secret")
        self.assertFalse(os.path.exists("/tmp/x"))
        self.assertNotIn("shell", env.kinds())  # nothing ran

    def test_unknown_options_and_unsafe_clusters_are_refused(self):
        for cmd in ("ls -z", "ls --color=always", "ls -lz", "cat -x a", "sort -k1 a", "cp --reflink a b", "rm --preserve-root=x a",
                    "grep --include=*.py x", "head -n", "head -n abc a", "head -5 a", "sort --compress-program=sh a", "git status --porcelain=x", "git log --all", "date -d now", "date now", "echo -e hi", "pwd x", "true x"):
            with self.assertRaises(cg.Blocked, msg=cmd):
                cg.parse_shell(cmd, "/s")

    def test_known_boolean_clusters_and_inside_values_still_work(self):
        env = Env(self)
        env.grant()
        os.makedirs(os.path.join(env.scope, "d"))
        open(os.path.join(env.scope, "a.txt"), "w").write("b\na\n")
        for cmd in ("ls -la", "ls -lah d", "rm -rf d", "sort -o sorted.txt a.txt", "cp -t . a.txt", "mkdir -p x/y", "grep -in a a.txt",
                    "grep -e a a.txt", "head -n 1 a.txt", "echo -n hi", "date +%F", "ls --", "ls -- a.txt"):
            out = cg.shell(cmd)
            self.assertFalse(out.startswith("Tool error"), f"{cmd!r}: {out}")
        self.assertEqual(open(os.path.join(env.scope, "sorted.txt")).read(), "a\nb\n")
        self.assertFalse(os.path.exists(os.path.join(env.scope, "d")))

    def test_the_folder_itself_cannot_be_removed_or_moved(self):
        env = Env(self)
        for cmd in ("rm -rf .", f"rm -rf {env.scope}", "rm -r ./", "mv . elsewhere", f"mv {env.scope} x"):
            with self.assertRaises(cg.Blocked, msg=cmd):
                cg.parse_shell(cmd, env.scope)
        os.makedirs(os.path.join(env.scope, "sub"))
        self.assertEqual(cg.parse_shell("rm -r sub", env.scope)[-1], "sub")

    def test_grep_cannot_follow_symlinks_out(self):
        for cmd in ("grep -R x .", "grep -rR x .", "grep -iR x ."):
            with self.assertRaises(cg.Blocked, msg=cmd):
                cg.parse_shell(cmd, "/s")
        self.assertEqual(cg.parse_shell("grep -r x .", "/s")[1], "-r")

    def test_spending_and_outbound_are_not_representable(self):
        for prog in ("curl", "wget", "ssh", "sudo", "mail", "sendmail", "docker", "npm"):
            with self.assertRaises(cg.Blocked):
                cg.parse_shell(f"{prog} x", "/s")
        self.assertFalse({"curl", "wget", "ssh", "nc", "sudo", "pip", "docker"} & cg.SHELL_ALLOW)

    def test_allowed_commands_run_in_scope(self):
        env = Env(self)
        env.grant()
        open(os.path.join(env.scope, "t.txt"), "w").write("data")
        self.assertEqual(cg.shell("cat t.txt"), "exit 0\ndata")
        self.assertEqual(cg.shell("pwd").split("\n")[1], env.scope)

    def test_env_is_scrubbed(self):
        with mock.patch.dict(os.environ, {"CREW_SERVICE_TOKEN": "tok", "POSTGRES_PASSWORD": "pw", "CREW_ROUTER_KEY": "k"}):
            e = cg.scrub_env()
            g = cg.scrub_env(cg._GIT_ENV)
        for bad in ("CREW_SERVICE_TOKEN", "POSTGRES_PASSWORD", "CREW_ROUTER_KEY"):
            self.assertNotIn(bad, e)
            self.assertNotIn(bad, g)
        self.assertEqual((g["GIT_CONFIG_GLOBAL"], g["GIT_CONFIG_NOSYSTEM"], g["GIT_TERMINAL_PROMPT"]), ("/dev/null", "1", "0"))

    def test_interpreters_and_git_writes_are_refused(self):
        env = Env(self)
        env.grant()
        for cmd in ("python -m pytest", "python3 -m unittest", "python -m py_compile a.py", "pytest", "pytest test_x.py -q",
                    "node x.js", "sh", "bash -c id", "env", "xargs ls", "find . -name x", "find . -exec id", "awk 1 a", "sed s/a/b/ a",
                    "perl -e 1", "make", "tee x", "timeout 1 ls", "nohup ls", "time ls", "tar x", "dd if=a",
                    "git commit -m x", "git add .", "git commit -am x", "git init", "git branch x", "git show", "git checkout x",
                    "git config core.hooksPath x", "git stash", "git reset --hard", "git push", "git -c alias.x=!id x",
                    "/bin/ls", "./ls", "/usr/bin/python3 -V", "../ls"):
            out = cg.shell(cmd)
            self.assertTrue(out.startswith("Tool error: not allowed"), f"{cmd!r}: {out}")
        self.assertNotIn("shell", env.kinds())
        self.assertFalse({"python", "python3", "pytest", "sh", "bash", "env", "xargs", "find", "node", "awk", "sed"} & cg.SHELL_ALLOW)

    def test_git_is_read_only_and_hardened(self):
        argv = cg.parse_shell("git status -s", "/s")
        self.assertEqual(argv[0], "git")
        self.assertEqual(argv[-2:], ["status", "-s"])
        pairs = [(argv[i], argv[i + 1]) for i in range(1, len(argv) - 2, 2)]
        for want in ("core.hooksPath=/dev/null", "core.fsmonitor=", "core.pager=cat", "protocol.allow=never"):
            self.assertIn(("-c", want), pairs)
        for sub in ("diff", "log"):
            self.assertEqual(cg.parse_shell(f"git {sub}", "/s")[-1], sub)
        for cmd in ("git diff --ext-diff", "git diff --textconv", "git log --all", "git -C x status"):
            with self.assertRaises(cg.Blocked, msg=cmd):
                cg.parse_shell(cmd, "/s")

    def test_dot_git_paths_are_refused_everywhere(self):
        env = Env(self)
        env.grant()
        os.makedirs(os.path.join(env.scope, ".git"))
        open(os.path.join(env.scope, ".git", "config"), "w").write("[core]\n")
        os.symlink(os.path.join(env.scope, ".git"), os.path.join(env.scope, "g"))
        for cmd in ("cat .git/config", "ls .git", "ls -a .git/", "git diff -- .git/config", "cp a .git/x", "cp .git/config c",
                    "mv a .git/hooks", "rm -rf .git", "touch .git/x", "mkdir .git/hooks", "cat g/config", "ls g", "sort -o .git/x a",
                    "cp -t .git a", "head -n 1 sub/.git/x", "grep x -r .git"):
            out = cg.shell(cmd)
            self.assertTrue(out.startswith("Tool error: not allowed"), f"{cmd!r}: {out}")
        self.assertIn("(path)", cg.fs_read(".git/config"))
        self.assertIn("(path)", cg.fs_read("g/config"))
        self.assertIn("Tool error", cg.fs_write(json.dumps({"path": ".git/hooks/pre-commit", "content": "x"})))
        self.assertEqual(open(os.path.join(env.scope, ".git", "config")).read(), "[core]\n")

    def test_nonzero_exit_and_output_redaction_and_cap(self):
        env = Env(self)
        env.grant()
        open(os.path.join(env.scope, "s.txt"), "w").write(f"password=hunter2 {SECRET}\n" + "line\n" * 5000)
        out = cg.shell("cat s.txt")
        self.assertNotIn("hunter2", out)
        self.assertNotIn(SECRET, out)
        self.assertLessEqual(len(out), cg.SHELL_OUTPUT_CHARS + 20)
        self.assertTrue(cg.shell("false").startswith("exit 1"))

    def test_audit_has_target_exit_code_and_duration_but_no_output_or_secrets(self):
        env = Env(self)
        env.grant()
        open(os.path.join(env.scope, "s.txt"), "w").write(f"OUTPUT-MARKER {SECRET}\n")
        cg.shell(f"grep -c token={SECRET} s.txt")
        cg.shell("cat s.txt")
        blob = json.dumps([r.__dict__ for r in env.audit.rows])
        self.assertNotIn(SECRET, blob)
        self.assertNotIn("OUTPUT-MARKER", blob)
        rows = [r for r in env.audit.rows if r.kind == "shell"]
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[1].target, "cat s.txt")
        self.assertEqual((rows[1].exit_code, rows[0].checkpoint), (0, "abc1234"))
        self.assertIsNotNone(rows[1].duration_ms)

    def _sleeper(self, env):
        """The allowlist has no long-running program, so feed the enforcement loop a sleep."""
        procs = []
        real = cg.subprocess.Popen

        def popen(argv, **kw):
            p = real(["sleep", "60"], **kw)
            procs.append(p)
            return p
        return procs, mock.patch.object(cg, "parse_shell", return_value=["sleep", "60"]), mock.patch.object(cg.subprocess, "Popen", popen)

    def test_timeout_stops_the_command(self):
        env = Env(self)
        env.grant()
        env.ctx.shell_timeout_s = 0.6
        procs, p1, p2 = self._sleeper(env)
        t0 = time.time()
        with p1, p2:
            out = cg.shell("ls")
        self.assertIn("timed out", out)
        self.assertLess(time.time() - t0, 10)
        self.assertIsNotNone(procs[0].poll())

    def test_revoke_stops_a_running_child(self):
        env = Env(self)
        env.grant()
        procs, p1, p2 = self._sleeper(env)
        res = {}
        with p1, p2:
            th = threading.Thread(target=lambda: res.setdefault("out", cg.shell("ls")))
            th.start()
            for _ in range(50):
                if procs:
                    break
                time.sleep(0.05)
            self.assertIsNone(procs[0].poll())  # running
            env.svc.revoke("Research")
            th.join(15)
        self.assertFalse(th.is_alive())
        self.assertIn("grant ended", res["out"])
        self.assertIsNotNone(procs[0].wait(5))  # the child is gone
        self.assertIn("revoke", env.kinds())

    def test_expiry_stops_a_running_child(self):
        env = Env(self)
        env.grant(duration="1h")
        procs, p1, p2 = self._sleeper(env)
        res = {}
        with p1, p2:
            th = threading.Thread(target=lambda: res.setdefault("out", cg.shell("ls")))
            th.start()
            for _ in range(50):
                if procs:
                    break
                time.sleep(0.05)
            env.clock.t += 3601
            th.join(15)
        self.assertIn("grant ended", res["out"])
        self.assertIsNotNone(procs[0].wait(5))


class AuditStoreTests(unittest.TestCase):
    def test_audit_is_newest_first_and_paged(self):
        a = cg.MemoryAudit()
        for i in range(5):
            a.add(dict(at=i, actor="x", crew="c", run_id=None, kind="info", action=str(i)))
        self.assertEqual([r.action for r in a.list("c", 2)], ["4", "3"])
        self.assertEqual([r.action for r in a.list("c", 2, before=4)], ["2", "1"])
        self.assertEqual(a.list("other"), [])

    def test_audit_action_is_redacted(self):
        env = Env(self)
        env.svc.log("x", "Research", "info", f"token={SECRET}")
        self.assertNotIn(SECRET, env.audit.rows[-1].action)

    def test_audit_endpoint_shapes_rows(self):
        env = Env(self)
        env.grant()
        with mock.patch.object(main, "_GRANT_SERVICE", env.svc):
            rows = main.get_tool_audit("Research", 50, None)
        self.assertEqual(rows[0]["kind"], "confirm")
        self.assertEqual(set(rows[0]), {"id", "at", "actor", "kind", "text", "target", "exit_code", "duration_ms", "checkpoint", "run_id"})
        from fastapi import HTTPException
        with self.assertRaises(HTTPException):
            main.get_tool_audit("../x", 50, None)

    def test_pg_stores_issue_the_expected_sql(self):
        log = []
        class C:
            def __enter__(s): return s
            def __exit__(s, *a): return False
            def execute(s, sql, params=()):
                log.append((sql, params))
                class R:
                    rowcount = 1
                    def fetchone(s): return None
                    def fetchall(s): return []
                return R()
        store = cg.PgGrantStore(C)
        self.assertIsNone(store.latest("c"))
        store.add(cg.Grant("c", ("shell",), "/s", "1h", "taisun", 1.0, 2.0))
        store.revoke("c", "taisun", "off", 1.0)
        store.end_run("r", 1.0)
        store.expire_due(1.0)
        cg.PgAudit(C).add(dict(at=1.0, actor="a", crew="c", kind="info", action="x"))
        cg.PgAudit(C).list("c", 5, before=3)
        sql = " ".join(q for q, _ in log)
        for frag in ("crew_tool_grants", "revoked_at IS NULL", "crew_tool_audit", "id < %s"):
            self.assertIn(frag, sql)
        self.assertIn("tool_grant", main.SCHEMA)
        self.assertIn("tool_grant", main.MIGRATE_APPROVAL_KINDS)
        self.assertIn("crew_tool_audit", cg.GRANT_SCHEMA)


class RequestFlowTests(unittest.TestCase):
    def test_request_tool_is_ask_only_and_a_denial_does_not_abort_the_run(self):
        sent, aborted = [], []
        class Resp:
            def __init__(s, ans): s.ans = ans
            def poll(s, t=None): return True
            def recv(s): return {"k": "decision", "approve": s.ans}
        for ans in (True, False):
            ap = main.ChildApprover(sent.append, Resp(ans), timeout_s=1, abort=lambda: aborted.append(1))
            with mock.patch.object(main, "_RUN_APPROVER", ap):
                out = main.request_tool_grant_fn(json.dumps({"tools": ["shell", "sudo"], "duration": "run", "reason": f"need tests token={SECRET}"}))
            self.assertEqual(out, "approved" if ans else "Tool error: the request was denied")
        self.assertEqual(aborted, [])
        msg = sent[0]
        self.assertEqual((msg["kind"], msg["payload"]["tools"], msg["payload"]["duration"]), ("tool_grant", ["shell"], "run"))
        self.assertNotIn(SECRET, json.dumps(msg))
        with mock.patch.object(main, "_RUN_APPROVER", None):
            self.assertIn("Tool error", main.request_tool_grant_fn("{}"))
        with mock.patch.object(main, "_RUN_APPROVER", ap):
            self.assertIn("Tool error", main.request_tool_grant_fn("not json"))

    def test_ordinary_denial_still_aborts(self):
        aborted = []
        class Resp:
            def poll(s, t=None): return True
            def recv(s): return {"k": "decision", "approve": False}
        ap = main.ChildApprover(lambda m: None, Resp(), timeout_s=1, abort=lambda: aborted.append(1))
        self.assertFalse(ap.request("write", ["x"]))
        self.assertEqual(aborted, [1])

    def test_approval_row_is_high_risk_with_parent_built_items_and_one_hour_expiry(self):
        log = []
        class C:
            def __init__(s, l): s.l = l
            def __enter__(s): return s
            def __exit__(s, *a): return False
            def execute(s, sql, params=()):
                s.l.append((sql, params))
        with mock.patch.object(main, "db", return_value=C(log)), mock.patch.object(main, "db_crew_name", return_value="Research"):
            main.db_create_approval("rid", {"kind": "tool_grant", "items": ["attacker text"],
                                            "payload": {"tools": ["shell", "x"], "duration": "run", "reason": "r\nsecond"}})
        ins = log[0][1]
        self.assertEqual((ins[2], ins[6]), ("tool_grant", "High"))
        self.assertEqual(ins[4][:3], ["tools: shell", "folder: crew-sandbox/research", "duration: run"])
        self.assertNotIn("attacker text", json.dumps(ins))
        upd = [q for q, _ in log if "payload" in q][0]
        self.assertIn("make_interval", upd)
        self.assertEqual(cg.REQUEST_EXPIRY_S, 3600)

    def test_stale_requests_are_denied_by_the_sweep_query(self):
        log = []
        class C:
            def __enter__(s): return s
            def __exit__(s, *a): return False
            def execute(s, sql, params=()): log.append(sql)
        with mock.patch.object(main, "db", return_value=C()):
            main.db_expire_stale_requests()
        self.assertIn("expires_at <= now()", log[0])
        self.assertIn("approved=false", log[0])
        self.assertIn("expires_at IS NULL OR expires_at > now()", open(main.__file__).read())

    def test_approved_request_creates_a_grant_denied_one_does_not(self):
        env = Env(self)
        with mock.patch.object(main, "_GRANT_SERVICE", env.svc), \
                mock.patch.object(main, "db_approval_payload", return_value={"tools": ["fs_read", "shell"], "duration": "run", "reason": "x"}), \
                mock.patch.object(main, "db_crew_name", return_value="Research"), \
                mock.patch.object(main.time, "time", env.clock):
            self.assertTrue(main.create_grant_from_approval("aid", "run-1"))
        st = env.svc.state("Research")
        self.assertEqual((st["enabled"], st["tools"]), (True, ["fs_read", "shell"]))
        self.assertEqual(cg.shell("echo ok").splitlines()[0], "exit 0")  # claimed for the requesting run
        # decide endpoint: deny never creates anything, approve does; both answer the child
        env2 = Env(self, crew="Two")
        row = main.ApprovalOut(id="a", run_id="r", crew="Two", agents=[], kind="tool_grant", title="t", reason="r", risk="High", requested_at="x")
        for approve in (False, True):
            delivered, made = [], []
            with mock.patch.object(main, "db_decide_approval", return_value=("run-9",)), \
                    mock.patch.object(main, "db_get_approval", return_value=row), \
                    mock.patch.object(main, "create_grant_from_approval", lambda a, r: made.append(a) or True), \
                    mock.patch.object(main, "deliver_decision", lambda r, a, kind=None: delivered.append((r, a, kind))):
                main.decide_approval("11111111-2222-3333-4444-555555555555", main.DecideRequest(approve=approve))
            self.assertEqual(delivered, [("run-9", approve, "tool_grant")])
            self.assertEqual(len(made), 1 if approve else 0)

    def test_failed_grant_creation_answers_the_child_no(self):
        row = main.ApprovalOut(id="a", run_id="r", crew="Two", agents=[], kind="tool_grant", title="t", reason="r", risk="High", requested_at="x")
        delivered = []
        with mock.patch.object(main, "db_decide_approval", return_value=("run-9",)), \
                mock.patch.object(main, "db_get_approval", return_value=row), \
                mock.patch.object(main, "create_grant_from_approval", return_value=False), \
                mock.patch.object(main, "deliver_decision", lambda r, a, kind=None: delivered.append(a)):
            main.decide_approval("11111111-2222-3333-4444-555555555555", main.DecideRequest(approve=True))
        self.assertEqual(delivered, [False])

    def test_denied_grant_request_does_not_kill_the_run(self):
        r, w = main.mp.Pipe(duplex=False)
        main._resp_pipes["rid"] = w
        self.addCleanup(main._resp_pipes.pop, "rid", None)
        with mock.patch.object(main, "db_resume_run") as resume, mock.patch.object(main, "db_finish") as fin:
            main.deliver_decision("rid", False, kind="tool_grant")
        fin.assert_not_called()
        resume.assert_called_once_with("rid")
        self.assertEqual(r.recv(), {"k": "decision", "approve": False})

    def test_triggered_run_is_built_without_any_grant_or_request_tool(self):
        names = set(cg.GRANT_TOOLS) | {"request_tool_grant"}
        with mock.patch.object(main, "ALLOWED_TOOLS", frozenset({"kb_query"}) | names):
            self.assertFalse(names & main.child_allowed_tools(grants=False))
            self.assertIn("kb_query", main.child_allowed_tools(grants=False))

    def test_parent_refuses_a_grant_request_from_a_triggered_run_without_asking_taisun(self):
        import test_main
        r2, w2 = main.mp.Pipe(duplex=False)
        main._no_grant_runs.add("rid")
        main._resp_pipes["rid"] = w2
        self.addCleanup(main._resp_pipes.pop, "rid", None)
        calls = test_main.WatchTests().run_watch([
            {"k": "approval", "kind": "tool_grant", "items": [], "payload": {"tools": ["shell"], "duration": "24h"}},
            {"k": "final", "status": "completed", "result": "ok", "llm": "amara-fast", "tokens": 1, "requests": 1}])
        self.assertNotIn(("approval", "tool_grant"), calls)
        self.assertEqual(r2.recv(), {"k": "decision", "approve": False})
        self.assertNotIn("rid", main._no_grant_runs)  # forgotten when the run settles

    def test_tool_registry_absent_without_a_sandbox_root(self):
        self.assertNotIn("shell", main.TOOL_REGISTRY)  # this image sets no CREW_SANDBOX_ROOT
        self.assertEqual(main.child_allowed_tools(), main.ALLOWED_TOOLS)
        self.assertEqual(main.tools_for("executor", main.child_allowed_tools()), [])


if __name__ == "__main__":
    unittest.main()

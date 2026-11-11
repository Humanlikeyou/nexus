"""Regression cases for guard-bash.sh. Run from the repo root: python3 .claude/hooks/test-guard.py (exit 1 on a mismatch)."""
import json,subprocess,os,sys
R='~/Library/Application\\ Support/com.taisun.nexus/replays'
# (command, expected exit code): 2 = blocked, 0 = allowed
cases=[f"ls {R}\nrm {R}/x", f"cat <(rm {R}/x)", f"touch {R}/x", "x=push; git $x", "rm $F", "git $'push'",
       f"ls {R}", f"head -c 600 {R}/eeg_1.ndjson", "git status", "git co"+"mmit -m 'fix(router): keep deep warm'", "git log -1", "cargo test", "rm tmp.txt"]
expect=[2,2,2,2,2,2,0,0,0,0,0,0,0]
bad=0
for c,e in zip(cases,expect):
    r=subprocess.run(["bash",".claude/hooks/guard-bash.sh"],input=json.dumps({"tool_input":{"command":c}}),text=True,capture_output=True,env={**os.environ,"CLAUDE_PROJECT_DIR":os.getcwd()})
    ok=r.returncode==e; bad+=not ok
    print("ok  " if ok else "FAIL",r.returncode,"|",c.replace("\n","\\n"))
sys.exit(1 if bad else 0)

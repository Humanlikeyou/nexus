# Cleaning private data out of git history (procedure only; nothing has been run)

Decision 0050 stops new private data from entering the repository. It does **not** remove what is already in history. This page is the procedure for when Taisun decides to do that. Every step below is his call. A history rewrite and force-push are destructive, outward-facing actions, so the hard rules require his explicit OK for each one.

## What history contains (checked 2026-11-25, values never printed)

| Item | Commits | Public? |
|---|---|---|
| `config/terrance.profile.md` (name, profile) | 2 (first 2d9309b) | yes: already in `origin/main`; the GitHub repo is **public** |
| `config/terrance.identity.md` (persona with personal references) | 4 (first a47bf3b) | yes: in `origin/main` |
| Anthropic-shaped key strings | 8bd95d0, cdc98cb | a **synthetic** test value in a redaction unit test (33 characters, 7 distinct, repeated run), not a real key |
| Telegram bot tokens, `sk-` keys, AWS, GitHub, Slack, private-key blocks | none found | n/a |
| `.env`, `.pem`, `.key`, `memory/` | never committed | n/a |
| Project notes (decisions, handoffs) quoting his words and plans | many | yes (pushed 2026-11-23 and 2026-11-25) |

The current tree has no credentials. Its secret-pattern hits are deliberate test fakes. The real macOS username appears in 9 tracked files as paths.

## Recommended order

1. **Stop further exposure first (reversible).** Make the GitHub repository private: GitHub › Settings › General › Danger Zone › Change visibility, or `gh repo edit Humanlikeyou/nexus --visibility private --accept-visibility-change-consequences`. Anything already cloned, forked or cached stays out there, so treat the exposed content as public regardless.
2. **Rotate anything that was ever real.** No real credential was found, so there is nothing to rotate from this audit. If you know a real key was ever committed, rotate it at the provider. Rewriting history never un-leaks a secret.
3. **Decide whether a rewrite is worth it.** If the repo is private and the profile is not sensitive enough to justify the disruption, stopping here is reasonable.
4. **If you rewrite history** (destructive; coordinate every clone first):
   1. Make a full mirror backup: `git clone --mirror git@github.com:Humanlikeyou/nexus.git nexus-backup.git`.
   2. Install git-filter-repo: `brew install git-filter-repo`.
   3. Work in a fresh mirror clone, not in `~/projects/nexus` or `nexus-amara`.
   4. Remove the files from every commit: `git filter-repo --invert-paths --path config/terrance.profile.md --path config/terrance.identity.md`. To also scrub text, use `--replace-text replacements.txt`, with patterns only and no real values in that file.
   5. Check the result: `git log --all -- config/terrance.profile.md` should print nothing.
   6. Force-push all branches and tags: `git push --force --mirror` (or per branch). **This is the step that needs his explicit OK.**
   7. Ask GitHub Support to purge cached views and pull-request refs if the repo stayed public. Forks are not rewritten.
   8. Re-clone every working copy (`~/projects/nexus` and the `nexus-amara` worktree) from the rewritten remote. Old clones would push the history back. Keep uncommitted local edits safe first.
5. **Afterwards,** keep the `.gitignore` rules from decision 0050, and keep `npm run check:secrets` in the commit gate.

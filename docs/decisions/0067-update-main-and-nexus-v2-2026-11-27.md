# 0067: Update `main` and `nexus-v2` on GitHub (Taisun, 2026-11-27, in session)

## His words

> "maybe Maine and the other Nexus V2 contain personal profile and identity files that needs to be fixed as well so if you need to update everything with the new files with the new code and program then do it"

## Approved actions (exactly these)

1. **`nexus-v2`:** one `--no-ff` merge commit of `amara-integration` into `nexus-v2`, made without touching the `~/projects/nexus` checkout, then pushed to `origin/nexus-v2`.
2. **`main`:** a fast-forward of `origin/main` to the same `amara-integration` commit, then pushed.

Not approved: a history rewrite, force-pushes, PRs, or any other branch.

## Inspection before acting

- **`origin/main`** (`cfb48f2`) is an ancestor of `amara-integration`, so the fast-forward loses nothing.
- **`origin/nexus-v2`** (`cc1d828`) has 19 commits that `amara-integration` lacks, all of them merge commits. `git diff HEAD...origin/nexus-v2` is empty, so they carry no content of their own, and the merged tree equals `amara-integration`'s tree.
- Both tips still tracked `config/terrance.profile.md` and `config/terrance.identity.md`. After this update neither tip does.
  - **History still holds them** (since `2d9309b` and `a47bf3b`). Removing them from history would need a rewrite: see `docs/security/history-cleanup.md`.
  - The repo stays **public**. Making it private is Taisun's choice in GitHub settings.

## After

- **The local `~/projects/nexus` checkout is untouched** (read-only for agents), and its `nexus-v2` is behind. When Taisun pulls there, git deletes the two private files from that folder. The copies are kept in `<data>/config/` (migrated, verified 2026-11-26) and `~/Backups/nexus-private-config-20261125/`.
- **CI** (`ci.yml`) runs on pushes to `main` and `nexus-v2`, so these pushes start a CI run. Not watched here.

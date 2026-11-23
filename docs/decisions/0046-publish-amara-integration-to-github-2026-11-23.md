# 0046: Publish amara-integration to GitHub (Taisun, 2026-11-23, in session)

His words (verbatim): **"push what we have so far to github"**.

- This is the hard rule 1 OK for this one action: `git push` of the local branch `amara-integration` to `origin` (github.com/Humanlikeyou/nexus) as a new remote branch of the same name.
- At approval, origin had only `main` (cfb48f2) and `nexus-v2` (62ce254), and no `amara-integration`.
- `main` and `nexus-v2` are not touched. The merge into nexus-v2 (0044) is still his to run, after the 0045 cycle.
- The branch head includes 6774aab, 010624f, c39e781 and this record.
- His uncommitted docs and untracked files are not included, because only commits are pushed.
- Later pushes need a new OK.

## Addendum (same day)

His words: **"Is the full projets on my github if not push it too"**.

- This is the OK for one fast-forward update of `origin/nexus-v2`: local `nexus-v2` was 35 commits ahead of origin (62ce254), and origin was an ancestor, so nothing is rewritten.
- The design branches (design-1110g, design-1118, design-build-1111*, design-rings-1111) have no commits outside amara-integration, which is already on GitHub. They are not pushed.
- His 7 uncommitted doc edits and his untracked tasks/docs files are not committed without his separate OK.

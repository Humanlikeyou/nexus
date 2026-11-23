# 0037: Link goals to ventures, and save Content drafts (Taisun, 2026-11-18/19, in session)

Context: lane B4 (c7 commit after 1d44a2b) found two gaps. (1) Goals have no venture tag, so Ventures can't show a venture's goals. (2) "Draft with Amara" returns text but doesn't save it.

His words: **"yes fix the gaps"**

Scope:
1. An optional venture id on goals (must exist in ledger.json). Set or cleared through the single write path with History and Undo. Ventures › tracking then lists each venture's goals and their Path progress (server percent only).
2. Saving a Content draft: the drafted text, after review, is saved locally next to its pipeline item, through the single write path with History and Undo. Saving still posts nothing.

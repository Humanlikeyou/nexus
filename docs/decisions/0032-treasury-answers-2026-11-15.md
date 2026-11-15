# 0032: Treasury plan approval and answers (Taisun, 2026-11-15, in session)

Question: approve `tasks/plans/treasury.md` (Step 1) and answer its 14 questions.

His words (voice-typed, verbatim): **"approve. I'm not sure for number one you decide. Number two no number three Gmail number four you decide number five Japan and USD number six sure number 77 number eight in number nine yes number 10 sure number 11. It's whatever you decide. I'm 12. It's whatever you decidenumber 13 yes number 14 you can do that last. That's not important until everything is done"**

Plan: **approved.** Build order as in the prompt, one phase per run.

| # | Question | Answer | Who decided |
|---|---|---|---|
| 1 | Source of attention hours | Manual log only, minutes per stream per day. Tagged roadmap steps can be added later | Agent (he said "you decide") |
| 2 | Customer contacts in Amara | **No.** Counts per stage and segment only; names and emails stay with the provider | Taisun |
| 3 | Email provider | **Gmail** | Taisun |
| 4 | Placement | Under Enterprise, no new nav label (hard rule 4). Layout belongs to Design Lead | Agent ("you decide") |
| 5 | Currencies | **JPY and USD** | Taisun |
| 6 | Real export samples | **Yes**, he will supply a header row plus two scrubbed rows per source. Until then only the generic column mapper ships | Taisun |
| 7 | Days without an import = stale | **7 days** (he said "77", read as 7 from voice typing; confirm if 77 was meant), per-stream override | Taisun |
| 8 | Contacts in backups | **In.** Moot while answer 2 is "no": only counts are stored, and they are backed up with `business/` | Taisun |
| 9 | Treasury alert kind on the Telegram relay | **Yes.** A plain line with no amounts or names; still goes through the relay's native approval | Taisun |
| 10 | Real `ledger.json` | **Yes**, he will create it with the real venture ids | Taisun |
| 11 | FX source | The ledger's hand-entered dated rate only; `money/fx.json` is not read by Treasury | Agent ("whatever you decide") |
| 12 | `raw_ref` | Row number plus file SHA-256 only, never the CSV line text | Agent ("whatever you decide") |
| 13 | Refunds and fees | **Yes**, negative income in the same stream | Taisun |
| 14 | Legal check on consent (Japan opt-in, GDPR, CAN-SPAM) | **Last**, after everything else is done, and before the welcome sequence goes live | Taisun |

Agent flag on answer 3 (not legal advice). Gmail is a mailbox, not a sequence provider. It has no timed sequences or automatic unsubscribe handling, and it has daily send limits. Phase 4 therefore produces:
- the drafts and setup checklists as planned, written for a Gmail workflow (manual or scheduled sends, an unsubscribe line, opt-outs honoured by hand);
- a note that a sequence tool can be added later behind the same results-import interface.

Nothing in this changes the outbound rules. Amara still sends no email itself, and any Gmail API sync stays a [GATE] needing its own logged decision.

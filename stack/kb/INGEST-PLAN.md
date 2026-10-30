# Knowledge-base ingest plan

Verba is healthy but empty (never ingested). This plan fills it with the project docs only,
locally, with no secrets or personal data. The script is `scripts/kb-ingest.sh` (logic in
`scripts/kb-ingest.mjs`, tests in `test/scripts/kbIngest.test.ts`).

**Status:** dry run only. `--apply` waits for Taisun's OK.

## What goes in (allowlist)

`README.md`, `CLAUDE.md`, `STAGES.md`, `BUILD.md`, `docs/**/*.md`, `stack/SECURITY-NOTES.md`,
`stack/crew-service/README*`, and `.md` notes under `stack/n8n/`. Nothing else is a candidate.

## What never goes in (denylist, beats the allowlist)

`.env*`, `*.key`/`*.pem`, anything named `credential*`, `secrets/` dirs, `stack/data/`,
`.sqlite`/`.db`, OpenWhoop/NOOP/whoop/strain/recovery/sleep/HRV files, Muse/EEG/neuro
recordings, `.csv`/`.json` exports, logs, backups, `archive/` and `~/Archive`. Matching is on
the path, case-insensitive, so an allowlisted `docs/sleep-notes.md` is still skipped.

## Checks

1. **Local only.** The verba container's env is read (`docker exec stack-verba-1 env`) and the
   run aborts if any remote provider key is set (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
   `COHERE_API_KEY`, `GROQ_API_KEY`, `VOYAGE_API_KEY`, ...) or `OLLAMA_URL` /
   `WEAVIATE_URL_VERBA` / `OPENAI_BASE_URL` points off this machine. Only names are printed.
2. **`check:secrets`** must be clean (repo-wide gate).
3. **Per-file secret scan**: the `check:secrets` patterns plus Bearer tokens, JWTs, `sk-` keys,
   `api_key=`/`token=`/`secret=` and `password=` assignments. A hit skips the file, listed by
   path and pattern name only.
4. Symlinks and binary files are skipped.

## Running it

```sh
scripts/kb-ingest.sh             # same as --dry-run: files, bytes, skips with reasons, totals
scripts/kb-ingest.sh --apply     # only after Taisun's OK
```

`--dry-run` writes nothing and sends nothing. `--apply` additionally needs Verba healthy
(`GET :8001/api/health`) and the set capped at 200 files / 5 MB, then posts each file as
`kb.ingest` to the local Ingress (`:5678/webhook/terrance/ingress`) with `NEXUS_SECRET` from
`stack/.env` (never printed). The Ingress chunks the text and embeds it with local Ollama
`nomic-embed-text` into Weaviate. Each file is labelled `nexus-docs`.

## Dry run, 2026-10-30

8 files, 91,349 B: `BUILD.md`, `CLAUDE.md`, the three `docs/*.md`, `README.md`,
`stack/SECURITY-NOTES.md`, `STAGES.md`. No allowlisted file was skipped.

# Phase 8 live steps (proposed, not run)

Status: proposal only. Nothing below has been run. No docker, launchctl, compose, or app change was made to write this file.
Sources: `tasks/proposed/phase8-supervisor.md` §1 and §6, `TAISUN-STEPS.md` §9.9, `docs/decisions/0027`, `0029`, `tasks/HANDOFF-FOR-REVIEW.md` §23 and §23a.

## 1. Proposed replacement text for TAISUN-STEPS §9.9 (crew image rebuild)

**Why the old step is wrong.** Neither compose file sets `name:`, so both trees use compose project `stack` and containers `stack-<svc>-1`. The live dir `~/projects/nexus/stack` has `.env` and `data/`. `~/projects/nexus-amara/stack` has neither. Running `docker compose build crew` from the amara-integration tree would act on the same project name with no `.env` and no `data/`. Never run compose from `~/projects/nexus-amara/stack`.

**Why a plain rebuild is not enough.** The live crew image builds from `~/projects/nexus` (nexus-v2). Its `crew-service/main.py` is 627 lines. The amara-integration version is 1960 lines. A rebuild from the live dir alone picks up nothing new. That is why step (c) exists.

Corrected order (all steps not run):

- (a) **Router Host check first.** The router must accept `Host: host.docker.internal`. Not verified whether it does. A config or code change may be pending. Do not rebuild crew until this is confirmed.
- (b) **Back up.** Copy `~/projects/nexus/stack/docker-compose.yml` and `~/projects/nexus/stack/crew-service/` to a dated folder under `~/Backups/`. Record the current image and tag it: `docker tag stack-crew stack-crew:pre-phase8`.
- (c) **Bring the amara-integration crew-service sources into the live build.** This is effectively the frozen merge (decision 0001). It needs Taisun's explicit OK, logged in `docs/decisions/`.
- (d) **Build and start, from `~/projects/nexus/stack` only:** `docker compose build crew && docker compose up -d crew`.
- (e) **Checks.** `GET http://127.0.0.1:8400/health` returns 200. The new endpoints answer (not 404): `/crews/runs/{id}/steps`, approvals, tasks, memory, grants. A crew run from Crews makes an `amara-fast` line in `~/Library/Logs/amara-router/routes.jsonl`. `/health` 200 alone does not prove the new code is running.
- **Rollback:** re-tag `stack-crew:pre-phase8` as the image compose uses, restore the backed-up compose file if it changed, then `docker compose up -d --no-build crew` from `~/projects/nexus/stack`. Do not use `docker restart` alone: it restarts the old code in the running container.

## 2. Supervisor gates and Taisun steps (plan §6)

1. Ship observe mode first. Probes and events only, no restarts.
2. Flip `SUPERVISOR_ACT_ENABLED` only after a decision entry is logged in `docs/decisions/`.
3. Collect a week of observe-mode data before any act decision.
4. Run the cold-boot check after that week of data.
5. LaunchAgent retirement only after a week of clean starts, and only by Taisun. The app never does it.
6. Rebuild crew (section 1) only through the live dir, and only after the router Host check passes.
7. Any `docker compose up` is a separate gated step. The app never runs one.
8. Read `~/projects/amara-voice/amara` before any voice start or stop. Voice stays observe-only until then.
9. The app never stops Freqtrade. HA and Freqtrade use `docker start`, `stop` and `restart` only. Never `docker run`, never `compose up`.
10. Hermes key work is a gate. Key rotation comes last (hard rule 7).
11. The app never bootstraps, boots out, loads or unloads a LaunchAgent. LaunchAgent jobs get `kickstart -k` only, and only when loaded.

## 3. Open questions (plan §6)

- Does Hermes serve an HTTP `/health`? Today it is probed by TCP only.
- Does `./amara` touch launchd? Is amara-voice unloaded on purpose (decision 0024)?
- First ship: manual start and stop too, or restart only? Plan recommends restart only.
- Freqtrade: stop rules and dry-run policy?
- Should HA and Freqtrade move into compose?
- Docker Desktop missing or showing a license prompt: is a fixed `needs_you` state acceptable?
- Is `separate` a state or a pill? (Design question.)
- Does crew have a drain endpoint for shutdown?
- Per-service derived tokens (decision 0027 amendment) must land before any crew token mount.

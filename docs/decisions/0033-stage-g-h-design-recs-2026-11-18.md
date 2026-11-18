# 0033: Stage G, Stage H and Design go with Claude's recommendations (Taisun, 2026-11-18, in session)

His words (verbatim): **"Ok we go with your rec for stage H stage G and design. I deleted the backupas i was lloking to deal with the mac book storage issue. this project is building too much redundant files that are sapping my storage No we can just do it here no need for a new session"**

## Stage H (tasks/proposed/stage-h-gates.md)
| Q | Answer |
|---|---|
| 13.1 | SMAppService.mainApp login item (no LaunchAgent) |
| 13.2 | The Startup toggle is Design's; off until Taisun turns it on himself |
| 14.3 | Local release script only (CI release stays frozen with pushes, decision 0001) |
| 14.4 | First signed release is 0.2.0. Bump only when the signed build is made |
| 14.5 | Entitlements: Bluetooth only, as built; revisit only after a failed live test |
| 14.6 | arm64 only |
| 15.3 | `UPDATER_SWITCH` stays false until the host and his public key exist |
| 15.4 | auto_update and check_daily default on, once updates are live |
| 15.5 | Build the real swap host only after a signed vN to vN+1 run works |

**Still needed from Taisun** (no recommendation can stand in for these):
- 14.1: a Developer ID certificate
- 14.2: `notarytool store-credentials nexus-notary`
- 15.1: the update host
- 15.2: generate his updater key and give Claude the public key only

## Stage G (tasks/proposed/phase10-12-stage-g.md)
| Q | Answer |
|---|---|
| Q1 | `noop-syncd` source lives in the NOOP repo as its own Swift package, after that repo is backed up |
| Q2 | Takeover settings flag: yes, default off |
| Q3 | Real parity reports are not committed (they stay in `<data>/parity/`) |
| Q4 | After the takeover, parity compares the new store with the frozen legacy copy for 14 days |
| Q5 | DEEP/REM/RHR placeholders dropped |
| Q6 | Oura, Hevy and coach secrets go to the Keychain (`com.taisun.nexus.*`) via `keychain_migrate`, one approval per item |
| Q7 | Migrate the legacy plaintext `config.json` Telegram token into the Keychain. The plaintext copy is removed only after the Keychain copy is verified, with an approval |
| Q8 | Lucid cue = strap haptic plus local notification (no audio) |

G3 to G6 still need his hardware and NOOP.app steps; those stay Taisun steps.

## Design
1. Design Lead fixes the light-theme contrast of `.m-warn b` (the `--copper-text` warning text, 4.28:1, needs 4.5:1) without other visual change, so `takes-1111t:80` stops flaking.
2. Design Lead wires the Settings > General > Startup toggle to `login_item_status` / `login_item_enable` / `login_item_disable`.
3. Design Lead designs the Treasury screen under Enterprise (decision 0032 #4) once Treasury's backend phases are done.

## Storage
Taisun asked to stop the project's build files filling the disk.
- Cleared the regenerable dev build caches with `cargo clean --profile dev`: src-tauri 123.9 GiB of files (about 60 GB on disk), amara-router 4.5 GiB. Free space went from 26 GB to 86 GB.
- The dev profile now builds dependencies without debug info and our crate with line tables only, so it grows far more slowly.
- Two app rollback copies remain in `~/Backups` (131 MB each).

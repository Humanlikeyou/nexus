# Nexus Product Verification

Updated October 3, 2026. This is an evidence ledger, not a claim that all acceptance gates are complete.

## Scope and Baseline

- Active app: Tauri 2, React 19, TypeScript, Rust; frontend entry is `src-console`.
- Version remains 0.1.0; installed identity remains `com.taisun.nexus`; ARM64 macOS target remains 13.0.
- Initial working tree already contained extensive user changes, including the old frontend deletions and the NOOP integration. They have not been reverted.
- Baseline frontend build, Rust checks, 84 Rust tests, and production bundle succeeded.
- Five baseline unused-notification warnings were removed by excluding an unregistered, unused module. Its implementation file remains available.
- Local toolchain: Rust 1.93.0, Apple Swift 6.2.4. Nexus has no application Swift sources or Swift packages, so SwiftUI/MainActor migrations do not apply to its app code.

## Modernization

Verified installed frontend versions: React/React DOM 19.3.0; Tauri API 2.11.1; Tauri CLI 2.11.5; Vite 6.4.3; TypeScript 5.9.3; React plugin 4.7.0; Lucide 0.468.0.

Updated Rust dependencies include Tauri 2.11.6, tauri-build 2.6.3, dialog 2.7.3, shell 2.3.6, Tokio 1.53.1, Serde 1.0.229, serde_json 1.0.151, chrono 0.4.45, futures 0.3.34, glob 0.3.4, uuid 1.26.1, and rand 0.8.8.

Deliberately held breaking migrations: btleplug 0.11 to 0.13, rusqlite 0.31 to 0.40, reqwest 0.12 to 0.13, dirs 5 to 7, sha2 0.10 to 0.11, rand 0.8 to 0.10, Vite 6 to 8, TypeScript 5 to 7, React plugin 4 to 6, and Lucide 0.x to 1.x. BLE and persistence require dedicated migration validation. No claim is made that every dependency is on its newest major.

Upstream references reviewed during the audit: [Tauri releases](https://tauri.app/release/tauri/all-versions/), [Vite 8 migration announcement](https://vite.dev/blog/announcing-vite8), [TypeScript](https://www.typescriptlang.org/), package registries, and [Node release status](https://nodejs.org/en/about/previous-releases). The machine's Node 25 should be replaced with a supported LTS development runtime in a separate toolchain change.

## Product Changes

- Replaced the decorative dark-only console with semantic light/dark tokens, macOS system type, grouped navigation, restrained status indicators, and responsive layouts.
- Added Devices, Sessions, and Settings; retained Overview, Live, Health, Trajectory, Business, Revenue, and Activity.
- Shared components: headings, metrics, status indicators, segmented controls, buttons, and empty states.
- Preferences support System/Light/Dark and Reduce Motion.
- Keyboard navigation uses Command+1 through Command+9; other Command shortcuts and ordinary typing are ignored by the navigation handler.
- Live uses real discovery/connect/start/stop commands, real EEG buffers, per-electrode timestamps, measured frequency power, and signal-loss states.
- Restored access to the existing neurofeedback protocols/policy/gating logic through a calibrated Live workflow. Calibration requires valid, fresh data from every electrode. Contact failures, missing packets, and detected movement hold feedback. Training metrics and timeline are included in the existing session payload.
- Relative spectral power combines electrode powers rather than averaging waveforms, preventing opposite-phase cancellation. Frequency-band boundaries are non-overlapping.
- Session metadata reads current received-packet counters instead of delayed render state. Full backend recordings remain separate from bounded display buffers.
- Session finalization retains successful stop steps and a stable save payload across retries. Pending sessions keep stop/save controls available even after disconnect, block replacement recordings, and retain their original stop time. Concurrent stop requests are serialized. Disk-flush failure retains the backend writer and its buffered samples for retry.
- Fixed a native WebKit rendering failure caused by a page entrance animation. Screens now paint without that animation.
- Removed control color transitions that retained old sidebar colors during native theme changes. Appearance is applied in a layout effect before paint; Reduce Motion now disables transitions and animation rather than running near-zero-duration animations. No app reload or component remount is used.
- `npm run bundle:local` explicitly ad-hoc signs local bundles. Earlier unsigned linker-only bundles failed strict verification; explicitly signed bundles pass.

## Verification Evidence

| Gate | Evidence | State |
| --- | --- | --- |
| Frontend compile | `npm run build` after training integration | Passed |
| Rust regressions | October 3 `cargo test --all-targets`: 85 passing, including failed recording flush/retry; release compilation succeeds | Passed |
| Spectral math | `npm run test:neuro`: tones, phase inversion, mixed amplitudes, DC, invalid input | Passed |
| Training lifecycle | Valid calibration, summary capture, single-channel staleness, flatline/invalid data, signal interruption | Passed with synthetic input |
| Stop/save recovery | Four frontend finalization tests cover stable save retries, failed recorder flush, concurrent stop requests, and stream-stop failure; 12 total frontend tests pass | Passed fault-injection tests; physical UI workflow still pending |
| Product launch | Latest signed build installed at `/Applications/Nexus.app`; executable SHA-256 matches build; native window inspected | Passed |
| Appearance | October 2 installed patch: Dark to Light to Dark updates main surfaces, labels, sidebar selection, and controls without restart | Passed native inspection |
| Layout | 1400x900 and 900x700 browser screenshots, no horizontal overflow | Passed before training strip; repeat its layout |
| Existing history | Native Sessions loads September 26 recording (83,760 samples) and February 18 recording (38,760); selecting February 18 shows focus 33, relaxation 48 | Passed including detail selection |
| Muse discovery | October 2 native scan enters Searching and returns no headsets | Passed; no physical headset found |
| WHOOP local records | Native Health and Overview load NOOP health records | Passed for local record reading |
| WHOOP live connectivity | September 21 NOOP Today displays Heart Rate Live, 87 bpm, strap history Syncing, battery 8%; previously disconnected | Live UI observed; end-to-end Nexus packet flow not yet verified |
| Muse live acquisition and recovery | Requires available headset | Not verified |
| Live stop/save/reload | Source wired; actual headset recording not exercised | Not verified |
| Preference persistence | Light and Reduce Motion true survived installation/restart; restored original Dark and Reduce Motion false | Passed |
| Native training UI | Installed Live exposes protocol selector, Off state, and real disconnected/scan controls | Passed for disconnected state; physical training pending |
| Installer integrity | September 21 DMG verified; October 2 app-only bundle signs and verifies strictly, installed executable matches build SHA-256 `92ba10ca89b074395783d87e0c4ee8be4f0658cf1c8fa9981eedcfe69cda3f53` | Latest app passed; DMG predates theme patch |
| Distribution | Ad-hoc local signing; no Developer ID/notarization credentials supplied | Local installation only |

## Protected Boundaries

This pass has not changed the Muse BLE protocol, backend sample acquisition, WHOOP/NOOP storage schema, historical session schema, approval rules, or user-data locations. At the time, the legacy `com.taisun.electron` data directory remained intentional; it has since been superseded by `com.taisun.nexus` with a one-time non-destructive copy on first launch (f30acbb). The training summary is additional content inside the already-flexible session data payload. The recorder stop path now retains its writer on flush failure; this is a targeted data-loss fix, not a protocol or acquisition rewrite.

Current active frontend files include `App.tsx`, `bridge.ts`, `types.ts`, `styles.css`, `design/ProductUI.tsx`, `design/SystemPrimitives.tsx`, `design/useAppearance.ts`, and the `neuro` modules/tests. Legacy signal-quality code only exports its existing pure channel assessor for reuse. Vite/TypeScript aliases permit reuse of the existing domain modules.

## Remaining Acceptance Work

1. Repeat responsive layout with the training strip; full native Live layout and session-detail selection passed October 2.
2. Exercise the new stop/save retry controls with a physical recording. In-memory recovery is tested; pending metadata does not yet survive a process crash or forced quit. The UI explicitly asks the user to keep Nexus open while a save is pending.
3. Exercise real Muse connection, packet flow, calibration, stop/save/reload, and reconnect when the headset is available.
4. Confirm live WHOOP flow in NOOP; stored health data is not proof of a live connection.
5. Complete final diff review and the requested 23-item report with these limitations explicitly stated.

The broad goal remains active while these gates are open. Synthetic input proves software behavior under those inputs, not physical Bluetooth or physiological validity.

October 3 installation: signed app-only bundle installed with previous build backed up at `~/Applications/Nexus Backups/Nexus-before-save-recovery-20261003.app`. Native launch and disconnected Live controls verified; existing two-session history remains visible. The DMG has not been regenerated for this patch.

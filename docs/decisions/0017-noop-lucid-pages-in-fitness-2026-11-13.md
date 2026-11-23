# 0017: NOOP / Lucid data, pages and tabs inside Amara (Taisun, 2026-11-13, in session)

His words: "because we are absorbing the noop lucid preview app and code etc i should be able to see all the data, pages and tabs somewhere in fitness and the settings somewhere in settings. i dont know if you were already working on this but it should be added"

Meaning:
- Every NOOP / Lucid preview screen and data view gets an Amara home under **Mind–Body › Fitness**, as extra tabs or sections there. Nothing is lost when NOOP.app retires (handover Phase 12).
- NOOP's settings get a home in **Amara Settings** (sources, strap, sync, Oura/Hevy/coach links, Lucid cues, notifications).

How it is built:
1. Inventory (read-only): every NOOP / Lucid page, tab, data field and setting, from ~/projects/noop-lucid-current (and NOOP.app), mapped to existing Amara screens and what's missing. Written to `docs/noop/page-inventory.md`.
2. Design Lead designs the new Fitness tabs and Settings sections. Markup, CSS and nav are Design's; nav labels and shortcuts don't change without Taisun.
3. Backend: read-only data commands over the absorbed health store (Phase 10 sidecars), then settings writes through reviewed commands.

This adds to Phases 10–12. It doesn't change the WHOOP takeover gate (Taisun's hardware check) or the 2-week parity before NOOP.app retires.

Still frozen (0001): merge into `nexus-v2`, push, PR.

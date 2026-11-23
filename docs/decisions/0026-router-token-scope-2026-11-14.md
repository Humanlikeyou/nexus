# 0026: Router launch-token scope (Taisun, 2026-11-14, in session)

Question: Hermes can't send the per-launch token header, and `~/.hermes` is off-limits. Should the router require the token only on its own endpoints for now?

His words: **"only /amara and /api for now, keep going"**.

- When enforcement is turned on (a router config switch, default off, plus a router restart; both are Taisun steps), the router requires `X-Amara-Launch-Token` on `/amara/*` and `/api/*` only.
- `/v1/*` (Hermes's OpenAI-compatible path) and `/health` stay as they are. Their existing bearer, Host and Origin checks are unchanged.
- How Hermes authenticates to the router is still open, for a later decision.

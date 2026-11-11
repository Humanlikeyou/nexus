# R13-A: crew-service model calls go through the Amara Router
- **Owner:** executor (Python). No live container rebuild/restart (Taisun's step).
## Acceptance criteria
- [x] crew-service builds its LLM against the router's OpenAI API (`CREW_ROUTER_URL`, default `http://host.docker.internal:8787/v1`), model alias `amara-fast` / `amara-deep` per role tier, plus `X-Amara-Route: fast|deep` where the client allows headers. Never `:11434`; no `keep_alive` / `num_ctx` sent.
- [x] A mocked-router test shows the request carries the route pin (header or alias), no request reaches `:11434`, `CREWAI_DISABLE_TELEMETRY` stays true, and `CrewInfo.llm` names a router model (`amara-fast`/`amara-deep`), never a raw Ollama or cloud model name.
- [x] MiniMax/direct-Anthropic path: **not removed** (open question for Taisun) but fail-closed: used only when `CREW_ALLOW_CLOUD=1` AND `NEXUS_OUTBOUND_DISABLED` is unset AND a key is set. Default off. Tests for all three gates.
- [x] `OLLAMA_BASE_URL` / `CREW_OLLAMA_MODEL` no longer used for model calls (kept readable only if something else needs them; say what).
- [x] Existing test_main.py tests still pass; docker-compose / .env.example updated with the new vars (no secrets).

## Evidence (executor, 2026-11-11)
- Files: stack/crew-service/main.py, test_main.py; stack/docker-compose.yml; stack/.env.example. Nothing committed.
- Design: `router_llm_kwargs(tier)` -> crewai `LLM(model="amara-fast|amara-deep", custom_openai=True, base_url=CREW_ROUTER_URL, api_key=CREW_ROUTER_KEY or dummy, default_headers={"X-Amara-Route": tier})` (native OpenAI provider; no litellm). Tier per agent in AGENT_CATALOG (researcher fast, analyst deep, writer fast). A router URL on the Ollama port is refused. No keep_alive/num_ctx. `CrewInfo.llm` added (stored at start as planned router model(s), updated on finish). MiniMax kept, gated by `cloud_allowed()` (CREW_ALLOW_CLOUD=1 AND NEXUS_OUTBOUND_DISABLED unset/empty AND key). OLLAMA_BASE_URL/CREW_OLLAMA_MODEL removed from crew code and crew compose env (OLLAMA_BASE_URL stays in .env.example for other services).
- Tests: `docker build crew-base`, `docker build -f Dockerfile.test crew-test`, `docker run --rm --network none crew-test` -> 56 passed (42 existing + 14 new: mock local http.server router asserting path/host:port/model/X-Amara-Route/Authorization/no keep_alive,num_ctx; 11434 refusal and source check; cloud gate x3; CrewInfo.llm; telemetry env true).
- Not run: live router call, live container rebuild/restart (Taisun step), host venv, Rust/TS gates (untouched). First test-image build hit a transient pip failure; rebuild succeeded.

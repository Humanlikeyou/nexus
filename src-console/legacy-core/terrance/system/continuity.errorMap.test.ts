import { describe, expect, it } from "vitest";
import { BACKEND_UNREACHABLE, jobFailureReason } from "./continuity";

// OpenClaw-specific failures read as backend-neutral copy; raw error text never reaches the UI.
describe("jobFailureReason: backend-neutral OpenClaw errors", () => {
  it.each([
    "unavailable: openclaw not installed",
    "OpenClaw gateway unreachable: error sending request for url (http://127.0.0.1:18789/v1/chat/completions)",
    "connect ECONNREFUSED 127.0.0.1:18789",
    "launchctl kickstart failed — is the LaunchAgent installed?",
    "Could not find service \"ai.openclaw.gateway\" in domain for port",
    "OpenClaw not running",
  ])("%s → Assistant backend not reachable", (raw) => {
    expect(jobFailureReason(new Error(raw))).toBe(BACKEND_UNREACHABLE);
    expect(BACKEND_UNREACHABLE).toBe("Assistant backend not reachable");
  });

  it("other OpenClaw errors are a fixed backend error, never the raw text", () => {
    const out = jobFailureReason("OpenClaw: invalid token sk-abc123 for http://x");
    expect(out).toBe("Assistant backend error");
    for (const raw of ["sk-abc123", "http://x", "OpenClaw", "18789"]) expect(out).not.toContain(raw);
  });

  it.each([
    ["declined: Taisun declined the restart of openclaw", "declined: not approved"],
    ["busy: svc:openclaw is already running", "busy: already in progress"],
    ["expired: The restart request's deadline passed; nothing was run.", "expired: the request timed out"],
    ["refused: launchd job ai.openclaw.gateway", "refused: not allowed"],
  ])("keeps the gate prefix: %s", (raw, want) => {
    const out = jobFailureReason(raw);
    expect(out).toBe(want);
    expect(out).not.toMatch(/openclaw|launchd|18789/i);
  });

  it("unrelated errors are unchanged", () => {
    expect(jobFailureReason("Ollama request failed")).toBe("local model (Ollama) error");
    expect(jobFailureReason("boom")).toBe("generation failed");
  });
});

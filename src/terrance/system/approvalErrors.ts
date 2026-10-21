/**
 * Machine-readable error prefixes from the native approval gate
 * (src-tauri/src/commands/approval_gate.rs, restart_services.rs).
 * Match on the prefix only; the text after it is human-readable and may change.
 */
export type ApprovalOutcome = "declined" | "busy" | "expired" | "refused" | "changed" | "error";

const PREFIXES: Exclude<ApprovalOutcome, "error">[] = ["declined", "busy", "expired", "refused", "changed"];

export function approvalOutcome(error: unknown): ApprovalOutcome {
  const text = (error instanceof Error ? error.message : String(error ?? "")).trimStart();
  for (const p of PREFIXES) if (text.startsWith(`${p}:`)) return p;
  return "error";
}

export function isDeclined(error: unknown): boolean {
  return approvalOutcome(error) === "declined";
}

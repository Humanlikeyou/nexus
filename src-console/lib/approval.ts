import { isDeclined } from "@/terrance/system/approvalErrors";

/**
 * terrence_actions approve returns Ok(action) with status "cancelled" and an
 * error starting "declined:" when the user clicks Deny. That is a normal,
 * quiet outcome, not a failure.
 */
export function isDeclinedAction(action: { status?: string; error?: string | null } | null | undefined): boolean {
  return !!action && action.status === "cancelled" && isDeclined(action.error ?? "");
}

export function approvalNotice(title: string, outcome: { result?: { status?: string; error?: string | null } | null; error?: unknown }): string {
  if ("error" in outcome && outcome.error !== undefined) {
    return isDeclined(outcome.error) ? `${title} declined` : `Approving ${title} unavailable`;
  }
  return isDeclinedAction(outcome.result) ? `${title} declined` : `Approving ${title} complete`;
}

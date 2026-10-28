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

export interface ApprovalDeps<R> {
  approve: () => Promise<R>;
  refresh: () => Promise<void>;
  setBusy: (busy: boolean) => void;
  setNotice: (notice: string | null) => void;
}

/**
 * Approve flow for a Amara action. On any error (including a Deny that
 * rejects with "declined:") the snapshot is still refreshed so the row stops
 * showing "awaiting approval" with live buttons; a refresh failure is swallowed.
 */
export async function runApproval<R extends { status?: string; error?: string | null } | null | undefined>(
  title: string,
  deps: ApprovalDeps<R>,
): Promise<void> {
  deps.setBusy(true);
  deps.setNotice(`Approving ${title} in progress`);
  try {
    const result = await deps.approve();
    deps.setNotice(approvalNotice(title, { result }));
    await deps.refresh();
  } catch (error) {
    deps.setBusy(false);
    deps.setNotice(approvalNotice(title, { error }));
    try {
      await deps.refresh();
    } catch (refreshError) {
      console.warn("[console] refresh after approval error failed", refreshError);
    }
  }
}

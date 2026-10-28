import { describe, it, expect, vi } from "vitest";
import { approvalNotice, isDeclinedAction, runApproval } from "./approval";

describe("terrence_actions approve Deny handling", () => {
  it("Ok(cancelled) with a declined: error is a quiet decline", () => {
    const result = { status: "cancelled", error: "declined: Denied in the native approval dialog." };
    expect(isDeclinedAction(result)).toBe(true);
    expect(approvalNotice("Send report", { result })).toBe("Send report declined");
  });
  it("a rejected invoke with declined: is also a decline; other errors are unavailable", () => {
    expect(approvalNotice("X", { error: "declined: no" })).toBe("X declined");
    expect(approvalNotice("X", { error: "boom" })).toBe("Approving X unavailable");
  });
  it("a normal approval completes", () => {
    expect(approvalNotice("X", { result: { status: "approved", error: null } })).toBe("Approving X complete");
    expect(isDeclinedAction({ status: "cancelled", error: "user cancelled" })).toBe(false);
  });
  it("approve_terrence_action Deny now rejects with declined: and is shown as a quiet decline", () => {
    const rejection = new Error("declined: Denied in the native approval dialog. Nothing was changed.");
    expect(approvalNotice("Send report", { error: rejection })).toBe("Send report declined");
  });
});

describe("runApproval refreshes after errors", () => {
  const deps = (approve: () => Promise<{ status?: string; error?: string | null } | null | undefined>, refresh = vi.fn(async () => {})) =>
    ({ approve, refresh, setBusy: vi.fn(), setNotice: vi.fn() });

  it("a declined: rejection refreshes so the row leaves 'awaiting approval'", async () => {
    const d = deps(async () => { throw new Error("declined: Denied in the native approval dialog."); });
    await runApproval("Send report", d);
    expect(d.refresh).toHaveBeenCalledTimes(1);
    expect(d.setNotice).toHaveBeenLastCalledWith("Send report declined");
    expect(d.setBusy).toHaveBeenLastCalledWith(false);
  });

  it("any other error also refreshes, and a failing refresh does not throw", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const d = deps(async () => { throw new Error("boom"); }, vi.fn(async () => { throw new Error("offline"); }));
    await expect(runApproval("X", d)).resolves.toBeUndefined();
    expect(d.refresh).toHaveBeenCalledTimes(1);
    expect(d.setNotice).toHaveBeenLastCalledWith("Approving X unavailable");
  });

  it("success refreshes once", async () => {
    const d = deps(async () => ({ status: "approved", error: null }));
    await runApproval("X", d);
    expect(d.refresh).toHaveBeenCalledTimes(1);
    expect(d.setNotice).toHaveBeenLastCalledWith("Approving X complete");
  });
});

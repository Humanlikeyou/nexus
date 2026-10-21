import { describe, it, expect } from "vitest";
import { approvalNotice, isDeclinedAction } from "./approval";

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

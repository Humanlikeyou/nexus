import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NavyRoot } from "./NavyRoot";
import { SAMPLE_OFF_LINES, SAMPLE_ON_LINES, resetLines } from "./lines";
import { navyAdapter, unwiredNavy } from "./adapters";
import { enterNavy, handleNavyPrompt, navyStore, beginHold, reachHoldConfirm } from "./store";
import { NAVY_PREVIEW_KEY } from "./types";

const banned = /navy|red light|quiet mode/i;

describe("design-1111x navy look", () => {
  beforeEach(() => {
    navyStore.reset();
    resetLines();
    localStorage.setItem("nexus.sampleAdapters", "1");
    localStorage.removeItem(NAVY_PREVIEW_KEY);
    document.documentElement.removeAttribute("data-navy");
    document.documentElement.removeAttribute("data-navy-phase");
    document.documentElement.setAttribute("data-reduce-motion", "true");
  });
  afterEach(() => {
    cleanup();
    navyStore.reset();
    localStorage.removeItem("nexus.sampleAdapters");
    document.documentElement.removeAttribute("data-reduce-motion");
    document.documentElement.removeAttribute("data-navy");
    document.documentElement.removeAttribute("data-navy-phase");
  });

  it("sample lines never name the look", () => {
    for (const s of [...SAMPLE_ON_LINES, ...SAMPLE_OFF_LINES]) expect(s).not.toMatch(banned);
  });

  it("the live flag is not written to disk", () => {
    const spy = vi.spyOn(Storage.prototype, "setItem");
    handleNavyPrompt("Amara, go dark.", "chat");
    for (const call of spy.mock.calls) expect(call[0]).not.toBe("nexus.navy");
    expect(localStorage.getItem(NAVY_PREVIEW_KEY)).toBeNull();
    spy.mockRestore();
  });

  it("the phrase turns it on, and a restart (store reset) turns it off", async () => {
    render(<NavyRoot><div>home</div></NavyRoot>);
    act(() => { handleNavyPrompt("Amara, go dark.", "chat"); });
    await waitFor(() => expect(navyStore.get().phase).toBe("on"), { timeout: 2000 });
    expect(screen.getByRole("status")).toHaveTextContent(/Lights out|Going dark|with you|leaves this Mac|just us|got smaller|All quiet|off the grid/);
    expect(screen.getByRole("status")).toHaveTextContent("Sample data");
    expect(document.body.textContent).not.toMatch(banned);
    act(() => { navyStore.reset(); });
    await waitFor(() => expect(document.documentElement.getAttribute("data-navy")).toBeNull());
  });

  it("typed 'back to normal' and Confirm on the orb both turn it off", async () => {
    render(<NavyRoot><div>home</div></NavyRoot>);
    act(() => { enterNavy("chat"); });
    await waitFor(() => expect(navyStore.get().phase).toBe("on"), { timeout: 2000 });
    act(() => { handleNavyPrompt("Amara, back to normal.", "chat"); });
    await waitFor(() => expect(navyStore.get().phase).toBe("off"), { timeout: 2000 });

    act(() => { enterNavy("voice"); });
    await waitFor(() => expect(navyStore.get().phase).toBe("on"), { timeout: 2000 });
    act(() => { beginHold(); reachHoldConfirm(); });
    expect(screen.getByRole("dialog", { name: "Back to normal?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(navyStore.get().phase).toBe("off"));
  });

  it("speaking is only requested when he asked by voice; live mode does not invent a line from the adapter", () => {
    localStorage.removeItem("nexus.sampleAdapters");
    expect(unwiredNavy.speak("Lights out.").state).toBe("unwired");
    expect(unwiredNavy.localOnly().state).toBe("unwired");
    localStorage.setItem("nexus.sampleAdapters", "1");
    const w = navyAdapter().speak("Lights out.");
    expect(w.state).toBe("sample");
    if (w.state === "sample") expect(w.value.text).toBe("Lights out.");
  });

  it("handleNavyPrompt ignores unrelated chat", () => {
    expect(handleNavyPrompt("pull up bitcoin on TradingView", "chat")).toBe(false);
    expect(navyStore.get().phase).toBe("off");
  });
});

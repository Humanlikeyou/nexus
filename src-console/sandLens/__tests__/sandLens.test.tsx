import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const gl = vi.hoisted(() => ({ ok: true }));
vi.mock("../engine", () => {
  class FakeEngine {
    state = "off"; onWakeDone?: () => void; onModeChange?: () => void;
    setState(s: string) { this.state = s; } setMaterial() {} setCalm() {} setAmbience() {} setVoice() {} moment() {}
    start() {} stop() {} freeze() {} dispose() {} stats() { return { state: this.state }; }
  }
  return { SandEngine: FakeEngine, STATE_NUM: { off: 0, wake: 1, listen: 2, speak: 3 } };
});
vi.mock("../gl", () => ({ webgl2Available: () => gl.ok }));

import { NavyRoot } from "../../navyMode/NavyRoot";
import { enterNavy, navyStore } from "../../navyMode/store";
import { SAND_KEY, __resetSandSettings } from "../settings";
import { SandLensLayer, sandStateFor } from "../SandLens";

describe("design-1118b sand lens layer", () => {
  beforeEach(() => { gl.ok = true; localStorage.removeItem(SAND_KEY); __resetSandSettings(); });
  afterEach(() => { cleanup(); navyStore.reset(); localStorage.removeItem(SAND_KEY); __resetSandSettings(); document.documentElement.removeAttribute("data-reduce-motion"); });

  it("maps the voice to sand states (voice off = sand rests on the chat card)", () => {
    expect(sandStateFor("speaking", "")).toBe("speak");
    expect(sandStateFor("listening", "")).toBe("listen");
    expect(sandStateFor("thinking", "")).toBe("listen");
    expect(sandStateFor("idle", "Amara's voice is off")).toBe("off");
    expect(sandStateFor("error", "Can't reach Amara's voice")).toBe("off");
  });

  it("draws nothing where the lens was: only an invisible hit area plus the shared click-through canvas", () => {
    const { container } = render(<div className="landing__lens"><SandLensLayer voice="idle" line="Amara's voice is off" recovery={null} /></div>);
    const hit = screen.getByTestId("lens-hit");
    expect(hit).toHaveAttribute("aria-hidden", "true");
    expect(hit.childElementCount).toBe(0);
    expect(screen.getByTestId("sand-canvas")).toHaveClass("sand-lens__canvas");
    expect(container.querySelector(".amara-lens")).toBeNull(); // no ring, bezel or plate from the old lens
    expect(hit.getAttribute("data-sand-state")).toBe("off");
  });

  it("the canvas never takes clicks; the hit area does", () => {
    render(<SandLensLayer voice="listening" line="" recovery={null} />);
    expect(getComputedStyle(screen.getByTestId("sand-canvas")).pointerEvents).toBe("none"); // also !important in sandLens.css
    expect(getComputedStyle(screen.getByTestId("lens-hit")).pointerEvents).not.toBe("none");
    const onClick = vi.fn(); const hit = screen.getByTestId("lens-hit"); hit.addEventListener("click", onClick);
    fireEvent.click(hit); expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("falls back to the classic static lens without WebGL2, and when Classic is chosen in Settings", () => {
    gl.ok = false;
    const { container, unmount } = render(<SandLensLayer voice="listening" line="" recovery={null} />);
    expect(container.querySelector(".amara-lens")).not.toBeNull();
    expect(screen.queryByTestId("sand-canvas")).toBeNull();
    unmount();
    gl.ok = true;
    localStorage.setItem(SAND_KEY, JSON.stringify({ lens: "classic" })); __resetSandSettings();
    const r2 = render(<SandLensLayer voice="listening" line="" recovery={null} />);
    expect(r2.container.querySelector(".amara-lens")).not.toBeNull();
    expect(screen.queryByTestId("lens-hit")).toBeNull();
  });

  it("a long-press on the invisible hit area still starts the navy exit hold", async () => {
    document.documentElement.setAttribute("data-reduce-motion", "true");
    localStorage.setItem("nexus.sampleAdapters", "1");
    render(<NavyRoot><div className="landing__lens"><SandLensLayer voice="listening" line="" recovery={null} /></div></NavyRoot>);
    act(() => { enterNavy("chat"); });
    await waitFor(() => expect(navyStore.get().phase).toBe("on"), { timeout: 2000 });
    fireEvent.pointerDown(screen.getByTestId("lens-hit"));
    expect(navyStore.get().hold).toBe("holding");
    localStorage.removeItem("nexus.sampleAdapters");
  });
});

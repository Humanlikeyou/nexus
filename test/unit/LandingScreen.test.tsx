import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LANDING_ENTRIES } from "../../src-console/app/routes";
import { LandingField } from "../../src-console/screens/LandingScreen";

const heart = vi.hoisted(() => ({ value: null as null | { bpm: number; at: number; samples: Array<{ bpm: number; at: number }> } }));
vi.mock("../../src-console/hooks/useLiveHeartRate", () => ({
  useLiveHeartRate: () => heart.value,
  beatSeconds: (bpm: number | null | undefined) => (typeof bpm === "number" && bpm > 0 ? 60 / bpm : null),
}));
vi.mock("../../src-console/hooks/useHomeBrief", () => ({ useHomeBrief: () => null, freshBrief: () => null, usableWeather: () => null }));
vi.mock("../../src-console/hooks/useLandingVoice", () => ({ useLandingVoice: () => ({ state: "idle", line: "Ready when you are." }) }));
vi.mock("../../src-console/components/AmaraLens/AmaraLens", () => ({ AmaraLens: () => <i data-testid="lens" />, AmaraVoiceAnnouncer: () => null }));

describe("LandingField", () => {
  it("shows Home: a hidden h1, the voice line, and a button per landing entry", () => {
    heart.value = null;
    render(<LandingField onSelect={() => undefined} onToday={() => undefined} />);
    expect(screen.getByRole("heading", { level: 1, name: "Command Center" })).toBeInTheDocument();
    expect(screen.getByText("Ready when you are.")).toBeInTheDocument();
    const nav = screen.getByRole("navigation", { name: "Amara OS navigation" });
    expect(nav.querySelectorAll("button")).toHaveLength(LANDING_ENTRIES.length);
  });

  it("routes Today to onToday and every other entry to onSelect", () => {
    heart.value = null;
    const onSelect = vi.fn(), onToday = vi.fn();
    render(<LandingField onSelect={onSelect} onToday={onToday} />);
    for (const entry of LANDING_ENTRIES) {
      fireEvent.click(screen.getByRole("button", { name: new RegExp(entry.label) }));
      if (entry.id === "terrance") expect(onToday).toHaveBeenCalled(); else expect(onSelect).toHaveBeenCalledWith(entry.id);
    }
    expect(onToday).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledTimes(LANDING_ENTRIES.length - 1);
  });

  it("marks the chat slot collapsed or expanded, and omits it with no chat", () => {
    heart.value = null;
    const { container, rerender } = render(<LandingField onSelect={() => undefined} onToday={() => undefined} />);
    expect(container.querySelector(".landing")).not.toHaveAttribute("data-chat");
    rerender(<LandingField onSelect={() => undefined} onToday={() => undefined} chat={<p>chat</p>} />);
    expect(container.querySelector(".landing")).toHaveAttribute("data-chat", "collapsed");
    rerender(<LandingField onSelect={() => undefined} onToday={() => undefined} chat={<p>chat</p>} chatExpanded />);
    expect(container.querySelector(".landing")).toHaveAttribute("data-chat", "expanded");
    expect(screen.getByText("chat")).toBeInTheDocument();
  });

  it("shows no heart rate without a live reading, and the bpm when there is one", () => {
    heart.value = null;
    const { unmount } = render(<LandingField onSelect={() => undefined} onToday={() => undefined} />);
    expect(screen.queryByRole("img", { name: /Heart rate/ })).toBeNull();
    unmount();
    heart.value = { bpm: 62, at: 100, samples: [{ bpm: 60, at: 99 }, { bpm: 62, at: 100 }] };
    render(<LandingField onSelect={() => undefined} onToday={() => undefined} />);
    expect(screen.getByRole("img", { name: "Heart rate 62 beats per minute" })).toBeInTheDocument();
    expect(screen.getByText("62")).toBeInTheDocument();
  });
});

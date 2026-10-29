import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
vi.mock("@/lib/api", () => ({ packItUp: vi.fn(), unpackBackup: vi.fn(), pickBackupArchive: vi.fn() }));
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { SettingsField } from "./SettingsScreen";

const snap = (extra: Partial<SystemSnapshot> = {}): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const base = { theme: "system" as const, setTheme: vi.fn(), reduceMotion: false, setReduceMotion: vi.fn() };

describe("Settings (§11) + Devices (§9)", () => {
  it("groups in order with the About wordmark and version", () => {
    render(<SettingsField data={snap()} {...base} />);
    const titles = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent);
    expect(titles).toEqual(["Appearance", "Devices", "General", "Backup", "Restore", "About & Diagnostics"]);
    const mark = screen.getByRole("img", { name: "Amara OS" });
    expect(mark.tagName.toLowerCase()).toBe("svg");
    expect(mark).toHaveClass("wordmark--about");
    expect(screen.queryByText(/Wordmark (set in|drawn after)/)).toBeNull();
    expect(screen.getByText(/Version 0\.1\.0/)).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).toBeNull();
  });
  it("reduce motion toggles; general rows are facts, not toggles", () => {
    const setReduceMotion = vi.fn();
    render(<SettingsField data={snap()} {...base} setReduceMotion={setReduceMotion} />);
    fireEvent.click(screen.getByRole("switch", { name: "Reduce motion" }));
    expect(setReduceMotion).toHaveBeenCalledWith(true);
    expect(screen.getAllByRole("switch")).toHaveLength(1);
    expect(screen.getByText("Every 15 seconds")).toBeInTheDocument();
  });
  it("Muse: Find Muse scans; nearby headsets show signal words; error offers Try Again", () => {
    const onScan = vi.fn(); const onConnect = vi.fn();
    const device = { name: "Muse-1234", address: "aa", rssi: -70 } as never;
    render(<SettingsField data={snap()} {...base} devices={[device]} error="Bluetooth is unavailable." onScan={onScan} onConnect={onConnect} />);
    const devices = document.getElementById("devices")!;
    fireEvent.click(within(devices).getByRole("button", { name: "Find Muse" }));
    expect(onScan).toHaveBeenCalled();
    expect(within(devices).getByText("Fair signal")).toBeInTheDocument();
    fireEvent.click(within(devices).getByRole("button", { name: "Connect" }));
    expect(onConnect).toHaveBeenCalledWith(device);
    expect(within(devices).getByRole("alert")).toHaveTextContent("Bluetooth is unavailable.");
    expect(within(devices).getByRole("button", { name: "Try Again" })).toBeInTheDocument();
  });
  it("diagnostics show — before the first load and Unavailable for down sources", () => {
    const { unmount } = render(<SettingsField data={INITIAL_SNAPSHOT} {...base} />);
    expect(screen.getAllByText("—").length).toBeGreaterThan(2);
    unmount();
    render(<SettingsField data={snap({ unavailableSources: [{ command: "get_muse_state", label: "Muse" }] })} {...base} onRetry={vi.fn()} />);
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThanOrEqual(2);
  });
});

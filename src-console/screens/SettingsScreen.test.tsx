import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
vi.mock("../legacy-core/lib/api", () => ({ packItUp: vi.fn(), unpackBackup: vi.fn(), pickBackupArchive: vi.fn() }));
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { SettingsField } from "./SettingsScreen";

const snap = (extra: Partial<SystemSnapshot> = {}): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const base = { theme: "system" as const, setTheme: vi.fn(), reduceMotion: false, setReduceMotion: vi.fn() };

describe("Settings (§11) + Devices (§9)", () => {
  it("General tab: groups in order with the About wordmark and version", () => {
    render(<SettingsField data={snap()} {...base} />);
    const titles = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent);
    // design-1111s: Devices, Home Assistant and WHOOP moved to the Devices tab, Backup to the Backup tab.
    // design-1118b: the Home lens group (sand or classic lens, Calm, sound) sits right after Appearance.
    expect(titles).toEqual(["Appearance", "Home lens", "Update settings", "Startup", "General", "Assistant", "About & Diagnostics"]);
    expect(screen.getByRole("heading", { level: 2, name: "Updates" })).toBeInTheDocument();
    const mark = screen.getByRole("img", { name: "Amara OS" });
    expect(mark.tagName.toLowerCase()).toBe("svg");
    expect(mark).toHaveClass("wordmark--about");
    expect(screen.queryByText(/Wordmark (set in|drawn after)/)).toBeNull();
    expect(screen.getByText(/Amara OS · Version 0\.1\.0/)).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).toBeNull();
  });
  it("reduce motion toggles; general rows are facts, not toggles", () => {
    const setReduceMotion = vi.fn();
    render(<SettingsField data={snap()} {...base} setReduceMotion={setReduceMotion} />);
    fireEvent.click(screen.getByRole("switch", { name: "Reduce motion" }));
    expect(setReduceMotion).toHaveBeenCalledWith(true);
    expect(screen.getAllByRole("switch").map((s) => s.getAttribute("aria-label"))).toEqual(["Reduce motion", "Calm", "Canopy light", "Sound", "Auto-update", "Check daily", "Open at login", "Allow Claude"]);
    expect(screen.getByText("Every 15 seconds")).toBeInTheDocument();
  });
  it("Muse: Find Muse scans; nearby headsets show signal words; error offers Retry", () => {
    const onScan = vi.fn(); const onConnect = vi.fn();
    const device = { name: "Muse-1234", address: "aa", rssi: -70 } as never;
    render(<SettingsField data={snap()} {...base} view="devices" devices={[device]} error="Bluetooth is unavailable." onScan={onScan} onConnect={onConnect} />);
    const devices = document.getElementById("devices")!;
    fireEvent.click(within(devices).getByRole("button", { name: "Find Muse" }));
    expect(onScan).toHaveBeenCalled();
    expect(within(devices).getByText("Fair signal")).toBeInTheDocument();
    fireEvent.click(within(devices).getByRole("button", { name: "Connect" }));
    expect(onConnect).toHaveBeenCalledWith(device);
    expect(within(devices).getByRole("alert")).toHaveTextContent("Bluetooth is unavailable.");
    expect(within(devices).getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
  it("diagnostics show — before the first load and Unavailable for down sources", () => {
    const { unmount } = render(<SettingsField data={INITIAL_SNAPSHOT} {...base} />);
    expect(screen.getAllByText("—").length).toBeGreaterThan(2);
    unmount();
    render(<SettingsField data={snap({ unavailableSources: [{ command: "get_muse_state", label: "Muse" }] })} {...base} onRetry={vi.fn()} />);
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThanOrEqual(2);
  });
  it("design-1111s tabs: each tab renders its own page, old anchors land on their tab", () => {
    const { unmount } = render(<SettingsField data={snap()} {...base} view="devices" />);
    expect(screen.getByRole("heading", { level: 1, name: "Devices" })).toBeInTheDocument();
    expect(document.getElementById("home-assistant")).not.toBeNull();
    expect(document.getElementById("noop-settings")).not.toBeNull();
    unmount();
    render(<SettingsField data={snap()} {...base} view="home-assistant" />);
    expect(screen.getByRole("heading", { level: 1, name: "Devices" })).toBeInTheDocument();
  });
});

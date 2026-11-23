import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("../legacy-core/lib/api", () => ({ packItUp: vi.fn(), unpackBackup: vi.fn(), pickBackupArchive: vi.fn() }));
import { sampleStandalone, type Outcome, type RestorePreview, type StandaloneAdapter, type UpdateStatus } from "./adapters";
import { BackupView } from "./BackupView";
import { ServicesView } from "./ServicesView";
import { NotificationsView } from "./NotificationsView";
import { UpdatesSection } from "./UpdatesSection";

afterEach(() => { localStorage.clear(); });
const live = <T,>(value: T) => ({ state: "live", value }) as const;
const over = (o: Partial<StandaloneAdapter>): StandaloneAdapter => ({ ...sampleStandalone(), ...o });

describe("Backup › Restore (workflow)", () => {
  const preview = (checksumOk = true): Outcome<{ preview: RestorePreview }> => ({ state: "done", preview: { file: "amara-backup-oct-21.tar.gz", madeOn: "Oct 21, 3:00 AM", version: "", size: "8.1 GB", checksumOk, keyFileFound: true, contents: [{ title: "goals.json", sub: "1 file · 3 B" }, { title: "nexus-config/replays", sub: "2 files · 4 MB" }] } });
  const open = () => fireEvent.click(screen.getByRole("button", { name: "Restore from a Backup…" }));

  it("lets you pick a backup, shows its real contents (no hard-coded 'Matches'), and restores that one with a result and next step", async () => {
    const previewBackup = vi.fn(async () => preview());
    const restore = vi.fn(async (): Promise<Outcome<{ lines: string[] }>> => ({ state: "done", lines: ["Safety backup of current data saved to /x"] }));
    render(<BackupView adapter={over({ previewBackup, restore })} />);
    open();
    expect(screen.getByRole("button", { name: "Save Today, Then Restore" })).toBeDisabled();
    expect(screen.queryByText("Matches")).toBeNull();
    const group = screen.getByRole("group", { name: "Backups" });
    fireEvent.click(within(group).getByRole("button", { name: "Oct 21, 3:00 AM" }));
    expect(await screen.findByText(/nexus-config\/replays · 2 files · 4 MB/)).toBeInTheDocument();
    expect(previewBackup).toHaveBeenCalledWith("b-oct-21");
    const dlg = within(screen.getByRole("dialog", { name: /Restore from/ }));
    expect(dlg.getByText(/replaces current Amara OS config and local data/)).toBeInTheDocument();
    expect(dlg.getByText(/saved to a safety backup/)).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save Today, Then Restore" })); });
    expect(restore).toHaveBeenCalledWith("b-oct-21");
    expect(await screen.findByText(/Restored from the backup of Oct 21, 3:00 AM\. Restart Amara OS to load the restored data\./)).toBeInTheDocument();
    expect(screen.getByText("Safety backup of current data saved to /x")).toBeInTheDocument();
  });
  it("a failed check or an unreadable backup can't be restored", async () => {
    const restore = vi.fn();
    const { unmount } = render(<BackupView adapter={over({ previewBackup: async () => preview(false), restore: restore as never })} />);
    open(); fireEvent.click(screen.getByRole("button", { name: "Oct 21, 3:00 AM" }));
    expect(await screen.findByText(/Failed: this backup can't be restored/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save Today, Then Restore" })).toBeDisabled();
    unmount();
    render(<BackupView adapter={over({ previewBackup: async () => ({ state: "failed", message: "A backup is running. Try again when it finishes." }), restore: restore as never })} />);
    open(); fireEvent.click(screen.getByRole("button", { name: "Oct 21, 3:00 AM" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("A backup is running");
    expect(screen.getByRole("button", { name: "Save Today, Then Restore" })).toBeDisabled();
    expect(restore).not.toHaveBeenCalled();
  });
  it("declining the native approval says nothing changed", async () => {
    render(<BackupView adapter={over({ previewBackup: async () => preview(), restore: async () => ({ state: "declined", message: "Restore cancelled. Nothing was changed." }) })} />);
    open(); fireEvent.click(screen.getByRole("button", { name: "Oct 21, 3:00 AM" }));
    await screen.findByText(/goals\.json · 1 file/);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save Today, Then Restore" })); });
    expect(await screen.findByText("Restore cancelled. Nothing was changed.")).toBeInTheDocument();
    expect(screen.queryByText(/Restart Amara OS to load/)).toBeNull();
  });
  it("Change… opens the folder picker and says where backups go; cancel is silent; a refusal says why", async () => {
    const pick = vi.fn<StandaloneAdapter["pickBackupDir"]>(async () => ({ state: "done", dir: "~/Backups2" }));
    render(<BackupView adapter={over({ pickBackupDir: pick })} />);
    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    expect(await screen.findByText(/Backups will go to ~\/Backups2/)).toBeInTheDocument();
    pick.mockResolvedValueOnce({ state: "failed", message: "Choose a folder inside your home folder." });
    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    expect(await screen.findByText("Choose a folder inside your home folder.")).toBeInTheDocument();
    pick.mockResolvedValueOnce({ state: "cancelled" });
    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    await waitFor(() => expect(pick).toHaveBeenCalledTimes(3));
  });
  it("no dead Finder buttons are left", () => {
    render(<BackupView adapter={sampleStandalone()} />);
    expect(screen.queryByRole("button", { name: /Finder/ })).toBeNull();
  });
});

describe("Services restart (workflow)", () => {
  const click = async (a: StandaloneAdapter) => { render(<ServicesView adapter={a} />); await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Restart Automations (n8n)" })); }); };
  it.each([
    [{ state: "declined" }, /Cancelled\. Nothing changed\./],
    [{ state: "unsupported" }, /can't be restarted from here/],
    [{ state: "gave_up" }, /3 times and still isn't working/],
    [{ state: "failed" }, /didn't restart\. Check its logs/],
    [{ state: "done", healthy: true }, /Automations \(n8n\) is healthy again\./],
    [{ state: "done", healthy: false }, /restarted, but hasn't reported healthy yet/],
  ] as const)("outcome %j says %s", async (outcome, re) => {
    await click(over({ serviceAction: async () => outcome as never }));
    expect(screen.getByRole("status")).toHaveTextContent(re);
    expect(screen.getByRole("status")).not.toHaveTextContent(/needs wiring/);
  });
  it("a failure offers Open logs, which opens that service's logs", async () => {
    await click(over({ serviceAction: async () => ({ state: "failed" }) }));
    fireEvent.click(within(screen.getByRole("status")).getByRole("button", { name: "Open logs" }));
    expect(await screen.findByText(/A check took 6\.2 s to answer/)).toBeInTheDocument();
  });
  it("Copy for Claude copies the shown logs; Show in Finder is gone; Start is not offered without a backend", async () => {
    const write = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText: write }, configurable: true });
    render(<ServicesView adapter={sampleStandalone()} />);
    expect(screen.queryByRole("button", { name: /^Start / })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Logs for Automations (n8n)" }));
    await screen.findByText(/A check took 6\.2 s to answer/);
    expect(screen.queryByRole("button", { name: "Show in Finder" })).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Copy for Claude" })); });
    expect(write).toHaveBeenCalledTimes(1);
    const text = (write.mock.calls[0] as unknown as [string])[0];
    expect(text).toContain("A check took 6.2 s to answer");
    expect(text).toContain("14:01:07 WARN  health  check took 6214 ms");
    expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
  });
});

describe("Notifications (workflow)", () => {
  const relay = (enabled: boolean) => () => live({ enabled, kinds: { sleep: true, recovery: true, effort: true, battery: true, sync: true, lucid: false, illness: true }, lastSent: null });
  it("keys set but relay off: says 'Ready · turn it on' beside the toggle and gives Send Test a visible reason", () => {
    render(<NotificationsView adapter={over({ relay: relay(false) })} />);
    expect(screen.getByText(/Ready · turn it on/)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Telegram relay" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send Test…" })).toBeDisabled();
    expect(screen.getByText(/Turn the relay on to send a test/)).toBeInTheDocument();
  });
  it.each([
    ["off", /outbound is disabled for this session/],
    ["declined", /Cancelled\. Nothing was sent\./],
    ["telegram", /Telegram didn't accept the message/],
    ["missing", /Add your bot token and chat first/],
    ["done", /Test message sent/],
  ] as const)("Send Test outcome %s has its own message", async (state, re) => {
    render(<NotificationsView adapter={over({ relay: relay(true), relaySendTest: async () => ({ state }) })} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Send Test…" })); });
    expect(screen.getByRole("status")).toHaveTextContent(re);
  });
  it("Write another asks for a fresh preview and reports what happened", async () => {
    const again = vi.fn<StandaloneAdapter["writeAnotherPreview"]>(async () => ({ state: "done" }));
    render(<NotificationsView adapter={over({ writeAnotherPreview: again })} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Write another ›" })); });
    expect(again).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("Wrote a new preview. Nothing was sent.");
    again.mockResolvedValueOnce({ state: "failed", message: "No reading for this alert yet." });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Write another ›" })); });
    expect(screen.getByRole("status")).toHaveTextContent("No reading for this alert yet.");
    expect(document.body.textContent).not.toMatch(/Write another needs wiring/);
  });
});

describe("Updates (workflow)", () => {
  const status = (o: Partial<UpdateStatus>): UpdateStatus => ({ current: "0.1.0", available: null, lastCheck: "not checked this session", autoUpdate: true, checkDaily: true, history: [], ...o });
  it("an off updater says it is off and why, never 'up to date' and never a fake update", () => {
    const { container } = render(<UpdatesSection adapter={over({ updates: () => live(status({ backend: "not_configured" })) })} />);
    expect(screen.getByText("Updates are off")).toBeInTheDocument();
    expect(container.textContent).toMatch(/switched off in this build/);
    expect(container.textContent).not.toMatch(/is up to date|Update ready|Tonight at/);
    expect(screen.getByRole("button", { name: "Check Now" })).toBeDisabled();
    expect(container.textContent).toMatch(/no effect while updates are off/);
  });
  it("paused outbound is shown as paused with its reason", () => {
    render(<UpdatesSection adapter={over({ updates: () => live(status({ backend: "outbound_disabled" })) })} />);
    expect(screen.getByText("Update checks are paused")).toBeInTheDocument();
    expect(screen.getAllByText(/Outbound is disabled for this session/).length).toBeGreaterThan(0);
  });
  it("Check Now runs the check and says so; prefs save with a visible result", async () => {
    const check = vi.fn(async (): Promise<Outcome> => ({ state: "done" }));
    const prefs = vi.fn(async () => live({ ok: true as const }));
    render(<UpdatesSection adapter={over({ updates: () => live(status({ backend: "idle" })), checkUpdate: check, setUpdatePrefs: prefs })} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Check Now" })); });
    expect(check).toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent("Checked just now.");
    await act(async () => { fireEvent.click(screen.getByRole("switch", { name: "Auto-update" })); });
    expect(prefs).toHaveBeenCalledWith({ autoUpdate: false });
    expect(screen.getByRole("status")).toHaveTextContent("Saved.");
  });
  it("an announced update can be installed; a refusal says nothing changed and how to update instead", async () => {
    const install = vi.fn(async (): Promise<Outcome> => ({ state: "unsupported" }));
    render(<UpdatesSection adapter={over({ updates: () => live(status({ backend: "available", available: { version: "0.2.0", notes: [], scheduledFor: "" } })), installUpdate: install })} />);
    expect(screen.getByText("Update available: Amara OS 0.2.0")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Tonight at/ })).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Restart and Update Now" })); });
    expect(install).toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(/Install the new version from the DMG\. Nothing changed\./);
  });
});

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("../legacy-core/lib/api", () => ({ packItUp: vi.fn(), unpackBackup: vi.fn(), pickBackupArchive: vi.fn() }));
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { BACKUP_DEFAULTS, resetStandaloneAdapter, sampleStandalone, standaloneAdapter, unwiredStandalone, type StandaloneAdapter } from "./adapters";
import { ServicesView, servicesStatement } from "./ServicesView";
import { servicesFooter } from "./servicesSummary";
import { NotificationsView } from "./NotificationsView";
import { BackupView } from "./BackupView";
import { UpdatesSection } from "./UpdatesSection";
import { WhoopPanel, TAKEOVER_TITLE } from "./WhoopPanel";
import { FirstRun } from "./FirstRun";
import { HOST_VIEWS } from "../app/routes";
import { settingsTab } from "../app/settingsTabs";
import { valueOf } from "../lib/sampleAdapters";

const data: SystemSnapshot = { ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1 };
/** Absorbed into Amara (Taisun, 14:59 JST Nov 11): no NOOP / NOOP.app / "separate program" / OpenClaw framing anywhere. */
const FORBIDDEN = /NOOP|OpenClaw|separate program/;
const SENTINEL = "SENTINEL-telegram-7f3a-never-render";

afterEach(() => { localStorage.clear(); resetStandaloneAdapter(); });

const views = (a: StandaloneAdapter) => [
  <ServicesView key="s" adapter={a} />, <NotificationsView key="n" adapter={a} />, <BackupView key="b" adapter={a} />,
  <UpdatesSection key="u" adapter={a} />, <WhoopPanel key="w" adapter={a} data={data} />, <FirstRun key="f" adapter={a} onClose={() => {}} />,
];

describe("design-1111s adapters", () => {
  it("the app is unwired unless sample mode is on", () => {
    expect(standaloneAdapter()).toBe(unwiredStandalone);
    localStorage.setItem("nexus.sampleAdapters", "1");
    resetStandaloneAdapter();
    expect(standaloneAdapter().services().state).toBe("sample");
  });
  it("backup defaults: nightly 03:00 to ~/Amara Backups, 14 kept, one before every update", () => {
    expect(BACKUP_DEFAULTS).toMatchObject({ nightly: true, at: "03:00", dir: "~/Amara Backups", keep: 14, beforeUpdate: true });
  });
  it("Settings tabs use the Business tab bar, in Taisun's order", () => {
    expect(HOST_VIEWS.settings!.map((v) => v.label)).toEqual(["General", "Services", "Devices", "Notifications", "Backup", "Voice", "Memory"]);
    expect([settingsTab("home-assistant"), settingsTab("about"), settingsTab("nope"), settingsTab("backup")]).toEqual(["devices", "main", "main", "backup"]);
  });
  it("OpenClaw is gone from Services; health sync is a built-in Amara service", () => {
    const list = valueOf(sampleStandalone().services())!.services;
    expect(list.some((s) => /openclaw/i.test(`${s.id} ${s.name}`))).toBe(false);
    expect(list.filter((s) => s.builtIn).map((s) => s.name)).toEqual(["Health sync · reader", "Health sync"]);
    expect(JSON.stringify(list)).not.toMatch(FORBIDDEN);
  });
});

describe("no NOOP or OpenClaw framing in any screen", () => {
  for (const [mode, make] of [["unwired", () => unwiredStandalone], ["sample", () => sampleStandalone()], ["sample, transition", () => sampleStandalone({ whoopOwner: "legacy", relayKeys: false, backupRunning: true, updateReady: true })]] as const) {
    it(mode, () => {
      for (const v of views(make())) { const { container, unmount } = render(v); expect(container.textContent).not.toMatch(FORBIDDEN); unmount(); }
    });
  }
  it("unwired screens say Needs wiring and never Sample data", () => {
    for (const v of views(unwiredStandalone)) { const { container, unmount } = render(v); expect(container.textContent).toMatch(/Needs wiring/); expect(container.textContent).not.toMatch(/Sample data/); unmount(); }
  });
});

describe("Services", () => {
  it("all states in plain words; restarting can't be restarted; logs open in plain words", async () => {
    render(<ServicesView adapter={sampleStandalone()} />);
    expect(screen.getByRole("heading", { level: 1, name: "Services" })).toBeInTheDocument();
    expect(screen.getByText("1 needs attention")).toBeInTheDocument();
    for (const w of ["Healthy", "Slow", "Restarting", "Stopped"]) expect(screen.getAllByText(w).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Restart Crews" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Logs for Automations (n8n)" }));
    expect(await screen.findByText(/A check took 6\.2 s to answer/)).toBeInTheDocument();
  });
  it("transition: health sync waits while another app has the strap, with a Why? in Amara's words", () => {
    render(<ServicesView adapter={sampleStandalone({ whoopOwner: "legacy" })} />);
    const row = document.querySelector("[data-service=health-sync]")!;
    expect(row).toHaveTextContent("Waiting");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "Why?" }));
    expect(screen.getByText(/Another app is using your WHOOP; Amara will take over/)).toBeInTheDocument();
  });
  it("statement and sidebar footer", () => {
    expect(servicesStatement([])).toBe("All healthy");
    expect(servicesFooter(unwiredStandalone)).toBeNull();
    expect(servicesFooter(sampleStandalone())).toEqual({ text: "1 service needs attention", attention: 1 });
  });
});

describe("Notifications (Telegram relay)", () => {
  it("no quiet hours and no rate limit: only the switch, the keys, the alert choices and the preview", () => {
    const { container } = render(<NotificationsView adapter={sampleStandalone()} />);
    expect(container.textContent).not.toMatch(/Quiet hours|At most|\/ 30 min|per hour|quiet/i);
    expect(screen.getByRole("switch", { name: "Telegram relay" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("group", { name: "Alerts to send" }).querySelectorAll("button")).toHaveLength(7);
    expect(screen.getByRole("button", { name: "Lucid cues" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByText(/Morning, Taisun/)).toBeInTheDocument();
  });
  it("not set: relay is off and can't be turned on until the keys are added", () => {
    render(<NotificationsView adapter={sampleStandalone({ relayKeys: false })} />);
    expect(screen.getByText("Telegram relay is off")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Telegram relay" })).toBeDisabled();
    expect(screen.getAllByText("Not set").length).toBeGreaterThanOrEqual(2);
  });
  it("a pasted token goes to the Keychain setter and never reaches the DOM", async () => {
    const a = sampleStandalone({ relayKeys: false });
    const spy = vi.spyOn(a, "setSecret");
    const { container } = render(<NotificationsView adapter={a} />);
    fireEvent.click(screen.getAllByRole("button", { name: "Add…" })[0]);
    const input = container.querySelector<HTMLInputElement>("input[type=password]")!;
    fireEvent.change(input, { target: { value: SENTINEL } });
    expect(container.innerHTML).not.toContain(SENTINEL);
    fireEvent.click(screen.getAllByRole("button", { name: "Save to Keychain", hidden: true })[0]);
    await waitFor(() => expect(spy).toHaveBeenCalledWith("telegram_bot_token", SENTINEL));
    expect(input.value).toBe("");
    expect(container.innerHTML).not.toContain(SENTINEL);
    expect(document.body.innerHTML).not.toContain(SENTINEL);
  });
});

describe("Backup and Move to a new Mac", () => {
  it("defaults, keys never backed up, the n8n key file and Set/Not set only", () => {
    const { container } = render(<BackupView adapter={sampleStandalone()} />);
    expect(screen.getByText("Every night at 3:00 AM")).toBeInTheDocument();
    expect(screen.getByText("~/Amara Backups · keeps the last 14")).toBeInTheDocument();
    expect(screen.getByText("Before each update")).toBeInTheDocument();
    expect(screen.getByText(/Keys are never backed up/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "n8n key file" })).toBeInTheDocument();
    expect(screen.getByText(/Unlock the n8n key file/)).toBeInTheDocument();
    const keys = within(screen.getByRole("region", { name: "Keys on this Mac" }));
    expect(keys.getAllByText(/^(Set|Not set)$/)).toHaveLength(6);
    expect(container.textContent).toMatch(/Save Today, Then Restore/);
  });
  it("the key-file passphrase is write-only: mismatches are caught, and it never reaches the DOM", async () => {
    const a = sampleStandalone();
    const spy = vi.spyOn(a, "makeKeyFile");
    const { container } = render(<BackupView adapter={a} />);
    fireEvent.click(screen.getByRole("button", { name: "Make a New Key File…" }));
    const [p, q] = [...container.querySelectorAll<HTMLInputElement>("input[type=password]")];
    fireEvent.change(p, { target: { value: SENTINEL } });
    fireEvent.change(q, { target: { value: `${SENTINEL}x` } });
    fireEvent.click(screen.getByRole("button", { name: "Make Key File", hidden: true }));
    expect(await screen.findByText("The two passphrases don't match.")).toBeInTheDocument();
    fireEvent.change(q, { target: { value: SENTINEL } });
    fireEvent.click(screen.getByRole("button", { name: "Make Key File", hidden: true }));
    await waitFor(() => expect(spy).toHaveBeenCalledWith(SENTINEL));
    expect(p.value).toBe("");
    expect(document.body.innerHTML).not.toContain(SENTINEL);
  });
});

describe("WHOOP 4 (Settings › Devices)", () => {
  it("Amara owns the strap: no takeover notice, strap actions on", () => {
    render(<WhoopPanel adapter={sampleStandalone()} data={data} />);
    expect(screen.getByText("WHOOP 4 · Connected")).toBeInTheDocument();
    expect(screen.queryByText(TAKEOVER_TITLE)).toBeNull();
    expect(screen.getByRole("button", { name: "Sync Now" })).toBeEnabled();
  });
  it("transition only: 'another app is using your WHOOP; Amara will take over', strap actions off", () => {
    render(<WhoopPanel adapter={sampleStandalone({ whoopOwner: "legacy" })} data={data} />);
    expect(screen.getByRole("heading", { name: TAKEOVER_TITLE })).toBeInTheDocument();
    expect(TAKEOVER_TITLE).toBe("Another app is using your WHOOP; Amara will take over");
    for (const b of ["Sync Now", "Buzz Strap", "Re-pair…", "Forget"]) expect(screen.getByRole("button", { name: b })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Let Amara Take Over…" })).toBeEnabled();
  });
  it("unwired: no lock notice, actions disabled, Needs wiring", () => {
    const { container } = render(<WhoopPanel adapter={unwiredStandalone} data={data} />);
    expect(screen.queryByText(TAKEOVER_TITLE)).toBeNull();
    expect(screen.getByRole("button", { name: "Sync Now" })).toBeDisabled();
    expect(container.textContent).toMatch(/Needs wiring/);
  });
});

describe("First-run setup", () => {
  it("walks Welcome → Restore (n8n key file) → model by RAM → Getting ready → Preferences", () => {
    const onClose = vi.fn();
    render(<FirstRun adapter={sampleStandalone()} onClose={onClose} />);
    expect(screen.getByRole("dialog", { name: "Set up Amara OS" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Restore from a backup/ })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("heading", { level: 1, name: "Restore from a backup" })).toBeInTheDocument();
    expect(screen.getByText("n8n encryption key")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose Key File…" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(screen.getByRole("radio", { name: /Standard · recommended/ })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: /Large/ })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Download and Continue" }));
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });
  it("start fresh skips Restore; Preferences: cloud AI off, open at login on, auto-update on", () => {
    const onClose = vi.fn();
    render(<FirstRun adapter={{ ...sampleStandalone(), setupDownload: () => ({ state: "sample", value: { models: [], services: [] } }) }} onClose={onClose} />);
    fireEvent.click(screen.getByRole("radio", { name: /Start fresh/ }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("heading", { level: 1, name: "Pick the AI model size" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Download and Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("switch", { name: "Cloud AI" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("switch", { name: "Open at login" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("switch", { name: "Auto-update" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("button", { name: "Open Amara" }));
    expect(onClose).toHaveBeenCalled();
  });
});

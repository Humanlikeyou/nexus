import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import type { LoginItemStatus } from "../bindings.gen";

const status = vi.fn();
const enable = vi.fn();
const disable = vi.fn();
vi.mock("../bridge", () => ({ getLoginItemStatus: () => status(), enableLoginItem: () => enable(), disableLoginItem: () => disable() }));
import { OPEN_AT_LOGIN_COPY, StartupGroup, loginItemMessage } from "./OpenAtLogin";

const st = (p: Partial<LoginItemStatus> = {}): LoginItemStatus => ({ enabled: false, wanted: false, backend: "not_registered", needs_approval: false, mechanism: "smappservice", error: null, ...p } as LoginItemStatus);
const toggle = () => screen.getByRole("switch", { name: "Open at login" });

describe("StartupGroup (Open at login)", () => {
  beforeEach(() => { status.mockReset(); enable.mockReset(); disable.mockReset(); });

  it("reads status only on load, is off by default and never enables by itself", async () => {
    status.mockResolvedValue(st());
    render(<StartupGroup />);
    expect(toggle()).toBeDisabled();
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    expect(status).toHaveBeenCalledTimes(1);
    expect(enable).not.toHaveBeenCalled();
    expect(disable).not.toHaveBeenCalled();
  });

  it("stays off and disabled when the status read fails", async () => {
    status.mockRejectedValue(new Error("x"));
    render(<StartupGroup />);
    await Promise.resolve();
    expect(toggle()).toBeDisabled();
    expect(toggle()).toHaveAttribute("aria-checked", "false");
  });

  it("shows the real status when already enabled and turns it off", async () => {
    status.mockResolvedValue(st({ enabled: true, wanted: true, backend: "enabled" }));
    disable.mockResolvedValue(st());
    render(<StartupGroup />);
    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "true"));
    fireEvent.click(toggle());
    expect(disable).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "false"));
  });

  it("turning it on calls enable once and shows the new state", async () => {
    status.mockResolvedValue(st());
    enable.mockResolvedValue(st({ enabled: true, wanted: true, backend: "enabled" }));
    render(<StartupGroup />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    expect(enable).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "true"));
  });

  it("explains the requires-approval state in plain words", async () => {
    status.mockResolvedValue(st());
    enable.mockResolvedValue(st({ wanted: true, backend: "requires_approval", needs_approval: true }));
    render(<StartupGroup />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    await waitFor(() => expect(screen.getByText(OPEN_AT_LOGIN_COPY.needsApproval)).toBeInTheDocument());
    expect(toggle()).toHaveAttribute("aria-checked", "true");
  });

  it("refused: not_in_applications shows fixed copy, stays off, hides the raw code", async () => {
    status.mockResolvedValue(st());
    enable.mockRejectedValue("refused: not_in_applications");
    render(<StartupGroup />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    await waitFor(() => expect(screen.getByText(OPEN_AT_LOGIN_COPY.notInApplications)).toBeInTheDocument());
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByText(/refused/)).toBeNull();
  });

  it("a declined confirm stays off with no error; an unknown error gets generic copy", async () => {
    status.mockResolvedValue(st());
    enable.mockRejectedValueOnce("declined: user said no");
    render(<StartupGroup />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    await waitFor(() => expect(enable).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    expect(screen.queryByText(OPEN_AT_LOGIN_COPY.error)).toBeNull();
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    enable.mockRejectedValueOnce(new Error("EPERM /Users/x/secret"));
    fireEvent.click(toggle());
    await waitFor(() => expect(screen.getByText(OPEN_AT_LOGIN_COPY.error)).toBeInTheDocument());
    expect(screen.queryByText(/secret/)).toBeNull();
  });

  it("is disabled with a reason on unsupported systems", async () => {
    status.mockResolvedValue(st({ backend: "unsupported" }));
    render(<StartupGroup />);
    await waitFor(() => expect(screen.getByText(OPEN_AT_LOGIN_COPY.unsupported)).toBeInTheDocument());
    expect(toggle()).toBeDisabled();
  });

  it("message mapping", () => {
    expect(loginItemMessage("refused: not_in_applications")).toBe(OPEN_AT_LOGIN_COPY.notInApplications);
    expect(loginItemMessage("declined")).toBeNull();
    expect(loginItemMessage(42)).toBe(OPEN_AT_LOGIN_COPY.error);
  });
});

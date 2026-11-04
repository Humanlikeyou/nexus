import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NOOP_PATH_CHANGE_CANCELLED } from "../../src/lib/types";
import { NoopSettingsGroup, isUserCancel, pathError } from "./NoopSettingsGroup";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const ready = async () => { render(<NoopSettingsGroup />); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()); };

describe("NOOP settings", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (cmd: string) => (cmd === "get_noop_settings" ? { whoop_source: "noop", binary_path: null, db_path: null } : undefined));
  });

  it("path rule: empty or absolute", () => {
    expect(pathError("")).toBeNull();
    expect(pathError("/opt/noop")).toBeNull();
    expect(pathError("noop")).toMatch(/full path/);
  });

  it("cancel matcher: exact NOOP_PATH_CHANGE_CANCELLED only", () => {
    expect(isUserCancel("cancelled: user declined path change")).toBe(true);
    expect(isUserCancel("User cancelled")).toBe(false);
    expect(isUserCancel("cancelled: user declined path change ")).toBe(false);
    expect(isUserCancel("invalid_path: cancelled.db")).toBe(false);
    expect(isUserCancel("boom")).toBe(false);
  });

  it("defaults to NOOP (recommended) and offers Direct BLE", async () => {
    await ready();
    expect(screen.getByRole("radio", { name: "NOOP (recommended)" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Direct BLE" })).toHaveAttribute("aria-checked", "false");
  });

  it("blocks a relative path without calling set_noop_settings", async () => {
    await ready();
    fireEvent.change(screen.getByLabelText("NOOP binary path"), { target: { value: "bin/noop" } });
    expect(screen.getByLabelText("NOOP binary path")).toHaveAttribute("aria-invalid", "true");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Fix the paths above");
    expect(invokeMock).not.toHaveBeenCalledWith("set_noop_settings", expect.anything());
  });

  it("saves with { settings } only and announces Saved", async () => {
    await ready();
    fireEvent.click(screen.getByRole("radio", { name: "Direct BLE" }));
    fireEvent.change(screen.getByLabelText("NOOP binary path"), { target: { value: "/opt/noop" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("set_noop_settings", { settings: { whoop_source: "ble", binary_path: "/opt/noop", db_path: null } });
  });

  it("native-dialog cancel shows a neutral Not saved status, no alert", async () => {
    await ready();
    invokeMock.mockRejectedValueOnce(NOOP_PATH_CHANGE_CANCELLED);
    fireEvent.change(screen.getByLabelText("NOOP binary path"), { target: { value: "/opt/noop" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Not saved")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("other strings mentioning cancel show the generic error, not Not saved", async () => {
    await ready();
    invokeMock.mockRejectedValueOnce("operation cancelled by system");
    fireEvent.change(screen.getByLabelText("NOOP binary path"), { target: { value: "/opt/noop" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't save. Check the paths and try again.");
    expect(screen.queryByText("Not saved")).toBeNull();
  });

  it("invalid_path shows fixed copy, never the raw detail", async () => {
    await ready();
    invokeMock.mockRejectedValueOnce("invalid_path: /Users/someone/secret/noop is not a file");
    fireEvent.change(screen.getByLabelText("NOOP binary path"), { target: { value: "/opt/noop" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("That path isn't usable.");
    expect(alert).not.toHaveTextContent("/Users/someone");
  });
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setHaConfigBackend, type HaConfigBackend } from "../lib/haConfig";
import { HomeAssistantSettingsGroup } from "./HomeAssistantSettingsGroup";

const TOKEN = "unit-fake-token-xyz";
let restore = () => {};
afterEach(() => restore());

function backend(over: Partial<HaConfigBackend> = {}) {
  const b: HaConfigBackend = {
    getConfig: vi.fn(async () => ({ url: null, tokenSet: false, tokenSource: null })),
    setConfig: vi.fn(async () => {}),
    clearToken: vi.fn(async () => ({ fallbackActive: false })),
    testConnection: vi.fn(async () => ({ ok: true, version: "2026.10" })),
    ...over,
  };
  restore = setHaConfigBackend(b);
  return b;
}

describe("HomeAssistantSettingsGroup", () => {
  it("missing command shows Coming soon, not an error", async () => {
    backend({ getConfig: vi.fn(async () => { throw "Command ha_get_config not found"; }) });
    render(<HomeAssistantSettingsGroup />);
    expect(await screen.findByText("Coming soon: needs Nexus update.")).toBeInTheDocument();
  });

  it("saves URL + token, then clears the token and never renders it", async () => {
    const b = backend();
    const { container } = render(<HomeAssistantSettingsGroup />);
    const token = await screen.findByLabelText("Access token");
    expect(token).toHaveAttribute("type", "password");
    expect(token).toHaveAttribute("autocomplete", "off");
    expect(token).toHaveValue("");
    fireEvent.change(token, { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved.");
    expect(b.setConfig).toHaveBeenCalledWith({ token: TOKEN });
    expect(screen.getByLabelText("Access token")).toHaveValue("");
    expect(container.innerHTML).not.toContain(TOKEN);
  });

  it("a leaked token in getConfig is never shown; saved token shows only Saved", async () => {
    backend({ getConfig: vi.fn(async () => ({ url: "http://127.0.0.1:8123", tokenSet: true, tokenSource: "app", token: TOKEN }) as never) });
    const { container } = render(<HomeAssistantSettingsGroup />);
    expect(await screen.findByText("Saved")).toBeInTheDocument();
    expect(container.querySelector('input[type="password"]')).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Replace token" }));
    expect(screen.getByLabelText("New access token")).toHaveValue("");
    expect(container.innerHTML).not.toContain(TOKEN);
  });

  it("rejects non-loopback URLs client-side", async () => {
    const b = backend();
    render(<HomeAssistantSettingsGroup />);
    const url = await screen.findByLabelText("Address");
    fireEvent.change(url, { target: { value: "http://example.com" } });
    fireEvent.blur(url);
    expect(screen.getByText(/Use a local address on this Mac/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(b.setConfig).not.toHaveBeenCalled();
  });

  it("save error shows fixed copy without the token", async () => {
    backend({ setConfig: vi.fn(async () => { throw `invalid_input: ${TOKEN}`; }) });
    const { container } = render(<HomeAssistantSettingsGroup />);
    fireEvent.change(await screen.findByLabelText("Access token"), { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText(/didn't accept those details/);
    expect(container.innerHTML).not.toContain(TOKEN);
  });

  it("test connection shows a busy state while it runs", async () => {
    let done = () => {};
    backend({ getConfig: vi.fn(async () => ({ url: "http://127.0.0.1:8123", tokenSet: true, tokenSource: "app" as const })), testConnection: vi.fn(() => new Promise<{ ok: boolean }>((r) => { done = () => r({ ok: true }); })) });
    render(<HomeAssistantSettingsGroup />);
    fireEvent.click(await screen.findByRole("button", { name: "Test connection" }));
    expect(screen.getByRole("button", { name: "Test connection" })).toHaveAttribute("aria-busy", "true");
    done();
    expect(await screen.findByText("Connected to Home Assistant.")).toBeInTheDocument();
  });

  it("test connection maps kinds to fixed copy", async () => {
    backend({ getConfig: vi.fn(async () => ({ url: "http://127.0.0.1:8123", tokenSet: true, tokenSource: "app" as const })), testConnection: vi.fn(async () => ({ ok: false, kind: "unreachable" })) });
    render(<HomeAssistantSettingsGroup />);
    fireEvent.click(await screen.findByRole("button", { name: "Test connection" }));
    await waitFor(() => expect(screen.getByText(/isn't reachable/)).toBeInTheDocument());
  });

  it("shows the token source in plain words; env tokens can't be cleared here", async () => {
    backend({ getConfig: vi.fn(async () => ({ url: null, tokenSet: true, tokenSource: "env" as const })) });
    render(<HomeAssistantSettingsGroup />);
    expect(await screen.findByText("From environment")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear token" })).not.toBeInTheDocument();
  });

  it("clear with an env fallback says the environment token is still in use", async () => {
    backend({ getConfig: vi.fn(async () => ({ url: null, tokenSet: true, tokenSource: "app" as const })), clearToken: vi.fn(async () => ({ fallbackActive: true })) });
    render(<HomeAssistantSettingsGroup />);
    expect(await screen.findByText("Saved in Nexus")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear token" }));
    expect(await screen.findByText(/A token from the environment is still in use\./)).toBeInTheDocument();
  });

  it("clearing the address sends url \"\"; https is rejected", async () => {
    const b = backend({ getConfig: vi.fn(async () => ({ url: "http://localhost:8123", tokenSet: true, tokenSource: "app" as const })) });
    render(<HomeAssistantSettingsGroup />);
    const url = await screen.findByLabelText("Address");
    fireEvent.change(url, { target: { value: "https://localhost:8123" } });
    fireEvent.blur(url);
    expect(screen.getByText("Use http://, not https://, for Home Assistant on this Mac.")).toBeInTheDocument();
    fireEvent.change(url, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(b.setConfig).toHaveBeenCalledWith({ url: "" }));
  });

  it("unsafe_storage on save shows fixed copy", async () => {
    backend({ setConfig: vi.fn(async () => { throw "unsafe_storage: details"; }) });
    render(<HomeAssistantSettingsGroup />);
    fireEvent.change(await screen.findByLabelText("Access token"), { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/couldn't store the token safely/)).toBeInTheDocument();
  });

  it("with the commands present, Coming soon never shows", async () => {
    backend();
    render(<HomeAssistantSettingsGroup />);
    expect(await screen.findByLabelText("Address")).toBeInTheDocument();
    expect(screen.queryByText(/Coming soon/)).not.toBeInTheDocument();
  });
});

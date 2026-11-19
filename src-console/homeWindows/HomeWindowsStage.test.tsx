import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const dict = vi.hoisted(() => ({ ready: false, text: null as string | null }));
vi.mock("../adds/dictation", () => ({ dictationReady: async () => dict.ready, dictateOnce: async () => dict.text }));
vi.mock("../components/AmaraLens/AmaraLens", () => ({ AmaraLens: () => <i />, AmaraVoiceAnnouncer: () => null }));
const add = vi.hoisted(() => ({ wired: true }));
vi.mock("../adds/adapters", () => ({
  addAdapter: () => ({
    command: "life_date_upsert", wired: add.wired,
    parse: async () => ({ state: "live", value: { kind: "preview", preview: { title: "Lease notice", fields: [["Date", "Mar 1"]] } } }),
    save: async () => ({ state: "live", value: { id: "d1", title: "Lease notice" } }),
    undo: async () => ({ state: "live", value: { ok: true } }),
    history: () => ({ state: "live", value: [] }),
  }),
}));

import { homeWindowsStore } from "./store";
import { requestFromCatalog } from "./catalog";
import { HomeWindowsStage } from "./HomeWindows";

beforeEach(() => { dict.ready = false; dict.text = null; add.wired = true; homeWindowsStore.reset(); });
afterEach(() => { homeWindowsStore.reset(); });

const openWindow = (id: string) => act(() => { homeWindowsStore.dispatch({ type: "open", request: requestFromCatalog(id, "chat"), at: Date.now(), contentState: "ready" }); });

describe("G9 command bar mic", () => {
  it("not ready: an OffButton with its reason, disabled (not silent)", async () => {
    render(<HomeWindowsStage />);
    openWindow("tradingview");
    expect(await screen.findByText("Needs wiring · on-device dictation")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Talk to Amara" })).toBeDisabled();
  });
  it("ready: the mic dictates once and asks what was heard through the same path as typing", async () => {
    dict.ready = true; dict.text = "what is on my calendar";
    const onAsk = vi.fn();
    render(<HomeWindowsStage onAsk={onAsk} />);
    openWindow("tradingview");
    await waitFor(() => expect(screen.getByRole("button", { name: "Talk to Amara" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Talk to Amara" }));
    await waitFor(() => expect(onAsk).toHaveBeenCalledWith("what is on my calendar"));
  });
  it("ready but nothing heard: nothing is asked", async () => {
    dict.ready = true; dict.text = null;
    const onAsk = vi.fn();
    render(<HomeWindowsStage onAsk={onAsk} />);
    openWindow("tradingview");
    await waitFor(() => expect(screen.getByRole("button", { name: "Talk to Amara" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Talk to Amara" }));
    await new Promise((r) => setTimeout(r, 20));
    expect(onAsk).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Talk to Amara" })).toBeEnabled();
  });
});

describe("G1 reply in the command bar", () => {
  it("pending shows a working line and no Open in chat yet", () => {
    render(<HomeWindowsStage ask={{ pending: true, reply: null }} />);
    openWindow("tradingview");
    expect(screen.getByText("Amara is answering…")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open in chat ›" })).toBeNull();
  });
  it("done shows Amara's reply; Open in chat closes the windows and calls onOpenChat", () => {
    const onOpenChat = vi.fn();
    render(<HomeWindowsStage ask={{ pending: false, reply: "Your lease notice is due Mar 1." }} onOpenChat={onOpenChat} />);
    openWindow("tradingview");
    expect(screen.getByText(/Your lease notice is due Mar 1\./)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open in chat ›" }));
    expect(onOpenChat).toHaveBeenCalledTimes(1);
    expect(homeWindowsStore.get().windows).toHaveLength(0);
  });
  it("no ask, no reply line", () => {
    render(<HomeWindowsStage />);
    openWindow("tradingview");
    expect(screen.queryByText("Amara is answering…")).toBeNull();
  });
});

describe("G10 PDF date add links to Life › Dates", () => {
  const savePdfDate = async () => {
    openWindow("lease");
    act(() => { const w = homeWindowsStore.get().windows[0]; homeWindowsStore.dispatch({ type: "commentReady", id: w.id, text: "Lease notice due" }); });
    fireEvent.click(await screen.findByRole("button", { name: /Add notice by Mar 1 to Dates/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Save date" }));
  };
  it("after Save, the saved line offers 'Also in Life › Dates ›' and it opens that route", async () => {
    const onOpen = vi.fn();
    render(<HomeWindowsStage onOpen={onOpen} />);
    await savePdfDate();
    fireEvent.click(await screen.findByRole("button", { name: "Also in Life › Dates ›" }));
    expect(onOpen).toHaveBeenCalledWith({ id: "life", view: "dates" });
  });
  it("without onOpen the saved line has Undo but no dead link", async () => {
    render(<HomeWindowsStage />);
    await savePdfDate();
    expect(await screen.findByText(/Added “Lease notice”/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Also in Life › Dates ›" })).toBeNull();
  });
});

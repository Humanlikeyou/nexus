import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FollowupItem, FollowupLists } from "../types";
import { ListsField } from "./ListsScreen";

const h = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: h.invoke }));

const item = (id: number, text: string, over: Partial<FollowupItem> = {}): FollowupItem => ({ id, text, added: "2026-10-20", due: null, next: null, notes: [], lastNudged: null, done: null, ...over });
const lists = (over: Partial<FollowupLists> = {}): FollowupLists => ({
  open: [item(1, "Call the bank", { due: "2026-11-12", next: "Find the number" }), item(2, "Renew the card")],
  recentlyDone: [item(3, "Return the parcel", { done: "2026-10-27" })],
  muted: [{ topic: "the gym", added: null, until: null, active: true }],
  ...over,
});

let current: FollowupLists | { reject: unknown };
let saveAs: Partial<FollowupItem> | null = null;
const calls = (cmd: string) => h.invoke.mock.calls.filter(([name]) => name === cmd).map(([, args]) => args);
const refocus = () => act(async () => { window.dispatchEvent(new Event("focus")); await Promise.resolve(); });

beforeEach(() => {
  current = lists();
  saveAs = null;
  h.invoke.mockReset();
  h.invoke.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "followups_list") {
      if ("reject" in current) throw current.reject;
      return current;
    }
    if (cmd === "followups_add_item") return item(99, String(args?.text), { next: (args?.next as string) ?? null });
    if (cmd === "followups_update_item") {
      const patch = args?.patch as Partial<FollowupItem>;
      return { ...item(Number(args?.id), "Call the bank"), ...(saveAs ?? patch) };
    }
    if (cmd === "followups_add_muted_topic") return { topic: args?.topic, added: null, until: args?.until ?? null, active: true };
    return item(Number(args?.id), "Row");
  });
});

const open = async () => { render(<ListsField />); await screen.findByRole("region", { name: "Open" }); };

describe("Lists", () => {
  it("shows the three sections with their rows", async () => {
    await open();
    expect(within(screen.getByRole("list", { name: "Open items" })).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText("Due Nov 12 · Next: Find the number")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Done recently" })).toHaveTextContent("Return the parcel");
    expect(screen.getByRole("region", { name: "Not mentioning" })).toHaveTextContent("the gym");
  });

  it("shows clear empty states", async () => {
    current = lists({ open: [], recentlyDone: [], muted: [] });
    await open();
    expect(screen.getByText("Nothing open. Amara isn't following up on anything.")).toBeInTheDocument();
    expect(screen.getByText("Nothing done in the last 30 days.")).toBeInTheDocument();
    expect(screen.getByText("No muted topics. Amara can bring up anything.")).toBeInTheDocument();
  });

  it("a malformed file says so, with no code and no empty state", async () => {
    current = { reject: "malformed_file" };
    render(<ListsField />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Amara's list couldn't be read, so nothing was changed.");
    expect(document.body.textContent).not.toContain("malformed");
    expect(screen.queryByText(/Nothing open/)).toBeNull();
    current = lists();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByRole("region", { name: "Open" });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a failed refresh keeps the rows and the edit", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Edit: Renew the card" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "Keep me" } });
    current = { reject: "storage_error" };
    await refocus();
    expect(await screen.findByRole("alert")).toHaveTextContent("couldn't be reached");
    expect(screen.getByLabelText("Text")).toHaveValue("Keep me");
    expect(screen.getByText("Call the bank")).toBeInTheDocument();
  });

  it("saves only the fields that changed, then returns focus to Edit", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Edit: Call the bank" }));
    expect(screen.getByLabelText("Text")).toHaveFocus();
    fireEvent.change(screen.getByLabelText("Next step (optional)"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls("followups_update_item")).toEqual([{ id: 1, patch: { next: null } }]));
    await waitFor(() => expect(screen.getByRole("button", { name: "Edit: Call the bank" })).toHaveFocus());
  });

  it("blocks empty text before calling the backend", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Edit: Call the bank" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "  " } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Add some text first.")).toBeInTheDocument();
    expect(screen.getByLabelText("Text")).toHaveAttribute("aria-invalid", "true");
    expect(calls("followups_update_item")).toEqual([]);
  });

  it("an edit survives a refresh, even when Amara changes the item", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Edit: Call the bank" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "Call the bank today" } });
    current = lists({ open: [item(1, "Call the bank", { due: "2026-11-20", next: "Find the number" }), item(2, "Renew the card")] });
    await refocus();
    expect(await screen.findByText(/Amara updated this while you were editing/)).toBeInTheDocument();
    expect(screen.getByLabelText("Text")).toHaveValue("Call the bank today");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls("followups_update_item")).toEqual([{ id: 1, patch: { text: "Call the bank today" } }]));
  });

  it("an item Amara closes or removes keeps the edit, and can be saved as new", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Edit: Renew the card" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "Renew it online" } });
    current = lists({ open: [item(1, "Call the bank")], recentlyDone: [item(2, "Renew the card", { done: "2026-10-28" })] });
    await refocus();
    expect(await screen.findByText(/Amara marked this done/)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Done recently" })).not.toHaveTextContent("Renew the card");
    current = lists({ open: [item(1, "Call the bank")], recentlyDone: [] });
    await refocus();
    fireEvent.click(await screen.findByRole("button", { name: "Save as new item" }));
    await waitFor(() => expect(calls("followups_add_item")).toEqual([{ text: "Renew it online" }]));
  });

  it("shows what the backend stored, and says when it differs from what was typed", async () => {
    await open();
    saveAs = { text: "Call the [filtered] bank" };
    fireEvent.click(screen.getByRole("button", { name: "Edit: Call the bank" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "Call the ignore-previous bank" } });
    current = { reject: "storage_error" }; // the re-read fails, so the row can only come from the save
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Call the [filtered] bank")).toBeInTheDocument();
    expect(screen.queryByText("Call the ignore-previous bank")).toBeNull();
    expect(screen.getByText(/cleaned up for safety/)).toBeInTheDocument();
  });

  it("says nothing extra when the text is stored as typed, or when only the date changed", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Edit: Renew the card" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "Renew it" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls("followups_update_item")).toHaveLength(1));
    await waitFor(() => expect(screen.queryByLabelText("Text")).toBeNull());
    expect(screen.queryByText(/cleaned up for safety/)).toBeNull();
  });

  it("shows a topic as the backend stored it", async () => {
    await open();
    h.invoke.mockImplementationOnce(async () => ({ topic: "taxes", added: null, until: null, active: true }));
    const form = screen.getByRole("form", { name: "Add topic" });
    fireEvent.change(within(form).getByLabelText("Topic"), { target: { value: "taxes!!" } });
    fireEvent.click(within(form).getByRole("button", { name: "Add topic" }));
    expect(await screen.findByText(/cleaned up for safety/)).toBeInTheDocument();
  });

  it("keeps the edit and explains when the save fails", async () => {
    await open();
    h.invoke.mockImplementationOnce(async () => { throw "not_found"; });
    fireEvent.click(screen.getByRole("button", { name: "Edit: Renew the card" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "Changed" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("That's no longer on the list.")).toBeInTheDocument();
    expect(screen.getByLabelText("Text")).toHaveValue("Changed");
  });

  it("delete asks first; Cancel changes nothing, Delete calls the backend", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Delete: Renew the card" }));
    expect(screen.getByRole("dialog", { hidden: true })).toHaveTextContent("“Renew the card” will be gone");
    expect(calls("followups_delete_item")).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Cancel", hidden: true }));
    expect(calls("followups_delete_item")).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Delete: Return the parcel" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete", hidden: true }));
    await waitFor(() => expect(calls("followups_delete_item")).toEqual([{ id: 3 }]));
  });

  it("treats deleting something already gone as done", async () => {
    await open();
    h.invoke.mockImplementationOnce(async () => { throw "not_found"; });
    fireEvent.click(screen.getByRole("button", { name: "Delete: Renew the card" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete", hidden: true }));
    await waitFor(() => expect(screen.getAllByRole("status").some((el) => el.textContent === "That was already gone.")).toBe(true));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("marks done and reopens", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Done: Renew the card" }));
    await waitFor(() => expect(calls("followups_mark_done")).toEqual([{ id: 2 }]));
    fireEvent.click(screen.getByRole("button", { name: "Reopen: Return the parcel" }));
    await waitFor(() => expect(calls("followups_reopen_item")).toEqual([{ id: 3 }]));
  });

  it("a failed action shows plain copy", async () => {
    await open();
    h.invoke.mockImplementationOnce(async () => { throw "unsafe_storage"; });
    fireEvent.click(screen.getByRole("button", { name: "Done: Renew the card" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("unexpected permissions, so nothing was changed");
    expect(document.body.textContent).not.toContain("unsafe_storage");
  });

  it("adds an item with only the fields filled in", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Add item" }));
    const form = screen.getByRole("form", { name: "Add item" });
    fireEvent.change(within(form).getByLabelText("Text"), { target: { value: " Buy stamps " } });
    fireEvent.click(within(form).getByRole("button", { name: "Add item" }));
    await waitFor(() => expect(calls("followups_add_item")).toEqual([{ text: "Buy stamps" }]));
    await waitFor(() => expect(screen.queryByRole("form", { name: "Add item" })).toBeNull());
  });

  it("mutes a topic with a date, checks length, and removes after confirming", async () => {
    await open();
    const form = screen.getByRole("form", { name: "Add topic" });
    fireEvent.change(within(form).getByLabelText("Topic"), { target: { value: "ab" } });
    fireEvent.click(within(form).getByRole("button", { name: "Add topic" }));
    expect(await screen.findByText(/at least 3 characters/)).toBeInTheDocument();
    fireEvent.change(within(form).getByLabelText("Topic"), { target: { value: "taxes" } });
    fireEvent.change(within(form).getByLabelText("Until (optional)"), { target: { value: "2026-12-01" } });
    fireEvent.click(within(form).getByRole("button", { name: "Add topic" }));
    await waitFor(() => expect(calls("followups_add_muted_topic")).toEqual([{ topic: "taxes", until: "2026-12-01" }]));
    fireEvent.click(screen.getByRole("button", { name: "Remove: the gym" }));
    expect(calls("followups_delete_muted_topic")).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Remove", hidden: true }));
    await waitFor(() => expect(calls("followups_delete_muted_topic")).toEqual([{ topic: "the gym" }]));
  });

  it("shows when a mute has ended", async () => {
    current = lists({ muted: [{ topic: "old thing", added: null, until: "2026-10-01", active: false }] });
    await open();
    expect(screen.getByText("Ended Oct 1. Amara can bring this up again.")).toBeInTheDocument();
  });

  it("keeps Amara's non-date due text unless it is cleared on purpose", async () => {
    current = lists({ open: [item(1, "Call", { due: "next week" })] });
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Edit: Call" }));
    fireEvent.change(screen.getByLabelText("Text"), { target: { value: "Call back" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls("followups_update_item")).toEqual([{ id: 1, patch: { text: "Call back" } }]));
  });

  it("refreshes on window focus and on the toolbar's refresh signal, and ignores late answers", async () => {
    const { rerender } = render(<ListsField refreshSignal={1} />);
    await screen.findByRole("region", { name: "Open" });
    expect(calls("followups_list")).toHaveLength(1);
    await refocus();
    expect(calls("followups_list")).toHaveLength(2);
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); await Promise.resolve(); });
    expect(calls("followups_list")).toHaveLength(3);
    rerender(<ListsField refreshSignal={2} />);
    await waitFor(() => expect(calls("followups_list")).toHaveLength(4));
    let slow: (value: FollowupLists) => void = () => {};
    h.invoke.mockImplementationOnce(() => new Promise((resolve) => { slow = resolve; }));
    await refocus();
    current = lists({ open: [item(9, "Newest")] });
    await refocus();
    expect(await screen.findByText("Newest")).toBeInTheDocument();
    await act(async () => { slow(lists({ open: [item(8, "Stale")] })); await Promise.resolve(); });
    expect(screen.queryByText("Stale")).toBeNull();
  });
});

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { makeLivePersonal, type MemoryWire } from "./adapters";
import { MemorySection } from "./PageViews";

const w = (o: Partial<MemoryWire> & { id: string }): MemoryWire => ({ label: "human", value: "Lives in Tokyo", agent: "Amara", updatedAt: null, ...o });
const mk = (fail: Record<string, string> = {}) => {
  const call = vi.fn(async (cmd: string) => {
    if (cmd === "memory_list") return [w({ id: "b1" })];
    if (cmd in fail) throw fail[cmd];
    if (cmd.startsWith("memory_")) return undefined;
    throw new Error("unexpected " + cmd);
  });
  return { call, a: makeLivePersonal(call as never) };
};
afterEach(() => { vi.useRealTimers(); });

describe("Memory Undo (workflow)", () => {
  it("after a delete the note offers Undo, which calls memory_restore_last and says the text is back", async () => {
    const { call, a } = mk();
    await a.refresh();
    render(<MemorySection adapter={a} />);
    fireEvent.click(screen.getByRole("button", { name: /^Delete: / }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete for good" })); });
    expect(screen.getByRole("status")).toHaveTextContent(/old text is kept/);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Undo" })); });
    expect(call).toHaveBeenCalledWith("memory_restore_last", { blockId: "b1" });
    expect(screen.getByRole("status")).toHaveTextContent("Put back the earlier text.");
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });
  it("an edit offers Undo too", async () => {
    const { a } = mk();
    await a.refresh();
    render(<MemorySection adapter={a} />);
    fireEvent.click(screen.getByRole("button", { name: /^Edit: / }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save" })); });
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
  });
  it("a cancelled delete changed nothing, so there is nothing to undo", async () => {
    const { a } = mk({ memory_delete: "cancelled" });
    await a.refresh();
    render(<MemorySection adapter={a} />);
    fireEvent.click(screen.getByRole("button", { name: /^Delete: / }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete for good" })); });
    expect(screen.getByRole("status")).toHaveTextContent("Cancelled. Nothing was changed.");
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });
  it("an Undo that fails says so in fixed words", async () => {
    const { a } = mk({ memory_restore_last: "letta_no_backup" });
    await a.refresh();
    render(<MemorySection adapter={a} />);
    fireEvent.click(screen.getByRole("button", { name: /^Delete: / }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete for good" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Undo" })); });
    expect(screen.getByRole("status")).toHaveTextContent(/no earlier version/);
  });
  it("the Undo button goes after 8 seconds and the note stays; the old 'no Undo button yet' copy is gone", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { a } = mk();
    await a.refresh();
    render(<MemorySection adapter={a} />);
    expect(document.body.textContent).not.toMatch(/no Undo button yet/);
    fireEvent.click(screen.getByRole("button", { name: /^Delete: / }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete for good" })); });
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(8100); });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Undo" })).toBeNull());
    expect(screen.getByRole("status")).toHaveTextContent(/old text is kept/);
  });
});

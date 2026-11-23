import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { makeLivePersonal, memoriesFrom, type MemoryWire } from "./adapters";
import { MemorySection } from "./PageViews";

const w = (o: Partial<MemoryWire> & { id: string }): MemoryWire => ({ label: "human", value: "v", agent: "Amara", updatedAt: null, ...o });
const PASSWORD = "SENTINEL-letta-pw";

describe("memoriesFrom", () => {
  it("maps Letta blocks to the Memory shape without inventing anything", () => {
    const [a, b, c] = memoriesFrom([
      w({ id: "b1", label: "human", value: "Lives in Tokyo", updatedAt: "2026-11-01T00:00:00Z" }),
      w({ id: "b2", label: "persona", value: "Direct and brief", truncated: true }),
      w({ id: "b3", label: "scratch", value: "note" }),
    ]);
    expect(a).toMatchObject({ id: "b1", text: "Lives in Tokyo", category: "about" });
    expect(a.source).toBe("Amara · human block · updated Nov 1");
    expect(b).toMatchObject({ text: "Direct and brief …", category: "preferences", source: "Amara · persona block" });
    expect(c.source).toBe("Amara · scratch block");
  });
  it("skips rows that are not blocks", () => {
    expect(memoriesFrom([{ nope: 1 }, null, w({ id: "ok" })] as never)).toHaveLength(1);
  });
});

describe("live memory adapter", () => {
  it("is unwired until memory_list succeeds, then live; pending and decide stay unwired", async () => {
    const call = vi.fn(async (cmd: string) => (cmd === "memory_list" ? [w({ id: "b1" })] : Promise.reject("nope")));
    const a = makeLivePersonal(call as never);
    expect(a.memories().state).toBe("unwired");
    await a.refresh();
    expect(a.memories().state).toBe("live");
    expect(a.memoriesNote?.()).toBeNull();
    expect(a.pendingMemories().state).toBe("unwired");
    expect((await a.decideMemory("b1", true)).state).toBe("unwired");
    const names = call.mock.calls.map((c) => c[0]);
    expect(names).toContain("memory_list");
    // no source exists for proposed memories, so nothing is invoked for them
    expect(names.filter((n) => /memory_(decide|pending|propos)/.test(n))).toEqual([]);
  });
  it.each([
    ["letta_no_key", /Add the Letta password in Settings › Secrets/],
    ["letta_off", /isn't answering/],
    ["letta_unauthorized", /refused the saved password/],
    ["letta_bad_response", /couldn't read/],
    ["something else entirely", /couldn't read/],
  ])("error %s becomes an honest note and no values", async (code, re) => {
    const a = makeLivePersonal(((cmd: string) => (cmd === "memory_list" ? Promise.reject(code) : Promise.reject("x"))) as never);
    await a.refresh();
    expect(a.memories().state).toBe("unwired");
    expect(a.memoriesNote?.()).toMatch(re);
    expect(a.memoriesNote?.()).not.toContain(PASSWORD);
  });
});

describe("MemorySection honesty", () => {
  it("a live list shows rows, no Sample data tag, and does not promise edits", async () => {
    const a = makeLivePersonal((async (cmd: string) => (cmd === "memory_list" ? [w({ id: "b1", value: "Lives in Tokyo" })] : Promise.reject("x"))) as never);
    await a.refresh();
    render(<MemorySection adapter={a} />);
    expect(screen.getByText(/Lives in Tokyo/)).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(document.body.textContent).not.toMatch(/you can edit or delete any of them/);
    expect(document.body.textContent).toMatch(/1 memory block/);
  });
  it("an error shows the note instead of the build-time promise", async () => {
    const a = makeLivePersonal((async (cmd: string) => (cmd === "memory_list" ? Promise.reject("letta_no_key") : Promise.reject("x"))) as never);
    await a.refresh();
    render(<MemorySection adapter={a} />);
    await waitFor(() => expect(screen.getByText(/Add the Letta password in Settings › Secrets/)).toBeInTheDocument());
    expect(document.body.textContent).not.toMatch(/once Claude adds memory_list/);
  });
});

describe("live memory writes", () => {
  const mk = (writes: Record<string, unknown> = {}) => {
    const call = vi.fn(async (cmd: string) => {
      if (cmd === "memory_list") return [w({ id: "b1", value: "Lives in Tokyo" }), w({ id: "b2", value: "" })];
      if (cmd in writes) { const r = writes[cmd]; if (typeof r === "string") throw r; return undefined; }
      throw new Error("unexpected " + cmd);
    });
    return { call, a: makeLivePersonal(call as never) };
  };
  it("a cleared block is not listed", async () => {
    const { a } = mk();
    await a.refresh();
    expect((a.memories() as { value: { id: string }[] }).value.map((m) => m.id)).toEqual(["b1"]);
  });
  it("editMemory calls memory_edit with blockId and value, refreshes, and is live", async () => {
    const { call, a } = mk({ memory_edit: true });
    await a.refresh();
    expect((await a.editMemory("b1", "Lives in Kyoto")).state).toBe("live");
    expect(call).toHaveBeenCalledWith("memory_edit", { blockId: "b1", value: "Lives in Kyoto" });
    expect(call.mock.calls.filter((c) => c[0] === "memory_list")).toHaveLength(2);
    expect(a.memoryWriteNote?.()).toMatch(/old text is kept/);
  });
  it("deleteMemory calls memory_delete; a denied dialog says nothing changed and never claims success", async () => {
    const { call, a } = mk({ memory_delete: "cancelled" });
    await a.refresh();
    expect((await a.deleteMemory("b1")).state).toBe("unwired");
    expect(call).toHaveBeenCalledWith("memory_delete", { blockId: "b1" });
    expect(a.memoryWriteNote?.()).toBe("Cancelled. Nothing was changed.");
    expect(a.memories().state).toBe("live");
  });
  it("restoreMemory calls memory_restore_last", async () => {
    const { call, a } = mk({ memory_restore_last: true });
    await a.refresh();
    expect((await a.restoreMemory!("b1")).state).toBe("live");
    expect(call).toHaveBeenCalledWith("memory_restore_last", { blockId: "b1" });
  });
  it.each([
    ["letta_backup_failed", /couldn't save the old text first/],
    ["letta_value_too_long", /2000 characters/],
    ["letta_block_too_long", /longer than the app shows/],
    ["letta_no_backup", /no earlier version/],
    ["letta_off", /isn't answering/],
    ["letta_unconfirmed", /may or may not have changed — reopen Memory to check/],
    ["changed", /changed while you were deciding/],
    ["letta_busy", /Another memory change is still open/],
    ["weird", /may or may not have changed — reopen Memory to check/],
  ])("write error %s shows fixed words only", async (code, re) => {
    const { a } = mk({ memory_edit: code });
    await a.refresh();
    expect((await a.editMemory("b1", "x")).state).toBe("unwired");
    expect(a.memoryWriteNote?.()).toMatch(re);
  });
  it("the page shows the write result and an honest hero (no read-only claim)", async () => {
    const { a } = mk({ memory_delete: "cancelled" });
    await a.refresh();
    render(<MemorySection adapter={a} />);
    expect(document.body.textContent).not.toMatch(/read only for now/);
    expect(document.body.textContent).toMatch(/the old text is saved first/);
    fireEvent.click(screen.getByRole("button", { name: /^Delete: / }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete for good" })); });
    expect(screen.getByRole("status")).toHaveTextContent("Cancelled. Nothing was changed.");
  });
});

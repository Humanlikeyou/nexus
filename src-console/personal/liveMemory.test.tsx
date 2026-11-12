import { render, screen, waitFor } from "@testing-library/react";
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
  it("is unwired until memory_list succeeds, then live; pending and writes stay unwired", async () => {
    const call = vi.fn(async (cmd: string) => (cmd === "memory_list" ? [w({ id: "b1" })] : Promise.reject("nope")));
    const a = makeLivePersonal(call as never);
    expect(a.memories().state).toBe("unwired");
    await a.refresh();
    expect(a.memories().state).toBe("live");
    expect(a.memoriesNote?.()).toBeNull();
    expect(a.pendingMemories().state).toBe("unwired");
    expect((await a.editMemory("b1", "x")).state).toBe("unwired");
    expect((await a.deleteMemory("b1")).state).toBe("unwired");
    expect((await a.decideMemory("b1", true)).state).toBe("unwired");
    const names = call.mock.calls.map((c) => c[0]);
    expect(names).toContain("memory_list");
    expect(names.filter((n) => /memory_(update|delete|edit|decide|set)/.test(n))).toEqual([]);
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

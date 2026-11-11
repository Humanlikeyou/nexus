import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommandPalette, filterCommands, paletteCommands } from "./CommandPalette";

describe("⌘K command palette", () => {
  it("lists the 7 sections with ⌘1–7 and the sub-views", () => {
    const cmds = paletteCommands();
    expect(cmds.slice(0, 7).map((c) => `${c.label} ${c.hint}`)).toEqual(["Home ⌘1", "Agents ⌘2", "Mind–Body ⌘3", "Life ⌘4", "Enterprise ⌘5", "Settings ⌘6", "Smart home ⌘7"]);
    expect(cmds.map((c) => c.label)).toEqual(expect.arrayContaining(["Agents › Activity", "Agents › Crews", "Mind–Body › Fitness", "Mind–Body › Neuro", "Mind–Body › Neuro history", "Today › To-dos", "Settings › Voice", "Enterprise › Finances", "Enterprise › Ventures", "Enterprise › Tasks", "Enterprise › Decisions", "Enterprise › Content", "Life › Goals", "Life › Weekly review", "Life › Dates & deadlines", "Life › Personal money", "Settings › Devices", "About Amara OS"]));
    for (const gone of ["Today › Lists", "Today › Life admin", "Today › Weekly review", "Home › Goals", "Business › Money", "Business › Work", "Business › Personal"]) expect(cmds.map((c) => c.label)).not.toContain(gone);
    expect(filterCommands(cmds, "devi").map((c) => c.id)).toEqual(["settings:devices"]);
    expect(filterCommands(cmds, "crews").map((c) => c.label)).toContain("Agents › Crews");
  });

  it("design-1111v: ⌘K labels snapshot", () => {
    expect(paletteCommands().map((c) => c.label)).toMatchInlineSnapshot(`
      [
        "Home",
        "Agents",
        "Mind–Body",
        "Life",
        "Enterprise",
        "Settings",
        "Smart home",
        "Agents › Crews",
        "Mind–Body › Fitness",
        "Mind–Body › Neuro",
        "Mind–Body › Neuro history",
        "Mind–Body › Patterns",
        "Life › Goals",
        "Life › Weekly review",
        "Life › Dates & deadlines",
        "Life › Personal money",
        "Enterprise › Decisions",
        "Enterprise › Ventures",
        "Enterprise › Finances",
        "Enterprise › Tasks",
        "Enterprise › Content",
        "Settings › Services",
        "Settings › Devices",
        "Settings › Notifications",
        "Settings › Backup",
        "Settings › Voice",
        "Settings › Memory",
        "Home › Today",
        "Today › Needs you today",
        "Today › Check-in",
        "Today › To-dos",
        "Agents › Activity",
        "About Amara OS",
        "Settings › General › Updates",
      ]
    `);
  });

  it("design-1111v: an old name typed from habit ranks where that page went ('rev' → Finances, before Weekly review)", () => {
    const labels = (q: string) => filterCommands(paletteCommands(), q).map((c) => c.label);
    expect(labels("rev")[0]).toBe("Enterprise › Finances");
    expect(labels("rev")).toContain("Life › Weekly review");
    expect(labels("review")[0]).toBe("Life › Weekly review");
    expect(labels("life admin")[0]).toBe("Life › Dates & deadlines");
    expect(labels("work")[0]).toBe("Enterprise › Tasks");
  });

  it("navigates with arrows + Return and closes", () => {
    const onNavigate = vi.fn(); const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} onNavigate={onNavigate} onAsk={vi.fn()} />);
    const input = screen.getByRole("combobox", { name: "Search or ask Amara" });
    expect(input).toHaveAttribute("placeholder", "Search or ask Amara…");
    fireEvent.change(input, { target: { value: "revenue" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onNavigate).toHaveBeenCalledWith({ id: "enterprise", view: "finances" });
    expect(onClose).toHaveBeenCalled();
  });

  it("no match: top result is Ask Amara with the text", () => {
    const onAsk = vi.fn();
    render(<CommandPalette open onClose={vi.fn()} onNavigate={vi.fn()} onAsk={onAsk} />);
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: "what is my HRV trend" } });
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveTextContent("Ask Amara: “what is my HRV trend”");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onAsk).toHaveBeenCalledWith("what is my HRV trend");
  });

  it("has no action buttons (content-only Dialog)", () => {
    render(<CommandPalette open onClose={vi.fn()} onNavigate={vi.fn()} onAsk={vi.fn()} />);
    expect(screen.queryByRole("button")).toBeNull();
  });
});

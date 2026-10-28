import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommandPalette, filterCommands, paletteCommands } from "./CommandPalette";

describe("⌘K command palette", () => {
  it("lists the 6 sections with ⌘1–6 and the sub-views", () => {
    const cmds = paletteCommands();
    expect(cmds.slice(0, 6).map((c) => c.hint)).toEqual(["⌘1", "⌘2", "⌘3", "⌘4", "⌘5", "⌘6"]);
    expect(cmds.map((c) => c.label)).toEqual(expect.arrayContaining(["Chat & Agents › Activity", "Neuro › History", "Business › Revenue", "Settings › Devices", "About Amara OS"]));
    expect(filterCommands(cmds, "devi").map((c) => c.id)).toEqual(["settings:devices"]);
  });

  it("navigates with arrows + Return and closes", () => {
    const onNavigate = vi.fn(); const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} onNavigate={onNavigate} onAsk={vi.fn()} />);
    const input = screen.getByRole("combobox", { name: "Search or ask Amara" });
    expect(input).toHaveAttribute("placeholder", "Search or ask Amara…");
    fireEvent.change(input, { target: { value: "revenue" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onNavigate).toHaveBeenCalledWith({ id: "business", view: "revenue" });
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

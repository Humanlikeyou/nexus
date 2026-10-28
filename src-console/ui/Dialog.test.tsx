import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Dialog } from "./index";

function Harness({ onConfirm = () => {}, destructive = false }: { onConfirm?: () => void; destructive?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>Restore from Backup…</button>
      <Dialog open={open} title="Restore from backup?" confirmLabel="Restore" destructive={destructive} onConfirm={() => { onConfirm(); setOpen(false); }} onCancel={() => setOpen(false)}>
        <p>This replaces local data.</p>
      </Dialog>
    </>
  );
}

describe("Dialog", () => {
  it("opens as a named dialog with focus on Cancel", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "Restore from Backup…" }));
    const dialog = screen.getByRole("dialog", { name: "Restore from backup?" });
    expect(dialog).toHaveAttribute("open");
    expect(dialog).toHaveAccessibleDescription("This replaces local data.");
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("Cancel closes and returns focus to the opener; confirm fires once", async () => {
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);
    const opener = screen.getByRole("button", { name: "Restore from Backup…" });
    await userEvent.click(opener);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(opener).toHaveFocus();
    await userEvent.click(opener);
    await userEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("Esc (native cancel event) acts as Cancel", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "Restore from Backup…" }));
    const dialog = screen.getByRole("dialog");
    fireEvent(dialog, new Event("cancel", { cancelable: true }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("uses the destructive confirm variant when flagged", async () => {
    render(<Harness destructive />);
    await userEvent.click(screen.getByRole("button", { name: "Restore from Backup…" }));
    expect(screen.getByRole("button", { name: "Restore" })).toHaveAttribute("data-variant", "destructive");
  });
});

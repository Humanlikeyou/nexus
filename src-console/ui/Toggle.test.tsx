import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createRef, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Toggle } from "./index";

function Harness({ onCheckedChange = (_: boolean) => {} }) {
  const [on, setOn] = useState(false);
  return <Toggle checked={on} onCheckedChange={(v) => { setOn(v); onCheckedChange(v); }}>Reduce motion</Toggle>;
}

describe("Toggle", () => {
  it("is a button with role=switch named by its label", () => {
    const ref = createRef<HTMLButtonElement>();
    render(<Toggle ref={ref} checked={false} onCheckedChange={() => {}} className="x">Wi-Fi</Toggle>);
    const toggle = screen.getByRole("switch", { name: "Wi-Fi" });
    expect(toggle.tagName).toBe("BUTTON");
    expect(toggle).toHaveAttribute("type", "button");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toHaveClass("ui-toggle", "x");
    expect(ref.current).toBe(toggle);
  });

  it("toggles aria-checked on click, Space and Enter", async () => {
    const onCheckedChange = vi.fn();
    render(<Harness onCheckedChange={onCheckedChange} />);
    const toggle = screen.getByRole("switch", { name: "Reduce motion" });
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(toggle).toHaveAttribute("data-checked", "true");
    await userEvent.keyboard(" ");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await userEvent.keyboard("{Enter}");
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(onCheckedChange.mock.calls.map((c) => c[0])).toEqual([true, false, true]);
  });

  it("supports aria-label when there is no visible label, and disabled", async () => {
    const onCheckedChange = vi.fn();
    render(<Toggle aria-label="Notifications" checked onCheckedChange={onCheckedChange} disabled />);
    const toggle = screen.getByRole("switch", { name: "Notifications" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await userEvent.click(toggle);
    expect(onCheckedChange).not.toHaveBeenCalled();
  });

  it("a caller's onClick can veto the change with preventDefault", async () => {
    const onCheckedChange = vi.fn();
    render(<Toggle checked={false} onCheckedChange={onCheckedChange} onClick={(e) => e.preventDefault()}>Locked</Toggle>);
    await userEvent.click(screen.getByRole("switch", { name: "Locked" }));
    expect(onCheckedChange).not.toHaveBeenCalled();
  });
});

export function _typeChecks() {
  // @ts-expect-error a switch needs a name: children, aria-label or aria-labelledby
  void <Toggle checked onCheckedChange={() => {}} />;
}

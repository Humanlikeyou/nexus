import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Moon, Sun } from "lucide-react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { SegmentedControl } from "./index";

type Theme = "system" | "light" | "dark";
const OPTIONS = [
  { value: "system" as const, label: "System" },
  { value: "light" as const, label: "Light", icon: Sun },
  { value: "dark" as const, label: "Dark", icon: Moon },
];

function Harness({ initial = "light" as Theme, onChange = (_: Theme) => {} }) {
  const [value, setValue] = useState<Theme>(initial);
  return <SegmentedControl label="Theme" value={value} options={OPTIONS} onChange={(v) => { setValue(v); onChange(v); }} />;
}

describe("SegmentedControl", () => {
  it("is a named radiogroup of radios with aria-checked", () => {
    render(<Harness />);
    expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeInTheDocument();
    const radios = screen.getAllByRole("radio");
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "true", "false"]);
  });

  it("roving tabindex: only the selected radio is tabbable, Tab enters on it", async () => {
    render(<><button>before</button><Harness /><button>after</button></>);
    expect(screen.getAllByRole("radio").map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
    await userEvent.tab();
    await userEvent.tab();
    expect(screen.getByRole("radio", { name: "Light" })).toHaveFocus();
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "after" })).toHaveFocus();
  });

  it("arrow keys move focus and selection, wrapping at the ends", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    screen.getByRole("radio", { name: "Light" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("radio", { name: "Dark" })).toHaveFocus();
    expect(screen.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("radio", { name: "System" })).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(screen.getByRole("radio", { name: "Dark" })).toHaveFocus();
    await userEvent.keyboard("{ArrowUp}");
    expect(screen.getByRole("radio", { name: "Light" })).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(onChange.mock.calls.map((c) => c[0])).toEqual(["dark", "system", "dark", "light", "dark"]);
    expect(screen.getAllByRole("radio").map((r) => r.tabIndex)).toEqual([-1, -1, 0]);
  });

  it("Home and End jump to the first and last option", async () => {
    render(<Harness />);
    screen.getByRole("radio", { name: "Light" }).focus();
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
    await userEvent.keyboard("{Home}");
    expect(screen.getByRole("radio", { name: "System" })).toHaveFocus();
    expect(screen.getByRole("radio", { name: "System" })).toHaveAttribute("aria-checked", "true");
  });

  it("click selects; clicking the selected option does not re-fire onChange", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await userEvent.click(screen.getByRole("radio", { name: "Light" }));
    expect(onChange).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("radio", { name: "System" }));
    expect(onChange).toHaveBeenCalledWith("system");
  });

  it("with no matching value, the first radio is the tab stop", () => {
    render(<SegmentedControl label="Theme" value={"other" as Theme} options={OPTIONS} onChange={() => {}} />);
    expect(screen.getAllByRole("radio").map((r) => r.tabIndex)).toEqual([0, -1, -1]);
  });

  it("passes className, native attributes and data-size", () => {
    render(<SegmentedControl label="Range" value="a" options={[{ value: "a", label: "A" }]} onChange={() => {}} className="x" data-testid="seg" size="small" />);
    const group = screen.getByTestId("seg");
    expect(group).toHaveClass("ui-segmented", "x");
    expect(group).toHaveAttribute("data-size", "small");
  });
});

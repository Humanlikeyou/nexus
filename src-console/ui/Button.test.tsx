import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Plus } from "lucide-react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";
import { ActionButton as ProductActionButton } from "../design/ProductUI";
import { ActionButton, Button } from "./index";

describe("Button", () => {
  it("defaults to type=button, secondary/regular, and passes className and native attributes", () => {
    const ref = createRef<HTMLButtonElement>();
    render(<Button ref={ref} className="extra" title="Tip" data-testid="b">Save</Button>);
    const button = screen.getByRole("button", { name: "Save" });
    expect(button).toHaveAttribute("type", "button");
    expect(button).toHaveAttribute("data-variant", "secondary");
    expect(button).toHaveAttribute("data-size", "regular");
    expect(button).toHaveClass("ui-button", "extra");
    expect(button).toHaveAttribute("title", "Tip");
    expect(ref.current).toBe(button);
  });

  it("does not submit an enclosing form by default", async () => {
    const onSubmit = vi.fn((event: Event) => event.preventDefault());
    render(<form onSubmit={(e) => onSubmit(e.nativeEvent)}><Button>Go</Button></form>);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("activates with Enter and Space from the keyboard", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Run</Button>);
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "Run" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    await userEvent.keyboard(" ");
    expect(onClick).toHaveBeenCalledTimes(2);
  });

  it("sets variant and size as data attributes", () => {
    render(<Button variant="destructive" size="small">Delete</Button>);
    const button = screen.getByRole("button", { name: "Delete" });
    expect(button).toHaveAttribute("data-variant", "destructive");
    expect(button).toHaveAttribute("data-size", "small");
  });

  it("icon-only buttons are named by aria-label and hide the icon", () => {
    render(<Button iconOnly icon={Plus} aria-label="Add item" />);
    const button = screen.getByRole("button", { name: "Add item" });
    expect(button).toHaveAttribute("data-icon-only", "true");
    expect(button.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("loading sets aria-busy, stays focusable, keeps its label in the DOM and ignores activation", async () => {
    const onClick = vi.fn();
    render(<Button loading onClick={onClick}>Saving</Button>);
    const button = screen.getByRole("button", { name: "Saving" });
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).not.toBeDisabled();
    expect(button.querySelector(".ui-button__label")).toHaveTextContent("Saving");
    await userEvent.click(button);
    button.focus();
    await userEvent.keyboard("{Enter}");
    expect(onClick).not.toHaveBeenCalled();
  });

  it("ActionButton keeps the legacy variants working (also via design/ProductUI)", () => {
    render(<><ActionButton variant="danger">Stop</ActionButton><ProductActionButton variant="quiet">Retry</ProductActionButton></>);
    expect(screen.getByRole("button", { name: "Stop" })).toHaveAttribute("data-variant", "destructive");
    expect(screen.getByRole("button", { name: "Retry" })).toHaveAttribute("data-variant", "plain");
  });
});

// Type-level contract, checked by `tsc --noEmit` (never executed).
export function _typeChecks() {
  // @ts-expect-error icon-only buttons must have an aria-label
  void <Button iconOnly icon={Plus} />;
  // @ts-expect-error icon-only buttons take no children
  void <Button iconOnly icon={Plus} aria-label="Add">Add</Button>;
  // @ts-expect-error unknown variant
  void <Button variant="ghost">x</Button>;
}

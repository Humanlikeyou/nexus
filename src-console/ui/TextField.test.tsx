import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Search } from "lucide-react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";
import { TextField } from "./index";

describe("TextField", () => {
  it("labels the input and accepts typing", async () => {
    const ref = createRef<HTMLInputElement>();
    render(<TextField ref={ref} label="Name" placeholder="Your name" />);
    const input = screen.getByRole("textbox", { name: "Name" });
    expect(ref.current).toBe(input);
    await userEvent.click(screen.getByText("Name"));
    expect(input).toHaveFocus();
    await userEvent.keyboard("Taisun");
    expect(input).toHaveValue("Taisun");
  });

  it("hideLabel keeps the accessible name", () => {
    render(<TextField label="Search" hideLabel leadingIcon={Search} />);
    expect(screen.getByRole("textbox", { name: "Search" })).toBeInTheDocument();
    expect(screen.getByText("Search")).toHaveClass("ui-visually-hidden");
  });

  it("wires description and error into aria-describedby and sets aria-invalid", () => {
    const { rerender } = render(<TextField label="Port" description="Between 1024 and 65535." />);
    const input = screen.getByRole("textbox", { name: "Port" });
    expect(input).toHaveAccessibleDescription("Between 1024 and 65535.");
    expect(input).not.toHaveAttribute("aria-invalid");

    rerender(<TextField label="Port" description="Between 1024 and 65535." error="Port is in use." />);
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("Between 1024 and 65535. Port is in use.");
  });

  it("keeps a caller's own aria-describedby and id, and passes native attributes", () => {
    render(<><p id="hint">External hint</p><TextField id="custom" label="Token" aria-describedby="hint" error="Required" type="password" required className="wide" /></>);
    const input = screen.getByLabelText("Token");
    expect(input).toHaveAttribute("id", "custom");
    expect(input).toHaveAttribute("type", "password");
    expect(input).toBeRequired();
    expect(input).toHaveAccessibleDescription("External hint Required");
    expect(input.closest(".ui-text-field")).toHaveClass("wide");
  });

  it("gives each instance unique ids", () => {
    render(<><TextField label="A" description="a" /><TextField label="B" description="b" /></>);
    const a = screen.getByRole("textbox", { name: "A" });
    const b = screen.getByRole("textbox", { name: "B" });
    expect(a.id).not.toBe(b.id);
    expect(a).toHaveAccessibleDescription("a");
    expect(b).toHaveAccessibleDescription("b");
  });
});

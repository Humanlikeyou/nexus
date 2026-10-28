import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Select } from "./index";

const OPTIONS = [{ value: "off", label: "Recording only" }, { value: "alpha", label: "Alpha up" }, { value: "smr", label: "SMR" }];

describe("Select", () => {
  it("is a labelled native select with the given options", async () => {
    const onChange = vi.fn();
    render(<Select label="Protocol" value="off" onChange={onChange} options={OPTIONS} description="Locked during a session." />);
    const select = screen.getByRole("combobox", { name: "Protocol" });
    expect(select.tagName).toBe("SELECT");
    expect(select).toHaveAccessibleDescription("Locked during a session.");
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Recording only", "Alpha up", "SMR"]);
    await userEvent.selectOptions(select, "smr");
    expect(onChange).toHaveBeenCalled();
  });

  it("can be disabled and hide its label visually", () => {
    render(<Select label="Protocol" hideLabel disabled value="off" onChange={() => {}} options={OPTIONS} />);
    expect(screen.getByRole("combobox", { name: "Protocol" })).toBeDisabled();
    expect(screen.getByText("Protocol")).toHaveClass("ui-visually-hidden");
  });
});

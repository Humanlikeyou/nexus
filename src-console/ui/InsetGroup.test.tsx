import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Bluetooth, ChevronRight, Wifi } from "lucide-react";
import { createRef, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { InsetGroup, ListRow, Toggle } from "./index";

describe("InsetGroup + ListRow", () => {
  it("renders a ul role=list labelled by its title and described by its footer", () => {
    render(<InsetGroup title="Network" footer="Changes apply immediately."><ListRow title="Wi-Fi" /><ListRow title="Ethernet" /></InsetGroup>);
    const list = screen.getByRole("list", { name: "Network" });
    expect(list.tagName).toBe("UL");
    expect(list).toHaveAttribute("role", "list");
    expect(list).toHaveAccessibleDescription("Changes apply immediately.");
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
  });

  it("non-selectable rows contain no button", () => {
    render(<InsetGroup><ListRow title="Version" value="0.1.0" /></InsetGroup>);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText("0.1.0")).toBeInTheDocument();
  });

  it("selectable rows are buttons; the selected one gets aria-current", () => {
    render(
      <InsetGroup title="Sources">
        <ListRow icon={Wifi} title="Wi-Fi" subtitle="Connected" onSelect={() => {}} selected />
        <ListRow icon={Bluetooth} title="Bluetooth" onSelect={() => {}} />
      </InsetGroup>,
    );
    const wifi = screen.getByRole("button", { name: /Wi-Fi/ });
    const bt = screen.getByRole("button", { name: /Bluetooth/ });
    expect(wifi).toHaveAttribute("type", "button");
    expect(wifi).toHaveAttribute("aria-current", "true");
    expect(bt).not.toHaveAttribute("aria-current");
    expect(wifi.closest("li")).toHaveAttribute("data-selected", "true");
  });

  it("keyboard: Tab reaches each row, Enter and Space select", async () => {
    function Harness() {
      const [sel, setSel] = useState("a");
      return <InsetGroup title="Pick">{["a", "b"].map((k) => <ListRow key={k} title={`Row ${k}`} onSelect={() => setSel(k)} selected={sel === k} />)}</InsetGroup>;
    }
    render(<Harness />);
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "Row a" })).toHaveFocus();
    await userEvent.tab();
    const b = screen.getByRole("button", { name: "Row b" });
    expect(b).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(b).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: "Row a" })).not.toHaveAttribute("aria-current");
    await userEvent.keyboard("{Shift>}{Tab}{/Shift}");
    await userEvent.keyboard(" ");
    expect(screen.getByRole("button", { name: "Row a" })).toHaveAttribute("aria-current", "true");
  });

  it("accessory controls sit outside the row button and stay independently operable", async () => {
    const onSelect = vi.fn();
    const onToggle = vi.fn();
    render(<InsetGroup><ListRow title="Sync" onSelect={onSelect} accessory={<Toggle aria-label="Sync enabled" checked={false} onCheckedChange={onToggle} />} /></InsetGroup>);
    const row = screen.getByRole("button", { name: "Sync" });
    const toggle = screen.getByRole("switch", { name: "Sync enabled" });
    expect(row).not.toContainElement(toggle);
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledWith(true);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("disabled selectable rows do not select; className, ref and native attributes pass through", async () => {
    const onSelect = vi.fn();
    const ref = createRef<HTMLLIElement>();
    render(<InsetGroup className="g" data-testid="list"><ListRow ref={ref} title="Locked" onSelect={onSelect} disabled className="r" accessory={<ChevronRight aria-hidden="true" />} /></InsetGroup>);
    await userEvent.click(screen.getByRole("button", { name: "Locked" }));
    expect(onSelect).not.toHaveBeenCalled();
    expect(ref.current).toHaveClass("ui-list-row", "r");
    expect(screen.getByTestId("list").tagName).toBe("UL");
    expect(screen.getByTestId("list").parentElement).toHaveClass("ui-inset-group", "g");
  });
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FxRateForm, validateFx } from "./FxRateForm";

describe("manual FX rate", () => {
  it("validates the rate and the date", () => {
    expect(validateFx("151.20", "2026-11-10", "2026-11-12")).toEqual({ ok: true, rate: 151.2, date: "2026-11-10" });
    expect(validateFx("0", "2026-11-10", "2026-11-12")).toMatchObject({ ok: false, field: "rate" });
    expect(validateFx("1000", "2026-11-10", "2026-11-12")).toMatchObject({ ok: false, field: "rate" });
    expect(validateFx("abc", "2026-11-10", "2026-11-12")).toMatchObject({ ok: false, field: "rate" });
    expect(validateFx("151", "2026-13-40", "2026-11-12")).toMatchObject({ ok: false, field: "date" });
    expect(validateFx("151", "2026-11-13", "2026-11-12")).toMatchObject({ ok: false, field: "date", error: expect.stringContaining("future") });
  });
  it("shows the stored rate and date, saves, and Undo restores a manual one", async () => {
    const setRate = vi.fn(async () => undefined);
    const changed = vi.fn();
    render(<FxRateForm api={{ setRate }} fx={{ rate: 150, date: "2026-11-01", source: "manual" }} today="2026-11-12" onChanged={changed} />);
    expect(screen.getByText(/Stored rate: 150 JPY per 1 USD, as of 2026-11-01 \(typed by you\)/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("JPY per 1 USD"), { target: { value: "151.2" } });
    fireEvent.click(screen.getByRole("button", { name: "Save rate" }));
    await waitFor(() => expect(setRate).toHaveBeenCalledWith(151.2, "2026-11-12"));
    expect(await screen.findByRole("status")).toHaveTextContent("Saved: 151.2 JPY per 1 USD as of 2026-11-12.");
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(setRate).toHaveBeenLastCalledWith(150, "2026-11-01"));
    expect(changed).toHaveBeenCalledTimes(2);
  });
  it("shows a validation error and sends nothing; no Undo after an ECB rate", async () => {
    const setRate = vi.fn(async () => undefined);
    render(<FxRateForm api={{ setRate }} fx={{ rate: 150, date: "2026-11-01", source: "ecb" }} today="2026-11-12" onChanged={() => undefined} />);
    fireEvent.change(screen.getByLabelText("JPY per 1 USD"), { target: { value: "-4" } });
    fireEvent.click(screen.getByRole("button", { name: "Save rate" }));
    expect(await screen.findByText(/above 0 and below 1,000/)).toBeInTheDocument();
    expect(setRate).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("JPY per 1 USD"), { target: { value: "151" } });
    fireEvent.click(screen.getByRole("button", { name: "Save rate" }));
    await waitFor(() => expect(setRate).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });
  it("says so when no rate is stored", () => {
    render(<FxRateForm api={{ setRate: async () => undefined }} fx={null} today="2026-11-12" onChanged={() => undefined} />);
    expect(screen.getByText(/No exchange rate stored/)).toBeInTheDocument();
  });
});

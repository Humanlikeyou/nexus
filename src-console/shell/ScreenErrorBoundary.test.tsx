import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Suspense, lazy } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ScreenErrorBoundary } from "./ScreenErrorBoundary";

afterEach(cleanup);

describe("ScreenErrorBoundary", () => {
  it("a failed lazy chunk shows a quiet Retry, and Retry re-imports", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let fail = true;
    const load = vi.fn(async () => {
      if (fail) throw new Error("Failed to fetch dynamically imported module");
      return { default: () => <p>Loaded screen</p> };
    });
    let Screen = lazy(load);
    const Host = () => <Screen />;
    render(
      <div>
        <nav>Sidebar stays</nav>
        <ScreenErrorBoundary onRetry={() => { Screen = lazy(load); }}>
          <Suspense fallback={null}><Host /></Suspense>
        </ScreenErrorBoundary>
      </div>,
    );
    expect(await screen.findByText("Couldn’t load this screen.")).toBeTruthy();
    expect(screen.getByRole("status")).toBeTruthy();
    expect(screen.getByText("Sidebar stays")).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
    fail = false;
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry" })); });
    expect(await screen.findByText("Loaded screen")).toBeTruthy();
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("changing resetKey (navigating) clears the error", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Boom = () => { throw new Error("x"); };
    const { rerender } = render(<ScreenErrorBoundary resetKey="a"><Boom /></ScreenErrorBoundary>);
    expect(screen.getByText("Couldn’t load this screen.")).toBeTruthy();
    rerender(<ScreenErrorBoundary resetKey="b"><p>Other screen</p></ScreenErrorBoundary>);
    expect(screen.getByText("Other screen")).toBeTruthy();
  });
});

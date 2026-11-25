import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BaselineChart } from "../MindBodyCharts";
import { SeriesGfx } from "./SeriesGfx";
import { LoadGfx } from "./LoadGfx";
import { SleepNightGfx } from "./SleepNightGfx";
import { ReadinessDrivers } from "./ReadinessGfx";
import { METRICS, metricReading } from "../../lib/mindBodyDeep";
import * as shared from "./shared";

const days = Array.from({ length: 10 }, (_, i) => ({ day: `2026-11-${String(i + 1).padStart(2, "0")}`, value: 50 + i }));

afterEach(() => { shared.resetGfxSupportForTests(null); vi.restoreAllMocks(); });

describe("WebGL fallbacks", () => {
  it("jsdom has no WebGL2: the existing SVG chart renders", () => {
    const { container } = render(<BaselineChart points={days} reading={metricReading(METRICS.recovery, days, "2026-11-10")} />);
    expect(container.querySelector("svg")).not.toBeNull();
    expect(container.querySelector("canvas")).toBeNull();
  });
  it("sleep night, load and readiness fall back too", () => {
    const { container } = render(<>
      <SleepNightGfx night={null} stages={[]} fallback={<p>stage fallback</p>} />
      <LoadGfx points={days} ratio={1} fallback={<p>load fallback</p>} />
      <ReadinessDrivers items={[metricReading(METRICS.recovery, days, "2026-11-10")]} />
    </>);
    expect(screen.getByText("stage fallback")).toBeInTheDocument();
    expect(screen.getByText("load fallback")).toBeInTheDocument();
    expect(container.querySelector(".mb-delta")).not.toBeNull();
    expect(container.querySelector("canvas")).toBeNull();
  });
  it("falls back when three or the context fails to load", async () => {
    shared.resetGfxSupportForTests(true);
    vi.spyOn(shared, "acquireShared").mockResolvedValue(null);
    render(<SeriesGfx values={[1, 2, 3]} labels={["a", "b", "c"]} label="x" format={String} usual={null} height={100} lastLabel="Today" readout={() => null} rows={[]} fallback={<p>svg fallback</p>} />);
    expect(await screen.findByText("svg fallback")).toBeInTheDocument();
  });
  it("shows the hidden table and a canvas while WebGL is working, and releases on unmount", async () => {
    shared.resetGfxSupportForTests(true);
    const acquire = vi.spyOn(shared, "acquireShared").mockResolvedValue({ lost: true } as never);
    const release = vi.spyOn(shared, "releaseShared").mockImplementation(() => undefined);
    const { container, unmount } = render(<SeriesGfx values={[1, 2, 3]} labels={["a", "b", "c"]} label="Recovery" format={String} usual={null} height={100} lastLabel="Today" readout={() => null} rows={[{ day: "Nov 1", value: "1" }]} fallback={<p>svg fallback</p>} />);
    expect(container.querySelector("canvas")).not.toBeNull();
    expect(screen.getByRole("table", { hidden: true })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: /Recovery/ })).toHaveAttribute("tabindex", "0");
    await vi.waitFor(() => expect(acquire).toHaveBeenCalled());
    unmount();
    await vi.waitFor(() => expect(release).toHaveBeenCalled());
  });
});

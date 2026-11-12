import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MoneyAccounts } from "./money/MoneyView";
import { unwiredMoney, type MoneyAdapter } from "./money/adapters";
import { RetentionCard } from "./screens/RecordingsPanels";
import { RETENTION_DAYS, type RecordingsAdapter, type RetentionState } from "./neuro/recordings";
import { ServicesView } from "./standalone/ServicesView";
import { sampleStandalone, unwiredStandalone, type StandaloneAdapter } from "./standalone/adapters";
import { unwiredTakes } from "./takes/adapters";
import { useTakeSlots } from "./takes/TakeExtras";
import { CrewsField } from "./screens/CrewsScreen";
import { INITIAL_SNAPSHOT } from "./bridge";
import { unwiredCrews } from "./crews/adapters";
import type { CrewInfo } from "./types";

const WIRE = "Needs wiring";

describe("wire tag only when unwired: Connected accounts", () => {
  it("live accounts: no wire tag on the card", () => {
    const live: MoneyAdapter = { ...unwiredMoney, accounts: () => ({ state: "live", value: [] }) };
    const { container } = render(<MoneyAccounts adapter={live} />);
    expect(container.querySelector(".pk-acc .data-tag")).toBeNull();
  });
  it("unwired accounts: wire tag shows", () => {
    const { container } = render(<MoneyAccounts adapter={unwiredMoney} />);
    expect(container.querySelector(".pk-acc .data-tag")?.textContent).toBe(WIRE);
  });
});

describe("wire tag only when unwired: Keep raw recordings", () => {
  const rec = (retention: RecordingsAdapter["retention"]) => ({ retention, removeConfirmed: () => ({ state: "unwired" }) }) as unknown as RecordingsAdapter;
  const state: RetentionState = { retentionDays: RETENTION_DAYS, expiring: [] };
  it("live: no wire tag in the card header", () => {
    render(<RetentionCard adapter={rec(() => ({ state: "live", value: state }))} />);
    expect(screen.getByRole("heading", { name: "Keep raw recordings" }).closest(".rec-card")!.querySelector(".data-tag")).toBeNull();
  });
  it("unwired: wire tag shows", () => {
    render(<RetentionCard adapter={rec(() => ({ state: "unwired" }))} />);
    expect(screen.getAllByText(WIRE).length).toBeGreaterThan(0);
  });
});

describe("wire tag only when unwired: Services footer note", () => {
  const note = (a: StandaloneAdapter) => { const { container } = render(<ServicesView adapter={a} />); return Array.from(container.querySelectorAll("p.pk-note")).find((p) => p.textContent?.includes("listens on this Mac only")); };
  it("live services: no tag on the note", () => {
    const sv = sampleStandalone().services();
    if (sv.state === "unwired") throw new Error("sample services expected");
    const live: StandaloneAdapter = { ...sampleStandalone(), services: () => ({ state: "live", value: sv.value }) };
    expect(note(live)!.querySelector(".data-tag")).toBeNull();
  });
  it("sample services: sample tag, never the wire tag", () => {
    const n = note(sampleStandalone())!;
    expect(n.textContent).toContain("Sample data");
    expect(n.textContent).not.toContain(WIRE);
  });
  it("unwired services: the page says Needs wiring", () => {
    const { container } = render(<ServicesView adapter={unwiredStandalone} />);
    expect(container.textContent).toContain(WIRE);
  });
});

describe("Life take: no false wire tag for a page with no suggestion source", () => {
  function Probe({ page }: { page: "life" | "neuro" }) { return <div>{useTakeSlots(page, false, unwiredTakes).block}</div>; }
  it("life has no suggestion source, so no empty box and no tag", () => {
    const { container } = render(<Probe page="life" />);
    expect(container.textContent).not.toContain(WIRE);
    expect(container.textContent).not.toContain("Today's one thing");
  });
  it("a page with a suggestion source that is unwired still shows the tag", () => {
    render(<Probe page="neuro" />);
    expect(screen.getByText(WIRE)).toBeInTheDocument();
  });
});

describe("Crews runs table metrics", () => {
  const run = (over: Partial<CrewInfo>): CrewInfo => ({ id: "r1", name: "daily", status: "completed", task: "t", agents: ["a"], started_at: "2026-11-12T01:00:00Z", finished_at: "2026-11-12T01:00:30Z", tokens: 1200, requests: 3, cost_usd: 0, ...over } as CrewInfo);
  const data = (runs: CrewInfo[]) => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, crew: { health: "online" as const, runs, tools: [] } });
  it("every run reported: values shown, no wire tag on the runs note", () => {
    render(<CrewsField data={data([run({})])} adapter={unwiredCrews} />);
    const note = screen.getByText(/Duration, tokens and cost per run/);
    expect(note.querySelector(".data-tag")).toBeNull();
    expect(screen.getByText("30 s")).toBeInTheDocument();
  });
  it("a run without metrics: dash and the wire tag", () => {
    render(<CrewsField data={data([run({ finished_at: null, tokens: null })])} adapter={unwiredCrews} />);
    expect(screen.getByText(/Duration, tokens and cost per run/).querySelector(".data-tag")).not.toBeNull();
  });
});

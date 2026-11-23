import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: Array<{ cmd: string; args: { event: { data: Record<string, unknown>; kind: string; page: string; day: string } } }> = [];
const refuse = vi.hoisted(() => ({ on: false }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: unknown) => { calls.push({ cmd, args: args as never }); if (refuse.on) throw new Error("refused"); return true; }),
}));

import { askMore } from "../amaraTake/takeView";
import { TakeRating } from "./TakeRating";
import { recordTakeAsked, resetTakeEventsForTest, setTakeEvents, todayJst } from "./events";

const appended = () => calls.filter((c) => c.cmd === "take_events_append").map((c) => c.args.event);
beforeEach(() => { calls.length = 0; refuse.on = false; resetTakeEventsForTest(); });

describe("G6 take rating", () => {
  it("Helpful writes one take_rating {takeId, helpful:true} and shows Thanks in place of the buttons", async () => {
    render(<TakeRating page="home" takeId="home-1" />);
    fireEvent.click(screen.getByRole("button", { name: "Helpful" }));
    expect(await screen.findByText("Thanks")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Not helpful" })).toBeNull();
    expect(appended()).toHaveLength(1);
    expect(appended()[0]).toMatchObject({ kind: "take_rating", page: "home", data: { takeId: "home-1", helpful: true } });
  });
  it("Not helpful records false", async () => {
    render(<TakeRating page="fitness" takeId="fitness-9" />);
    fireEvent.click(screen.getByRole("button", { name: "Not helpful" }));
    await screen.findByText("Thanks");
    expect(appended()[0].data).toEqual({ takeId: "fitness-9", helpful: false });
  });
  it("one rating per take per day: a take already rated today shows Thanks, no buttons", () => {
    setTakeEvents([{ id: "r", kind: "take_rating", at: 1, day: todayJst(), page: "home", data: { takeId: "home-1", helpful: true } }]);
    render(<TakeRating page="home" takeId="home-1" />);
    expect(screen.getByText("Thanks")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Helpful" })).toBeNull();
    expect(appended()).toHaveLength(0);
  });
  it("a rating from an earlier day does not block today's", () => {
    setTakeEvents([{ id: "r", kind: "take_rating", at: 1, day: "2020-01-01", page: "home", data: { takeId: "home-1", helpful: true } }]);
    render(<TakeRating page="home" takeId="home-1" />);
    expect(screen.getByRole("button", { name: "Helpful" })).toBeInTheDocument();
  });
  it("error: when the store refuses, it says so and keeps the buttons (never a false Thanks)", async () => {
    refuse.on = true;
    render(<TakeRating page="home" takeId="home-1" />);
    fireEvent.click(screen.getByRole("button", { name: "Helpful" }));
    expect(await screen.findByText("Couldn't save that")).toBeInTheDocument();
    expect(screen.queryByText("Thanks")).toBeNull();
    expect(screen.getByRole("button", { name: "Not helpful" })).toBeInTheDocument();
  });
});

describe("G2 + G7 Ask more is logged", () => {
  it("recordTakeAsked appends take_asked with exactly {takeId} and the JST day", async () => {
    expect(await recordTakeAsked("home", "home-5")).toBe(true);
    const [e] = appended();
    expect(e).toMatchObject({ kind: "take_asked", page: "home", day: todayJst(), data: { takeId: "home-5" } });
    expect(Object.keys(e.data)).toEqual(["takeId"]);
  });
  it("a live take logs once and carries its text and id in the event detail", async () => {
    const seen: unknown[] = [];
    const on = (e: Event) => seen.push((e as CustomEvent).detail);
    window.addEventListener("amara:ask-more", on);
    askMore({ from: "Home", chips: [], question: "q", sample: false, text: "the take", takeId: "home-7", page: "home" });
    window.removeEventListener("amara:ask-more", on);
    expect(seen[0]).toMatchObject({ text: "the take", takeId: "home-7" });
    await waitFor(() => expect(appended()).toHaveLength(1));
    expect(appended()[0].data).toEqual({ takeId: "home-7" });
  });
  it("sample takes and takes without an id are not logged; a refused append does not throw", async () => {
    askMore({ from: "Home", chips: [], question: "q", sample: true, takeId: "home-7", page: "home" });
    askMore({ from: "Home", chips: [], question: "q", sample: false });
    refuse.on = true;
    askMore({ from: "Home", chips: [], question: "q", sample: false, takeId: "home-8", page: "home" });
    await new Promise((r) => setTimeout(r, 20));
    expect(appended().every((e) => e.data.takeId === "home-8")).toBe(true);
  });
});

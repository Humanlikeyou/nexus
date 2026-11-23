import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useRouteNavigation } from "../../src-console/app/useRouteNavigation";

vi.mock("../../src-console/app/lazyScreens", () => ({ preloadScreen: () => null, preloadAllScreens: () => () => undefined }));
vi.mock("../../src-console/app/routeMotion", () => ({ routeMotion: () => "none", transitionRoute: (_k: string, _t: unknown, commit: () => void) => commit() }));

const home = { id: "terrance", view: "main" } as const;
const settings = { id: "settings", view: "main" } as const;

/** Runs requestAnimationFrame callbacks by hand, one frame per call. */
let frames: FrameRequestCallback[] = [];
const nextFrame = () => act(() => { const run = frames; frames = []; run.forEach((cb) => cb(0)); });

beforeEach(() => {
  frames = [];
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { frames.push(cb); return frames.length; });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
  document.body.innerHTML = "";
});
afterEach(() => { document.body.innerHTML = ""; });

function mountField(withHeading = true) {
  const field = document.createElement("div");
  field.className = "active-field";
  const heading = document.createElement("h1");
  if (withHeading) field.appendChild(heading);
  document.body.appendChild(field);
  return { field, heading };
}

describe("useRouteNavigation", () => {
  it("starts on the initial route and commits a navigation", () => {
    const { result } = renderHook(() => useRouteNavigation(home));
    expect(result.current.route).toEqual(home);
    act(() => result.current.navigate(settings));
    expect(result.current.route).toEqual(settings);
  });

  it("leaves launch focus alone, then focuses the new page's heading", () => {
    const { heading } = mountField();
    const { result } = renderHook(() => useRouteNavigation(home));
    nextFrame();
    expect(document.activeElement).not.toBe(heading);
    act(() => result.current.navigate(settings));
    nextFrame();
    expect(document.activeElement).toBe(heading);
  });

  it("keeps trying while the heading can't take focus yet, instead of giving up after one call", () => {
    const { heading } = mountField();
    const { result } = renderHook(() => useRouteNavigation(home));
    let ready = false;
    const real = heading.focus.bind(heading);
    vi.spyOn(heading, "focus").mockImplementation((options) => { if (ready) real(options); });
    act(() => result.current.navigate(settings));
    nextFrame();
    nextFrame();
    expect(document.activeElement).not.toBe(heading);
    ready = true;
    nextFrame();
    expect(document.activeElement).toBe(heading);
  });

  it("waits for a lazy screen's heading to appear", () => {
    const { field } = mountField(false);
    const { result } = renderHook(() => useRouteNavigation(home));
    act(() => result.current.navigate(settings));
    nextFrame();
    const heading = document.createElement("h1");
    field.appendChild(heading);
    nextFrame();
    expect(document.activeElement).toBe(heading);
  });

  it("doesn't steal focus from a control already inside the page", () => {
    const { field, heading } = mountField();
    const radio = document.createElement("button");
    field.appendChild(radio);
    const { result } = renderHook(() => useRouteNavigation(home));
    act(() => result.current.navigate(settings));
    radio.focus();
    nextFrame();
    expect(document.activeElement).toBe(radio);
    expect(document.activeElement).not.toBe(heading);
  });
});

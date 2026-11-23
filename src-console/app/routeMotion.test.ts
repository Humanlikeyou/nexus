// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { routeMotion, transitionRoute } from "./routeMotion";

const landing = { id: "terrance", view: "main" } as const;
const today = { id: "terrance", view: "today" } as const;
const chat = { id: "chat", view: "main" } as const;
const crews = { id: "chat", view: "crews" } as const;
const settings = { id: "settings", view: "main" } as const;
const devices = { id: "settings", view: "devices" } as const;

describe("routeMotion", () => {
  it("treats the lens page as its own space, in both directions", () => {
    expect(routeMotion(landing, chat)).toBe("space");
    expect(routeMotion(chat, landing)).toBe("space");
    expect(routeMotion(landing, today)).toBe("space");
  });
  it("moves sections and sub-views differently", () => {
    expect(routeMotion(chat, settings)).toBe("section");
    expect(routeMotion(today, chat)).toBe("section");
    expect(routeMotion(chat, crews)).toBe("subview");
  });
  it("stays still when the screen doesn't change", () => {
    expect(routeMotion(chat, chat)).toBe("none");
    expect(routeMotion(settings, devices)).toBe("none");
  });
});

describe("transitionRoute", () => {
  afterEach(() => { vi.unstubAllGlobals(); delete document.documentElement.dataset.reduceMotion; Reflect.deleteProperty(document, "startViewTransition"); });

  it("commits without View Transitions", () => {
    const commit = vi.fn();
    transitionRoute("section", chat, commit);
    expect(commit).toHaveBeenCalledOnce();
  });

  it("commits once inside the transition and clears its markers when it ends", async () => {
    const commit = vi.fn();
    const finished = Promise.resolve();
    Object.assign(document, { startViewTransition: vi.fn((update: () => void) => { update(); return { ready: Promise.resolve(), finished }; }) });
    transitionRoute("subview", crews, commit);
    expect(commit).toHaveBeenCalledOnce();
    expect(document.documentElement.dataset.routeMotion).toBe("subview");
    await vi.waitFor(() => expect(document.documentElement.dataset.routeMotion).toBeUndefined());
  });

  it("never starts a transition under reduced motion", () => {
    document.documentElement.dataset.reduceMotion = "true";
    const start = vi.fn();
    Object.assign(document, { startViewTransition: start });
    const commit = vi.fn();
    transitionRoute("space", chat, commit);
    expect(commit).toHaveBeenCalledOnce();
    expect(start).not.toHaveBeenCalled();
  });

  it("still navigates when the transition rejects", async () => {
    const commit = vi.fn();
    Object.assign(document, { startViewTransition: (update: () => void) => { update(); return { ready: Promise.reject(new Error("skipped")), finished: Promise.resolve() }; } });
    transitionRoute("section", chat, commit);
    await Promise.resolve();
    expect(commit).toHaveBeenCalledOnce();
  });
});

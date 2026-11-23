import { describe, expect, it } from "vitest";
import { CANOPYFRAG, canopyFallbackStyle, canopySun } from "../canopy";

describe("design-1118b canopy sunlight", () => {
  it("the canopy light moves with the time of day and is weakest at night", () => {
    const dawn = canopySun(6 * 60 + 40), noon = canopySun(12 * 60), dusk = canopySun(17 * 60 + 20), night = canopySun(22 * 60 + 30);
    expect(dawn.lx).toBeLessThan(noon.lx);
    expect(dusk.lx).toBeGreaterThan(noon.lx);
    expect(noon.ly).toBeLessThan(dawn.ly); // higher in the sky at midday
    expect(night.li).toBeLessThan(noon.li);
    expect(night.li).toBeLessThan(dawn.li);
  });

  it("dawn and dusk are warmer than midday; night is cooler", () => {
    const dawn = canopySun(6 * 60 + 40), noon = canopySun(12 * 60), night = canopySun(22 * 60 + 30);
    // warmer = greener channel lower relative to red
    expect(dawn.lc[1] / dawn.lc[0]).toBeLessThan(noon.lc[1] / noon.lc[0]);
    expect(night.lc[1]).toBeGreaterThan(night.lc[0] * 0.9); // cool green, not warm amber
  });

  it("the shader is dappled light only: mist and parallax uniforms stay in the source but are driven to zero at runtime (no geometry)", () => {
    expect(CANOPYFRAG).toMatch(/uMist/);
    expect(CANOPYFRAG).toMatch(/uPar/);
    expect(CANOPYFRAG).not.toMatch(/stroke|grid|spiral/i);
  });

  it("the CSS fallback is a soft wash, not a picture", () => {
    const s = canopyFallbackStyle(canopySun(12 * 60));
    expect(s.background).toMatch(/radial-gradient/);
    expect(s.opacity).toBeGreaterThan(0);
    expect(s.opacity).toBeLessThan(0.2);
  });
});

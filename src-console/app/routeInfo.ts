import type { AmaraTierInfo, TerrenceInteraction } from "../types";

/**
 * Where Amara's reply was routed, from the router's response (never guessed from /deep or the
 * Deep toggle). Single swap point: change only this function if Rust's field names change.
 *
 * The router (confirmed) sends X-Amara-Route (fast|deep|claude, the tier that actually answered
 * after any fallback), X-Amara-Model and X-Amara-Route-Reason on every chat response, incl.
 * 502/503; the JSON `model` field is the real model id. The webview never sees those headers:
 * Rust (terrence_runtime_bridge.rs) makes the HTTP call, so Rust must pass them through on
 * TerrenceInteraction. Read order:
 *  1. `interaction.route` = { tier: X-Amara-Route, model: X-Amara-Model ?? JSON model,
 *     reason: X-Amara-Route-Reason } (proposed shape; not sent by Rust yet).
 *  2. flat `routeTier` / `servedModel` (alternative), then `routerModel` (JSON model fallback).
 *  3. `interaction.tier`: what Rust sends today (amara.tier body / X-Amara-Tier / route log /
 *     dry-run prediction).
 * claude → its own "Claude" badge. `interaction.model` is Hermes's configured default and is
 * never used as the served model. Absent or unknown → null (no badge; header says "Amara Router").
 */
export type RouteInfo = { tier: AmaraTierInfo | null; model: string | null };

const ROUTER_TIER: Record<string, AmaraTierInfo["tier"]> = { fast: "fast", light: "fast", deep: "deep", claude: "claude" };
const str = (v: unknown): string | null => (typeof v === "string" && v.trim() !== "" ? v.trim() : null);

export function routeInfoFrom(interaction: TerrenceInteraction): RouteInfo {
  const raw = interaction as unknown as Record<string, unknown>;
  const route = raw.route && typeof raw.route === "object" ? (raw.route as Record<string, unknown>) : null;
  const routeTierName = str(route?.tier) ?? str(raw.routeTier);
  const mapped = routeTierName ? ROUTER_TIER[routeTierName.toLowerCase()] : undefined;
  const tier: AmaraTierInfo | null = mapped
    ? { tier: mapped, reason: str(route?.reason) ?? "", source: "router_reply" }
    : interaction.tier && ROUTER_TIER[interaction.tier.tier] ? interaction.tier : null;
  const model = str(route?.model) ?? str(raw.servedModel) ?? str(raw.routerModel);
  return { tier, model };
}

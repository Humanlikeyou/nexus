import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

// Decision 0063: Enterprise › Workspaces in the built app (mocked backend), both themes.
const NOW = "2026-11-26T01:00:00Z";
const biz = { businessId: "taiyo-skin", name: "TaiyoSkin", description: "Skin care brand.", ventureId: "taiyo-skin", status: "active", createdAt: NOW, updatedAt: NOW };
const SECTIONS = [["identity", "Brand identity and voice"], ["products", "Product catalogue"], ["formulations", "Approved formulations"], ["ingredients", "Ingredient information"], ["audiences", "Target audiences"], ["positioning", "Brand positioning"], ["objectives", "Business objectives"], ["website", "Website structure"], ["documents", "Approved business documents"], ["decisions", "Historical decisions and recommendations"]];
const connector = (kind: string, label: string, state: string, detail: string, stored = false) => ({
  connectorId: kind, kind, label, description: `${label}.`, state, detail, checkedAt: NOW, externalWrites: false, check: kind === "local_repo" ? "local" : "network",
  config: [{ name: kind === "local_repo" ? "path" : "project_url", label: kind === "local_repo" ? "Folder" : "Project URL", hint: "hint", value: kind === "local_repo" ? "/Users/me/site" : null }],
  secrets: kind === "local_repo" ? [] : [{ name: "service_role_key", label: `${label} key`, hint: "Kept in the Keychain; never shown again", stored }],
  sync: kind === "local_repo" ? { state: "succeeded", detail: "7 found, 7 added or updated", last: { runId: "r1", connectorId: kind, startedAt: NOW, finishedAt: NOW, status: "succeeded", itemsSeen: 7, itemsChanged: 7, conflicts: 0, detail: "7 found, 7 added or updated" } } : { state: "unavailable", detail: "Sync arrives with read-only integrations (Phase 12).", last: null },
});
const connectors = [
  connector("supabase", "Supabase", "auth_required", "Key stored in the Keychain but not verified.", true),
  connector("hostinger", "Hostinger", "disconnected", "Not set up."),
  connector("instagram", "Instagram", "error", "Couldn't read the Keychain."),
  connector("local_repo", "Website source on this Mac", "connected", "Folder found and readable on this Mac."),
];
const item = { itemId: "i1", section: "products", title: "FORGE", body: "Price in the source: set by FORGE_PRICE_USD in the source", status: "unverified", source: "sync", sourceRef: "local_repo:src/data/products.ts#forge", version: 1, approvedAt: null, createdAt: NOW, updatedAt: NOW };

function data() {
  return {
    ...responses(),
    business_list: [biz],
    business_overview: { business: biz, sections: SECTIONS.map(([section, label]) => ({ section, label, approved: 0, draft: 0, unverified: section === "products" ? 1 : 0 })), connectors: connectors.map((c) => ({ connectorId: c.connectorId, label: c.label, state: c.state })), documents: 0, approvedDocuments: 0, pendingProposals: 0, memoryNotes: 0 },
    business_context_list: [item],
    business_memory_list: [],
    business_documents: [],
    business_connectors: connectors,
    business_permissions: [{ actor: "model", capability: "read_approved_context", label: "Read approved context", allowed: true, updatedAt: NOW }, { actor: "model", capability: "read_unverified_context", label: "Read drafts and unverified context", allowed: false, updatedAt: NOW }],
    business_context_for_amara: [],
    business_proposals: [],
    business_activity: [{ activityId: "a1", scope: "business", kind: "business.created", subjectRef: "biz:taiyo-skin/business", occurredAt: NOW, recordedAt: NOW, source: "scheduler", provenance: "automated", certainty: "confirmed", sourceRef: null, actor: "system", summary: "Workspace created: TaiyoSkin", dataJson: null }],
    business_recommendations: [],
    business_site_changes: [],
    business_report: { report: "website", generatedAt: NOW, notes: [], insights: [
      { id: "web.views.total", group: "Page performance", title: "Page views", kind: "measured", status: "available", statement: "2279 page views recorded by the site's own analytics.", value: 2279, unit: "views", n: 2279, rows: [], window: "2026-03-22 to 2026-11-20", sources: ["Supabase page_views (synced 2026-11-26)"], note: null },
      { id: "web.pages.top", group: "Page performance", title: "Most viewed pages", kind: "calculated", status: "available", statement: "Views per page.", value: null, unit: null, n: 2279, rows: [{ label: "/shop", value: 900, share: 0.4 }], window: null, sources: ["Supabase page_views"], note: null },
      { id: "search.clicks", group: "Search", title: "Search clicks", kind: "measured", status: "unavailable", statement: "Not available.", value: null, unit: null, n: null, rows: [], window: null, sources: [], note: "Search Console is deferred (decision 0064)." },
    ] },
  };
}

async function openWorkspaces(page: Page, theme: "light" | "dark") {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  await navButton(page, "Enterprise").click();
  const field = activeField(page, "Enterprise");
  await field.getByRole("radio", { name: "Workspaces", exact: true }).click();
  await expect(field.getByRole("heading", { level: 1, name: "Workspaces" })).toBeVisible();
  return field;
}

async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}

test("Workspaces: TaiyoSkin's overview names every connector state, and every section opens with no unmocked call", async ({ page, theme, consoleErrors }) => {
  const field = await openWorkspaces(page, theme);
  await expect(field.getByText('linked to the ledger venture "taiyo-skin"', { exact: false })).toBeVisible();
  const states = field.getByRole("list", { name: "Integration status" });
  // StatusText = glyph + word in one element.
  for (const s of ["Authentication required", "Not connected", "Error", "Connected"]) await expect(states.getByText(new RegExp(`${s}$`))).toBeVisible();
  for (const tab of ["Context & memory", "Documents", "Integrations", "Intelligence", "Website changes", "Permissions", "Approvals", "Activity", "Summary"]) {
    await field.getByRole("radio", { name: tab, exact: true }).click();
    await expect(field.getByRole("radio", { name: tab, exact: true })).toHaveAttribute("aria-checked", "true");
    await expect(field.getByText(/^Loading /)).toHaveCount(0);
  }
  expect(await page.evaluate(() => (window as unknown as { __E2E_UNMOCKED: string[] }).__E2E_UNMOCKED.filter((c) => c.startsWith("business_")))).toEqual([]);
  expect(consoleErrors).toEqual([]);
});

test("Workspaces: a stored key is a password field that is never filled, and only the checked folder reads as connected", async ({ page, theme }) => {
  const field = await openWorkspaces(page, theme);
  await field.getByRole("radio", { name: "Integrations", exact: true }).click();
  const key = field.getByLabel("Supabase key");
  await expect(key).toHaveAttribute("type", "password");
  await expect(key).toHaveValue("");
  await expect(field.getByText("Key stored in the Keychain but not verified.")).toBeVisible();
  await expect(field.getByRole("region", { name: "Website source on this Mac" }).or(field.locator("section", { has: field.getByRole("heading", { name: "Website source on this Mac" }) })).getByRole("button", { name: "Sync now" })).toBeEnabled();
  await expect(field.getByText("Writes outside the app: none.")).toHaveCount(4);
  // The Context tab labels unverified material.
  await field.getByRole("radio", { name: "Context & memory", exact: true }).click();
  await field.getByLabel("Section", { exact: true }).selectOption("products");
  await expect(field.getByRole("list", { name: "Product catalogue" }).getByText("Unverified", { exact: true })).toBeVisible();
});

for (const tab of ["Summary", "Context & memory", "Integrations", "Intelligence", "Website changes", "Permissions"]) {
  test(`Workspaces › ${tab}: no serious or critical axe violations`, async ({ page, theme }) => {
    const field = await openWorkspaces(page, theme);
    await field.getByRole("radio", { name: tab, exact: true }).click();
    await page.waitForTimeout(400);
    expect(await blocking(page)).toEqual([]);
  });
}

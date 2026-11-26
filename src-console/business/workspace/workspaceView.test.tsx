import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CONNECTOR_STATE, INSIGHT_KIND, SECTIONS, sourceText, type Business, type Connector, type ContextItem, type Insight, type Overview, type Permission, type Proposal, type Report, type WorkspaceCalls } from "./workspaceApi";
import { WorkspaceView } from "./WorkspaceView";

const biz = (id: string, name: string): Business => ({ businessId: id, name, description: null, ventureId: id === "taiyo-skin" ? "taiyo-skin" : null, status: "active", createdAt: "2026-11-26T00:00:00Z", updatedAt: "2026-11-26T00:00:00Z" });
const conn = (kind: string, state: string, detail: string, stored = false): Connector => ({
  connectorId: kind, kind, label: kind === "local_repo" ? "Website source on this Mac" : kind[0].toUpperCase() + kind.slice(1), description: `${kind} description`,
  config: [{ name: kind === "local_repo" ? "path" : "project_url", label: kind === "local_repo" ? "Folder" : "Project URL", hint: "hint", value: kind === "local_repo" ? "/Users/x/site" : null }],
  secrets: kind === "local_repo" ? [] : [{ name: "service_role_key", label: "Service-role key", hint: "Kept in the Keychain", stored }],
  state, detail, checkedAt: "2026-11-26T01:00:00Z", check: kind === "local_repo" ? "local" : "network",
  sync: kind === "local_repo" ? { state: "never", detail: "Not synchronized yet.", last: null } : { state: "unavailable", detail: "Sync arrives with read-only integrations (Phase 12).", last: null },
  externalWrites: false,
});
const item = (over: Partial<ContextItem>): ContextItem => ({ itemId: "i1", section: "products", title: "FORGE", body: "Body", status: "unverified", source: "sync", sourceRef: "local_repo:src/data/products.ts#forge", version: 1, approvedAt: null, createdAt: "", updatedAt: "2026-11-26T00:00:00Z", ...over });
const overview = (b: Business): Overview => ({
  business: b,
  sections: SECTIONS.map((s) => ({ section: s.value, label: s.label, approved: s.value === "identity" ? 1 : 0, draft: 0, unverified: s.value === "products" ? 3 : 0 })),
  connectors: [{ connectorId: "supabase", label: "Supabase", state: "auth_required" }, { connectorId: "hostinger", label: "Hostinger", state: "disconnected" }, { connectorId: "instagram", label: "Instagram", state: "error" }, { connectorId: "local_repo", label: "Website source on this Mac", state: "connected" }],
  documents: 2, approvedDocuments: 1, pendingProposals: 1, memoryNotes: 0,
});
const perms: Permission[] = [
  { actor: "model", capability: "read_approved_context", label: "Read approved context", allowed: true, updatedAt: "" },
  { actor: "model", capability: "read_unverified_context", label: "Read drafts and unverified context", allowed: false, updatedAt: "" },
  { actor: "scheduler", capability: "sync_connectors", label: "Start a synchronization", allowed: false, updatedAt: "" },
];
const proposal: Proposal = { proposalId: "p1", kind: "context_update", itemId: "i1", baseVersion: 1, title: "Positioning", body: "New text", rationale: "Clearer", status: "pending", proposedBy: "amara", proposedAt: "2026-11-26T00:00:00Z", decidedAt: null, executionId: null, detail: null };

function calls(over: Partial<WorkspaceCalls> = {}): WorkspaceCalls & Record<string, ReturnType<typeof vi.fn>> {
  const two = [biz("taiyo-skin", "TaiyoSkin"), biz("second-brand", "Second Brand")];
  return {
    list: vi.fn(async () => two),
    create: vi.fn(async (name: string) => biz("new-co", name)),
    update: vi.fn(async (id: string, name: string) => ({ ...biz(id, name) })),
    overview: vi.fn(async (id: string) => overview(two.find((b) => b.businessId === id) ?? two[0])),
    context: vi.fn(async (_id: string, section: string | null) => (section === "products" ? [item({}), item({ itemId: "i2", title: "SHIELD", status: "approved", source: "user", sourceRef: null, approvedAt: "x", version: 3 })] : [])),
    saveContext: vi.fn(async () => item({ status: "draft" })),
    setContextStatus: vi.fn(async () => item({ status: "approved" })),
    contextHistory: vi.fn(async () => []),
    contextForAmara: vi.fn(async () => [item({ title: "SHIELD", status: "approved" })]),
    memory: vi.fn(async () => []),
    addMemory: vi.fn(async () => ({ memoryId: "m1", text: "x", status: "kept", source: "user", createdAt: "", decidedAt: "" })),
    decideMemory: vi.fn(),
    documents: vi.fn(async () => []),
    addDocument: vi.fn(async () => null),
    setDocumentStatus: vi.fn(),
    search: vi.fn(async () => []),
    connectors: vi.fn(async () => [conn("supabase", "auth_required", "Key stored in the Keychain but not verified.", true), conn("local_repo", "connected", "Folder found and readable.")]),
    configure: vi.fn(),
    pickFolder: vi.fn(async () => null),
    setSecret: vi.fn(async () => conn("supabase", "auth_required", "Key stored in the Keychain but not verified.", true)),
    disconnect: vi.fn(),
    sync: vi.fn(async () => ({ runId: "r1", connectorId: "local_repo", startedAt: "", finishedAt: "", status: "succeeded", itemsSeen: 7, itemsChanged: 7, conflicts: 0, detail: "7 found, 7 added or updated" })),
    permissions: vi.fn(async () => perms),
    setPermission: vi.fn(async () => perms),
    proposals: vi.fn(async () => [proposal]),
    approveProposal: vi.fn(async () => ({ ...proposal, status: "applied" })),
    rejectProposal: vi.fn(async () => ({ ...proposal, status: "rejected" })),
    activity: vi.fn(async () => [{ activityId: "a1", scope: "business", kind: "business.created", subjectRef: "biz:taiyo-skin/business", occurredAt: "2026-11-26T00:00:00Z", recordedAt: "", source: "scheduler", provenance: "automated", certainty: "confirmed", sourceRef: null, actor: "system", summary: "Workspace created: TaiyoSkin", dataJson: null }]),
    recommendations: vi.fn(async () => []),
    check: vi.fn(async () => ({ ...conn("supabase", "connected", "Signed in as nexus_reader: read-only, no personal columns (checked just now).", true) })),
    pickFile: vi.fn(async () => null),
    setupSql: vi.fn(async () => "create role nexus_reader with login password 'REPLACE-WITH-A-LONG-RANDOM-PASSWORD';"),
    report: vi.fn(async (_id: string, which: "website" | "customers") => (which === "website" ? websiteReport : customerReport)),
    interpret: vi.fn(async () => ({ ...websiteReport, notes: [{ noteId: "n1", kind: "interpretation", statement: "The shop page draws most views.", basedOn: ["web.pages.top"], modelTier: "light", createdAt: "2026-11-26T02:00:00Z" }] })),
    ...over,
  } as never;
}

const insight = (over: Partial<Insight>): Insight => ({ id: "x", group: "Page performance", title: "T", kind: "measured", status: "available", statement: "S", value: null, unit: null, n: null, rows: [], window: null, sources: [], note: null, ...over });
const websiteReport: Report = {
  report: "website", generatedAt: "2026-11-26T01:00:00Z", notes: [],
  insights: [
    insight({ id: "web.views.total", title: "Page views", value: 2279, unit: "views", n: 2279, statement: "2279 page views recorded by the site's own analytics.", sources: ["Supabase page_views (synced 2026-11-26)"], window: "2026-03-22 to 2026-11-20" }),
    insight({ id: "web.pages.top", title: "Most viewed pages", kind: "calculated", rows: [{ label: "/shop", value: 900, share: 0.4 }] }),
    insight({ id: "search.clicks", group: "Search", title: "Search clicks", status: "unavailable", statement: "Not available.", note: "Search Console is deferred (decision 0064)." }),
  ],
};
const customerReport: Report = {
  report: "customers", generatedAt: "2026-11-26T01:00:00Z", notes: [],
  insights: [
    insight({ id: "cust.concerns", group: "Frequent customer concerns", title: "Concerns chosen in the skin quiz", status: "too_few", rows: [{ label: "hyperpigmentation", value: 7, share: 0.58 }], note: "Answers given by fewer than 5 people are combined and not shown." }),
    insight({ id: "cust.faq", group: "Frequently asked questions", title: "Contact request topics", status: "suppressed", statement: "Every answer here was given by fewer than 5 people, so none is shown." }),
    insight({ id: "cust.content_gaps", group: "Content gaps", title: "Concerns no page seems to cover", kind: "hypothesis", statement: "Unverified: these concerns were named by 5 or more people." }),
  ],
};

const openTab = (name: string) => fireEvent.click(screen.getByRole("radio", { name }));

describe("decision 0063: business workspaces", () => {
  it("opens on TaiyoSkin's overview with every connector state in words", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading workspaces");
    await screen.findByText(/linked to the ledger venture "taiyo-skin"/);
    expect(c.overview).toHaveBeenCalledWith("taiyo-skin");
    const states = within(screen.getByRole("list", { name: "Integration status" }));
    for (const s of ["Authentication required", "Not connected", "Error", "Connected"]) expect(states.getByText(s)).toBeInTheDocument();
    expect(screen.getByText(/3 unverified/)).toBeInTheDocument();
    expect(screen.getByText(/ARM trading is kept separate/)).toBeInTheDocument();
  });

  it("switching business loads only that business", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    fireEvent.change(screen.getByLabelText("Business"), { target: { value: "second-brand" } });
    await screen.findByText(/not linked to a ledger venture/);
    expect(c.overview).toHaveBeenLastCalledWith("second-brand");
  });

  it("shows a load failure with a retry, never an empty workspace", async () => {
    const c = calls({ list: vi.fn().mockRejectedValueOnce("store locked").mockResolvedValue([biz("taiyo-skin", "TaiyoSkin")]) });
    render(<WorkspaceView calls={c} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load workspaces: store locked");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByText(/linked to the ledger venture/);
  });

  it("labels unverified and approved material and approves against the shown version", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Context & memory");
    fireEvent.change(await screen.findByLabelText("Section"), { target: { value: "products" } });
    const list = within(await screen.findByRole("list", { name: "Product catalogue" }));
    expect(list.getByText("Unverified")).toBeInTheDocument();
    expect(list.getByText("Approved")).toBeInTheDocument();
    expect(list.getByText(/From the website source: src\/data\/products.ts/)).toBeInTheDocument();
    expect(list.queryByRole("button", { name: "Approve SHIELD" })).toBeNull();
    fireEvent.click(list.getByRole("button", { name: "Approve FORGE" }));
    await waitFor(() => expect(c.setContextStatus).toHaveBeenCalledWith("taiyo-skin", "i1", "approved", 1));
    fireEvent.change(screen.getByLabelText("Section"), { target: { value: "formulations" } });
    expect(await screen.findByText(/Formulations are never taken from the website source/)).toBeInTheDocument();
  });

  it("never shows a key, clears the field after storing it, and keeps a stored key short of connected", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Integrations");
    const field = (await screen.findByLabelText("Service-role key")) as HTMLInputElement;
    expect(field.type).toBe("password");
    expect(field.value).toBe("");
    expect(screen.getAllByText("Stored").length).toBeGreaterThan(0);
    expect(screen.getByText("Key stored in the Keychain but not verified.")).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "sk-secret-123" } });
    fireEvent.click(screen.getByRole("button", { name: "Store in Keychain" }));
    await waitFor(() => expect(c.setSecret).toHaveBeenCalledWith("taiyo-skin", "supabase", "service_role_key", "sk-secret-123"));
    expect(field.value).toBe("");
    expect(document.body.textContent).not.toContain("sk-secret-123");
    expect(screen.getByText("No sync in this phase", { exact: false })).toBeInTheDocument();
    expect(screen.getAllByText("Writes outside the app: none.").length).toBe(2);
  });

  it("syncs the local source and reports what the run did", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Integrations");
    const card = (await screen.findByRole("heading", { name: "Website source on this Mac" })).closest("section") as HTMLElement;
    fireEvent.click(within(card).getByRole("button", { name: "Sync now" }));
    expect(await screen.findByText(/Last sync succeeded: 7 found, 7 added or updated/)).toBeInTheDocument();
    expect(c.sync).toHaveBeenCalledWith("taiyo-skin", "local_repo");
  });

  it("permission switches call Rust for this business and show what Amara can see", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Permissions");
    const sw = await screen.findByRole("switch", { name: "Read drafts and unverified context" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    await waitFor(() => expect(c.setPermission).toHaveBeenCalledWith("taiyo-skin", "model", "read_unverified_context", true));
    expect(await screen.findByText(/1 item: SHIELD \(Approved\)/)).toBeInTheDocument();
  });

  it("says plainly when Amara may read nothing", async () => {
    const c = calls({ contextForAmara: vi.fn().mockRejectedValue("not permitted: model may not read_approved_context for this business") });
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Permissions");
    expect(await screen.findByText("Nothing: reading this business's context is off.")).toBeInTheDocument();
  });

  it("approvals go through the native dialog command and report the verified result", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Approvals");
    expect(await screen.findByText("Why: Clearer")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Approve…" }));
    expect(await screen.findByText("Applied and verified: Positioning")).toBeInTheDocument();
    expect(c.approveProposal).toHaveBeenCalledWith("taiyo-skin", "p1");
  });

  it("activity is the business's own history", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Activity");
    expect(await screen.findByText("Workspace created: TaiyoSkin")).toBeInTheDocument();
    expect(screen.getByText(/None yet. Recommendations about this business/)).toBeInTheDocument();
    expect(c.activity).toHaveBeenCalledWith("taiyo-skin");
  });

  it("helpers name every state and source", () => {
    expect(Object.keys(CONNECTOR_STATE).sort()).toEqual(["auth_required", "connected", "disconnected", "error"]);
    expect(SECTIONS).toHaveLength(10);
    expect(sourceText("sync", "local_repo:README.md")).toBe("From the website source: README.md");
    expect(sourceText("user", null)).toBe("Added by you");
  });

  it("a network connector is checked on request, and a deferred one has no controls", async () => {
    const supaSet = { ...conn("supabase", "auth_required", "Set up but not checked yet. Choose Check connection.", true), config: [{ name: "pooler_host", label: "Connection pooler host", hint: "", value: "aws-1-ap-northeast-1.pooler.supabase.com" }, { name: "ca_cert", label: "Supabase CA certificate", hint: "", value: "/Users/me/prod-ca-2021.crt" }] };
    const c = calls({ connectors: vi.fn(async () => [supaSet, { ...conn("google_analytics", "disconnected", "Deferred (decision 0064): the live site doesn't load the GA4 tag."), config: [], secrets: [], check: "deferred", label: "Google Analytics 4" }]) });
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Integrations");
    const ga = (await screen.findByRole("heading", { name: "Google Analytics 4" })).closest("section") as HTMLElement;
    expect(within(ga).getByText(/Deferred \(decision 0064\)/)).toBeInTheDocument();
    expect(within(ga).queryByRole("button")).toBeNull();
    const supa = screen.getByRole("heading", { name: "Supabase" }).closest("section") as HTMLElement;
    expect(within(supa).getByText(/not checked yet/)).toBeInTheDocument();
    expect(within(supa).getByRole("button", { name: "Sync now" })).toBeDisabled();
    expect(within(supa).getByText(/prod-ca-2021.crt/)).toBeInTheDocument();
    expect(within(supa).queryByLabelText("Supabase CA certificate")).toBeNull();
    fireEvent.click(within(supa).getByRole("button", { name: "Show the setup SQL" }));
    expect(await within(supa).findByLabelText("Setup SQL")).toHaveTextContent("REPLACE-WITH-A-LONG-RANDOM-PASSWORD");
    fireEvent.click(within(supa).getByRole("button", { name: "Check connection" }));
    await waitFor(() => expect(c.check).toHaveBeenCalledWith("taiyo-skin", "supabase"));
    expect(await within(supa).findByText(/read-only, no personal columns/)).toBeInTheDocument();
  });

  it("the website report labels every line and says when a figure isn't available", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Intelligence");
    const views = within(await screen.findByRole("list", { name: "Page performance" }));
    expect(views.getAllByText("Measured").length).toBeGreaterThan(0);
    expect(views.getByText("Calculated")).toBeInTheDocument();
    expect(views.getByText(/n = 2279/)).toBeInTheDocument();
    expect(views.getByText(/Source: Supabase page_views/)).toBeInTheDocument();
    expect(views.getByText("/shop: 900 (40%)")).toBeInTheDocument();
    const search = within(screen.getByRole("list", { name: "Search" }));
    expect(search.getByText("Not available")).toBeInTheDocument();
    expect(search.getByText(/Search Console is deferred/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Ask Amara to interpret" }));
    const notes = within(await screen.findByRole("list", { name: "Amara's notes" }));
    expect(notes.getByText("AI interpretation")).toBeInTheDocument();
    expect(notes.getByText(/based on Most viewed pages/)).toBeInTheDocument();
    expect(c.interpret).toHaveBeenCalledWith("taiyo-skin", "website");
  });

  it("the customer report hides small groups and marks hypotheses as unverified", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Intelligence");
    fireEvent.click(await screen.findByRole("radio", { name: "Customers" }));
    expect(await screen.findByText("Too few to tell")).toBeInTheDocument();
    expect(screen.getByText("Hidden: groups under 5")).toBeInTheDocument();
    expect(screen.getByText(INSIGHT_KIND.hypothesis.text)).toBeInTheDocument();
    expect(screen.getByText(/groups under 5 people are hidden/)).toBeInTheDocument();
  });
});

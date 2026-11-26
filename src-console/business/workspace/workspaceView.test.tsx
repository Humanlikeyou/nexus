import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CONNECTOR_STATE, SECTIONS, sourceText, type Business, type Connector, type ContextItem, type Overview, type Permission, type Proposal, type WorkspaceCalls } from "./workspaceApi";
import { WorkspaceView } from "./WorkspaceView";

const biz = (id: string, name: string): Business => ({ businessId: id, name, description: null, ventureId: id === "taiyo-skin" ? "taiyo-skin" : null, status: "active", createdAt: "2026-11-26T00:00:00Z", updatedAt: "2026-11-26T00:00:00Z" });
const conn = (kind: string, state: string, detail: string, stored = false): Connector => ({
  connectorId: kind, kind, label: kind === "local_repo" ? "Website source on this Mac" : kind[0].toUpperCase() + kind.slice(1), description: `${kind} description`,
  config: [{ name: kind === "local_repo" ? "path" : "project_url", label: kind === "local_repo" ? "Folder" : "Project URL", hint: "hint", value: kind === "local_repo" ? "/Users/x/site" : null }],
  secrets: kind === "local_repo" ? [] : [{ name: "service_role_key", label: "Service-role key", hint: "Kept in the Keychain", stored }],
  state, detail, checkedAt: "2026-11-26T01:00:00Z",
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
    ...over,
  } as never;
}

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
    expect(screen.getByText("Sync not available yet", { exact: false })).toBeInTheDocument();
    expect(screen.getAllByText("Writes outside the app: none.").length).toBe(2);
  });

  it("syncs the local source and reports what the run did", async () => {
    const c = calls();
    render(<WorkspaceView calls={c} />);
    await screen.findByText(/linked to the ledger venture/);
    openTab("Integrations");
    fireEvent.click(await screen.findByRole("button", { name: "Sync now" }));
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
});

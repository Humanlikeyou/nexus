/**
 * Decision 0063 (Phase 11): business workspace commands. Every record is the Rust store's, scoped to
 * one business by id; this file only calls it. No secret ever comes back: the Keychain fields say
 * `stored` true or false only.
 */
import { invokeBound } from "../../bridge";
import type {
  ActivityRow_Serialize,
  BusinessConnector_Serialize,
  BusinessContextItem_Serialize,
  BusinessContextVersion_Serialize,
  BusinessDocument_Serialize,
  BusinessKnowledgeHit_Serialize,
  BusinessMemoryNote_Serialize,
  BusinessPermission,
  BusinessProposal_Serialize,
  BusinessReport_Serialize,
  BusinessInsight_Serialize,
  BusinessSiteChange_Serialize,
  BusinessSiteEvent_Serialize,
  BusinessSiteMatch_Serialize,
  BusinessSiteSnapshot,
  BusinessSyncRun_Serialize,
  BusinessWorkspaceOverview_Serialize,
  Business_Serialize,
  RecommendationRow_Serialize,
} from "../../bindings.gen";

export type Business = Business_Serialize;
export type Overview = BusinessWorkspaceOverview_Serialize;
export type ContextItem = BusinessContextItem_Serialize;
export type ContextVersion = BusinessContextVersion_Serialize;
export type MemoryNote = BusinessMemoryNote_Serialize;
export type BizDocument = BusinessDocument_Serialize;
export type KnowledgeHit = BusinessKnowledgeHit_Serialize;
export type Connector = BusinessConnector_Serialize;
export type SyncRun = BusinessSyncRun_Serialize;
export type Permission = BusinessPermission;
export type Proposal = BusinessProposal_Serialize;
export type Activity = ActivityRow_Serialize;
export type Recommendation = RecommendationRow_Serialize;
export type Report = BusinessReport_Serialize;
export type Insight = BusinessInsight_Serialize;
export type SiteChange = BusinessSiteChange_Serialize;
export type SiteEvent = BusinessSiteEvent_Serialize;
export type SiteMatch = BusinessSiteMatch_Serialize;
export type SiteSnapshot = BusinessSiteSnapshot;
export type SiteChangeInput = { contentType: string; value: string; path: string | null; documentId: string | null; sourceRefs: string[] };

/** Decision 0065: what a change can be today. Bundled content waits for a source that reproduces the live build. */
export const SITE_TYPES: Array<{ value: string; label: string; available: boolean }> = [
  { value: "seo_description", label: "SEO description", available: true },
  { value: "seo_title", label: "SEO title", available: true },
  { value: "sitemap", label: "Sitemap", available: true },
  { value: "robots", label: "robots.txt", available: true },
  { value: "asset_replace", label: "Replace an image or asset", available: true },
  { value: "asset_delete", label: "Delete a file", available: true },
  { value: "product_page", label: "Product page (not available yet)", available: false },
  { value: "product_description", label: "Product description (not available yet)", available: false },
  { value: "ingredients", label: "Ingredient information (not available yet)", available: false },
  { value: "faq", label: "FAQ (not available yet)", available: false },
  { value: "blog", label: "Blog content (not available yet)", available: false },
  { value: "landing_page", label: "Landing page (not available yet)", available: false },
];
export const SITE_STATUS: Record<string, string> = {
  draft: "Draft", approved: "Approved", packaged: "Ready to upload", verified: "Published and verified", verify_failed: "Verification failed",
  rolled_back: "Rolled back", rollback_failed: "Rollback not verified", rejected: "Rejected",
};
export const SITE_FLAG: Record<string, string> = { pricing: "Pricing", formulation: "Formulation", customer_communication: "Customer communication", deletion: "Deletion" };

/** Decision 0064: what each insight is. Words, never colour alone. */
export const INSIGHT_KIND: Record<string, { text: string; tone: "info" | "neutral" | "accent" | "warning" }> = {
  measured: { text: "Measured", tone: "info" },
  calculated: { text: "Calculated", tone: "neutral" },
  interpretation: { text: "AI interpretation", tone: "accent" },
  hypothesis: { text: "Unverified hypothesis", tone: "warning" },
};
export const INSIGHT_STATUS: Record<string, string> = { unavailable: "Not available", too_few: "Too few to tell", suppressed: "Hidden: groups under 5" };
export type ContextInput = { itemId: string | null; section: string; title: string; body: string; status: "draft" | "unverified"; expectedVersion: number | null };

/** The ten sections, in the Rust order (business_workspace.rs SECTIONS). */
export const SECTIONS: Array<{ value: string; label: string }> = [
  { value: "identity", label: "Brand identity and voice" },
  { value: "products", label: "Product catalogue" },
  { value: "formulations", label: "Approved formulations" },
  { value: "ingredients", label: "Ingredient information" },
  { value: "audiences", label: "Target audiences" },
  { value: "positioning", label: "Brand positioning" },
  { value: "objectives", label: "Business objectives" },
  { value: "website", label: "Website structure" },
  { value: "documents", label: "Approved business documents" },
  { value: "decisions", label: "Historical decisions and recommendations" },
];
export const sectionLabel = (s: string) => SECTIONS.find((x) => x.value === s)?.label ?? s;

/** Material status as words; never colour alone. */
export const MATERIAL: Record<string, { text: string; tone: "success" | "neutral" | "warning" | "info" }> = {
  approved: { text: "Approved", tone: "success" },
  draft: { text: "Draft", tone: "info" },
  unverified: { text: "Unverified", tone: "warning" },
  archived: { text: "Archived", tone: "neutral" },
};

/** Connector states from Rust (connectors.rs derive_state). */
export const CONNECTOR_STATE: Record<string, { text: string; kind: "ok" | "off" | "risk" | "fail" }> = {
  connected: { text: "Connected", kind: "ok" },
  disconnected: { text: "Not connected", kind: "off" },
  auth_required: { text: "Authentication required", kind: "risk" },
  error: { text: "Error", kind: "fail" },
};
export const SYNC_STATE: Record<string, string> = {
  unavailable: "No sync in this phase",
  never: "Never synchronized",
  running: "Synchronizing…",
  succeeded: "Last sync succeeded",
  failed: "Last sync failed",
  interrupted: "Last sync was interrupted",
};

export const ACTOR_LABEL: Record<string, string> = { model: "Amara", scheduler: "Background jobs", integration: "Integrations" };

export type WorkspaceCalls = {
  list: () => Promise<Business[]>;
  create: (name: string, description: string | null) => Promise<Business>;
  update: (id: string, name: string, description: string | null, ventureId: string | null, status: string) => Promise<Business>;
  overview: (id: string) => Promise<Overview>;
  context: (id: string, section: string | null) => Promise<ContextItem[]>;
  saveContext: (id: string, input: ContextInput) => Promise<ContextItem>;
  setContextStatus: (id: string, itemId: string, status: string, expectedVersion: number) => Promise<ContextItem>;
  contextHistory: (id: string, itemId: string) => Promise<ContextVersion[]>;
  contextForAmara: (id: string) => Promise<ContextItem[]>;
  memory: (id: string) => Promise<MemoryNote[]>;
  addMemory: (id: string, text: string) => Promise<MemoryNote>;
  decideMemory: (id: string, memoryId: string, status: "kept" | "dismissed") => Promise<MemoryNote>;
  documents: (id: string) => Promise<BizDocument[]>;
  addDocument: (id: string) => Promise<BizDocument | null>;
  setDocumentStatus: (id: string, docId: string, status: string, expectedVersion: number) => Promise<BizDocument>;
  search: (id: string, query: string) => Promise<KnowledgeHit[]>;
  connectors: (id: string) => Promise<Connector[]>;
  configure: (id: string, connectorId: string, values: Record<string, string>) => Promise<Connector>;
  pickFolder: (id: string, connectorId: string) => Promise<Connector | null>;
  setSecret: (id: string, connectorId: string, field: string, value: string) => Promise<Connector>;
  disconnect: (id: string, connectorId: string) => Promise<Connector>;
  sync: (id: string, connectorId: string) => Promise<SyncRun>;
  check: (id: string, connectorId: string) => Promise<Connector>;
  pickFile: (id: string, connectorId: string, field: string) => Promise<Connector | null>;
  setupSql: () => Promise<string>;
  report: (id: string, report: "website" | "customers") => Promise<Report>;
  interpret: (id: string, report: "website" | "customers") => Promise<Report>;
  siteSnapshot: (id: string) => Promise<SiteSnapshot>;
  siteLocate: (id: string, query: string) => Promise<SiteMatch[]>;
  siteDraft: (id: string, input: SiteChangeInput) => Promise<SiteChange>;
  siteChanges: (id: string) => Promise<SiteChange[]>;
  siteEvents: (id: string, changeId: string) => Promise<SiteEvent[]>;
  siteApprove: (id: string, changeId: string) => Promise<SiteChange>;
  siteReject: (id: string, changeId: string) => Promise<SiteChange>;
  siteConfirmBackup: (id: string, changeId: string) => Promise<SiteChange>;
  sitePackage: (id: string, changeId: string) => Promise<SiteChange>;
  siteVerify: (id: string, changeId: string, rollback: boolean) => Promise<SiteChange>;
  permissions: (id: string) => Promise<Permission[]>;
  setPermission: (id: string, actor: string, capability: string, allowed: boolean) => Promise<Permission[]>;
  proposals: (id: string) => Promise<Proposal[]>;
  approveProposal: (id: string, proposalId: string) => Promise<Proposal>;
  rejectProposal: (id: string, proposalId: string) => Promise<Proposal>;
  activity: (id: string) => Promise<Activity[]>;
  recommendations: (id: string) => Promise<Recommendation[]>;
};

const call = <T,>(cmd: string, args?: Record<string, unknown>) => invokeBound<T>(cmd, args);

export const liveWorkspaceCalls: WorkspaceCalls = {
  list: () => call("business_list"),
  create: (name, description) => call("business_create", { name, description, ventureId: null }),
  update: (businessId, name, description, ventureId, status) => call("business_update", { businessId, name, description, ventureId, status }),
  overview: (businessId) => call("business_overview", { businessId }),
  context: (businessId, section) => call("business_context_list", { businessId, section }),
  saveContext: (businessId, input) => call("business_context_save", { businessId, input }),
  setContextStatus: (businessId, itemId, status, expectedVersion) => call("business_context_set_status", { businessId, itemId, status, expectedVersion }),
  contextHistory: (businessId, itemId) => call("business_context_history", { businessId, itemId }),
  contextForAmara: (businessId) => call("business_context_for_amara", { businessId }),
  memory: (businessId) => call("business_memory_list", { businessId }),
  addMemory: (businessId, text) => call("business_memory_add", { businessId, text }),
  decideMemory: (businessId, memoryId, status) => call("business_memory_decide", { businessId, memoryId, status }),
  documents: (businessId) => call("business_documents", { businessId }),
  addDocument: (businessId) => call("business_document_add", { businessId }),
  setDocumentStatus: (businessId, docId, status, expectedVersion) => call("business_document_set_status", { businessId, docId, status, expectedVersion }),
  search: (businessId, query) => call("business_knowledge_search", { businessId, query }),
  connectors: (businessId) => call("business_connectors", { businessId }),
  configure: (businessId, connectorId, values) => call("business_connector_configure", { businessId, connectorId, values }),
  pickFolder: (businessId, connectorId) => call("business_connector_pick_folder", { businessId, connectorId }),
  setSecret: (businessId, connectorId, field, value) => call("business_connector_secret_set", { businessId, connectorId, field, value }),
  disconnect: (businessId, connectorId) => call("business_connector_disconnect", { businessId, connectorId }),
  sync: (businessId, connectorId) => call("business_sync_start", { businessId, connectorId }),
  check: (businessId, connectorId) => call("business_connector_check", { businessId, connectorId }),
  pickFile: (businessId, connectorId, field) => call("business_connector_pick_file", { businessId, connectorId, field }),
  setupSql: () => call("business_supabase_setup_sql"),
  report: (businessId, report) => call("business_report", { businessId, report }),
  interpret: (businessId, report) => call("business_report_interpret", { businessId, report }),
  siteSnapshot: (businessId) => call("business_site_snapshot", { businessId }),
  siteLocate: (businessId, query) => call("business_site_locate", { businessId, query }),
  siteDraft: (businessId, input) => call("business_site_draft", { businessId, input }),
  siteChanges: (businessId) => call("business_site_changes", { businessId }),
  siteEvents: (businessId, changeId) => call("business_site_events", { businessId, changeId }),
  siteApprove: (businessId, changeId) => call("business_site_approve", { businessId, changeId }),
  siteReject: (businessId, changeId) => call("business_site_reject", { businessId, changeId }),
  siteConfirmBackup: (businessId, changeId) => call("business_site_confirm_backup", { businessId, changeId }),
  sitePackage: (businessId, changeId) => call("business_site_package", { businessId, changeId }),
  siteVerify: (businessId, changeId, rollback) => call("business_site_verify", { businessId, changeId, rollback }),
  permissions: (businessId) => call("business_permissions", { businessId }),
  setPermission: (businessId, actor, capability, allowed) => call("business_permission_set", { businessId, actor, capability, allowed }),
  proposals: (businessId) => call("business_proposals", { businessId, status: null }),
  approveProposal: (businessId, proposalId) => call("business_proposal_approve", { businessId, proposalId }),
  rejectProposal: (businessId, proposalId) => call("business_proposal_reject", { businessId, proposalId }),
  activity: (businessId) => call("business_activity", { businessId, limit: 100 }),
  recommendations: (businessId) => call("business_recommendations", { businessId }),
};

export const errText = (e: unknown) => (e instanceof Error ? e.message : typeof e === "string" ? e : "Something went wrong.");
export const whenText = (iso: string | null | undefined) => {
  if (!iso) return "Not recorded";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString("en-US", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit", timeZone: "Asia/Tokyo" });
};
/** "From src/data/products.ts" for synced items; who added it otherwise. */
export const sourceText = (source: string, ref: string | null) =>
  source === "sync" && ref ? `From the website source: ${ref.replace(/^local_repo:/, "").replace(/#.*$/, "")}` : source === "amara" ? "Proposed by Amara" : source === "import" ? "Imported" : "Added by you";

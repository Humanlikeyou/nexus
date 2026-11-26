import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SiteChange, WorkspaceCalls } from "./workspaceApi";
import { SiteOpsTab } from "./SiteOpsView";

const change = (over: Partial<SiteChange>): SiteChange => ({
  changeId: "c1", contentType: "seo_description", target: "/index.html", title: "SEO description", status: "draft",
  before: "<meta name=\"description\" content=\"Old\">", after: "<title>Taiyo Skin</title><meta name=\"description\" content=\"New calm text\">",
  diff: [{ op: "-", text: "<meta name=\"description\" content=\"Old\">" }, { op: "+", text: "<meta name=\"description\" content=\"New calm text\">" }],
  checks: [{ level: "ok", text: "48 characters." }], flags: [], sourceRefs: [], baseSnapshot: "s1", hpanelBackupConfirmed: false, packageDir: null, verifyDetail: null,
  createdAt: "2026-11-26T01:00:00Z", updatedAt: "2026-11-26T01:00:00Z", ...over,
});
function calls(list: SiteChange[]) {
  return {
    siteChanges: vi.fn(async () => list),
    context: vi.fn(async () => [{ itemId: "i1", section: "positioning", title: "Positioning", body: "x", status: "approved", source: "user", sourceRef: null, version: 2, approvedAt: "t", createdAt: "", updatedAt: "" }]),
    documents: vi.fn(async () => []),
    siteSnapshot: vi.fn(async () => ({ snapshotId: "s1", createdAt: "2026-11-26T01:00:00Z", files: [{ path: "/index.html", sha256: "x", bytes: 10 }], limits: "Take an hPanel backup as well." })),
    siteLocate: vi.fn(async () => []),
    siteDraft: vi.fn(async () => change({})),
    siteEvents: vi.fn(async () => [{ seq: 1, at: "2026-11-26T01:00:00Z", fromStatus: null, toStatus: "draft", actor: "taisun", detail: "Drafted from the live snapshot" }]),
    siteApprove: vi.fn(async () => change({ status: "approved" })),
    siteReject: vi.fn(), siteConfirmBackup: vi.fn(async () => change({})), sitePackage: vi.fn(async () => change({})), siteVerify: vi.fn(async () => change({})),
  } as unknown as WorkspaceCalls & Record<string, ReturnType<typeof vi.fn>>;
}

describe("decision 0065: website changes", () => {
  it("drafts from the live snapshot with approved sources, and shows the diff, checks and preview", async () => {
    const c = calls([change({})]);
    render(<SiteOpsTab id="taiyo-skin" calls={c} />);
    fireEvent.click(screen.getByRole("button", { name: "Take a snapshot" }));
    expect(await screen.findByText(/Snapshot of 1 files/)).toHaveTextContent("hPanel");
    fireEvent.click(await screen.findByRole("switch", { name: "Positioning" }));
    fireEvent.change(screen.getByLabelText("New text"), { target: { value: "New calm text" } });
    fireEvent.click(screen.getByRole("button", { name: "Draft and check" }));
    await waitFor(() => expect(c.siteDraft).toHaveBeenCalledWith("taiyo-skin", { contentType: "seo_description", value: "New calm text", path: null, documentId: null, sourceRefs: ["i1"] }));
    expect(screen.getByLabelText("Changes in SEO description")).toHaveTextContent("+ <meta name=\"description\" content=\"New calm text\">");
    expect(within(screen.getByLabelText("Search result preview")).getByText("New calm text")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Approve…" }));
    await waitFor(() => expect(c.siteApprove).toHaveBeenCalledWith("taiyo-skin", "c1"));
  });

  it("bundled content can't be drafted, and a change with errors can't be approved", async () => {
    const c = calls([change({ checks: [{ level: "error", text: "This touches a price but cites no approved product item." }], flags: ["pricing"] })]);
    render(<SiteOpsTab id="taiyo-skin" calls={c} />);
    expect(await screen.findByText("Pricing: needs explicit approval")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve…" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("What to change"), { target: { value: "product_description" } });
    expect(screen.getByText(/compiled into the site's JavaScript bundle/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Draft and check" })).toBeDisabled();
  });

  it("packaging waits for the hPanel backup, and a packaged change is verified against the live site", async () => {
    const c = calls([change({ changeId: "a", status: "approved" }), change({ changeId: "p", title: "robots.txt", status: "packaged", packageDir: "/data/site/packages/p" })]);
    render(<SiteOpsTab id="taiyo-skin" calls={c} />);
    expect(await screen.findByRole("button", { name: "Back up and package" })).toBeDisabled();
    fireEvent.click(screen.getAllByRole("switch", { name: "I took an hPanel backup" })[0]);
    await waitFor(() => expect(c.siteConfirmBackup).toHaveBeenCalledWith("taiyo-skin", "a"));
    expect(screen.getByText(/upload.tar.gz, rollback.tar.gz/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Verify the live site" }));
    await waitFor(() => expect(c.siteVerify).toHaveBeenCalledWith("taiyo-skin", "p", false));
    fireEvent.click(screen.getAllByRole("button", { name: "History" })[0]);
    expect(await screen.findByText(/Drafted from the live snapshot/)).toBeInTheDocument();
  });
});

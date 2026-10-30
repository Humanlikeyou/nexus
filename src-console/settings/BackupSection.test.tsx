// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("@/lib/api", () => ({ packItUp: vi.fn(), unpackBackup: vi.fn(), pickBackupArchive: vi.fn() }));
import { BACKUP_COPY, BackupSection, backupError, RESTORE_SAFETY_NOTE, RESTORE_WARNING, SkippedFilesList, middleTruncate, restorePathError, skippedFiles } from "./BackupSection";

const base = { path: "/tmp/nexus.tar.gz", size_mb: 1.5, includes: ["config"] };

describe("backup skipped files", () => {
  it("renders the skipped list when non-empty", () => {
    render(<SkippedFilesList info={{ ...base, skipped: ["kv/huge.bin", "link"] }} />);
    expect(screen.getByText("2 files skipped")).toBeTruthy();
    expect(screen.getByText("kv/huge.bin")).toBeTruthy();
  });

  it("renders nothing for an empty list or an older backend without the field", () => {
    const empty = render(<SkippedFilesList info={{ ...base, skipped: [] }} />);
    expect(empty.container.innerHTML).toBe("");
    const legacy = render(<SkippedFilesList info={base as unknown as Parameters<typeof skippedFiles>[0]} />);
    expect(legacy.container.innerHTML).toBe("");
    expect(skippedFiles({ ...base, skipped: "nope" } as unknown as Parameters<typeof skippedFiles>[0])).toEqual([]);
  });

  it("shows skipped files after creating a backup", async () => {
    const pack = vi.fn(async () => ({ ...base, skipped: ["a/big.db"] }));
    render(<BackupSection pack={pack} />);
    fireEvent.click(screen.getByRole("button", { name: "Create Backup" }));
    expect(await screen.findByText("1 file skipped")).toBeTruthy();
    expect(screen.getByText("a/big.db")).toBeTruthy();
    expect(screen.getByText("1.5 MB")).toBeTruthy();
    expect(screen.getByText(`1 item included. ${RESTORE_SAFETY_NOTE}`)).toBeTruthy();
  });

  it("shows a readable error, never raw config", async () => {
    const pack = vi.fn(async () => { throw new Error("disk full"); });
    render(<BackupSection pack={pack} />);
    fireEvent.click(screen.getByRole("button", { name: "Create Backup" }));
    expect((await screen.findByRole("alert")).textContent).toBe(BACKUP_COPY.backup.disk);
  });

  it("middle-truncates long archive paths", () => {
    expect(middleTruncate("/Users/someone/Library/Application Support/amara/backups/amara-2026-10-28.tar.gz", 30)).toMatch(/^\/Users.*….*tar\.gz$/);
  });
});

describe("restore from backup", () => {
  const setup = (over: Partial<Parameters<typeof BackupSection>[0]> = {}) => {
    const restore = vi.fn(async (_p: string) => ["Safety backup of current data saved to /data/restore-backups/x-full", "Amara OS data: 3 files restored"]);
    const pick = vi.fn(async () => "/Users/me/Desktop/nexus-backup-1.tar.gz");
    render(<BackupSection pack={vi.fn()} restore={restore} pick={pick} {...over} />);
    return { restore, pick };
  };

  it("validates the path before Restore is enabled", () => {
    setup();
    const restoreBtn = screen.getByRole("button", { name: "Restore from Backup…" });
    expect(restoreBtn).toBeDisabled();
    expect(restoreBtn).toHaveAttribute("aria-disabled", "true");
    fireEvent.change(screen.getByLabelText("Backup file"), { target: { value: "notes.txt" } });
    expect(screen.getByText("Use a full path, like ~/Desktop/nexus-backup.tar.gz")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Backup file"), { target: { value: "/tmp/notes.txt" } });
    expect(screen.getByText("Choose a .tar.gz backup made by Amara OS")).toBeTruthy();
    expect(restoreBtn).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Backup file"), { target: { value: "~/Desktop/b.tar.gz" } });
    expect(restoreBtn).not.toBeDisabled();
  });

  it("Choose… fills the path from the picker", async () => {
    const { pick } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
    await vi.waitFor(() => expect((screen.getByLabelText("Backup file") as HTMLInputElement).value).toBe("/Users/me/Desktop/nexus-backup-1.tar.gz"));
    expect(pick).toHaveBeenCalledOnce();
  });

  it("asks with a destructive dialog first and only restores on confirm", async () => {
    const { restore } = setup();
    fireEvent.change(screen.getByLabelText("Backup file"), { target: { value: "/tmp/b.tar.gz" } });
    fireEvent.click(screen.getByRole("button", { name: "Restore from Backup…" }));
    expect(screen.getByText(RESTORE_WARNING)).toBeTruthy();
    expect(restore).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(restore).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Restore from Backup…" }));
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(await screen.findByText("Amara OS data: 3 files restored")).toBeTruthy();
    expect(restore).toHaveBeenCalledWith("/tmp/b.tar.gz");
    expect(screen.getByText(/Safety backup of current data/)).toBeTruthy();
  });

  it("a Deny in the native approval gate is a quiet note, not an error", async () => {
    setup({ restore: vi.fn(async () => { throw new Error("declined: denied by user"); }) });
    fireEvent.change(screen.getByLabelText("Backup file"), { target: { value: "/tmp/b.tar.gz" } });
    fireEvent.click(screen.getByRole("button", { name: "Restore from Backup…" }));
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(await screen.findByText("Restore cancelled. Nothing was changed.")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows other failures as a readable alert", async () => {
    setup({ restore: vi.fn(async () => { throw new Error("Archive file not found"); }) });
    fireEvent.change(screen.getByLabelText("Backup file"), { target: { value: "/tmp/b.tar.gz" } });
    fireEvent.click(screen.getByRole("button", { name: "Restore from Backup…" }));
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect((await screen.findByRole("alert")).textContent).toBe(BACKUP_COPY.restore.missing);
  });

  it("restorePathError", () => {
    expect(restorePathError("")).toBeNull();
    expect(restorePathError("/a/b.TGZ")).toBeNull();
    expect(restorePathError("b.tar.gz")).not.toBeNull();
  });
});

describe("restore copy and layout (Design review nits)", () => {
  it("Restore sits inside Backup with the safety line; labels use U+2026; dialog copy", () => {
    render(<BackupSection pack={vi.fn()} restore={vi.fn()} pick={vi.fn()} />);
    const titles = [...document.querySelectorAll(".ui-inset-group__title")].map((h) => h.textContent);
    expect(titles).not.toContain("Restore");
    const backup = screen.getByText("Back Up Amara OS").closest(".ui-inset-group")!;
    expect(backup.textContent).toContain("Restore from Backup");
    expect(backup.textContent).toContain(RESTORE_SAFETY_NOTE);
    for (const name of ["Choose…", "Restore from Backup…"]) expect(screen.getByRole("button", { name })).toBeTruthy();
    expect(document.body.textContent).not.toContain("...");
    expect(RESTORE_WARNING).toBe("This replaces current Amara OS config and local data with the archive's contents. This can't be undone.");
  });
});

describe("backup errors: fixed copy only", () => {
  const RAW = "Error: tar failed at /Users/me/secret-dir/.env token=sk-test-123";

  it("never renders raw backend text (backup and restore)", async () => {
    const pack = vi.fn(async () => { throw new Error(RAW); });
    const { container, unmount } = render(<BackupSection pack={pack} />);
    fireEvent.click(screen.getByRole("button", { name: "Create Backup" }));
    expect((await screen.findByRole("alert")).textContent).toBe(BACKUP_COPY.backup.generic);
    expect(container.textContent).not.toContain("secret-dir");
    expect(container.textContent).not.toContain("sk-test-123");
    unmount();
  });

  it.each([
    ["declined: user said no", "declined"],
    ["busy: another approval is open", "busy"],
    ["expired: confirmation timed out", "expired"],
    ["refused: not allowed", "refused"],
    ["write failed: ENOSPC no space left on device", "disk"],
    ["open /x: Permission denied (EACCES)", "permission"],
    ["ENOENT: no such file or directory", "missing"],
    ["something odd happened", "generic"],
  ] as const)("maps %s to %s copy", (raw, key) => {
    expect(backupError(new Error(raw))).toBe(BACKUP_COPY.backup[key]);
    expect(backupError(raw, "restore")).toBe(BACKUP_COPY.restore[key]);
  });

  it("generic fallback for empty or non-Error values", () => {
    expect(backupError(undefined)).toBe("Backup didn't finish. Try again, or check that the backup folder is available.");
    expect(backupError({ weird: true }, "restore")).toBe(BACKUP_COPY.restore.generic);
  });
});

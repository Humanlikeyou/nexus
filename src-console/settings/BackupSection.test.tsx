// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("@/lib/api", () => ({ packItUp: vi.fn() }));
import { BackupSection, SkippedFilesList, skippedFiles } from "./BackupSection";

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
    fireEvent.click(screen.getByRole("button", { name: "Create backup" }));
    expect(await screen.findByText("1 file skipped")).toBeTruthy();
    expect(screen.getByText("a/big.db")).toBeTruthy();
  });
});

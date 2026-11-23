import { describe, expect, it } from "vitest";
import { mapLast, type VerifyRow } from "../../src-console/standalone/liveAdapter";

const rows = [
  { id: "amara-backup-20261112-030000.tar.gz", label: "Nov 12, 3:00 AM", madeAt: "2026-11-12T03:00:00+09:00", sizeBytes: 8_400_000_000, daysOld: 0 },
  { id: "amara-backup-20261101-030000.tar.gz", label: "Nov 1, 3:00 AM", madeAt: "2026-11-01T03:00:00+09:00", sizeBytes: 1000, daysOld: 11 },
];
const ver = (o: Partial<VerifyRow> = {}): VerifyRow => ({ file: rows[0].id, at: rows[0].madeAt, size: rows[0].sizeBytes, ok: true, checked: 12, mismatched: [], mode: "structure", ...o });

describe("backup last mapping", () => {
  it("is null with no backups (honest none)", () => expect(mapLast([], null)).toBeNull());
  it("maps the newest row's time and size; checksum unknown without a verify", () => {
    const l = mapLast(rows, null)!;
    expect(l.size).toBe("8.4 GB");
    expect(l.at).toMatch(/Nov 12, 2026/);
    expect(l.checksumOk).toBeNull();
  });
  it("takes the checksum from backup_verify_last only for the same file", () => {
    expect(mapLast(rows, ver())).toMatchObject({ checksumOk: true, checks: "structure" });
    expect(mapLast(rows, ver({ mode: "sha256" }))).toMatchObject({ checksumOk: true, checks: "sha256" });
    expect(mapLast(rows, ver({ ok: false, mismatched: ["x"] }))!.checksumOk).toBe(false);
    expect(mapLast(rows, ver({ file: rows[1].id }))!.checksumOk).toBeNull();
  });
  it("picks the newest by madeAt whatever the list order, and ignores unparseable dates", () => {
    expect(mapLast([rows[1], rows[0]], null)!.size).toBe("8.4 GB");
    expect(mapLast([{ ...rows[0], madeAt: "garbage" }, rows[1]], null)!.size).toBe("1.0 KB");
    expect(mapLast([rows[1], rows[0]], ver())).toMatchObject({ checksumOk: true });
  });
});

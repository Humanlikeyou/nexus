import { useState } from "react";
import { packItUp } from "@/lib/api";
import type { PackageInfo } from "@/lib/types";

/** Older backends may omit `skipped`, or send something unexpected. */
export function skippedFiles(info: Partial<PackageInfo> | null | undefined): string[] {
  const s = info?.skipped;
  return Array.isArray(s) ? s.filter((x): x is string => typeof x === "string") : [];
}

/** Quiet, collapsible list of files left out of a backup. Renders nothing when empty. */
export function SkippedFilesList({ info }: { info: Partial<PackageInfo> | null | undefined }) {
  const skipped = skippedFiles(info);
  if (skipped.length === 0) return null;
  return (
    <details className="backup-skipped">
      <summary>{skipped.length} file{skipped.length === 1 ? "" : "s"} skipped</summary>
      <ul>{skipped.map((path) => <li key={path}><code>{path}</code></li>)}</ul>
    </details>
  );
}

export function BackupSection({ pack = packItUp }: { pack?: () => Promise<PackageInfo> }) {
  const [busy, setBusy] = useState(false);
  const [info, setInfo] = useState<PackageInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setInfo(await pack());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="setting-row backup-section">
      <div>
        <strong>Backup</strong>
        <p>Package config and local data into an archive.</p>
        {info && <p className="backup-result">Saved {info.size_mb.toFixed(1)} MB to <code>{info.path}</code></p>}
        {error && <p className="backup-error" role="status">Backup unavailable: {error}</p>}
        <SkippedFilesList info={info} />
      </div>
      <button type="button" className="action-button" onClick={() => void run()} disabled={busy}>
        {busy ? "Packing…" : "Create backup"}
      </button>
    </div>
  );
}

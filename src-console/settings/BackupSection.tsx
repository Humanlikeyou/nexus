import { useState } from "react";
import { Archive } from "lucide-react";
import { packItUp } from "@/lib/api";
import type { PackageInfo } from "@/lib/types";
import { readableError } from "../lib/format";
import { Button, InsetGroup, ListRow } from "../ui";

/** Older backends may omit `skipped`, or send something unexpected. */
export function skippedFiles(info: Partial<PackageInfo> | null | undefined): string[] {
  const s = info?.skipped;
  return Array.isArray(s) ? s.filter((x): x is string => typeof x === "string") : [];
}

/** Quiet, collapsible list of files left out of a backup (paths only). Renders nothing when empty. */
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

/** "/Users/me/very/long/path/archive.tar.gz" → "/Users/me/ve…/archive.tar.gz" */
export function middleTruncate(text: string, max = 44): string {
  if (text.length <= max) return text;
  const tail = Math.ceil((max - 1) * 0.6);
  return `${text.slice(0, max - 1 - tail)}…${text.slice(-tail)}`;
}

/** Settings › Backup (screen-specs §11). Shows only paths, sizes and counts, never file contents
 *  or config values (the archive may hold keys). No restore yet, so nothing destructive here. */
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
      setError(readableError(e));
    } finally {
      setBusy(false);
    }
  };

  const includes = Array.isArray(info?.includes) ? info!.includes.length : 0;
  return (
    <div className="backup-section">
      <InsetGroup title="Backup" footer={info ? `${includes} item${includes === 1 ? "" : "s"} included` : undefined}>
        <ListRow icon={Archive} title="Back Up Amara OS" subtitle="Packages config and local data into an archive."
          accessory={<Button variant="secondary" size="small" loading={busy} onClick={() => void run()}>{busy ? "Packing…" : "Create Backup"}</Button>} />
        {info && <ListRow title="Last backup" subtitle={<span title={info.path}>{middleTruncate(info.path)}</span>} value={`${info.size_mb.toFixed(1)} MB`} />}
      </InsetGroup>
      <SkippedFilesList info={info} />
      {error && <p className="settings__alert" role="alert">Backup didn&apos;t finish: {error}</p>}
    </div>
  );
}

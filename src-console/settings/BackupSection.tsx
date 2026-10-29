import { useState } from "react";
import { Archive, RotateCcw } from "lucide-react";
import { packItUp, pickBackupArchive, unpackBackup } from "@/lib/api";
import { approvalOutcome } from "@/terrance/system/approvalErrors";
import type { PackageInfo } from "@/lib/types";
import { Button, Dialog, InsetGroup, ListRow, TextField } from "../ui";

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

/** Backend message without an "Error:" prefix. (lib/format's readableError is Muse/Bluetooth-specific:
 *  it turns any "not found" into a Muse pairing hint.) */
export function backupError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error ?? "");
  return message.replace(/^Error:\s*/i, "").trim() || "Something went wrong.";
}

export const RESTORE_WARNING = "This replaces your local Amara OS data with the backup. This can't be undone.";

/** Only archives pack_it_up can produce; the backend re-checks everything (allow-list + tar-slip guard). */
export function restorePathError(path: string): string | null {
  const p = path.trim();
  if (!p) return null;
  if (!p.startsWith("/") && !p.startsWith("~")) return "Use a full path, like ~/Desktop/nexus-backup.tar.gz";
  if (!/\.(tar\.gz|tgz)$/i.test(p)) return "Choose a .tar.gz backup made by Amara OS";
  return null;
}

/** Restore message for an approval-gate outcome, or null for a plain error. */
function restoreOutcomeText(error: unknown): string | null {
  switch (approvalOutcome(error)) {
    case "declined": return "Restore cancelled. Nothing was changed.";
    case "busy": return "A restore is already running.";
    case "expired": return "The confirmation timed out. Nothing was restored; try again.";
    case "changed": return "Your data changed while the confirmation was open. Nothing was restored; try again.";
    default: return null;
  }
}

export type BackupSectionProps = {
  pack?: () => Promise<PackageInfo>;
  restore?: (archivePath: string) => Promise<string[]>;
  pick?: () => Promise<string | null>;
};

/** Settings › Backup (screen-specs §11). Shows only paths, sizes and counts, never file contents
 *  or config values (the archive may hold keys). Restore is behind a destructive Dialog, then the
 *  native approval gate; the backend saves a full safety copy of the current data first. */
export function BackupSection({ pack = packItUp, restore = unpackBackup, pick = pickBackupArchive }: BackupSectionProps) {
  const [busy, setBusy] = useState(false);
  const [info, setInfo] = useState<PackageInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setInfo(await pack());
    } catch (e) {
      setError(backupError(e));
    } finally {
      setBusy(false);
    }
  };

  const [archive, setArchive] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [restored, setRestored] = useState<string[] | null>(null);
  const [restoreNote, setRestoreNote] = useState<string | null>(null);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const pathError = restorePathError(archive);
  const canRestore = archive.trim() !== "" && !pathError && !restoring;

  const choose = async () => {
    setRestoreError(null);
    try {
      const picked = await pick();
      if (picked) setArchive(picked);
    } catch {
      // No picker (e.g. a browser preview): the path field still works.
      setRestoreNote("The file picker isn't available here. Type the backup's path instead.");
    }
  };

  const runRestore = async () => {
    setRestoring(true);
    setRestored(null);
    setRestoreNote(null);
    setRestoreError(null);
    try {
      const lines = await restore(archive.trim());
      setRestored(Array.isArray(lines) ? lines.filter((l): l is string => typeof l === "string") : []);
    } catch (e) {
      const outcome = restoreOutcomeText(e);
      if (outcome) setRestoreNote(outcome);
      else setRestoreError(backupError(e));
    } finally {
      setRestoring(false);
      setConfirming(false);
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

      <InsetGroup title="Restore" footer="Your current data is saved to a safety backup in the Amara OS data folder before anything is replaced.">
        <ListRow icon={RotateCcw} title="Restore from Backup" subtitle="Replaces local data with a backup made by Create Backup."
          accessory={<Button variant="secondary" size="small" onClick={() => void choose()} disabled={restoring}>Choose…</Button>} />
        <li className="backup-restore__path">
          <TextField label="Backup file" value={archive} placeholder="~/Desktop/nexus-backup-20261029_101500.tar.gz"
            onChange={(e) => { setArchive(e.target.value); setRestored(null); setRestoreNote(null); setRestoreError(null); }}
            error={pathError ?? undefined} spellCheck={false} autoComplete="off" />
          <Button variant="destructive" size="small" disabled={!canRestore} loading={restoring} onClick={() => setConfirming(true)}>Restore…</Button>
        </li>
      </InsetGroup>
      {restored && (
        <div className="backup-restore__result" role="status">
          <p>Restore finished. Restart Amara OS to load the restored data.</p>
          <ul>{restored.map((line, i) => <li key={`${i}-${line}`}>{line}</li>)}</ul>
        </div>
      )}
      {restoreNote && <p className="backup-restore__note" role="status">{restoreNote}</p>}
      {restoreError && <p className="settings__alert" role="alert">Restore didn&apos;t finish: {restoreError}</p>}
      <Dialog open={confirming} title="Restore from backup?" destructive confirmLabel="Restore" busy={restoring}
        onConfirm={() => void runRestore()} onCancel={() => setConfirming(false)}>
        <p>{RESTORE_WARNING}</p>
        <p className="backup-restore__file"><code title={archive.trim()}>{middleTruncate(archive.trim(), 56)}</code></p>
      </Dialog>
    </div>
  );
}

import { invokeBound } from "../bridge";
import { useEffect, useState, type FormEvent } from "react";
import { NOOP_PATH_CHANGE_CANCELLED } from "../../src/lib/types";
import type { NoopSettings, WhoopSource } from "../types";
import { Button, InsetGroup, ListRow, SegmentedControl, TextField } from "../ui";

export const getNoopSettings = () => invokeBound<NoopSettings>("get_noop_settings");
/** Rust shows its own native Confirm/Cancel dialog for path changes; cancelling rejects. */
export const setNoopSettings = (settings: NoopSettings) => invokeBound<void>("set_noop_settings", { settings });

const DEFAULTS: NoopSettings = { whoop_source: "noop", binary_path: null, db_path: null };
const SOURCES = [{ value: "noop", label: "Health sync (recommended)" }, { value: "ble", label: "Direct Bluetooth" }] as const;

/** Empty is allowed (use NOOP's default); otherwise the path must be absolute. */
export function pathError(value: string): string | null {
  const v = value.trim();
  return v === "" || v.startsWith("/") ? null : "Use a full path that starts with /.";
}

/** True only for the exact rejection Rust returns when the user cancels its native confirm dialog. */
export function isUserCancel(error: unknown): boolean {
  return error === NOOP_PATH_CHANGE_CANCELLED;
}

export type SaveOutcome = { state: "saved" } | { state: "cancelled" } | { state: "error"; message: string };

/** Maps a save result to fixed copy; raw backend details (which may contain paths) are never shown. */
export function saveOutcome(error: unknown): SaveOutcome {
  if (isUserCancel(error)) return { state: "cancelled" };
  const text = typeof error === "string" ? error : "";
  if (text.startsWith("invalid_path:")) return { state: "error", message: "That path isn't usable. Use a full path to an existing file, starting with /." };
  return { state: "error", message: "Couldn't save. Check the paths and try again." };
}

type Save = { state: "idle" | "saving" | "saved" | "cancelled" } | { state: "error"; message: string };

/** Settings › WHOOP: source, NOOP binary and database paths (get/set_noop_settings). */
export function NoopSettingsGroup() {
  const [loaded, setLoaded] = useState(false);
  const [source, setSource] = useState<WhoopSource>(DEFAULTS.whoop_source);
  const [binary, setBinary] = useState("");
  const [db, setDb] = useState("");
  const [save, setSave] = useState<Save>({ state: "idle" });

  useEffect(() => {
    let live = true;
    getNoopSettings().then((s) => {
      if (!live) return;
      setSource(s.whoop_source === "ble" ? "ble" : "noop");
      setBinary(s.binary_path ?? "");
      setDb(s.db_path ?? "");
      setLoaded(true);
    }, () => { if (live) setLoaded(true); });
    return () => { live = false; };
  }, []);

  const binaryError = pathError(binary);
  const dbError = pathError(db);
  const onSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (binaryError || dbError) { setSave({ state: "error", message: "Fix the paths above, then save." }); return; }
    setSave({ state: "saving" });
    try {
      await setNoopSettings({ whoop_source: source, binary_path: binary.trim() || null, db_path: db.trim() || null });
      setSave({ state: "saved" });
    } catch (error) {
      setSave(saveOutcome(error));
    }
  };
  const edit = () => setSave({ state: "idle" });

  return (
    <form id="noop-settings" aria-label="WHOOP connection" onSubmit={(e) => { void onSubmit(e); }}>
      <InsetGroup title="WHOOP connection" footer="With Health sync selected, Amara reads your WHOOP data from its health store and doesn't connect over Bluetooth itself. Leave the paths empty to use the defaults.">
        <ListRow title="WHOOP source" accessory={
          <SegmentedControl label="WHOOP source" size="small" value={source} options={SOURCES} onChange={(v) => { setSource(v); edit(); }} />
        } />
      </InsetGroup>
      <div className="settings__fields">
        <TextField label="Health reader path" value={binary} placeholder="/usr/local/bin/noop" error={binaryError ?? undefined} spellCheck={false} autoComplete="off"
          onChange={(e) => { setBinary(e.target.value); edit(); }} />
        <TextField label="Health store path" value={db} placeholder="Leave empty to use the default" error={dbError ?? undefined} spellCheck={false} autoComplete="off"
          onChange={(e) => { setDb(e.target.value); edit(); }} />
        <div className="settings__row-actions">
          <Button type="submit" variant="primary" size="small" loading={save.state === "saving"} disabled={!loaded}>Save</Button>
          <span role="status" aria-live="polite" className="settings__save-state">
            {save.state === "saved" ? "Saved" : save.state === "saving" ? "Saving…" : save.state === "cancelled" ? "Not saved" : ""}
          </span>
        </div>
        {save.state === "error" && <p role="alert" className="settings__save-error">{save.message}</p>}
      </div>
    </form>
  );
}

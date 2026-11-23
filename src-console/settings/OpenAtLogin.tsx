import { useEffect, useState } from "react";
import { disableLoginItem, enableLoginItem, getLoginItemStatus } from "../bridge";
import type { LoginItemStatus } from "../bindings.gen";
import { Button, InsetGroup, ListRow, Toggle } from "../ui";

export const OPEN_AT_LOGIN_COPY = {
  title: "Open at login",
  subtitle: "Starts Amara OS when you sign in to this Mac.",
  footer: "Run setup again after moving to a bigger Mac to pick a larger model.",
  needsApproval: "Allow Amara OS in System Settings > General > Login Items to finish.",
  notInApplications: "Move Amara OS to your Applications folder first, then try again.",
  unsupported: "Open at login needs macOS 13 or newer.",
  error: "Couldn't change this setting. Try again.",
};

/** Maps a backend rejection to short plain copy. Fixed codes only; raw text is never shown. A declined confirm is the user's choice, so it says nothing. */
export function loginItemMessage(err: unknown): string | null {
  const raw = typeof err === "string" ? err : err instanceof Error ? err.message : "";
  if (raw.startsWith("refused: not_in_applications")) return OPEN_AT_LOGIN_COPY.notInApplications;
  if (raw.startsWith("declined")) return null;
  if (raw.startsWith("Open at login needs macOS")) return OPEN_AT_LOGIN_COPY.unsupported;
  return OPEN_AT_LOGIN_COPY.error;
}

/** Settings › General › Startup. Shows the system's real status; nothing is registered until the user turns it on (the backend asks first). */
export function StartupGroup({ onRunSetup }: { onRunSetup?: () => void }) {
  const [status, setStatus] = useState<LoginItemStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    getLoginItemStatus().then((s) => { if (active) setStatus(s); }).catch(() => {});
    return () => { active = false; };
  }, []);
  const unsupported = status?.backend === "unsupported";
  const checked = status != null && (status.enabled || status.needs_approval);
  const change = async (next: boolean) => {
    setMessage(null);
    setBusy(true);
    try {
      const s = await (next ? enableLoginItem() : disableLoginItem());
      setStatus(s);
      if (s.error) setMessage(loginItemMessage(s.error));
    } catch (e) {
      setMessage(loginItemMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const note = message ?? (unsupported ? OPEN_AT_LOGIN_COPY.unsupported : status?.needs_approval ? OPEN_AT_LOGIN_COPY.needsApproval : null);
  return (
    <InsetGroup title="Startup" footer={note ?? OPEN_AT_LOGIN_COPY.footer}>
      <ListRow title={OPEN_AT_LOGIN_COPY.title} subtitle={OPEN_AT_LOGIN_COPY.subtitle}
        accessory={<Toggle aria-label={OPEN_AT_LOGIN_COPY.title} checked={checked} disabled={status == null || unsupported || busy} onCheckedChange={(v) => { void change(v); }} />} />
      <ListRow title="Set up Amara again" subtitle="Restore, model size, downloads and preferences, step by step." accessory={<Button variant="secondary" size="small" onClick={onRunSetup} disabled={!onRunSetup}>Run Setup Again…</Button>} />
    </InsetGroup>
  );
}

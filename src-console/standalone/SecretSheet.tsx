import { useId, useRef, useState } from "react";
import { Button, Dialog } from "../ui";
import type { SecretName, StandaloneAdapter } from "./adapters";
import { KeyState } from "./parts";

/**
 * Write-only secure field (design-1111s s5b). The value is read from the input once on Save, handed to
 * the adapter (Keychain), and the input is cleared. It is never kept in state, rendered, logged or returned.
 */
export function SecretSheet({ open, name, title, intro, set, adapter, onClose }: { open: boolean; name: SecretName; title: string; intro: string; set: boolean; adapter: StandaloneAdapter; onClose: (saved: boolean, wired: boolean) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const id = useId();
  const [busy, setBusy] = useState(false);
  const [empty, setEmpty] = useState(false);
  const save = async () => {
    const el = input.current;
    const value = el?.value ?? "";
    if (!value.trim()) { setEmpty(true); return; }
    if (el) el.value = "";
    setBusy(true);
    const r = await adapter.setSecret(name, value);
    setBusy(false);
    onClose(r.state !== "unwired", r.state !== "unwired");
  };
  const cancel = () => { if (input.current) input.current.value = ""; setEmpty(false); onClose(false, true); };
  return (
    <Dialog open={open} title={title} onCancel={cancel} className="sa-sheet">
      <p>{intro}</p>
      <div className="pk-field">
        <label htmlFor={id}>{title}</label>
        <span className="pk-secure"><span aria-hidden="true">🔒</span>
          <input ref={input} id={id} className="sa-secure__input" type="password" autoComplete="off" spellCheck={false} placeholder="Paste here" aria-describedby={`${id}-note`} onChange={() => setEmpty(false)} />
          <span className="pk-dim">Hidden as you type</span></span>
        {empty && <span role="alert" className="sa-error">Paste a value first.</span>}
      </div>
      <div className="pk-keyrow"><div>Status<small>Amara shows only whether it&apos;s set</small></div><KeyState set={set} /><span /></div>
      <div className="pk-info" id={`${id}-note`}>Never shown again, never in backups, logs, chat or crash reports. To change it, paste a new one.</div>
      <div className="pk-sheet__actions">
        <Button size="small" variant="secondary" onClick={cancel} disabled={busy}>Cancel</Button>
        <Button size="small" variant="primary" onClick={() => { void save(); }} loading={busy}>Save to Keychain</Button>
      </div>
    </Dialog>
  );
}

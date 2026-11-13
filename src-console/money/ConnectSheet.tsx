import { useState } from "react";
import { Button, DataTag, Dialog } from "../ui";
import type { MoneyAdapter, Institution } from "./adapters";
import { guardWatchInput, type GuardResult } from "./seedGuard";

/** m2: connect an account, read-only. Ledger is watch-only: public addresses or an xpub, never a recovery phrase or key. */
export function ConnectSheet({ open, onClose, adapter, initial = "ledger" }: { open: boolean; onClose: () => void; adapter: MoneyAdapter; initial?: Institution }) {
  const routes = adapter.routes();
  const [pick, setPick] = useState<Institution>(initial);
  // Only ever holds text the guard did NOT flag as secret; a phrase or key is dropped before it reaches state.
  const [value, setValue] = useState("");
  const [nick, setNick] = useState("Ledger · Bitcoin");
  const [refused, setRefused] = useState<null | "phrase" | "private_key">(null);
  const [note, setNote] = useState<string | null>(null);
  const check: GuardResult = guardWatchInput(value);
  const route = routes.find((r) => r.id === pick) ?? routes[0];
  const onInput = (next: string) => {
    const g = guardWatchInput(next);
    if (g.kind === "secret") { setValue(""); setRefused(g.why); setNote(null); return; }
    setRefused(null); setValue(next);
  };
  const add = async () => {
    if (pick === "ledger") {
      if (check.kind !== "public") return;
      // Typed balance: the adapter accepts one (money_balance_set_manual), but the field is Design's to place (design-questions 2026-11-13).
      const r = await adapter.addWatchOnly(check.chain, value.trim(), nick.trim() || "Ledger", null);
      const extra = r.state === "live" ? adapter.connectNote() : null;
      setNote(r.state === "unwired" ? "Watch-only needs wiring, so nothing was saved." : r.state === "sample" ? "Sample data: added in the preview only. Nothing was saved." : `Added. Amara can see balances at these addresses and nothing else.${extra ? ` ${extra}` : ""}`);
      if (r.state !== "unwired") setValue("");
    } else if (pick === "jp_bank") {
      const r = await adapter.importStatement();
      setNote(r.state === "sample" ? "Sample data: no file is read in the preview." : adapter.connectNote() ?? (r.state === "unwired" ? "Statement import needs wiring, so no file was read." : "Statement imported."));
    }
  };
  const ready = route.ready.state !== "unwired";
  return (
    <Dialog open={open} title="Connect an account (read-only)" onCancel={onClose} className="m-sheet">
      <div className="m-routes" role="radiogroup" aria-label="Account type">
        {routes.map((r) => (
          <button key={r.id} type="button" role="radio" aria-checked={pick === r.id} className="m-route" data-on={pick === r.id || undefined} onClick={() => { setPick(r.id); setNote(null); }}>
            <span className="m-mono" aria-hidden="true">{r.mono}</span>
            <span className="m-route__t"><b>{r.name}</b><small>{r.sub}</small>{r.ready.state === "unwired" && <DataTag kind="wire" />}</span>
          </button>
        ))}
      </div>
      {pick === "ledger" && (
        <>
          <label className="m-field"><span>Bitcoin xpub or address, or an Ethereum address</span>
            <span className="m-input"><input value={value} onChange={(e) => onInput(e.target.value)} autoComplete="off" autoCorrect="off" autoCapitalize="off" spellCheck={false} placeholder="xpub…, bc1… or 0x…" aria-describedby="m-guard" />
              <span id="m-guard" className="m-guard" data-k={check.kind} role="status">{check.kind === "public" ? `✓ ${check.label}` : check.kind === "invalid" ? "Not a public address or xpub" : ""}</span></span>
          </label>
          <label className="m-field"><span>Nickname</span><span className="m-input"><input value={nick} maxLength={60} onChange={(e) => setNick(e.target.value)} /></span></label>
          <div className="m-warn" role={refused ? "alert" : "note"}>
            {refused ? <><b>That looked like a {refused === "phrase" ? "recovery phrase" : "private key"}. Amara cleared it and didn&apos;t save it anywhere.</b>Treat it as exposed: if it was your real one, move your coins to a new Ledger wallet. Only public addresses and xpubs are accepted.</>
              : <><b>Seed phrases are refused.</b>If you paste 12 or 24 words here, Amara clears the field at once, doesn&apos;t save it anywhere, and tells you to treat that phrase as exposed. Only public addresses and xpubs are accepted.</>}
          </div>
        </>
      )}
      {pick === "jp_bank" && <p className="pk-note">Drop a monthly statement CSV from your bank (Shift-JIS is fine). Amara reads balances and transactions in yen; account numbers are never stored. A read-only bank API can be added later where your bank offers one.</p>}
      {(pick === "chime" || pick === "fidelity") && <p className="pk-note">{route.name} connects through a read-only data service (which one is still being chosen) <DataTag kind="wire" />. You sign in on their page; Amara only gets a read-only token, kept in the Keychain. Nothing to connect yet.</p>}
      <p className="pk-note">Chime and Fidelity connect through a read-only data service. Japanese banks: drop a statement CSV each month, or a read-only API where your bank offers one. No connection can move money.</p>
      {note && <p className="pk-note" role="status">{note}</p>}
      <div className="pk-sheet__actions">
        <Button variant="secondary" onClick={onClose}>Cancel</Button>
        {pick === "ledger" && <Button variant="primary" disabled={!ready || check.kind !== "public"} onClick={() => { void add(); }}>Add watch-only</Button>}
        {pick === "jp_bank" && <Button variant="primary" disabled={!ready} onClick={() => { void add(); }}>Import a statement…</Button>}
      </div>
    </Dialog>
  );
}

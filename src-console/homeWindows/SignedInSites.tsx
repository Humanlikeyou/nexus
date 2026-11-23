/** Settings › Services › Sites you're signed in to inside Amara. Status only — never cookies or tokens. */
import { useEffect, useState } from "react";
import { Button, DataTag } from "../ui";
import { sampleAdaptersEnabled, valueOf } from "../lib/sampleAdapters";
import { SECRET_PATTERN, homeWindowsAdapter, type SignedSite } from "./adapters";
import "./homeWindows.css";

export function siteStatus(s: SignedSite): string {
  return s.signed === "in" ? "Signed in" : s.signed === "out" ? "Signed out" : "Not signed in";
}

export function SignedInSites() {
  const wired = homeWindowsAdapter().signedSites();
  const list = valueOf(wired);
  const sample = sampleAdaptersEnabled();
  const [note, setNote] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => { void homeWindowsAdapter().refreshSites?.().then(() => setTick((n) => n + 1)).catch(() => undefined); }, []);
  const signOut = async (s: SignedSite) => {
    try {
      const r = await homeWindowsAdapter().signOut(s.id);
      setNote(r.state === "unwired" ? `Sign out for ${s.host} needs wiring.` : `Signed out of ${s.host}. The profile for this site on this Mac was deleted.`);
    } catch {
      setNote(`Sign out of ${s.host} didn't happen. Nothing was changed.`);
    }
    setTick((n) => n + 1);
  };
  void tick;
  const rows = list ?? [];
  return (
    <section className="w-sites" aria-labelledby="w-sites-title" data-sites="amara">
      <h2 id="w-sites-title" className="w-sites__heading">Sites you're signed in to inside Amara</h2>
      <p className="w-sites__lead">
        Each site has its own private browser profile on this Mac. Amara never reads what you type on a sign-in page, and this list never shows cookies, tokens or passwords.
        {sample ? <> <DataTag kind="sample" /></> : wired.state === "unwired" ? <> <DataTag kind="wire" /></> : null}
      </p>
      {wired.state === "live" && rows.length === 0 && <p className="w-sites__lead">No sites yet. A site appears here after you open it in a Home window.</p>}
      {wired.state === "unwired" && !sample && (
        <p className="w-sites__lead">Needs wiring: a session store from Claude before signed-in sites appear here (HANDOFF-1111w). Nothing here is made up.</p>
      )}
      {rows.map((s) => (
        <div className="w-sites__row" key={s.id} data-site={s.id} data-signed={s.signed}>
          <div className="w-sites__name">
            <span>{s.label}</span>
            <small>{s.host}</small>
          </div>
          <span>{siteStatus(s)}</span>
          {s.signed === "in"
            ? <Button size="small" variant="secondary" aria-label={`Sign out of ${s.label}`} onClick={() => { void signOut(s); }}>Sign out</Button>
            : <span />}
        </div>
      ))}
      {note && <p className="w-sites__lead" role="status">{note}</p>}
    </section>
  );
}

/** For tests: the rendered sites section must never include secret-like values. */
export const sitesSecretPattern = SECRET_PATTERN;

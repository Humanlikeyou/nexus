import { useCallback, useEffect, useState } from "react";
import { INITIAL_SNAPSHOT, loadSystemSnapshot, migrateLegacyTerrenceContext } from "../bridge";
import type { SystemSnapshot } from "../types";
import { SNAPSHOT_EVENT } from "../lib/localStores";

/** System snapshot with a 15 s refresh and the one-time legacy context migration. */
export function useSnapshot() {
  const [data, setData] = useState<SystemSnapshot>(INITIAL_SNAPSHOT);
  const [busy, setBusy] = useState(true);
  const refresh = useCallback(async () => { setBusy(true); setData(await loadSystemSnapshot()); setBusy(false); }, []);
  // eslint-disable-next-line react-hooks/set-state-in-effect -- initial load + 15 s polling of an external source; refresh() sets busy before awaiting
  useEffect(() => { refresh(); const refreshTimer = window.setInterval(refresh, 15_000); return () => { window.clearInterval(refreshTimer); }; }, [refresh]);
  useEffect(() => { const on = () => { void refresh(); }; window.addEventListener(SNAPSHOT_EVENT, on); return () => { window.removeEventListener(SNAPSHOT_EVENT, on); }; }, [refresh]);
  useEffect(() => {
    let activeRequest = true;
    migrateLegacyTerrenceContext()
      .then(() => {
        if (activeRequest) return refresh();
      })
      .catch((error) => console.warn("[console] legacy context migration unavailable", error));
    return () => { activeRequest = false; };
  }, [refresh]);
  return { data, busy, setBusy, refresh };
}

import { useCallback, useEffect, useState } from "react";
import { INITIAL_SNAPSHOT, loadSystemSnapshot, migrateLegacyTerrenceContext } from "../bridge";
import type { SystemSnapshot } from "../types";

/** System snapshot with a 15 s refresh and the one-time legacy context migration. */
export function useSnapshot() {
  const [data, setData] = useState<SystemSnapshot>(INITIAL_SNAPSHOT);
  const [busy, setBusy] = useState(true);
  const refresh = useCallback(async () => { setBusy(true); setData(await loadSystemSnapshot()); setBusy(false); }, []);
  useEffect(() => { refresh(); const refreshTimer = window.setInterval(refresh, 15_000); return () => { window.clearInterval(refreshTimer); }; }, [refresh]);
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

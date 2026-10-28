import { RefreshCw } from "lucide-react";
import { IntelligenceOverlay } from "../design/SystemPrimitives";
import { Clock } from "./Clock";
import "./shell.css";

/** 52px unified toolbar under the overlay title bar. Both halves are window drag regions;
 *  their non-interactive children pass clicks through (shell.css) so dragging works anywhere but the buttons. */
export function Toolbar({ label, busy, onRefresh }: { label: string; busy: boolean; onRefresh: () => void }) {
  return (
    <IntelligenceOverlay>
      <header className="nexus-mark" data-tauri-drag-region><span className="wordmark-lockup" role="img" aria-label="Amara OS"><span className="wordmark" aria-hidden="true">Amara</span><span className="wordmark-os" aria-hidden="true">OS</span></span></header>
      <div className="field-clock" data-tauri-drag-region><span className="toolbar-location">{label}</span><Clock /><button onClick={onRefresh} disabled={busy} aria-label="Refresh all systems" title="Refresh all systems"><RefreshCw size={15} /></button></div>
    </IntelligenceOverlay>
  );
}

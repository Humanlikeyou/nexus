import { RefreshCw, Zap } from "lucide-react";
import { IntelligenceOverlay } from "../design/SystemPrimitives";

export function Toolbar({ label, now, busy, onRefresh }: { label: string; now: Date; busy: boolean; onRefresh: () => void }) {
  return (
    <IntelligenceOverlay>
      <header className="nexus-mark"><span className="nexus-symbol"><Zap size={16} /></span><div><strong>Nexus</strong><span>Local intelligence</span></div></header>
      <div className="field-clock"><span className="toolbar-location">{label}</span><span>{now.toLocaleDateString([], { month: "short", day: "numeric" })}</span><strong>{now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</strong><button onClick={onRefresh} disabled={busy} aria-label="Refresh all systems" title="Refresh all systems"><RefreshCw size={15} /></button></div>
    </IntelligenceOverlay>
  );
}

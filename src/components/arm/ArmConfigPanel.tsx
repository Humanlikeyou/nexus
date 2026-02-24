import { useState, useEffect } from "react";
import { Settings, Plus, Trash2, Save } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ArmConfig } from "@/lib/types";

interface ArmConfigPanelProps {
  config: ArmConfig | null;
  onSave: (config: ArmConfig) => void;
  onSetGlobalLimits: (capitalLimit: number, dailyLossCap: number) => void;
}

const defaultConfig: ArmConfig = {
  wallet_address: null,
  global_capital_limit: 100,
  global_daily_loss_cap: 10,
  poll_interval_secs: 300,
  kill_switch_active: false,
  worker_api_keys: [],
};

export function ArmConfigPanel({
  config,
  onSave,
  onSetGlobalLimits,
}: ArmConfigPanelProps) {
  const [form, setForm] = useState<ArmConfig>(config ?? defaultConfig);
  const [dirty, setDirty] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [newKeyValue, setNewKeyValue] = useState("");

  useEffect(() => {
    if (config) {
      setForm(config);
      setDirty(false);
    }
  }, [config]);

  const updateField = <K extends keyof ArmConfig>(key: K, value: ArmConfig[K]) => {
    setForm((f) => ({ ...f, [key]: value }));
    setDirty(true);
  };

  const handleSave = () => {
    onSave(form);
    onSetGlobalLimits(form.global_capital_limit, form.global_daily_loss_cap);
    setDirty(false);
  };

  const addApiKey = () => {
    if (!newKeyName.trim() || !newKeyValue.trim()) return;
    updateField("worker_api_keys", [
      ...form.worker_api_keys,
      { name: newKeyName, key: newKeyValue },
    ]);
    setNewKeyName("");
    setNewKeyValue("");
  };

  const removeApiKey = (idx: number) => {
    updateField(
      "worker_api_keys",
      form.worker_api_keys.filter((_, i) => i !== idx),
    );
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Settings size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">
            ARM Configuration
          </h3>
        </div>
        <button
          onClick={handleSave}
          disabled={!dirty}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors",
            dirty
              ? "bg-accent/15 text-accent hover:bg-accent/25"
              : "bg-muted/15 text-muted-foreground/30 cursor-not-allowed",
          )}
        >
          <Save size={12} /> Save
        </button>
      </div>

      {/* Wallet */}
      <div>
        <label className="text-[13px] text-muted-foreground mb-1 block">
          Wallet Address (display only)
        </label>
        <input
          type="text"
          value={form.wallet_address ?? ""}
          onChange={(e) => updateField("wallet_address", e.target.value || null)}
          placeholder="0x..."
          className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
        />
      </div>

      {/* Global Limits */}
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="text-[13px] text-muted-foreground mb-1 block">
            Global Capital Limit ($)
          </label>
          <input
            type="number"
            value={form.global_capital_limit}
            onChange={(e) =>
              updateField("global_capital_limit", parseFloat(e.target.value) || 0)
            }
            className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
          />
        </div>
        <div>
          <label className="text-[13px] text-muted-foreground mb-1 block">
            Global Daily Loss Cap ($)
          </label>
          <input
            type="number"
            value={form.global_daily_loss_cap}
            onChange={(e) =>
              updateField("global_daily_loss_cap", parseFloat(e.target.value) || 0)
            }
            className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
          />
        </div>
      </div>

      {/* Poll Interval */}
      <div>
        <label className="text-[13px] text-muted-foreground mb-1 block">
          Poll Interval (seconds)
        </label>
        <input
          type="number"
          value={form.poll_interval_secs}
          onChange={(e) =>
            updateField("poll_interval_secs", parseInt(e.target.value) || 300)
          }
          min={30}
          className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
        />
        <p className="text-xs text-muted-foreground/30 mt-1">
          How often to poll worker health and revenue endpoints (min 30s)
        </p>
      </div>

      {/* API Keys */}
      <div className="space-y-2">
        <label className="text-[13px] text-muted-foreground block">
          Worker API Keys
        </label>
        {form.worker_api_keys.map((k, i) => (
          <div
            key={i}
            className="flex items-center gap-2 px-3 py-2 rounded-lg bg-muted/10 border border-border/10"
          >
            <span className="text-xs font-medium flex-shrink-0">{k.name}</span>
            <span className="text-[13px] font-mono text-muted-foreground/40 flex-1 truncate">
              {k.key.slice(0, 8)}{"•".repeat(8)}
            </span>
            <button
              onClick={() => removeApiKey(i)}
              className="p-1 rounded hover:bg-destructive/15 transition-colors"
            >
              <Trash2 size={12} className="text-muted-foreground/30 hover:text-destructive" />
            </button>
          </div>
        ))}

        <div className="flex items-center gap-2">
          <input
            type="text"
            value={newKeyName}
            onChange={(e) => setNewKeyName(e.target.value)}
            placeholder="Name"
            className="w-24 px-2 py-1.5 rounded-lg bg-muted/15 border border-border/30 text-xs placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
          />
          <input
            type="password"
            value={newKeyValue}
            onChange={(e) => setNewKeyValue(e.target.value)}
            placeholder="API Key"
            className="flex-1 px-2 py-1.5 rounded-lg bg-muted/15 border border-border/30 text-xs font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
          />
          <button
            onClick={addApiKey}
            disabled={!newKeyName.trim() || !newKeyValue.trim()}
            className="p-1.5 rounded-lg bg-accent/15 text-accent hover:bg-accent/25 transition-colors disabled:opacity-30"
          >
            <Plus size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}

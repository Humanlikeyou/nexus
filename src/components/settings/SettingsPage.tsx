import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Save,
  Check,
  Package,
  Upload,
  Loader2,
  MessageCircle,
  FolderOpen,
  Shield,
  HardDrive,
  CheckCircle2,
  AlertCircle,
  Power,
  ArrowUpRight,
  ToggleLeft,
  ToggleRight,
  Palette,
  Sun,
  Moon,
  Monitor,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useNexus } from "@/context/NexusContext";
import { useTheme, type ThemeMode } from "@/context/ThemeContext";
import {
  getConfig,
  saveSettings,
  testTelegramConnection,
  packItUp,
  unpackBackup,
  getAppPaths,
  simulateGatewayDown,
  simulateGatewayUp,
} from "@/lib/api";
import type { NexusConfig, PackageInfo } from "@/lib/types";

export function SettingsPage() {
  const { toast, addNotification } = useNexus();
  const { mode: themeMode, setMode: setThemeMode } = useTheme();

  const [config, setConfig] = useState<NexusConfig>({
    telegram_bot_token: null,
    telegram_chat_id: null,
    openclaw_path: null,
    tasks_path: null,
    pipeline_path: null,
    gateway_url: null,
    gateway_auth_token: null,
    gateway_batch_size: null,
    gateway_retry_base_secs: null,
    gateway_retry_max_secs: null,
  });
  const [gatewaySimDown, setGatewaySimDown] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [telegramStatus, setTelegramStatus] = useState<string | null>(null);
  const [telegramTesting, setTelegramTesting] = useState(false);
  const [packing, setPacking] = useState(false);
  const [packResult, setPackResult] = useState<PackageInfo | null>(null);
  const [unpacking, setUnpacking] = useState(false);
  const [unpackResult, setUnpackResult] = useState<string[] | null>(null);
  const [paths, setPaths] = useState<Record<string, string>>({});
  const [autoStart, setAutoStart] = useState(false);

  useEffect(() => {
    getConfig().then(setConfig).catch(() => {});
    getAppPaths().then(setPaths).catch(() => {});
    // Check current autostart state
    invoke<boolean>("plugin:autostart|is_enabled")
      .then(setAutoStart)
      .catch(() => {});
  }, []);

  const handleSave = async () => {
    setSaving(true);
    setSaved(false);
    try {
      await saveSettings(config);
      setSaved(true);
      toast({ type: "success", title: "Settings Saved" });
      addNotification({ type: "success", title: "Settings Saved", message: "Configuration updated", source: "Settings" });
      setTimeout(() => setSaved(false), 3000);
    } catch (e) {
      toast({ type: "error", title: "Save Failed", description: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const handleTestTelegram = async () => {
    setTelegramTesting(true);
    setTelegramStatus(null);
    try {
      await saveSettings(config);
      const result = await testTelegramConnection();
      setTelegramStatus(result);
      toast({ type: "success", title: "Telegram Connected", description: result });
      addNotification({ type: "success", title: "Telegram Test", message: result, source: "Settings" });
    } catch (e) {
      setTelegramStatus(`Error: ${e}`);
      toast({ type: "error", title: "Telegram Test Failed", description: String(e) });
    } finally {
      setTelegramTesting(false);
    }
  };

  const handlePackItUp = async () => {
    setPacking(true);
    setPackResult(null);
    try {
      const result = await packItUp();
      setPackResult(result);
      toast({ type: "success", title: "Backup Created", description: `${result.size_mb} MB saved to Desktop` });
      addNotification({ type: "success", title: "Backup Created", message: `${result.size_mb} MB — ${result.includes.length} items packed`, source: "Packager" });
    } catch (e) {
      setPackResult({ path: "", size_mb: 0, includes: [`Error: ${e}`] });
      toast({ type: "error", title: "Backup Failed", description: String(e) });
    } finally {
      setPacking(false);
    }
  };

  const handleUnpack = async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".tar.gz,.nexus-backup";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      setUnpacking(true);
      setUnpackResult(null);
      try {
        const result = await unpackBackup((file as any).path || file.name);
        setUnpackResult(result);
        toast({ type: "success", title: "Backup Restored", description: `${result.length} items restored` });
        addNotification({ type: "success", title: "Backup Restored", message: `${result.length} items restored from archive`, source: "Packager" });
      } catch (e) {
        setUnpackResult([`Error: ${e}`]);
        toast({ type: "error", title: "Restore Failed", description: String(e) });
      } finally {
        setUnpacking(false);
      }
    };
    input.click();
  };

  const handleAutoStartToggle = async () => {
    try {
      if (autoStart) {
        await invoke("plugin:autostart|disable");
        setAutoStart(false);
        toast({ type: "info", title: "Auto-launch disabled" });
      } else {
        await invoke("plugin:autostart|enable");
        setAutoStart(true);
        toast({ type: "success", title: "Auto-launch enabled" });
      }
    } catch (e) {
      toast({ type: "error", title: "Failed to toggle auto-launch", description: String(e) });
    }
  };

  const updateField = (field: keyof NexusConfig, value: string) => {
    setConfig((prev) => ({ ...prev, [field]: value || null }));
    setSaved(false);
  };

  return (
    <div className="space-y-6 max-w-[720px]">
      {/* Telegram Configuration */}
      <section className="glass p-6 space-y-5">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-blue-500/15 flex items-center justify-center">
            <MessageCircle size={16} className="text-blue-400" />
          </div>
          <div>
            <h3 className="text-base font-semibold">Telegram Connection</h3>
            <p className="text-[13px] text-muted-foreground">
              Connect to your Terrance bot to see messages in Electron
            </p>
          </div>
        </div>

        <div className="space-y-3">
          <div>
            <label className="text-[13px] text-muted-foreground mb-1 block">
              Bot Token
            </label>
            <input
              type="password"
              value={config.telegram_bot_token || ""}
              onChange={(e) => updateField("telegram_bot_token", e.target.value)}
              placeholder="123456:ABC-DEF..."
              className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
            />
            <p className="text-xs text-muted-foreground/40 mt-1">
              Get this from @BotFather on Telegram
            </p>
          </div>

          <div>
            <label className="text-[13px] text-muted-foreground mb-1 block">
              Chat ID
            </label>
            <input
              type="text"
              value={config.telegram_chat_id || ""}
              onChange={(e) => updateField("telegram_chat_id", e.target.value)}
              placeholder="-1001234567890"
              className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
            />
            <p className="text-xs text-muted-foreground/40 mt-1">
              Your personal chat or group where Terrance operates
            </p>
          </div>

          <div className="flex items-center gap-3">
            <button
              onClick={handleTestTelegram}
              disabled={telegramTesting || !config.telegram_bot_token}
              className={cn(
                "flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs transition-colors",
                config.telegram_bot_token
                  ? "bg-blue-500/15 text-blue-400 hover:bg-blue-500/25"
                  : "bg-muted/10 text-muted-foreground/30 cursor-not-allowed",
              )}
            >
              {telegramTesting ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <Shield size={12} />
              )}
              Test Connection
            </button>
            {telegramStatus && (
              <span
                className={cn(
                  "text-[13px]",
                  telegramStatus.startsWith("Error")
                    ? "text-destructive"
                    : "text-success",
                )}
              >
                {telegramStatus}
              </span>
            )}
          </div>
        </div>
      </section>

      {/* Data Paths */}
      <section className="glass p-6 space-y-5">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center">
            <FolderOpen size={16} className="text-accent" />
          </div>
          <div>
            <h3 className="text-base font-semibold">Data Paths</h3>
            <p className="text-[13px] text-muted-foreground">
              Where Electron reads your data from
            </p>
          </div>
        </div>

        <div className="space-y-3">
          <div>
            <label className="text-[13px] text-muted-foreground mb-1 block">
              OpenClaw Path
            </label>
            <input
              type="text"
              value={config.openclaw_path || ""}
              onChange={(e) => updateField("openclaw_path", e.target.value)}
              placeholder={paths.openclaw_dir || "~/.openclaw"}
              className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
            />
          </div>

          <div>
            <label className="text-[13px] text-muted-foreground mb-1 block">
              TASKS.md Path
            </label>
            <input
              type="text"
              value={config.tasks_path || ""}
              onChange={(e) => updateField("tasks_path", e.target.value)}
              placeholder="~/.openclaw/workspace/TASKS.md"
              className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
            />
          </div>

          <div>
            <label className="text-[13px] text-muted-foreground mb-1 block">
              CONTENT-PIPELINE.md Path
            </label>
            <input
              type="text"
              value={config.pipeline_path || ""}
              onChange={(e) => updateField("pipeline_path", e.target.value)}
              placeholder="~/.openclaw/workspace/CONTENT-PIPELINE.md"
              className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
            />
          </div>
        </div>
      </section>

      {/* Gateway / Pipeline */}
      <section className="glass p-6 space-y-5">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-success/15 flex items-center justify-center">
            <ArrowUpRight size={16} className="text-success" />
          </div>
          <div>
            <h3 className="text-base font-semibold">Gateway / Pipeline</h3>
            <p className="text-[13px] text-muted-foreground">
              Configure the store-and-forward pipeline to Terrance
            </p>
          </div>
        </div>

        <div className="space-y-3">
          <div>
            <label className="text-[13px] text-muted-foreground mb-1 block">
              Gateway URL
            </label>
            <input
              type="text"
              value={config.gateway_url || ""}
              onChange={(e) => updateField("gateway_url", e.target.value)}
              placeholder="http://127.0.0.1:18789"
              className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
            />
          </div>

          <div>
            <label className="text-[13px] text-muted-foreground mb-1 block">
              Auth Token
            </label>
            <input
              type="password"
              value={config.gateway_auth_token || ""}
              onChange={(e) => updateField("gateway_auth_token", e.target.value)}
              placeholder="Optional bearer token"
              className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
            />
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="text-[13px] text-muted-foreground mb-1 block">
                Batch Size
              </label>
              <input
                type="number"
                value={config.gateway_batch_size ?? ""}
                onChange={(e) => setConfig((prev) => ({ ...prev, gateway_batch_size: e.target.value ? Number(e.target.value) : null }))}
                placeholder="50"
                className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
              />
            </div>
            <div>
              <label className="text-[13px] text-muted-foreground mb-1 block">
                Retry Base (s)
              </label>
              <input
                type="number"
                value={config.gateway_retry_base_secs ?? ""}
                onChange={(e) => setConfig((prev) => ({ ...prev, gateway_retry_base_secs: e.target.value ? Number(e.target.value) : null }))}
                placeholder="5"
                className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
              />
            </div>
            <div>
              <label className="text-[13px] text-muted-foreground mb-1 block">
                Retry Max (s)
              </label>
              <input
                type="number"
                value={config.gateway_retry_max_secs ?? ""}
                onChange={(e) => setConfig((prev) => ({ ...prev, gateway_retry_max_secs: e.target.value ? Number(e.target.value) : null }))}
                placeholder="300"
                className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
              />
            </div>
          </div>

          <div className="flex items-center gap-3 pt-1">
            <button
              onClick={async () => {
                if (gatewaySimDown) {
                  await simulateGatewayUp();
                  setGatewaySimDown(false);
                  toast({ type: "info", title: "Gateway simulation: UP" });
                } else {
                  await simulateGatewayDown();
                  setGatewaySimDown(true);
                  toast({ type: "warning", title: "Gateway simulation: DOWN" });
                }
              }}
              className={cn(
                "flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs transition-colors",
                gatewaySimDown
                  ? "bg-destructive/15 text-destructive hover:bg-destructive/25"
                  : "bg-muted/20 text-muted-foreground hover:bg-muted/30",
              )}
            >
              {gatewaySimDown ? <ToggleRight size={12} /> : <ToggleLeft size={12} />}
              {gatewaySimDown ? "Simulating Down" : "Simulate Down"}
            </button>
          </div>
        </div>
      </section>

      {/* Save Button */}
      <button
        onClick={handleSave}
        disabled={saving}
        className={cn(
          "w-full flex items-center justify-center gap-2 px-4 py-3 rounded-lg text-sm font-medium transition-all",
          saved
            ? "bg-success/20 text-success"
            : "bg-accent/20 text-accent hover:bg-accent/30",
        )}
      >
        {saving ? (
          <Loader2 size={14} className="animate-spin" />
        ) : saved ? (
          <Check size={14} />
        ) : (
          <Save size={14} />
        )}
        {saved ? "Settings Saved" : "Save Settings"}
      </button>

      {/* Auto-launch */}
      <section className="glass p-6 space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-success/15 flex items-center justify-center">
              <Power size={16} className="text-success" />
            </div>
            <div>
              <h3 className="text-base font-semibold">Launch on Login</h3>
              <p className="text-[13px] text-muted-foreground">
                Automatically start Electron when you log in to your Mac
              </p>
            </div>
          </div>
          <button
            onClick={handleAutoStartToggle}
            className={cn(
              "relative w-11 h-6 rounded-full transition-colors",
              autoStart ? "bg-success" : "bg-muted/30",
            )}
          >
            <div
              className={cn(
                "absolute top-0.5 w-5 h-5 rounded-full bg-white shadow transition-transform",
                autoStart ? "translate-x-[22px]" : "translate-x-0.5",
              )}
            />
          </button>
        </div>
      </section>

      {/* Theme */}
      <section className="glass p-6 space-y-5">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center">
            <Palette size={16} className="text-accent" />
          </div>
          <div>
            <h3 className="text-base font-semibold">Theme</h3>
            <p className="text-[13px] text-muted-foreground">
              Choose light, dark, or follow your system appearance
            </p>
          </div>
        </div>

        <div className="flex gap-2">
          {([
            { value: "light" as ThemeMode, label: "Light", icon: Sun },
            { value: "dark" as ThemeMode, label: "Dark", icon: Moon },
            { value: "system" as ThemeMode, label: "System", icon: Monitor },
          ]).map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              onClick={() => setThemeMode(value)}
              className={cn(
                "flex-1 flex items-center justify-center gap-2 px-3 py-2.5 rounded-lg text-xs font-medium transition-colors",
                themeMode === value
                  ? "bg-accent/15 text-accent border border-accent/30"
                  : "bg-muted/15 text-muted-foreground border border-border/15 hover:bg-muted/25",
              )}
            >
              <Icon size={14} />
              {label}
            </button>
          ))}
        </div>
      </section>

      {/* Pack It Up */}
      <section className="glass p-6 space-y-5">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-warning/15 flex items-center justify-center">
            <Package size={16} className="text-warning" />
          </div>
          <div>
            <h3 className="text-base font-semibold">Pack It Up</h3>
            <p className="text-[13px] text-muted-foreground">
              Export everything — config, data, sessions — into a single portable file.
              Drop it on a drive, unpack on another machine, and you're back in business.
            </p>
          </div>
        </div>

        <div className="flex gap-3">
          <button
            onClick={handlePackItUp}
            disabled={packing}
            className="flex items-center gap-2 px-4 py-2.5 rounded-lg bg-warning/15 text-warning text-sm font-medium hover:bg-warning/25 transition-colors"
          >
            {packing ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <HardDrive size={14} />
            )}
            {packing ? "Packing..." : "Pack It Up"}
          </button>

          <button
            onClick={handleUnpack}
            disabled={unpacking}
            className="flex items-center gap-2 px-4 py-2.5 rounded-lg bg-muted/20 text-muted-foreground text-sm hover:bg-muted/30 transition-colors"
          >
            {unpacking ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Upload size={14} />
            )}
            {unpacking ? "Unpacking..." : "Unpack Backup"}
          </button>
        </div>

        {packResult && (
          <div className="p-4 rounded-lg bg-muted/10 border border-border/20 space-y-2">
            {packResult.path ? (
              <>
                <div className="flex items-center gap-2">
                  <CheckCircle2 size={14} className="text-success" />
                  <span className="text-xs font-medium text-success">
                    Backup created — {packResult.size_mb} MB
                  </span>
                </div>
                <p className="text-[13px] text-muted-foreground font-mono break-all">
                  {packResult.path}
                </p>
                <div className="space-y-1 mt-2">
                  {packResult.includes.map((item, i) => (
                    <div key={i} className="flex items-center gap-1.5">
                      <Check size={10} className="text-muted-foreground/50" />
                      <span className="text-xs text-muted-foreground/60">{item}</span>
                    </div>
                  ))}
                </div>
              </>
            ) : (
              <div className="flex items-center gap-2">
                <AlertCircle size={14} className="text-destructive" />
                <span className="text-xs text-destructive">{packResult.includes[0]}</span>
              </div>
            )}
          </div>
        )}

        {unpackResult && (
          <div className="p-4 rounded-lg bg-muted/10 border border-border/20 space-y-2">
            <div className="flex items-center gap-2">
              <CheckCircle2 size={14} className="text-success" />
              <span className="text-xs font-medium text-success">
                Backup restored
              </span>
            </div>
            <div className="space-y-1">
              {unpackResult.map((item, i) => (
                <div key={i} className="flex items-center gap-1.5">
                  <Check size={10} className="text-muted-foreground/50" />
                  <span className="text-xs text-muted-foreground/60">{item}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </section>

      {/* App Info */}
      <section className="glass p-6 space-y-3">
        <h3 className="text-base font-semibold">About Electron</h3>
        <div className="grid grid-cols-2 gap-3 text-xs">
          <div>
            <p className="text-muted-foreground/50">Version</p>
            <p className="font-mono">{__APP_VERSION__}</p>
          </div>
          <div>
            <p className="text-muted-foreground/50">Build</p>
            <p className="font-mono">{__BUILD_HASH__}</p>
          </div>
          <div>
            <p className="text-muted-foreground/50">Built At</p>
            <p className="font-mono text-xs">{new Date(__BUILD_TIME__).toLocaleString()}</p>
          </div>
          <div>
            <p className="text-muted-foreground/50">Framework</p>
            <p className="font-mono">Tauri {__TAURI_VERSION__} + React 19</p>
          </div>
          <div>
            <p className="text-muted-foreground/50">Data Directory</p>
            <p className="font-mono text-xs break-all">{paths.data_dir || "—"}</p>
          </div>
          <div>
            <p className="text-muted-foreground/50">OpenClaw Directory</p>
            <p className="font-mono text-xs break-all">{paths.openclaw_dir || "—"}</p>
          </div>
        </div>
      </section>
    </div>
  );
}

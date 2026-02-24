import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import * as Dialog from "@radix-ui/react-dialog";
import {
  Cpu,
  MessageCircle,
  FolderOpen,
  ArrowRight,
  ArrowLeft,
  Check,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { checkOnboarding, completeOnboarding, saveSettings, getConfig } from "@/lib/api";
import type { NexusConfig } from "@/lib/types";

const steps = [
  {
    icon: Cpu,
    title: "Welcome to Electron",
    description:
      "Your centralized command center for Terrance, TaiyoSkin, neurofeedback, fitness tracking, and life trajectory. Let's get you set up.",
  },
  {
    icon: MessageCircle,
    title: "Telegram Connection",
    description:
      "Connect your Terrance bot to see messages directly inside Electron. You can configure this later in Settings.",
  },
  {
    icon: FolderOpen,
    title: "OpenClaw Path",
    description:
      "Point Electron to your OpenClaw installation so it can read status, cron jobs, and skills data.",
  },
];

const LOCAL_ONBOARDED_KEY = "nexus:onboarded";

export function OnboardingModal() {
  const [open, setOpen] = useState(false);
  const [syncPending, setSyncPending] = useState(false);
  const [step, setStep] = useState(0);
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
    n8n_webhook_secret: null,
    preferred_muse_address: null,
    preferred_whoop_address: null,
  });

  useEffect(() => {
    const localDone = localStorage.getItem(LOCAL_ONBOARDED_KEY) === "1";
    checkOnboarding()
      .then((backendDone) => {
        if (!backendDone && !localDone) {
          // Neither local nor backend says done — show modal
          setOpen(true);
          getConfig().then(setConfig).catch(() => {});
        } else if (localDone && !backendDone) {
          // Local says done but backend doesn't know — show sync pending indicator
          setSyncPending(true);
          // Try to sync in the background
          completeOnboarding().then(() => setSyncPending(false)).catch(() => {});
        }
      })
      .catch(() => {
        // Backend unavailable — trust local flag only, don't show modal if local says done
        if (!localDone) setOpen(true);
        else setSyncPending(true); // backend unreachable, flag it
      });
  }, []);

  const handleComplete = async () => {
    localStorage.setItem(LOCAL_ONBOARDED_KEY, "1");
    try {
      await saveSettings(config);
      await completeOnboarding();
      setSyncPending(false);
    } catch {
      // Backend failed — local flag set, will show sync pending on next launch
      setSyncPending(true);
    }
    setOpen(false);
  };

  const updateField = (field: keyof NexusConfig, value: string) => {
    setConfig((prev) => ({ ...prev, [field]: value || null }));
  };

  const currentStep = steps[step];
  const Icon = currentStep.icon;

  return (
    <>
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <AnimatePresence>
        {open && (
          <Dialog.Portal forceMount>
            <Dialog.Overlay asChild>
              <motion.div
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="fixed inset-0 z-[95] bg-black/60 backdrop-blur-sm"
              />
            </Dialog.Overlay>
            <Dialog.Content asChild>
              <motion.div
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
                transition={{ duration: 0.2, ease: "easeOut" }}
                className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[96] w-[480px] glass-elevated rounded-2xl border border-border/30 shadow-2xl overflow-hidden"
              >
                {/* Progress dots */}
                <div className="flex items-center justify-center gap-2 pt-6 pb-2">
                  {steps.map((_, i) => (
                    <div
                      key={i}
                      className={cn(
                        "w-2 h-2 rounded-full transition-colors",
                        i === step ? "bg-accent" : i < step ? "bg-success" : "bg-muted/30",
                      )}
                    />
                  ))}
                </div>

                <AnimatePresence mode="wait">
                  <motion.div
                    key={step}
                    initial={{ opacity: 0, x: 30 }}
                    animate={{ opacity: 1, x: 0 }}
                    exit={{ opacity: 0, x: -30 }}
                    transition={{ duration: 0.2 }}
                    className="px-8 py-6 space-y-5"
                  >
                    <div className="flex flex-col items-center text-center space-y-3">
                      <div className="w-12 h-12 rounded-xl bg-accent/15 flex items-center justify-center">
                        <Icon size={24} className="text-accent" />
                      </div>
                      <Dialog.Title className="text-lg font-semibold">
                        {currentStep.title}
                      </Dialog.Title>
                      <Dialog.Description className="text-sm text-muted-foreground">
                        {currentStep.description}
                      </Dialog.Description>
                    </div>

                    {/* Step 1: Telegram config */}
                    {step === 1 && (
                      <div className="space-y-3">
                        <div>
                          <label className="text-[13px] text-muted-foreground mb-1 block">
                            Bot Token
                          </label>
                          <input
                            type="password"
                            value={config.telegram_bot_token || ""}
                            onChange={(e) =>
                              updateField("telegram_bot_token", e.target.value)
                            }
                            placeholder="123456:ABC-DEF..."
                            className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
                          />
                        </div>
                        <div>
                          <label className="text-[13px] text-muted-foreground mb-1 block">
                            Chat ID
                          </label>
                          <input
                            type="text"
                            value={config.telegram_chat_id || ""}
                            onChange={(e) =>
                              updateField("telegram_chat_id", e.target.value)
                            }
                            placeholder="-1001234567890"
                            className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
                          />
                        </div>
                      </div>
                    )}

                    {/* Step 2: OpenClaw path */}
                    {step === 2 && (
                      <div>
                        <label className="text-[13px] text-muted-foreground mb-1 block">
                          OpenClaw Path
                        </label>
                        <input
                          type="text"
                          value={config.openclaw_path || ""}
                          onChange={(e) =>
                            updateField("openclaw_path", e.target.value)
                          }
                          placeholder="~/.openclaw"
                          className="w-full px-3 py-2.5 rounded-xl bg-muted/15 border border-border/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
                        />
                      </div>
                    )}
                  </motion.div>
                </AnimatePresence>

                {/* Navigation */}
                <div className="flex items-center justify-between px-8 pb-6">
                  {step > 0 ? (
                    <button
                      onClick={() => setStep((s) => s - 1)}
                      className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm text-muted-foreground hover:text-foreground hover:bg-muted/20 transition-colors"
                    >
                      <ArrowLeft size={14} /> Back
                    </button>
                  ) : (
                    <button
                      onClick={() => {
                        localStorage.setItem(LOCAL_ONBOARDED_KEY, "1");
                        completeOnboarding()
                          .then(() => setSyncPending(false))
                          .catch(() => setSyncPending(true));
                        setOpen(false);
                      }}
                      className="text-xs text-muted-foreground/50 hover:text-muted-foreground transition-colors"
                    >
                      Skip setup
                    </button>
                  )}

                  {step < steps.length - 1 ? (
                    <button
                      onClick={() => setStep((s) => s + 1)}
                      className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent/15 text-accent text-sm font-medium hover:bg-accent/25 transition-colors"
                    >
                      Next <ArrowRight size={14} />
                    </button>
                  ) : (
                    <button
                      onClick={handleComplete}
                      className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-success/15 text-success text-sm font-medium hover:bg-success/25 transition-colors"
                    >
                      <Check size={14} /> Get Started
                    </button>
                  )}
                </div>
              </motion.div>
            </Dialog.Content>
          </Dialog.Portal>
        )}
      </AnimatePresence>
    </Dialog.Root>

    {/* Sync pending indicator — shown when local says done but backend hasn't confirmed */}
    {syncPending && !open && (
      <div className="fixed bottom-4 right-4 z-[90] flex items-center gap-2 px-3 py-2 rounded-xl glass-elevated border border-warning/20 text-warning text-xs shadow-lg">
        <span className="w-1.5 h-1.5 rounded-full bg-warning animate-pulse" />
        Setup sync pending
        <button
          onClick={() => {
            completeOnboarding()
              .then(() => setSyncPending(false))
              .catch(() => {});
          }}
          className="ml-1 underline opacity-70 hover:opacity-100"
        >
          Retry
        </button>
      </div>
    )}
    </>
  );
}

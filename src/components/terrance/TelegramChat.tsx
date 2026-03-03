import { useState, useEffect, useRef, useCallback } from "react";
import { Send, Bot, User, Loader2, AlertTriangle, Cpu, BookOpen } from "lucide-react";
import { cn } from "@/lib/utils";
import { chatWithRouter, getConfig, createChatSession, saveChatMessage, loadChatHistory } from "@/lib/api";
import { buildRouterConfig, formatCost, formatProvider } from "@/lib/llm-router";
import { buildSystemPrompt, summarizeAndStoreSession, summarizeAndStoreSessionSilent } from "@/terrance/system/continuity";
import type { LlmRouterConfig, RouterResponse } from "@/lib/types";

interface ChatMessage {
  id: number;
  role: "user" | "terrance";
  text: string;
  ts: number;
  meta?: Pick<RouterResponse, "provider" | "model" | "latency_ms" | "estimated_cost_usd">;
}

let msgCounter = 0;

export function TelegramChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [systemPrompt, setSystemPrompt] = useState<string>("");
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [summarizing, setSummarizing] = useState(false);
  const [routerConfig, setRouterConfig] = useState<LlmRouterConfig | null>(null);
  const [anthropicApiKey, setAnthropicApiKey] = useState<string | undefined>(undefined);
  const scrollRef = useRef<HTMLDivElement>(null);
  const sessionIdRef = useRef<string | null>(null);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hasAutoSavedRef = useRef(false);
  const [autoSaved, setAutoSaved] = useState(false);

  // Load router config + build system prompt with identity + memory context
  useEffect(() => {
    getConfig()
      .then(async (cfg) => {
        setRouterConfig(buildRouterConfig(cfg));
        setAnthropicApiKey(cfg.anthropic_api_key ?? undefined);
        const prompt = await buildSystemPrompt();
        setSystemPrompt(prompt);
      })
      .catch(() => {});

    // Create a new chat session and load any prior messages
    createChatSession(new Date().toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" }))
      .then(async (session) => {
        sessionIdRef.current = session.id;
        try {
          const history = await loadChatHistory(session.id);
          if (history.length > 0) {
            setMessages(history.map((m) => ({
              id: m.id,
              role: m.role,
              text: m.text,
              ts: m.ts,
            })));
          }
        } catch {}
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, sending]);

  // Auto-save: reset idle timer whenever messages change (after a response arrives)
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

  const resetIdleTimer = useCallback(() => {
    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    if (hasAutoSavedRef.current) return;
    idleTimerRef.current = setTimeout(async () => {
      const msgs = messagesRef.current;
      if (msgs.length < 4 || hasAutoSavedRef.current) return;
      hasAutoSavedRef.current = true;
      const entries = msgs.map((m) => ({ role: m.role as "user" | "terrance", text: m.text, ts: m.ts }));
      const result = await summarizeAndStoreSessionSilent(entries);
      if (result.saved) setAutoSaved(true);
    }, 2 * 60 * 1000); // 2 minutes
  }, []);

  // Cleanup: auto-save on unmount if not already saved
  useEffect(() => {
    return () => {
      if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
      const msgs = messagesRef.current;
      if (msgs.length >= 4 && !hasAutoSavedRef.current) {
        hasAutoSavedRef.current = true;
        const entries = msgs.map((m) => ({ role: m.role as "user" | "terrance", text: m.text, ts: m.ts }));
        summarizeAndStoreSessionSilent(entries);
      }
    };
  }, []);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || sending) return;

    const userMsg: ChatMessage = {
      id: ++msgCounter,
      role: "user",
      text,
      ts: Date.now(),
    };

    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setSending(true);
    setError(null);

    // Persist user message to SQLite
    if (sessionIdRef.current) {
      saveChatMessage({
        id: 0, session_id: sessionIdRef.current,
        role: "user", text, ts: userMsg.ts,
      }).catch(() => {});
    }

    try {
      const cfg = routerConfig ?? buildRouterConfig({});
      // Prepend system prompt to the message for identity injection
      const fullText = systemPrompt
        ? `${systemPrompt}\n\n---\n\nUser: ${text}`
        : text;
      const resp = await chatWithRouter(fullText, cfg, [], undefined, anthropicApiKey);
      const terranceTs = Date.now();
      setMessages((prev) => [
        ...prev,
        {
          id: ++msgCounter,
          role: "terrance",
          text: resp.content,
          ts: terranceTs,
          meta: {
            provider: resp.provider,
            model: resp.model,
            latency_ms: resp.latency_ms,
            estimated_cost_usd: resp.estimated_cost_usd,
          },
        },
      ]);

      // Persist Terrance response to SQLite
      if (sessionIdRef.current) {
        saveChatMessage({
          id: 0, session_id: sessionIdRef.current,
          role: "terrance", text: resp.content, ts: terranceTs,
          provider: resp.provider, model: resp.model,
          latency_ms: resp.latency_ms, cost_usd: resp.estimated_cost_usd,
        }).catch(() => {});
      }

      // Reset auto-save idle timer after each response
      resetIdleTimer();
    } catch (e) {
      const msg = String(e).replace(/^Error:\s*/, "");
      setError(msg);
    } finally {
      setSending(false);
    }
  };

  // Summarize and store the session when explicitly requested
  const handleSummarize = useCallback(async () => {
    if (messages.length < 2 || summarizing) return;
    // Cancel idle timer and prevent auto-save
    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    hasAutoSavedRef.current = true;
    setSummarizing(true);
    try {
      const entries = messages.map((m) => ({
        role: m.role as "user" | "terrance",
        text: m.text,
        ts: m.ts,
      }));
      const summary = await summarizeAndStoreSession(entries);
      if (summary) {
        setMessages((prev) => [
          ...prev,
          {
            id: ++msgCounter,
            role: "terrance",
            text: `Session summarized and stored:\n\n${summary}`,
            ts: Date.now(),
          },
        ]);
      }
    } finally {
      setSummarizing(false);
    }
  }, [messages, summarizing]);

  const computeMode = routerConfig?.mode ?? "hybrid";
  const canSummarize = messages.length >= 4 && !summarizing;

  return (
    <div className="glass p-6 space-y-3 flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Cpu size={16} className="text-accent" />
          <h3 className="text-sm font-medium text-muted-foreground/70 uppercase tracking-wide">Terrance</h3>
          <span className="text-xs px-2 py-0.5 rounded-md font-medium bg-accent/10 text-accent capitalize">
            {computeMode}
          </span>
        </div>
        {autoSaved ? (
          <span className="flex items-center gap-1 text-xs text-muted-foreground/30">
            <BookOpen size={11} />
            Auto-saved
          </span>
        ) : canSummarize && (
          <button
            onClick={handleSummarize}
            disabled={summarizing}
            className="flex items-center gap-1 text-xs text-muted-foreground/40 hover:text-accent transition-colors"
            title="Summarize and store this session to memory"
          >
            {summarizing ? (
              <Loader2 size={11} className="animate-spin" />
            ) : (
              <BookOpen size={11} />
            )}
            Save to memory
          </button>
        )}
      </div>

      {/* Messages */}
      <div
        ref={scrollRef}
        className="h-[320px] overflow-y-auto space-y-3 pr-1"
      >
        {messages.length === 0 && !sending && (
          <div className="flex flex-col items-center justify-center h-full gap-3">
            <Bot size={28} className="text-muted-foreground/20" />
            <p className="text-xs text-muted-foreground/40 text-center">
              Ask Terrance anything.
            </p>
          </div>
        )}

        {messages.map((msg) => (
          <div
            key={msg.id}
            className={cn(
              "flex gap-2 max-w-[88%]",
              msg.role === "user" ? "ml-auto flex-row-reverse" : "",
            )}
          >
            <div
              className={cn(
                "w-6 h-6 rounded-full flex items-center justify-center shrink-0 mt-0.5",
                msg.role === "user" ? "bg-muted/30" : "bg-accent/15",
              )}
            >
              {msg.role === "user" ? (
                <User size={12} className="text-muted-foreground" />
              ) : (
                <Bot size={12} className="text-accent" />
              )}
            </div>
            <div
              className={cn(
                "px-3 py-2 rounded-xl text-xs leading-relaxed",
                msg.role === "user"
                  ? "bg-muted/20 border border-border/12 rounded-tr-sm"
                  : "bg-accent/8 border border-accent/20 rounded-tl-sm",
              )}
            >
              <p className="whitespace-pre-wrap break-words">{msg.text}</p>
              <div className="flex items-center gap-2 mt-1">
                <p className="text-[11px] text-muted-foreground/30">
                  {new Date(msg.ts).toLocaleTimeString([], {
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </p>
                {msg.meta && (
                  <p className="text-[11px] text-muted-foreground/25">
                    {formatProvider(msg.meta.provider)} · {msg.meta.model.split(":")[0]}
                    {msg.meta.estimated_cost_usd > 0 && ` · ${formatCost(msg.meta.estimated_cost_usd)}`}
                  </p>
                )}
              </div>
            </div>
          </div>
        ))}

        {/* Typing indicator */}
        {sending && (
          <div className="flex gap-2 max-w-[88%]">
            <div className="w-6 h-6 rounded-full flex items-center justify-center shrink-0 mt-0.5 bg-accent/15">
              <Bot size={12} className="text-accent" />
            </div>
            <div className="px-3 py-2.5 rounded-xl rounded-tl-sm bg-accent/8 border border-accent/20">
              <div className="flex gap-1 items-center">
                <span className="w-1.5 h-1.5 rounded-full bg-accent/50 animate-bounce [animation-delay:0ms]" />
                <span className="w-1.5 h-1.5 rounded-full bg-accent/50 animate-bounce [animation-delay:150ms]" />
                <span className="w-1.5 h-1.5 rounded-full bg-accent/50 animate-bounce [animation-delay:300ms]" />
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Error */}
      {error && (
        <div className="flex items-start gap-2 px-3 py-2 rounded-lg bg-destructive/8 border border-destructive/20 text-destructive text-xs">
          <AlertTriangle size={12} className="mt-0.5 flex-shrink-0" />
          <span className="leading-relaxed">{error}</span>
        </div>
      )}

      {/* Input */}
      <div className="flex gap-2">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && handleSend()}
          placeholder="Message Terrance..."
          disabled={sending}
          className="flex-1 px-3 py-2 rounded-lg bg-muted/20 border border-border/30 text-sm placeholder:text-muted-foreground/30 focus:outline-none focus:border-accent/40 transition-colors disabled:opacity-50"
        />
        <button
          onClick={handleSend}
          disabled={!input.trim() || sending}
          className={cn(
            "px-3 py-2 rounded-lg transition-colors flex items-center gap-1.5",
            input.trim() && !sending
              ? "bg-accent/20 text-accent hover:bg-accent/30"
              : "bg-muted/10 text-muted-foreground/30 cursor-not-allowed",
          )}
        >
          {sending ? (
            <Loader2 size={14} className="animate-spin" />
          ) : (
            <Send size={14} />
          )}
        </button>
      </div>
    </div>
  );
}

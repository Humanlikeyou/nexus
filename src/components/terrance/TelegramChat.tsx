import { useState, useEffect, useRef, useCallback } from "react";
import { Send, Bot, User, Loader2, AlertTriangle, Cpu, BookOpen, Search } from "lucide-react";
import { cn } from "@/lib/utils";
import { chatWithRouter, getConfig, createChatSession, saveChatMessage, loadChatHistory } from "@/lib/api";
import { buildRouterConfig, formatCost, formatProvider } from "@/lib/llm-router";
import { buildSystemPrompt, summarizeAndStoreSession, summarizeAndStoreSessionSilent } from "@/terrance/system/continuity";
import { compressIfNeeded, resetCompressor, type CompressibleMessage } from "@/terrance/system/contextCompressor";
import { deliberate } from "@/terrance/system/deliberation";
import { runResearchTask } from "@/terrance/system/research";
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
  const [gatewayUrl, setGatewayUrl] = useState<string | undefined>(undefined);
  const [gatewayAuthToken, setGatewayAuthToken] = useState<string | undefined>(undefined);
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
        setGatewayUrl(cfg.gateway_url ?? undefined);
        setGatewayAuthToken(cfg.gateway_auth_token ?? undefined);
        const prompt = await buildSystemPrompt();
        setSystemPrompt(prompt);
      })
      .catch(() => {});

    // Reset context compressor for fresh session
    resetCompressor();

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
      if (result.saved) {
        setAutoSaved(true);
      }
    }, 45 * 1000); // 45 seconds
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
      // Handle /deliberate command — adversarial bull/bear analysis
      if (text.startsWith("/deliberate ")) {
        const topic = text.slice("/deliberate ".length).trim();
        if (topic) {
          setMessages((prev) => [
            ...prev,
            {
              id: ++msgCounter,
              role: "terrance",
              text: `Deliberating: "${topic}"...\n(Running bull/bear/synthesis passes)`,
              ts: Date.now(),
            },
          ]);
          const result = await deliberate(topic);
          const terranceTs = Date.now();
          const output = result
            ? `**Bull Case (Optimistic):**\n${result.bullCase}\n\n**Bear Case (Skeptical):**\n${result.bearCase}\n\n**Synthesis:**\n${result.synthesis}\n\n_Confidence: ${result.confidence}_`
            : "Deliberation failed — check gateway connection.";
          setMessages((prev) => [
            ...prev.filter((m) => !m.text.startsWith("Deliberating:")),
            {
              id: ++msgCounter,
              role: "terrance",
              text: output,
              ts: terranceTs,
            },
          ]);
          if (sessionIdRef.current) {
            saveChatMessage({
              id: 0, session_id: sessionIdRef.current,
              role: "terrance", text: output, ts: terranceTs,
            }).catch(() => {});
          }
          resetIdleTimer();
          setSending(false);
          return;
        }
      }

      // Handle /research command
      if (text.startsWith("/research ")) {
        const topic = text.slice("/research ".length).trim();
        if (topic) {
          setMessages((prev) => [
            ...prev,
            {
              id: ++msgCounter,
              role: "terrance",
              text: `Researching: "${topic}"...`,
              ts: Date.now(),
            },
          ]);
          const result = await runResearchTask(topic);
          const terranceTs = Date.now();
          setMessages((prev) => [
            ...prev.filter((m) => !m.text.startsWith("Researching:")),
            {
              id: ++msgCounter,
              role: "terrance",
              text: result.summary ?? "Research produced no results.",
              ts: terranceTs,
            },
          ]);
          if (sessionIdRef.current) {
            saveChatMessage({
              id: 0, session_id: sessionIdRef.current,
              role: "terrance", text: result.summary ?? "", ts: terranceTs,
            }).catch(() => {});
          }
          resetIdleTimer();
          setSending(false);
          return;
        }
      }

      const cfg = routerConfig ?? buildRouterConfig({});

      // Compress conversation context if it's getting long
      const compressible: CompressibleMessage[] = [
        ...(systemPrompt ? [{ role: "system" as const, text: systemPrompt, ts: 0 }] : []),
        ...messages.map((m) => ({ role: m.role as "user" | "terrance", text: m.text, ts: m.ts })),
        { role: "user" as const, text, ts: Date.now() },
      ];
      const compression = await compressIfNeeded(compressible);

      // Build the full prompt from (possibly compressed) history
      const contextMessages = compression.messages;
      const fullText = contextMessages.map((m) => {
        if (m.role === "system") return m.text;
        return `${m.role === "user" ? "User" : "Terrance"}: ${m.text}`;
      }).join("\n\n---\n\n");

      const resp = await chatWithRouter(fullText, cfg, [], undefined, anthropicApiKey, gatewayUrl, gatewayAuthToken);
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
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setSummarizing(false);
    }
  }, [messages, summarizing]);

  const computeMode = routerConfig?.mode ?? "hybrid";
  const canSummarize = messages.length >= 4 && !summarizing;

  return (
    <div className="panel p-4 space-y-2.5 flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h3 className="heading-section">Terrance Interface</h3>
          <span className="badge-status bg-accent/12 text-accent/80 border border-accent/20">
            {computeMode}
          </span>
        </div>
        {autoSaved ? (
          <span className="flex items-center gap-1 text-[10px] font-mono text-muted-foreground/45 uppercase tracking-wider">
            <BookOpen size={9} />
            Saved
          </span>
        ) : canSummarize && (
          <button
            onClick={handleSummarize}
            disabled={summarizing}
            className="flex items-center gap-1 text-[10px] font-mono text-muted-foreground/50 hover:text-accent/75 transition-colors uppercase tracking-wider"
            title="Summarize and store this session to memory"
          >
            {summarizing ? (
              <Loader2 size={9} className="animate-spin" />
            ) : (
              <BookOpen size={9} />
            )}
            Save
          </button>
        )}
      </div>

      {/* Messages */}
      <div
        ref={scrollRef}
        className="h-[280px] overflow-y-auto space-y-2 pr-1"
      >
        {messages.length === 0 && !sending && (
          <div className="flex flex-col items-center justify-center h-full gap-2">
            <Bot size={20} className="text-muted-foreground/30" />
            <p className="text-[10px] font-mono text-muted-foreground/45 uppercase tracking-wider">
              Awaiting input
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
                "w-5 h-5 rounded flex items-center justify-center shrink-0 mt-0.5",
                msg.role === "user" ? "bg-muted/25" : "bg-accent/15",
              )}
            >
              {msg.role === "user" ? (
                <User size={10} className="text-muted-foreground/65" />
              ) : (
                <Bot size={10} className="text-accent/80" />
              )}
            </div>
            <div
              className={cn(
                "px-2.5 py-1.5 rounded text-[11px] font-mono leading-relaxed",
                msg.role === "user"
                  ? "bg-muted/15 border border-border/30"
                  : "bg-accent/8 border border-accent/18",
              )}
            >
              <p className="whitespace-pre-wrap break-words">{msg.text}</p>
              <div className="flex items-center gap-2 mt-1">
                <p className="text-[10px] text-muted-foreground/40">
                  {new Date(msg.ts).toLocaleTimeString([], {
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </p>
                {msg.meta && (
                  <p className="text-[10px] text-muted-foreground/35">
                    {formatProvider(msg.meta.provider)} / {msg.meta.model.split(":")[0]}
                    {msg.meta.estimated_cost_usd > 0 && ` / ${formatCost(msg.meta.estimated_cost_usd)}`}
                  </p>
                )}
              </div>
            </div>
          </div>
        ))}

        {/* Typing indicator */}
        {sending && (
          <div className="flex gap-2 max-w-[88%]">
            <div className="w-5 h-5 rounded flex items-center justify-center shrink-0 mt-0.5 bg-accent/15">
              <Bot size={10} className="text-accent/80" />
            </div>
            <div className="px-2.5 py-2 rounded bg-accent/5 border border-accent/18">
              <div className="flex gap-1 items-center">
                <span className="w-1 h-1 rounded-full bg-accent/45 animate-pulse [animation-delay:0ms]" />
                <span className="w-1 h-1 rounded-full bg-accent/45 animate-pulse [animation-delay:200ms]" />
                <span className="w-1 h-1 rounded-full bg-accent/45 animate-pulse [animation-delay:400ms]" />
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Error */}
      {error && (
        <div className="flex items-start gap-1.5 px-2.5 py-1.5 rounded-sm bg-destructive/8 border border-destructive/20 text-destructive/80 text-[10px] font-mono">
          <AlertTriangle size={10} className="mt-0.5 flex-shrink-0" />
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
          className="flex-1 px-2.5 py-1.5 rounded-sm bg-[#0C1018] border border-border/45 text-xs font-mono placeholder:text-muted-foreground/35 focus:outline-none focus:border-accent/40 transition-colors disabled:opacity-40"
        />
        <button
          onClick={handleSend}
          disabled={!input.trim() || sending}
          className={cn(
            "px-2.5 py-1.5 rounded-sm transition-colors flex items-center gap-1",
            input.trim() && !sending
              ? "bg-accent/15 text-accent/85 hover:bg-accent/25"
              : "bg-muted/8 text-muted-foreground/35 cursor-not-allowed",
          )}
        >
          {sending ? (
            <Loader2 size={12} className="animate-spin" />
          ) : (
            <Send size={12} />
          )}
        </button>
      </div>
    </div>
  );
}

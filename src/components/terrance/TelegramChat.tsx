import { useState, useEffect, useRef } from "react";
import { Send, RefreshCw, MessageCircle, Bot, User, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { getTelegramMessages, sendTelegramMessage, getConfig } from "@/lib/api";
import type { TelegramMessage } from "@/lib/types";

export function TelegramChat() {
  const [messages, setMessages] = useState<TelegramMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [sending, setSending] = useState(false);
  const [configured, setConfigured] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  const loadMessages = async () => {
    setLoading(true);
    try {
      const msgs = await getTelegramMessages(50);
      setMessages(msgs);
    } catch {
      // silently fail — likely not configured
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    getConfig().then((cfg) => {
      const hasToken = !!cfg.telegram_bot_token;
      const hasChatId = !!cfg.telegram_chat_id;
      setConfigured(hasToken && hasChatId);
      if (hasToken && hasChatId) {
        loadMessages();
      }
    });
  }, []);

  useEffect(() => {
    if (!configured) return;
    const interval = setInterval(loadMessages, 10000);
    return () => clearInterval(interval);
  }, [configured]);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || sending) return;

    setSending(true);
    setInput("");
    try {
      const msg = await sendTelegramMessage(text);
      setMessages((prev) => [...prev, msg]);
      // Refresh to get Terrance's response after a short delay
      setTimeout(loadMessages, 3000);
    } catch (e) {
      setInput(text);
    } finally {
      setSending(false);
    }
  };

  if (!configured) {
    return (
      <div className="glass p-6 space-y-3">
        <div className="flex items-center gap-2">
          <MessageCircle size={16} className="text-blue-400" />
          <h3 className="text-base font-medium text-muted-foreground">
            Terrance — Telegram
          </h3>
        </div>
        <div className="flex flex-col items-center justify-center py-8 gap-3">
          <Bot size={32} className="text-muted-foreground/30" />
          <p className="text-xs text-muted-foreground/60 text-center max-w-[280px]">
            Connect your Telegram bot to chat with Terrance here.
            Go to <span className="text-accent">Settings</span> to add your bot token and chat ID.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="glass p-6 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <MessageCircle size={16} className="text-blue-400" />
          <h3 className="text-base font-medium text-muted-foreground">
            Terrance — Telegram
          </h3>
          <span className="text-xs px-2 py-0.5 rounded-md font-medium bg-blue-500/10 text-blue-400">
            Live
          </span>
        </div>
        <button
          onClick={loadMessages}
          disabled={loading}
          className="p-1.5 rounded-lg text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-colors"
        >
          <RefreshCw size={14} className={cn(loading && "animate-spin")} />
        </button>
      </div>

      {/* Messages */}
      <div
        ref={scrollRef}
        className="h-[320px] overflow-y-auto space-y-2 pr-1"
      >
        {messages.length === 0 && !loading && (
          <div className="flex items-center justify-center h-full">
            <p className="text-xs text-muted-foreground/40">
              No messages yet. Send one below.
            </p>
          </div>
        )}
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={cn(
              "flex gap-2 max-w-[85%]",
              msg.is_outgoing ? "ml-auto flex-row-reverse" : "",
            )}
          >
            <div
              className={cn(
                "w-6 h-6 rounded-full flex items-center justify-center shrink-0 mt-0.5",
                msg.is_outgoing
                  ? "bg-accent/20"
                  : "bg-blue-500/20",
              )}
            >
              {msg.is_outgoing ? (
                <Bot size={12} className="text-accent" />
              ) : (
                <User size={12} className="text-blue-400" />
              )}
            </div>
            <div
              className={cn(
                "px-3 py-2 rounded-xl text-xs leading-relaxed",
                msg.is_outgoing
                  ? "bg-accent/10 border border-accent/20 rounded-tr-sm"
                  : "bg-muted/20 border border-border/20 rounded-tl-sm",
              )}
            >
              <p className="whitespace-pre-wrap break-words">{msg.text}</p>
              <p className="text-[13px] text-muted-foreground/40 mt-1">
                {new Date(msg.date * 1000).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </p>
            </div>
          </div>
        ))}
      </div>

      {/* Input */}
      <div className="flex gap-2">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSend()}
          placeholder="Message Terrance..."
          className="flex-1 px-3 py-2 rounded-lg bg-muted/20 border border-border/30 text-sm placeholder:text-muted-foreground/30 focus:outline-none focus:border-accent/40 transition-colors"
        />
        <button
          onClick={handleSend}
          disabled={!input.trim() || sending}
          className={cn(
            "px-3 py-2 rounded-lg transition-colors flex items-center gap-1.5",
            input.trim()
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

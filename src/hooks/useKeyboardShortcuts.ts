import { useEffect } from "react";
import { useNexus } from "@/context/NexusContext";
import type { PageId } from "../../src-console/legacy-core/lib/types";

// Mirrors the Amara OS 6-item nav (src-console/app/routes.ts SYSTEMS): Home, Chat & Agents,
// Fitness, Neuro, Business, Settings. The legacy app has no "chat" page, so ⌘2 opens its
// "assistant" page; trajectory and revenue stay reachable from their own pages.
const pageMap: Record<string, PageId> = {
  "1": "terrance",
  "2": "assistant",
  "3": "fitness",
  "4": "neuro",
  "5": "business",
  "6": "settings",
};

export function useKeyboardShortcuts() {
  const { navigate, setCommandPaletteOpen } = useNexus();

  useEffect(() => {
    function handler(e: KeyboardEvent) {
      // Skip when typing in inputs
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

      if (!e.metaKey && !e.ctrlKey) return;

      // Cmd+K — toggle command palette
      if (e.key === "k") {
        e.preventDefault();
        setCommandPaletteOpen((prev: boolean) => !prev);
        return;
      }

      // Cmd+, — settings
      if (e.key === ",") {
        e.preventDefault();
        navigate("settings");
        return;
      }

      // Cmd+R — refresh (prevent browser default)
      if (e.key === "r") {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("nexus:refresh"));
        return;
      }

      // Cmd+1-6 — page navigation (same order as the Amara OS sidebar)
      if (e.key in pageMap) {
        e.preventDefault();
        navigate(pageMap[e.key]);
      }
    }

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [navigate, setCommandPaletteOpen]);
}

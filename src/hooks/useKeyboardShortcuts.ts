import { useEffect } from "react";
import { useNexus } from "@/context/NexusContext";
import type { PageId } from "@/lib/types";

const pageMap: Record<string, PageId> = {
  "1": "terrance",
  "2": "business",
  "3": "assistant",
  "4": "neuro",
  "5": "fitness",
  "6": "trajectory",
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

      // Cmd+1-6 — page navigation
      if (e.key in pageMap) {
        e.preventDefault();
        navigate(pageMap[e.key]);
      }
    }

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [navigate, setCommandPaletteOpen]);
}

import {
  createContext,
  useContext,
  useState,
  useCallback,
  useRef,
  useEffect,
  type ReactNode,
} from "react";
import type {
  PageId,
  ToastItem,
  ToastOptions,
  MuseConnectionState,
  WhoopConnectionState,
} from "@/lib/types";

// ============================================================
// Context shape
// ============================================================

interface NexusContextValue {
  // Navigation
  activePage: PageId;
  navigate: (page: PageId) => void;

  // BLE device states
  museState: MuseConnectionState;
  setMuseState: (state: MuseConnectionState) => void;
  whoopState: WhoopConnectionState;
  setWhoopState: (state: WhoopConnectionState) => void;

  // Toast
  toasts: ToastItem[];
  toast: (options: ToastOptions) => void;
  dismissToast: (id: string) => void;

  // Command palette
  commandPaletteOpen: boolean;
  setCommandPaletteOpen: (open: boolean | ((prev: boolean) => boolean)) => void;
}

const NexusContext = createContext<NexusContextValue | null>(null);

// ============================================================
// Provider
// ============================================================

const MAX_TOASTS = 5;

let toastCounter = 0;

export function NexusProvider({ children }: { children: ReactNode }) {
  // Navigation
  const [activePage, setActivePage] = useState<PageId>("terrance");

  const navigate = useCallback((page: PageId) => {
    setActivePage(page);
  }, []);

  // BLE states
  const [museState, setMuseState] = useState<MuseConnectionState>({
    connected: false,
    device_name: null,
    streaming: false,
  });
  const [whoopState, setWhoopState] = useState<WhoopConnectionState>({
    connected: false,
    device_name: null,
    streaming: false,
  });

  // Toasts
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const toastTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  const dismissToast = useCallback((id: string) => {
    const timer = toastTimers.current.get(id);
    if (timer) {
      clearTimeout(timer);
      toastTimers.current.delete(id);
    }
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const toast = useCallback(
    (options: ToastOptions) => {
      const id = `toast-${++toastCounter}`;
      const item: ToastItem = { ...options, id, timestamp: Date.now() };
      setToasts((prev) => [...prev.slice(-(MAX_TOASTS - 1)), item]);

      const duration = options.duration ?? 4000;
      const timer = setTimeout(() => {
        dismissToast(id);
      }, duration);
      toastTimers.current.set(id, timer);
    },
    [dismissToast],
  );

  // Command palette
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);

  return (
    <NexusContext.Provider
      value={{
        activePage,
        navigate,
        museState,
        setMuseState,
        whoopState,
        setWhoopState,
        toasts,
        toast,
        dismissToast,
        commandPaletteOpen,
        setCommandPaletteOpen,
      }}
    >
      {children}
    </NexusContext.Provider>
  );
}

// ============================================================
// Hook
// ============================================================

export function useNexus() {
  const ctx = useContext(NexusContext);
  if (!ctx) throw new Error("useNexus must be used within NexusProvider");
  return ctx;
}

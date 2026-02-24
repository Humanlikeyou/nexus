import {
  createContext,
  useContext,
  useState,
  useCallback,
  useRef,
  type ReactNode,
} from "react";
import type {
  PageId,
  AppNotification,
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

  // Notifications
  notifications: AppNotification[];
  addNotification: (n: Omit<AppNotification, "id" | "timestamp" | "read">) => void;
  markRead: (id: string) => void;
  markAllRead: () => void;
  clearNotifications: () => void;

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

const MAX_NOTIFICATIONS = 50;
const MAX_TOASTS = 5;
const NOTIF_STORAGE_KEY = "nexus:notifications";

let notifCounter = 0;
let toastCounter = 0;

function loadStoredNotifications(): AppNotification[] {
  try {
    const raw = localStorage.getItem(NOTIF_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function persistNotifications(notifications: AppNotification[]) {
  try {
    localStorage.setItem(NOTIF_STORAGE_KEY, JSON.stringify(notifications.slice(0, MAX_NOTIFICATIONS)));
  } catch {
    // localStorage quota exceeded or unavailable — non-fatal
  }
}

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

  // Notifications — persisted to localStorage
  const [notifications, setNotifications] = useState<AppNotification[]>(() => loadStoredNotifications());

  const addNotification = useCallback(
    (n: Omit<AppNotification, "id" | "timestamp" | "read">) => {
      const notif: AppNotification = {
        ...n,
        id: `notif-${++notifCounter}`,
        timestamp: Date.now(),
        read: false,
      };
      setNotifications((prev) => {
        const updated = [notif, ...prev].slice(0, MAX_NOTIFICATIONS);
        persistNotifications(updated);
        return updated;
      });
    },
    [],
  );

  const markRead = useCallback((id: string) => {
    setNotifications((prev) => {
      const updated = prev.map((n) => (n.id === id ? { ...n, read: true } : n));
      persistNotifications(updated);
      return updated;
    });
  }, []);

  const markAllRead = useCallback(() => {
    setNotifications((prev) => {
      const updated = prev.map((n) => ({ ...n, read: true }));
      persistNotifications(updated);
      return updated;
    });
  }, []);

  const clearNotifications = useCallback(() => {
    localStorage.removeItem(NOTIF_STORAGE_KEY);
    setNotifications([]);
  }, []);

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
        notifications,
        addNotification,
        markRead,
        markAllRead,
        clearNotifications,
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

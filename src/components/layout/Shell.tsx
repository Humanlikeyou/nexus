import { type ReactNode } from "react";
import { Sidebar } from "./Sidebar";
import { Header } from "./Header";
import { ErrorBoundary } from "@/components/ui/ErrorBoundary";
import { useNexus } from "@/context/NexusContext";
import type { PageId } from "@/lib/types";

interface ShellProps {
  children: (activePage: PageId) => ReactNode;
  terranceOnline?: boolean;
  onRefresh?: () => void;
}

export function Shell({ children, terranceOnline, onRefresh }: ShellProps) {
  const { activePage, navigate } = useNexus();

  return (
    <div className="relative flex h-screen overflow-hidden bg-background">
      {/* Ambient glow orbs */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute top-0 left-0 w-[500px] h-[400px] rounded-full opacity-30"
        style={{
          background: "radial-gradient(ellipse, oklch(0.68 0.20 195 / 0.12) 0%, transparent 70%)",
          filter: "blur(40px)",
          transform: "translate(-30%, -30%)",
        }}
      />
      <div
        aria-hidden="true"
        className="pointer-events-none absolute bottom-0 right-0 w-[600px] h-[500px] rounded-full opacity-25"
        style={{
          background: "radial-gradient(ellipse, oklch(0.62 0.22 285 / 0.10) 0%, transparent 70%)",
          filter: "blur(48px)",
          transform: "translate(20%, 20%)",
        }}
      />

      {/* Layout */}
      <Sidebar activePage={activePage} onNavigate={navigate} />
      <div className="relative z-10 flex flex-col flex-1 overflow-hidden">
        <Header
          activePage={activePage}
          terranceOnline={terranceOnline}
          onRefresh={onRefresh}
        />
        <main className="flex-1 overflow-y-auto p-7">
          <ErrorBoundary>{children(activePage)}</ErrorBoundary>
        </main>
      </div>
    </div>
  );
}

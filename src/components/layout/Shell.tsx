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
      {/* Sidebar */}
      <Sidebar activePage={activePage} onNavigate={navigate} />

      {/* Main content area */}
      <div className="relative z-10 flex flex-col flex-1 overflow-hidden">
        <Header
          activePage={activePage}
          terranceOnline={terranceOnline}
          onRefresh={onRefresh}
        />
        <main className="flex-1 overflow-y-auto px-4 py-3">
          <ErrorBoundary>{children(activePage)}</ErrorBoundary>
        </main>
      </div>
    </div>
  );
}

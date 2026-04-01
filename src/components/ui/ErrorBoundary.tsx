import { Component, type ReactNode } from "react";
import { AlertTriangle, RotateCcw, Home } from "lucide-react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  handleReload = () => {
    this.setState({ hasError: false, error: null });
  };

  handleGoHome = () => {
    this.setState({ hasError: false, error: null });
    // Navigation will reset to default page on re-render
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex items-center justify-center min-h-[400px]">
          <div className="glass p-8 max-w-md w-full text-center space-y-5">
            <div className="w-12 h-12 rounded-full bg-destructive/15 flex items-center justify-center mx-auto">
              <AlertTriangle size={24} className="text-destructive" />
            </div>
            <div>
              <h3 className="text-lg font-semibold">Something went wrong</h3>
              <p className="text-sm text-muted-foreground mt-1">
                This page encountered an error. Your data is safe.
              </p>
            </div>
            {this.state.error && (
              <div className="px-3 py-2 rounded-lg bg-muted/15 border border-border/25">
                <p className="text-xs text-muted-foreground font-mono break-all">
                  {this.state.error.message}
                </p>
              </div>
            )}
            <div className="flex items-center justify-center gap-3">
              <button
                onClick={this.handleReload}
                className="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent/15 text-accent text-sm hover:bg-accent/25 transition-colors"
              >
                <RotateCcw size={14} />
                Reload
              </button>
              <button
                onClick={this.handleGoHome}
                className="flex items-center gap-2 px-4 py-2 rounded-lg bg-muted/20 text-muted-foreground text-sm hover:bg-muted/30 transition-colors"
              >
                <Home size={14} />
                Go Home
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

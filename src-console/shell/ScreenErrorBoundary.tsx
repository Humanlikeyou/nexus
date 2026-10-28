import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  /** Changing this (e.g. the active screen id) clears a previous error. */
  resetKey?: unknown;
  /** Called before re-rendering on Retry (e.g. drop cached failed chunks). */
  onRetry?: () => void;
}
interface State { error: unknown; resetKey: unknown }

/**
 * Keeps one failed screen (usually a lazy chunk that didn't load) from
 * blanking the app: shows a quiet inline message with Retry. Not modal.
 */
export class ScreenErrorBoundary extends Component<Props, State> {
  state: State = { error: null, resetKey: this.props.resetKey };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error ?? new Error("screen failed") };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.resetKey !== state.resetKey ? { error: null, resetKey: props.resetKey } : null;
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error("[nexus] screen failed to load", error, info.componentStack);
  }

  retry = () => {
    this.props.onRetry?.();
    this.setState({ error: null });
  };

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="screen-error" role="status">
        <p>Couldn’t load this screen.</p>
        <button type="button" className="screen-error-retry" onClick={this.retry}>Retry</button>
      </div>
    );
  }
}

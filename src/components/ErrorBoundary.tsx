import { Component, type ErrorInfo, type ReactNode } from "react";
import { Ms } from "./Icons";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * Last line of defence around the screen area. A render error in a Tauri
 * window leaves a blank app with no address bar to reload from, so catch it,
 * say what broke and offer the reload the window itself can't.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("render error:", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="content narrow">
        <div className="hs-empty" role="alert">
          <Ms name="error" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>This screen crashed.</p>
          <span className="hs-empty-sub">{error.message || String(error)}</span>
          <button
            type="button"
            className="hs-chip primary"
            onClick={() => location.reload()}
          >
            Reload
          </button>
        </div>
      </div>
    );
  }
}

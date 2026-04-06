import { Component, ErrorInfo, ReactNode } from "react";
import { appLog } from "../log";

type Props = {
  children: ReactNode;
};

type State = {
  hasError: boolean;
  message: string | null;
};

export class AppErrorBoundary extends Component<Props, State> {
  state: State = {
    hasError: false,
    message: null,
  };

  static getDerivedStateFromError(error: Error): State {
    return {
      hasError: true,
      message: error.message || "Unknown UI error",
    };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    appLog.error("ui", `Unhandled render error: ${error.message}`);
    if (errorInfo.componentStack) {
      appLog.error("ui", errorInfo.componentStack);
    }
  }

  render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    return (
      <main
        style={{
          minHeight: "100vh",
          display: "grid",
          placeItems: "center",
          padding: "32px",
          background: "linear-gradient(180deg, #f8fafc 0%, #eef2ff 100%)",
        }}
      >
        <div
          style={{
            width: "min(560px, 100%)",
            background: "rgba(255, 255, 255, 0.96)",
            border: "1px solid rgba(148, 163, 184, 0.35)",
            borderRadius: "18px",
            padding: "24px",
            boxShadow: "0 24px 60px rgba(15, 23, 42, 0.12)",
          }}
        >
          <h1 style={{ margin: "0 0 8px", fontSize: "1.2rem" }}>Ghost Protocol hit a UI error</h1>
          <p style={{ margin: "0 0 16px", color: "#475569", lineHeight: 1.5 }}>
            The session view crashed, but the app is still running. Reloading should recover the interface.
          </p>
          {this.state.message && (
            <pre
              style={{
                margin: "0 0 16px",
                padding: "12px",
                borderRadius: "12px",
                background: "#0f172a",
                color: "#e2e8f0",
                overflowX: "auto",
                whiteSpace: "pre-wrap",
              }}
            >
              {this.state.message}
            </pre>
          )}
          <button
            className="btn-primary"
            onClick={() => window.location.reload()}
            style={{ fontSize: "0.9rem", padding: "8px 14px" }}
          >
            Reload App
          </button>
        </div>
      </main>
    );
  }
}

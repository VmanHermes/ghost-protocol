import { useEffect, useState } from "react";
import type { TerminalSession, SessionMode } from "../types";
import type { ChatSessionMeta } from "../hooks/useChatSocket";

type Props = {
  session: TerminalSession;
  mode: SessionMode;
  meta: ChatSessionMeta | null;
  onSwitchMode: (mode: SessionMode) => void;
  onOpenCompanionTerminal: () => void;
  onReopenSession: () => void;
  onEndSession: () => void;
};

function formatDuration(startedAt: string | null | undefined): string {
  if (!startedAt) return "";
  const ms = Date.now() - new Date(startedAt).getTime();
  const secs = Math.floor(ms / 1000);
  const mins = Math.floor(secs / 60);
  const hrs = Math.floor(mins / 60);
  if (hrs > 0) return `${hrs}h ${mins % 60}m`;
  if (mins > 0) return `${mins}m ${secs % 60}s`;
  return `${secs}s`;
}

function formatTokens(tokens: number | null): string {
  if (tokens == null) return "";
  if (tokens >= 1000) return `${(tokens / 1000).toFixed(1)}k tokens`;
  return `${tokens} tokens`;
}

function formatSessionOrigin(session: TerminalSession): string {
  if (session.hostId) {
    return `remote · ${session.hostName ?? "mesh"}`;
  }
  return "local";
}

export function SessionHeader({
  session,
  mode,
  meta,
  onSwitchMode,
  onOpenCompanionTerminal,
  onReopenSession,
  onEndSession,
}: Props) {
  const [duration, setDuration] = useState(formatDuration(session.startedAt));
  useEffect(() => {
    if (session.status !== "running") return;
    const interval = setInterval(() => setDuration(formatDuration(session.startedAt)), 1000);
    return () => clearInterval(interval);
  }, [session.startedAt, session.status]);

  const statusColor = session.status === "running" ? "var(--accent-green)" : session.status === "error" ? "var(--accent-red)" : "var(--text-muted)";
  const contextPct = meta?.contextPct;
  const contextWarning = contextPct != null && contextPct > 80;
  const capabilities = session.capabilities ?? [];
  const isLive = session.status === "running" || session.status === "created";
  const isStructuredChat = session.driverKind === "structured_chat_driver" || session.driverKind === "api_driver";
  const canChat = isStructuredChat || capabilities.includes("supports_chat_view");
  const canTerminal = !isStructuredChat && capabilities.includes("supports_terminal_view");
  const canSafeSwitch = !isStructuredChat && capabilities.includes("supports_safe_mode_switch") && canChat && canTerminal;
  const canOpenCompanionTerminal = isLive && isStructuredChat && !!session.agentId;

  return (
    <div className="session-header">
      <div className="session-header-info">
        <span className="status-dot" style={{ background: statusColor }} />
        <span className="session-header-name">{session.name ?? "Shell"}</span>
        <span className="muted" style={{ fontSize: "0.82rem" }}>{session.workdir}</span>
        <span className="muted" style={{ fontSize: "0.78rem" }}>· {formatSessionOrigin(session)}</span>
        {session.parentSessionId && <span className="session-delegated-badge">Delegated</span>}
      </div>
      <div className="session-header-meta">
        {duration && <span className="session-meta-item">{duration}</span>}
        {meta?.tokens != null && <span className="session-meta-item">{formatTokens(meta.tokens)}</span>}
        {contextPct != null && (
          <span className={`session-meta-item ${contextWarning ? "session-context-warn" : ""}`}>
            <span className="session-context-bar">
              <span className="session-context-fill" style={{ width: `${Math.min(contextPct, 100)}%`, background: contextWarning ? "var(--accent-yellow)" : "var(--accent-blue)" }} />
            </span>
            {Math.round(contextPct)}%
          </span>
        )}
      </div>
      <div className="session-header-actions">
        {canSafeSwitch ? (
          <div className="session-mode-toggle">
            <button className={`session-mode-btn ${mode === "chat" ? "session-mode-active" : ""}`} onClick={() => onSwitchMode("chat")}>Chat</button>
            <button className={`session-mode-btn ${mode === "terminal" ? "session-mode-active" : ""}`} onClick={() => onSwitchMode("terminal")}>Terminal</button>
          </div>
        ) : (
          <div className="session-mode-toggle">
            {canChat && <button className={`session-mode-btn ${mode === "chat" ? "session-mode-active" : ""}`} disabled>Chat</button>}
            {canTerminal && <button className={`session-mode-btn ${mode === "terminal" ? "session-mode-active" : ""}`} disabled>Terminal</button>}
            {canOpenCompanionTerminal && (
              <button className="session-mode-btn" onClick={onOpenCompanionTerminal}>Open Companion Terminal</button>
            )}
          </div>
        )}
        <button className="btn-secondary" disabled title="code-server coming soon" style={{ opacity: 0.4, fontSize: "0.78rem", padding: "4px 10px" }}>Open IDE</button>
        {isLive ? (
          <button className="btn-secondary" onClick={onEndSession} style={{ fontSize: "0.78rem", padding: "4px 10px" }}>End Session</button>
        ) : (
          <button className="btn-secondary" onClick={onReopenSession} style={{ fontSize: "0.78rem", padding: "4px 10px" }}>Reopen Session</button>
        )}
      </div>
    </div>
  );
}

import type { TerminalSession } from "../types";
import { terminateCodeServerSession } from "../api";
import { openUrl } from "@tauri-apps/plugin-opener";

type Props = {
  session: TerminalSession;
  baseUrl: string;
  onRefresh: () => void;
};

export function CodeServerPanel({ session, baseUrl, onRefresh }: Props) {
  const isRunning = session.status === "running";
  const isTerminal = session.status === "terminated" || session.status === "exited";

  const handleOpenBrowser = async () => {
    if (session.url) {
      await openUrl(session.url);
    }
  };

  const handleTerminate = async () => {
    try {
      await terminateCodeServerSession(baseUrl, session.id);
      onRefresh();
    } catch (err) {
      console.error("Failed to terminate code-server:", err);
    }
  };

  const uptime = session.startedAt
    ? formatUptime(new Date(session.startedAt))
    : null;

  return (
    <div className="code-server-panel">
      <div className="code-server-panel-header">
        <div className="code-server-panel-icon">{"</>"}</div>
        <div>
          <div className="code-server-panel-title">{session.name ?? "code-server"}</div>
          <div className="code-server-panel-meta">
            {session.hostName && <span>{session.hostName} · </span>}
            <span>port {session.port}</span>
            {uptime && <span> · up {uptime}</span>}
          </div>
        </div>
        <div className="code-server-panel-status">
          <span
            className="status-dot"
            style={{
              background: isRunning
                ? "var(--accent-green)"
                : isTerminal
                  ? "var(--text-muted)"
                  : "var(--accent-blue)",
            }}
          />
          {session.status}
        </div>
      </div>

      <div className="code-server-panel-info">
        <div className="code-server-panel-row">
          <span className="label">Directory</span>
          <span className="value">{session.workdir}</span>
        </div>
        {session.url && (
          <div className="code-server-panel-row">
            <span className="label">URL</span>
            <span className="value" style={{ fontSize: "0.82rem", opacity: 0.7 }}>{session.url}</span>
          </div>
        )}
        <div className="code-server-panel-row">
          <span className="label">Type</span>
          <span className="value">{session.adopted ? "Adopted" : "Spawned by Ghost Protocol"}</span>
        </div>
      </div>

      <div className="code-server-panel-actions">
        {isRunning && session.url && (
          <button className="btn-primary" onClick={handleOpenBrowser}>
            Open in Browser
          </button>
        )}
        {isRunning && (
          <button className="btn-secondary btn-danger" onClick={handleTerminate}>
            Terminate
          </button>
        )}
        {isTerminal && (
          <div className="muted" style={{ fontSize: "0.82rem" }}>
            This code-server session has ended.
          </div>
        )}
      </div>
    </div>
  );
}

function formatUptime(started: Date): string {
  const diff = Date.now() - started.getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ${mins % 60}m`;
  return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}

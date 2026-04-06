import { useCallback, useRef, useState } from "react";
import type { TerminalSession } from "../types";

type Props = {
  activeSessions: TerminalSession[];
  previousSessions: TerminalSession[];
  activeSessionId: string | null;
  onSelectSession: (sessionId: string) => void;
};

const STATUS_BORDER_COLORS: Record<string, string> = {
  running: "var(--accent-green)", created: "var(--accent-green)",
  error: "var(--accent-red)", exited: "var(--border)", terminated: "var(--border)",
};
const STATUS_DOT_COLORS: Record<string, string> = {
  running: "var(--accent-green)", created: "var(--accent-blue)",
  error: "var(--accent-red)", exited: "var(--text-muted)", terminated: "var(--text-muted)",
};

function formatRelativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export function SessionSidebar({ activeSessions, previousSessions, activeSessionId, onSelectSession }: Props) {
  const [sidebarWidth, setSidebarWidth] = useState(260);
  const dragging = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    const handleMouseMove = (e: MouseEvent) => { if (dragging.current) setSidebarWidth(Math.max(200, Math.min(500, e.clientX))); };
    const handleMouseUp = () => { dragging.current = false; document.removeEventListener("mousemove", handleMouseMove); document.removeEventListener("mouseup", handleMouseUp); };
    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
  }, []);

  return (
    <div className="session-sidebar" style={{ width: sidebarWidth }}>
      <div className="session-sidebar-content">
        <div className="session-sidebar-section">
          <div className="session-sidebar-label">Active Sessions</div>
          {activeSessions.length === 0 && <div className="muted" style={{ fontSize: "0.78rem", padding: "8px 0" }}>No active sessions</div>}
          {activeSessions.map((session) => {
            const isSelected = session.id === activeSessionId;
            const borderColor = STATUS_BORDER_COLORS[session.status] ?? "var(--border)";
            const indent = session.parentSessionId ? 20 : 0;
            return (
              <div key={session.id} className={`session-card ${isSelected ? "session-card-selected" : ""}`}
                style={{ borderColor, marginLeft: indent }} onClick={() => onSelectSession(session.id)}>
                <div className="session-card-header">
                  <span className="status-dot" style={{ background: STATUS_DOT_COLORS[session.status] }} />
                  <span className="session-card-name">{session.name ?? "Shell"}</span>
                  <span className="session-card-type">{session.hostName ?? "local"}</span>
                </div>
                <div className="session-card-workdir">{session.workdir}</div>
              </div>
            );
          })}
        </div>
        {previousSessions.length > 0 && <div className="session-sidebar-separator" />}
        {previousSessions.length > 0 && (
          <div className="session-sidebar-section session-sidebar-previous">
            <div className="session-sidebar-label">Previous Sessions</div>
            {previousSessions.map((session) => (
              <div key={session.id} className="session-card session-card-previous" onClick={() => onSelectSession(session.id)}>
                <div className="session-card-header">
                  <span className="status-dot" style={{ background: "var(--text-muted)" }} />
                  <span className="session-card-name">{session.name ?? "Shell"}</span>
                  <span className="session-card-type">{session.finishedAt ? formatRelativeTime(session.finishedAt) : ""}</span>
                </div>
                <div className="session-card-workdir">{session.workdir}</div>
              </div>
            ))}
          </div>
        )}
      </div>
      <div className="session-sidebar-handle" onMouseDown={handleMouseDown} />
    </div>
  );
}

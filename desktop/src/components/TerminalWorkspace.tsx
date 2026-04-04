import { useEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTerminalSocket } from "../hooks/useTerminalSocket";
import { fmt } from "../api";
import type { TerminalSession } from "../types";

type Props = {
  baseUrl: string;
  sessions: TerminalSession[];
  activeSessionId: string | null;
  visible: boolean;
  onSelect: (sessionId: string) => void;
  onCreateSession: (mode: "agent" | "rescue_shell") => void;
  onSessionStatusChange: (session: TerminalSession) => void;
  onRefreshSessions: () => void;
  onKillSession: (sessionId: string) => void;
};

function formatUptime(createdAt: string | null | undefined): string {
  if (!createdAt) return "—";
  const ms = Date.now() - new Date(createdAt).getTime();
  const minutes = Math.floor(ms / 60000);
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

const ACTIVE_STATUSES = new Set(["created", "running"]);

const SESSION_DOT_COLORS: Record<string, string> = {
  running: "#10b981",
  created: "#3b82f6",
  exited: "#6b7280",
  terminated: "#ef4444",
  error: "#ef4444",
};

export function TerminalWorkspace({
  baseUrl,
  sessions,
  activeSessionId,
  visible,
  onSelect,
  onCreateSession,
  onSessionStatusChange,
  onRefreshSessions,
  onKillSession,
}: Props) {
  const terminalHostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [error, setError] = useState("");
  const [showPrevious, setShowPrevious] = useState(false);
  const dropdownRef = useRef<HTMLDivElement | null>(null);

  const activeSessions = useMemo(
    () => sessions.filter((s) => ACTIVE_STATUSES.has(s.status)),
    [sessions],
  );
  const inactiveSessions = useMemo(
    () => sessions.filter((s) => !ACTIVE_STATUSES.has(s.status)),
    [sessions],
  );

  const { sendInput, resize, terminate, sessionMeta } = useTerminalSocket({
    baseUrl,
    sessionId: activeSessionId,
    terminalRef,
    onSessionStatusChange,
    onError: setError,
  });

  const isTerminated = sessionMeta?.status === "terminated" || sessionMeta?.status === "exited" || sessionMeta?.status === "error";

  // Close dropdown on outside click
  useEffect(() => {
    if (!showPrevious) return;
    function handleClick(e: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setShowPrevious(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [showPrevious]);

  // Initialize xterm once
  useEffect(() => {
    if (!terminalHostRef.current || terminalRef.current) return;
    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: false,
      fontFamily: 'SFMono-Regular, Consolas, "Liberation Mono", Menlo, monospace',
      fontSize: 14,
      theme: {
        background: "#1a1f36",
        foreground: "#e2e8f0",
        cursor: "#93c5fd",
        green: "#10b981",
        blue: "#60a5fa",
        yellow: "#fbbf24",
        red: "#f87171",
        cyan: "#22d3ee",
        magenta: "#c084fc",
      },
      scrollback: 5000,
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(terminalHostRef.current);
    fitAddon.fit();
    terminal.onData((data) => sendInput(data, false));
    terminalRef.current = terminal;
    fitRef.current = fitAddon;
    resize(terminal.cols, terminal.rows);

    const handleResize = () => {
      fitAddon.fit();
      resize(terminal.cols, terminal.rows);
    };
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
    };
  }, [sendInput, resize]);

  // Re-fit when becoming visible
  useEffect(() => {
    if (!visible) return;
    const fitAddon = fitRef.current;
    const terminal = terminalRef.current;
    if (!fitAddon || !terminal) return;
    const timer = setTimeout(() => {
      fitAddon.fit();
      resize(terminal.cols, terminal.rows);
      terminal.focus();
    }, 50);
    return () => clearTimeout(timer);
  }, [visible, resize]);

  // Focus terminal when session changes
  useEffect(() => {
    if (!activeSessionId) return;
    const terminal = terminalRef.current;
    const fitAddon = fitRef.current;
    if (!terminal || !fitAddon) return;
    fitAddon.fit();
    resize(terminal.cols, terminal.rows);
    terminal.focus();
  }, [activeSessionId, resize]);

  const activeSession = useMemo(
    () => sessions.find((s) => s.id === activeSessionId) ?? null,
    [sessions, activeSessionId],
  );

  function handleCloseTab(e: React.MouseEvent, sessionId: string) {
    e.stopPropagation();
    onKillSession(sessionId);
  }

  return (
    <section className="terminal-workspace">
      {/* Header */}
      <div className="terminal-header">
        <div>
          <h2>Terminal</h2>
          <p className="muted">Agent execution environment</p>
        </div>
        <div className="terminal-header-actions">
          <div className="dropdown-wrapper" ref={dropdownRef}>
            <button
              className="btn-secondary"
              onClick={() => { onRefreshSessions(); setShowPrevious(!showPrevious); }}
            >
              Previous Sessions
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="6 9 12 15 18 9" />
              </svg>
            </button>
            {showPrevious && (
              <div className="dropdown-menu">
                {inactiveSessions.length === 0 && (
                  <div className="dropdown-empty">No previous sessions</div>
                )}
                {inactiveSessions.map((session) => (
                  <button
                    key={session.id}
                    className="dropdown-item"
                    onClick={() => { onSelect(session.id); setShowPrevious(false); }}
                  >
                    <span
                      className="terminal-tab-dot"
                      style={{ background: SESSION_DOT_COLORS[session.status] ?? "#6b7280" }}
                    />
                    <span className="dropdown-item-name">
                      {session.name || session.mode.replace("_", " ")}
                    </span>
                    <span className="dropdown-item-meta">
                      {session.status} · {fmt(session.finishedAt || session.createdAt)}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
          <button className="btn-primary" onClick={() => onCreateSession("agent")}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polygon points="5 3 19 12 5 21 5 3" />
            </svg>
            Run Agent
          </button>
        </div>
      </div>

      {/* Session tabs — active only */}
      <div className="terminal-tabs">
        {activeSessions.map((session) => (
          <button
            key={session.id}
            className={`terminal-tab ${session.id === activeSessionId ? "active" : ""}`}
            onClick={() => onSelect(session.id)}
          >
            <span
              className="terminal-tab-dot"
              style={{ background: SESSION_DOT_COLORS[session.status] ?? "#6b7280" }}
            />
            {session.name || session.mode.replace("_", " ")}
            <span
              className="terminal-tab-close"
              onClick={(e) => handleCloseTab(e, session.id)}
              title="Kill session"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </span>
          </button>
        ))}
        <button className="terminal-tab-add" onClick={() => onCreateSession("rescue_shell")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </button>
      </div>

      {error ? <div className="error-banner">{error}</div> : null}

      {/* Terminal area */}
      <div className="terminal-main">
        <div ref={terminalHostRef} className="terminal-host" />
      </div>

      {/* Status bar */}
      <div className="terminal-statusbar">
        <div className="terminal-statusbar-info">
          <span>Session: {activeSession?.name || activeSession?.mode || "none"}</span>
          <span className="terminal-statusbar-sep" />
          <span>Uptime: {formatUptime(activeSession?.startedAt)}</span>
          <span className="terminal-statusbar-sep" />
          <span>Memory: {activeSession ? "—" : "—"}</span>
        </div>
        <div className="terminal-statusbar-actions">
          {!isTerminated && activeSessionId && (
            <button className="terminal-stop-btn" onClick={terminate} title="Terminate session">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
              </svg>
            </button>
          )}
        </div>
      </div>
    </section>
  );
}

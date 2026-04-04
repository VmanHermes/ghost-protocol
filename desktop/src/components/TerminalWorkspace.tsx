import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTerminalSocket } from "../hooks/useTerminalSocket";
import { useLocalTerminal } from "../hooks/useLocalTerminal";
import type { HostConnection, LocalTerminalSession, SavedHost, TerminalSession } from "../types";

type RemoteSessionEntry = {
  hostId: string;
  hostName: string;
  session: TerminalSession;
};

type Props = {
  hosts: SavedHost[];
  hostConnections: Map<string, HostConnection>;
  localSessions: LocalTerminalSession[];
  allRemoteSessions: RemoteSessionEntry[];
  activeSessionId: string | null;
  visible: boolean;
  onSelect: (sessionId: string) => void;
  onCreateRemoteSession: (hostId: string, mode: "agent" | "rescue_shell" | "project") => void;
  onCreateLocalSession: () => void;
  onRemoteSessionStatusChange: (session: TerminalSession) => void;
  onLocalSessionStatusChange: (session: LocalTerminalSession) => void;
  onKillRemoteSession: (sessionId: string) => void;
  onKillLocalSession: (sessionId: string) => void;
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
  hosts,
  hostConnections,
  localSessions,
  allRemoteSessions,
  activeSessionId,
  visible,
  onSelect,
  onCreateRemoteSession,
  onCreateLocalSession,
  onRemoteSessionStatusChange,
  onLocalSessionStatusChange,
  onKillRemoteSession,
  onKillLocalSession,
}: Props) {
  const terminalHostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [error, setError] = useState("");
  const [showAddMenu, setShowAddMenu] = useState(false);
  const addMenuRef = useRef<HTMLDivElement | null>(null);

  const activeRemoteSessions = useMemo(
    () => allRemoteSessions.filter((e) => ACTIVE_STATUSES.has(e.session.status)),
    [allRemoteSessions],
  );
  const activeLocalSessions = useMemo(
    () => localSessions.filter((s) => s.status === "running"),
    [localSessions],
  );

  // Determine if the active session is local or remote
  const isLocalSession = useMemo(
    () => localSessions.some((s) => s.id === activeSessionId),
    [localSessions, activeSessionId],
  );

  const activeSession = useMemo(
    () => allRemoteSessions.find((e) => e.session.id === activeSessionId)?.session ?? null,
    [allRemoteSessions, activeSessionId],
  );

  const activeRemoteEntry = useMemo(
    () => allRemoteSessions.find((e) => e.session.id === activeSessionId) ?? null,
    [allRemoteSessions, activeSessionId],
  );
  const activeRemoteHost = activeRemoteEntry
    ? hosts.find((h) => h.id === activeRemoteEntry.hostId) ?? null
    : null;
  const activeBaseUrl = activeRemoteHost?.url ?? "http://localhost:8787";

  // Use both hooks — only the relevant one gets a sessionId
  const {
    sendInput: remoteSendInput,
    resize: remoteResize,
    terminate,
    sessionMeta: remoteSessionMeta,
  } = useTerminalSocket({
    baseUrl: activeBaseUrl,
    sessionId: isLocalSession ? null : activeSessionId,
    terminalRef,
    onSessionStatusChange: onRemoteSessionStatusChange,
    onError: setError,
  });

  const {
    sendInput: localSendInput,
    resize: localResize,
    kill: localKill,
    sessionMeta: localSessionMeta,
  } = useLocalTerminal({
    sessionId: isLocalSession ? activeSessionId : null,
    terminalRef,
    onSessionStatusChange: onLocalSessionStatusChange,
    onError: setError,
  });

  // Unified interface — wrap to normalize signatures
  const sendInput = isLocalSession
    ? (d: string) => localSendInput(d)
    : (d: string) => remoteSendInput(d, false);
  const resize = useCallback(
    (cols: number, rows: number) => (isLocalSession ? localResize : remoteResize)(cols, rows),
    [isLocalSession, localResize, remoteResize],
  );
  const sessionMeta = isLocalSession ? localSessionMeta : remoteSessionMeta;

  // Ref so xterm.onData always calls the current sendInput
  const sendInputRef = useRef(sendInput);
  useEffect(() => { sendInputRef.current = sendInput; }, [sendInput]);

  const isTerminated = sessionMeta?.status === "terminated" || sessionMeta?.status === "exited" || sessionMeta?.status === "error";

  // Close dropdown on outside click
  useEffect(() => {
    if (!showAddMenu) return;
    function handleClick(e: MouseEvent) {
      if (addMenuRef.current && !addMenuRef.current.contains(e.target as Node)) {
        setShowAddMenu(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [showAddMenu]);

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
    terminal.onData((data) => sendInputRef.current(data));
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
  }, [resize]);

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

  return (
    <section className="terminal-workspace">
      {/* Session tabs — local first, then remote */}
      <div className="terminal-tabs">
        {/* Local sessions */}
        {activeLocalSessions.map((session) => (
          <button
            key={session.id}
            className={`terminal-tab ${session.id === activeSessionId ? "active" : ""}`}
            onClick={() => onSelect(session.id)}
          >
            <span className="terminal-tab-dot" style={{ background: "#10b981" }} />
            <span className="terminal-tab-source">Local</span>
            {" · shell"}
            <span
              className="terminal-tab-close"
              onClick={(e) => { e.stopPropagation(); onKillLocalSession(session.id); }}
              title="Kill session"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </span>
          </button>
        ))}

        {/* Remote sessions */}
        {activeRemoteSessions.map((entry) => (
          <button
            key={entry.session.id}
            className={`terminal-tab ${entry.session.id === activeSessionId ? "active" : ""}`}
            onClick={() => onSelect(entry.session.id)}
          >
            <span
              className="terminal-tab-dot"
              style={{ background: SESSION_DOT_COLORS[entry.session.status] ?? "#6b7280" }}
            />
            <span className="terminal-tab-source">{entry.hostName}</span>
            {" · "}{entry.session.name || entry.session.mode.replace("_", " ")}
            <span
              className="terminal-tab-close"
              onClick={(e) => { e.stopPropagation(); onKillRemoteSession(entry.session.id); }}
              title="Kill session"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </span>
          </button>
        ))}

        {/* Add local session button */}
        <button className="terminal-tab-add" onClick={onCreateLocalSession} title="New local shell">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </button>

        {/* Remote session dropdown (only when hosts exist) */}
        {hosts.length > 0 && (
          <div className="terminal-add-wrapper" ref={addMenuRef}>
            <button className="terminal-remote-add" onClick={() => setShowAddMenu(!showAddMenu)} title="New remote session">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="6 9 12 15 18 9" />
              </svg>
            </button>
            {showAddMenu && (
              <div className="terminal-add-menu">
                {hosts.map((host) => {
                  const conn = hostConnections.get(host.id);
                  const isConnected = conn?.state === "connected";
                  return (
                    <div key={host.id} className="terminal-add-menu-group">
                      <div className={`terminal-add-menu-host ${isConnected ? "" : "disabled"}`}>
                        <span className={`status-dot ${conn?.state ?? "idle"}`} />
                        {host.name}
                        {!isConnected && " (unreachable)"}
                      </div>
                      {isConnected && (
                        <>
                          <button className="terminal-add-menu-item terminal-add-menu-sub" onClick={() => { onCreateRemoteSession(host.id, "rescue_shell"); setShowAddMenu(false); }}>rescue shell</button>
                          <button className="terminal-add-menu-item terminal-add-menu-sub" onClick={() => { onCreateRemoteSession(host.id, "agent"); setShowAddMenu(false); }}>agent</button>
                          <button className="terminal-add-menu-item terminal-add-menu-sub" onClick={() => { onCreateRemoteSession(host.id, "project"); setShowAddMenu(false); }}>project</button>
                        </>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </div>

      {error ? <div className="error-banner">{error}</div> : null}

      {/* Terminal area */}
      <div className="terminal-main">
        <div ref={terminalHostRef} className="terminal-host" />
      </div>

      {/* Status bar */}
      <div className="terminal-statusbar">
        <div className="terminal-statusbar-info">
          <span>
            {isLocalSession ? "Local" : activeRemoteEntry?.hostName ?? "Remote"}
            {" · "}
            {activeSession?.name || activeSession?.mode || (isLocalSession ? "shell" : "none")}
          </span>
          <span className="terminal-statusbar-sep" />
          <span>Uptime: {formatUptime(activeSession?.startedAt || localSessionMeta?.createdAt)}</span>
        </div>
        <div className="terminal-statusbar-actions">
          {!isTerminated && activeSessionId && (
            <button
              className="terminal-stop-btn"
              onClick={isLocalSession ? localKill : terminate}
              title="Terminate session"
            >
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

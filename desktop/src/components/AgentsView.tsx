import { useCallback, useEffect, useState } from "react";
import { listAgents, createChatSession, switchSessionMode } from "../api";
import { useChatSocket } from "../hooks/useChatSocket";
import { SessionSidebar } from "./SessionSidebar";
import { SessionHeader } from "./SessionHeader";
import { ChatRenderer } from "./ChatRenderer";
import { TerminalRenderer } from "./TerminalRenderer";
import type { AgentInfo, TerminalSession, SessionMode, LocalTerminalSession } from "../types";

type Props = {
  daemonUrl: string;
  sessions: TerminalSession[];
  localSessions: LocalTerminalSession[];
  visible: boolean;
  onCreateLocalSession: () => void;
  onRefreshSessions: () => void;
};

const LOCAL_DAEMON = "http://127.0.0.1:8787";

export function AgentsView({ daemonUrl, sessions, localSessions, visible, onCreateLocalSession, onRefreshSessions }: Props) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [selectedMode, setSelectedMode] = useState<SessionMode>("chat");
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [activeMode, setActiveMode] = useState<SessionMode>("chat");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listAgents(daemonUrl).then((a) => {
      setAgents(a);
      if (a.length > 0 && !selectedAgentId) setSelectedAgentId(a[0].id);
    }).catch(() => {});
  }, [daemonUrl]); // eslint-disable-line react-hooks/exhaustive-deps

  const activeSessions = sessions.filter((s) => s.status === "running" || s.status === "created");
  const previousSessions = sessions.filter((s) => s.status !== "running" && s.status !== "created");
  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null;
  const isLocalSession = localSessions.some((s) => s.id === activeSessionId);

  const chatSocket = useChatSocket({
    baseUrl: LOCAL_DAEMON,
    sessionId: activeMode === "chat" && activeSessionId && !isLocalSession ? activeSessionId : null,
    isActive: visible && activeMode === "chat" && !!activeSessionId && !isLocalSession,
    onError: setError,
  });

  const handleNewSession = useCallback(async () => {
    if (!selectedAgentId) return;
    setError(null);
    setLoading(true);
    try {
      if (selectedAgentId === "shell") {
        onCreateLocalSession();
        setActiveMode("terminal");
      } else if (selectedMode === "chat") {
        const result = await createChatSession(daemonUrl, selectedAgentId);
        const sessionId: string = result.session?.id ?? result.session;
        setActiveSessionId(sessionId);
        setActiveMode("chat");
        onRefreshSessions();
      } else {
        const resp = await fetch(`${daemonUrl}/api/terminal/sessions`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mode: "agent", agentId: selectedAgentId }),
        });
        const data = await resp.json();
        setActiveSessionId(data.id);
        setActiveMode("terminal");
        onRefreshSessions();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create session");
    } finally {
      setLoading(false);
    }
  }, [daemonUrl, selectedAgentId, selectedMode, onCreateLocalSession, onRefreshSessions]);

  const handleSwitchMode = useCallback(async (newMode: SessionMode) => {
    if (!activeSessionId || newMode === activeMode) return;
    setError(null);
    try {
      const result = await switchSessionMode(daemonUrl, activeSessionId, newMode);
      if (result.needsConfirmation) {
        const ok = window.confirm(result.warning ?? "Switching modes will end the current conversation. Continue?");
        if (!ok) return;
        await switchSessionMode(daemonUrl, activeSessionId, newMode, true);
      }
      setActiveMode(newMode);
      onRefreshSessions();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to switch mode");
    }
  }, [daemonUrl, activeSessionId, activeMode, onRefreshSessions]);

  const handleEndSession = useCallback(async () => {
    if (!activeSessionId) return;
    try {
      await fetch(`${daemonUrl}/api/terminal/sessions/${activeSessionId}/terminate`, { method: "POST" });
      setActiveSessionId(null);
      onRefreshSessions();
    } catch {} // eslint-disable-line no-empty
  }, [daemonUrl, activeSessionId, onRefreshSessions]);

  if (!visible) return null;

  return (
    <div className="agents-view">
      <div className="agents-topbar">
        <select value={selectedAgentId ?? ""} onChange={(e) => setSelectedAgentId(e.target.value || null)} disabled={agents.length === 0}>
          <option value="shell">Shell (local)</option>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>{a.name} {a.version ? `v${a.version}` : ""} ({a.agentType})</option>
          ))}
        </select>
        {selectedAgentId !== "shell" && (
          <div className="session-mode-toggle">
            <button className={`session-mode-btn ${selectedMode === "chat" ? "session-mode-active" : ""}`} onClick={() => setSelectedMode("chat")}>Chat</button>
            <button className={`session-mode-btn ${selectedMode === "terminal" ? "session-mode-active" : ""}`} onClick={() => setSelectedMode("terminal")}>Terminal</button>
          </div>
        )}
        <button className="btn-primary" onClick={() => void handleNewSession()} disabled={loading || !selectedAgentId} style={{ fontSize: "0.85rem", padding: "7px 16px" }}>
          {loading ? "Starting..." : "+ New Session"}
        </button>
        {error && <span style={{ color: "var(--accent-red)", fontSize: "0.78rem" }}>{error}</span>}
      </div>
      <div className="agents-main">
        <SessionSidebar activeSessions={activeSessions} previousSessions={previousSessions} activeSessionId={activeSessionId}
          onSelectSession={(id) => {
            setActiveSessionId(id);
            const session = sessions.find((s) => s.id === id);
            if (session) setActiveMode(session.mode === "chat" ? "chat" : "terminal");
          }}
        />
        <div className="agents-content">
          {activeSession ? (
            <>
              <SessionHeader session={activeSession} mode={activeMode} meta={activeMode === "chat" ? chatSocket.meta : null}
                onSwitchMode={handleSwitchMode} onEndSession={handleEndSession} />
              {activeMode === "chat" && !isLocalSession ? (
                <ChatRenderer messages={chatSocket.messages} streamingDelta={chatSocket.streamingDelta}
                  streamingMessageId={chatSocket.streamingMessageId} status={chatSocket.meta.status} onSendMessage={chatSocket.sendMessage} />
              ) : (
                <TerminalRenderer baseUrl={LOCAL_DAEMON} sessionId={activeSessionId} isLocal={isLocalSession} isActive={visible} onError={setError} />
              )}
            </>
          ) : (
            <div className="agents-empty">
              <p>Select a session or create a new one to get started.</p>
              {agents.length === 0 && <p className="muted">No agents detected. <a href="#" onClick={(e) => { e.preventDefault(); setSelectedAgentId("shell"); setSelectedMode("terminal"); }}>+ Set up an agent</a></p>}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

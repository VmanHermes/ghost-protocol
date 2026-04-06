import { useCallback, useEffect, useState } from "react";
import {
  createChatSession,
  createCompanionTerminal,
  listAgents,
  listProjects,
  reopenWorkSession,
  switchSessionMode,
} from "../api";
import { useChatSocket } from "../hooks/useChatSocket";
import { SessionSidebar } from "./SessionSidebar";
import { SessionHeader } from "./SessionHeader";
import { ChatRenderer } from "./ChatRenderer";
import { TerminalRenderer } from "./TerminalRenderer";
import type {
  AgentInfo,
  LocalTerminalSession,
  ProjectRecord,
  SessionMode,
  TerminalSession,
} from "../types";

type Props = {
  daemonUrl: string;
  sessions: TerminalSession[];
  localSessions: LocalTerminalSession[];
  activeSessionId: string | null;
  visible: boolean;
  onSelectSession: (sessionId: string | null) => void;
  onCreateLocalSession: (workdir?: string | null) => Promise<LocalTerminalSession | null | undefined>;
  onTerminateLocalSession: (sessionId: string) => Promise<void>;
  onLocalSessionStatusChange: (session: LocalTerminalSession) => void;
  onRefreshSessions: () => void;
};

const LOCAL_DAEMON = "http://127.0.0.1:8787";

export function AgentsView({
  daemonUrl,
  sessions,
  localSessions,
  activeSessionId,
  visible,
  onSelectSession,
  onCreateLocalSession,
  onTerminateLocalSession,
  onLocalSessionStatusChange,
  onRefreshSessions,
}: Props) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [projects, setProjects] = useState<ProjectRecord[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [selectedProjectId, setSelectedProjectId] = useState("");
  const [launchWorkdir, setLaunchWorkdir] = useState("~");
  const [selectedMode, setSelectedMode] = useState<SessionMode>("chat");
  const [activeMode, setActiveMode] = useState<SessionMode>("chat");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listAgents(daemonUrl)
      .then((items) => {
        setAgents(items);
        if (!selectedAgentId) {
          setSelectedAgentId(items[0]?.id ?? "shell");
        }
      })
      .catch(() => {});
  }, [daemonUrl]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    listProjects(daemonUrl).then(setProjects).catch(() => {});
  }, [daemonUrl]);

  useEffect(() => {
    const session = sessions.find((entry) => entry.id === activeSessionId);
    if (!session) return;

    setLaunchWorkdir(session.workdir || "~");
    setActiveMode(session.mode === "chat" ? "chat" : "terminal");

    const matchedProject = session.projectId
      ? projects.find((project) => project.id === session.projectId)
      : projects.find((project) => project.workdir === session.workdir);
    setSelectedProjectId(matchedProject?.id ?? "");
  }, [activeSessionId, projects, sessions]);

  const activeSessions = sessions.filter((session) => session.status === "running" || session.status === "created");
  const previousSessions = sessions.filter((session) => session.status !== "running" && session.status !== "created");
  const activeSession = sessions.find((session) => session.id === activeSessionId) ?? null;
  const isLocalSession = localSessions.some((session) => session.id === activeSessionId);

  const handleSessionRenamed = useCallback(() => {
    onRefreshSessions();
  }, [onRefreshSessions]);

  const chatSocket = useChatSocket({
    baseUrl: LOCAL_DAEMON,
    sessionId: activeMode === "chat" && activeSessionId && !isLocalSession ? activeSessionId : null,
    isActive: visible && activeMode === "chat" && !!activeSessionId && !isLocalSession,
    onError: setError,
    onSessionRenamed: handleSessionRenamed,
  });

  const handleProjectChange = useCallback((projectId: string) => {
    setSelectedProjectId(projectId);
    const project = projects.find((entry) => entry.id === projectId);
    if (project) {
      setLaunchWorkdir(project.workdir);
    }
  }, [projects]);

  const handleWorkdirChange = useCallback((value: string) => {
    setLaunchWorkdir(value);
    const matchedProject = projects.find((project) => project.workdir === value);
    setSelectedProjectId(matchedProject?.id ?? "");
  }, [projects]);

  const handleNewSession = useCallback(async () => {
    if (!selectedAgentId) return;

    setError(null);
    setLoading(true);

    try {
      const workdir = launchWorkdir.trim() || undefined;
      const projectId = selectedProjectId || undefined;

      if (selectedAgentId === "shell") {
        const session = await onCreateLocalSession(workdir);
        if (session?.id) {
          onSelectSession(session.id);
          setActiveMode("terminal");
        }
      } else if (selectedMode === "chat") {
        const result = await createChatSession(daemonUrl, selectedAgentId, projectId, workdir);
        const sessionId: string = result.session?.id ?? result.session;
        onSelectSession(sessionId);
        setActiveMode("chat");
        onRefreshSessions();
      } else {
        const resp = await fetch(`${daemonUrl}/api/terminal/sessions`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            mode: "agent",
            agentId: selectedAgentId,
            projectId,
            workdir,
          }),
        });
        const data = await resp.json();
        onSelectSession(data.id);
        setActiveMode("terminal");
        onRefreshSessions();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create session");
    } finally {
      setLoading(false);
    }
  }, [
    daemonUrl,
    launchWorkdir,
    onCreateLocalSession,
    onRefreshSessions,
    onSelectSession,
    selectedAgentId,
    selectedMode,
    selectedProjectId,
  ]);

  const handleSwitchMode = useCallback(async (newMode: SessionMode) => {
    if (!activeSessionId || newMode === activeMode) return;

    setError(null);

    try {
      let result = await switchSessionMode(daemonUrl, activeSessionId, newMode);
      if (result.needsConfirmation) {
        const ok = window.confirm(result.warning ?? "Switching modes will end the current conversation. Continue?");
        if (!ok) return;
        result = await switchSessionMode(daemonUrl, activeSessionId, newMode, true);
      }

      if (result.session?.id) {
        onSelectSession(result.session.id);
      }

      setActiveMode(newMode);
      onRefreshSessions();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to switch mode");
    }
  }, [activeMode, activeSessionId, daemonUrl, onRefreshSessions, onSelectSession]);

  const handleOpenCompanionTerminal = useCallback(async () => {
    if (!activeSessionId) return;

    setError(null);

    try {
      const session = await createCompanionTerminal(daemonUrl, activeSessionId);
      onSelectSession(session.id);
      setActiveMode("terminal");
      onRefreshSessions();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to open companion terminal");
    }
  }, [activeSessionId, daemonUrl, onRefreshSessions, onSelectSession]);

  const handleReopenSession = useCallback(async () => {
    if (!activeSessionId || !activeSession) return;

    setError(null);

    try {
      if (isLocalSession) {
        const reopened = await onCreateLocalSession(activeSession.workdir);
        if (reopened?.id) {
          onSelectSession(reopened.id);
          setActiveMode("terminal");
        }
        return;
      }

      if (activeSession.hostId && activeSession.hostName !== "local") {
        throw new Error("Reopen is currently only supported for local daemon sessions.");
      }

      const reopened = await reopenWorkSession(daemonUrl, activeSessionId);
      onSelectSession(reopened.id);
      setActiveMode(reopened.mode === "chat" ? "chat" : "terminal");
      onRefreshSessions();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to reopen session");
    }
  }, [
    activeSession,
    activeSessionId,
    daemonUrl,
    isLocalSession,
    onCreateLocalSession,
    onRefreshSessions,
    onSelectSession,
  ]);

  const handleEndSession = useCallback(async () => {
    if (!activeSessionId) return;

    try {
      if (isLocalSession) {
        await onTerminateLocalSession(activeSessionId);
      } else {
        await fetch(`${daemonUrl}/api/terminal/sessions/${activeSessionId}/terminate`, { method: "POST" });
        onRefreshSessions();
      }

      onSelectSession(null);
      setActiveMode("terminal");
    } catch {
      // ignore for now
    }
  }, [activeSessionId, daemonUrl, isLocalSession, onRefreshSessions, onSelectSession, onTerminateLocalSession]);

  if (!visible) return null;

  const workdirPlaceholder = projects[0]?.workdir ?? "~/projects/my-app";

  return (
    <div className="agents-view">
      <div className="agents-topbar">
        <select
          value={selectedAgentId ?? ""}
          onChange={(e) => setSelectedAgentId(e.target.value || null)}
        >
          <option value="shell">Shell (local)</option>
          {agents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name} {agent.version ? `v${agent.version}` : ""} ({agent.agentType})
            </option>
          ))}
        </select>

        {selectedAgentId !== "shell" && (
          <div className="session-mode-toggle">
            <button
              className={`session-mode-btn ${selectedMode === "chat" ? "session-mode-active" : ""}`}
              onClick={() => setSelectedMode("chat")}
            >
              Chat
            </button>
            <button
              className={`session-mode-btn ${selectedMode === "terminal" ? "session-mode-active" : ""}`}
              onClick={() => setSelectedMode("terminal")}
            >
              Terminal
            </button>
          </div>
        )}

        <button
          className="btn-primary"
          onClick={() => void handleNewSession()}
          disabled={loading || !selectedAgentId}
          style={{ fontSize: "0.85rem", padding: "7px 16px" }}
        >
          {loading ? "Starting..." : "+ New Session"}
        </button>

        {error && <span style={{ color: "var(--accent-red)", fontSize: "0.78rem" }}>{error}</span>}
      </div>

      <div className="agents-launchbar">
        <label className="agents-launch-field">
          <span>Project</span>
          <select value={selectedProjectId} onChange={(e) => handleProjectChange(e.target.value)}>
            <option value="">No project</option>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </select>
        </label>

        <label className="agents-launch-field agents-launch-field-grow">
          <span>Folder</span>
          <input
            type="text"
            value={launchWorkdir}
            onChange={(e) => handleWorkdirChange(e.target.value)}
            placeholder={workdirPlaceholder}
          />
        </label>
      </div>

      <div className="agents-main">
        <SessionSidebar
          activeSessions={activeSessions}
          previousSessions={previousSessions}
          activeSessionId={activeSessionId}
          onSelectSession={(sessionId) => {
            onSelectSession(sessionId);
            const session = sessions.find((entry) => entry.id === sessionId);
            if (session) {
              setActiveMode(session.mode === "chat" ? "chat" : "terminal");
            }
          }}
        />

        <div className="agents-content">
          {activeSession ? (
            <>
              <SessionHeader
                session={activeSession}
                mode={activeMode}
                meta={activeMode === "chat" ? chatSocket.meta : null}
                onSwitchMode={handleSwitchMode}
                onOpenCompanionTerminal={handleOpenCompanionTerminal}
                onReopenSession={handleReopenSession}
                onEndSession={handleEndSession}
              />

              {activeMode === "chat" && !isLocalSession ? (
                <ChatRenderer
                  messages={chatSocket.messages}
                  streamingDelta={chatSocket.streamingDelta}
                  streamingMessageId={chatSocket.streamingMessageId}
                  status={chatSocket.meta.status}
                  onSendMessage={chatSocket.sendMessage}
                />
              ) : (
                <TerminalRenderer
                  baseUrl={LOCAL_DAEMON}
                  sessionId={activeSessionId}
                  isLocal={isLocalSession}
                  isActive={visible}
                  interactive={activeSession.status === "running" || activeSession.status === "created"}
                  onSessionStatusChange={(session) => {
                    if (isLocalSession) {
                      onLocalSessionStatusChange(session as LocalTerminalSession);
                    } else {
                      onRefreshSessions();
                    }
                  }}
                  onError={setError}
                />
              )}
            </>
          ) : (
            <div className="agents-empty">
              <p>Select a session or create a new one to get started.</p>
              {agents.length === 0 && (
                <p className="muted">
                  No agents detected.{" "}
                  <a
                    href="#"
                    onClick={(e) => {
                      e.preventDefault();
                      setSelectedAgentId("shell");
                      setSelectedMode("terminal");
                    }}
                  >
                    + Set up an agent
                  </a>
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

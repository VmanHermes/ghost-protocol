import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
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
  HostConnection,
  LocalTerminalSession,
  ProjectRecord,
  SessionMode,
  TerminalSession,
} from "../types";

type Props = {
  daemonUrl: string;
  connections: HostConnection[];
  activeSessionBaseUrl: string;
  localHostName: string | null;
  localMachineIp: string | null;
  sessions: TerminalSession[];
  localSessions: LocalTerminalSession[];
  activeSessionId: string | null;
  visible: boolean;
  onSelectSession: (sessionId: string | null) => void;
  onCreateLocalSession: (workdir?: string | null) => Promise<LocalTerminalSession | null | undefined>;
  onTerminateLocalSession: (sessionId: string) => Promise<void>;
  onLocalSessionStatusChange: (session: LocalTerminalSession) => void;
  onRefreshSessions: () => Promise<void>;
};

export function AgentsView({
  daemonUrl,
  connections,
  activeSessionBaseUrl,
  localHostName,
  localMachineIp,
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
  const [selectedTargetId, setSelectedTargetId] = useState("local");
  const [activeMode, setActiveMode] = useState<SessionMode>("chat");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const activeSessions = sessions.filter((session) => session.status === "running" || session.status === "created");
  const previousSessions = sessions.filter((session) => session.status !== "running" && session.status !== "created");
  const activeSession = sessions.find((session) => session.id === activeSessionId) ?? null;
  const isLocalSession = localSessions.some((session) => session.id === activeSessionId);
  const activeSessionTargetId = activeSession
    ? (isLocalSession ? "local" : activeSession.hostId ?? "local")
    : null;
  const activeSessionWorkdir = activeSession?.workdir ?? "~";
  const activeSessionProjectId = activeSession?.projectId ?? "";
  const activeSessionViewMode = activeSession?.mode === "chat" ? "chat" : "terminal";

  const formatTargetLabel = useCallback((name: string, ip: string | null, isLocal: boolean) => {
    if (isLocal) {
      return ip ? `${name} (local · ${ip})` : `${name} (local)`;
    }
    return ip ? `${name} (${ip})` : `${name} (remote)`;
  }, []);

  const targetOptions = useMemo(() => [
    {
      id: "local",
      name: localHostName ?? "Local",
      baseUrl: daemonUrl,
      isLocal: true,
      ip: localMachineIp,
    },
    ...connections
      .filter((connection) => connection.state === "connected")
      .map((connection) => ({
        id: connection.host.id,
        name: connection.host.name,
        baseUrl: connection.host.url,
        isLocal: false,
        ip: connection.machineInfo?.tailscaleIp ?? connection.host.url.replace(/^https?:\/\//, "").replace(/:\d+$/, ""),
      })),
  ].map((target) => ({
    ...target,
    label: formatTargetLabel(target.name, target.ip, target.isLocal),
  })), [connections, daemonUrl, formatTargetLabel, localHostName, localMachineIp]);

  const selectedTarget = targetOptions.find((target) => target.id === selectedTargetId) ?? targetOptions[0];
  const launchDaemonUrl = selectedTarget?.baseUrl ?? daemonUrl;

  useEffect(() => {
    if (selectedTargetId === "local") return;
    if (!targetOptions.some((target) => target.id === selectedTargetId)) {
      setSelectedTargetId("local");
    }
  }, [selectedTargetId, targetOptions]);

  useEffect(() => {
    let cancelled = false;

    listAgents(launchDaemonUrl)
      .then((items) => {
        if (cancelled) return;
        setAgents(items);
        setSelectedAgentId((current) => {
          if (current === "shell") return current;
          if (current && items.some((agent) => agent.id === current)) return current;
          return items[0]?.id ?? "shell";
        });
      })
      .catch(() => {
        if (cancelled) return;
        setAgents([]);
        setSelectedAgentId((current) => (current === "shell" ? current : "shell"));
      });

    return () => {
      cancelled = true;
    };
  }, [launchDaemonUrl]);

  useEffect(() => {
    let cancelled = false;

    listProjects(launchDaemonUrl)
      .then((items) => {
        if (!cancelled) {
          setProjects(items);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setProjects([]);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [launchDaemonUrl]);

  useEffect(() => {
    if (!activeSessionId || !activeSessionTargetId) return;

    setSelectedTargetId(activeSessionTargetId);
    setLaunchWorkdir(activeSessionWorkdir);
    setActiveMode(activeSessionViewMode);
  }, [activeSessionId, activeSessionTargetId, activeSessionViewMode, activeSessionWorkdir]);

  useEffect(() => {
    if (!activeSession) return;

    const matchedProject = activeSessionProjectId
      ? projects.find((project) => project.id === activeSessionProjectId)
      : projects.find((project) => project.workdir === activeSessionWorkdir);
    setSelectedProjectId(matchedProject?.id ?? activeSessionProjectId);
  }, [activeSession, activeSessionProjectId, activeSessionWorkdir, projects]);

  const handleSessionRenamed = useCallback(() => {
    void onRefreshSessions();
  }, [onRefreshSessions]);

  const chatSocket = useChatSocket({
    baseUrl: activeSessionBaseUrl,
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
    if (!selectedAgentId || !selectedTarget) return;

    setError(null);
    setLoading(true);

    try {
      const workdir = launchWorkdir.trim() || undefined;
      const projectId = selectedProjectId || undefined;

      if (selectedAgentId === "shell") {
        if (selectedTarget.isLocal) {
          const session = await onCreateLocalSession(workdir);
          if (session?.id) {
            onSelectSession(session.id);
            setActiveMode("terminal");
          }
        } else {
          const session = await api<TerminalSession>(launchDaemonUrl, "/api/terminal/sessions", {
            method: "POST",
            body: JSON.stringify({
              mode: "terminal",
              name: "Shell",
              projectId,
              workdir,
            }),
          });
          await onRefreshSessions();
          onSelectSession(session.id);
          setActiveMode("terminal");
        }
      } else if (selectedMode === "chat") {
        const result = await createChatSession(launchDaemonUrl, selectedAgentId, projectId, workdir);
        const sessionId: string = result.session?.id ?? result.session;
        await onRefreshSessions();
        onSelectSession(sessionId);
        setActiveMode("chat");
      } else {
        const session = await api<TerminalSession>(launchDaemonUrl, "/api/terminal/sessions", {
          method: "POST",
          body: JSON.stringify({
            mode: "agent",
            agentId: selectedAgentId,
            projectId,
            workdir,
          }),
        });
        await onRefreshSessions();
        onSelectSession(session.id);
        setActiveMode("terminal");
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create session");
    } finally {
      setLoading(false);
    }
  }, [
    launchDaemonUrl,
    launchWorkdir,
    onCreateLocalSession,
    onRefreshSessions,
    onSelectSession,
    selectedAgentId,
    selectedMode,
    selectedProjectId,
    selectedTarget,
  ]);

  const handleSwitchMode = useCallback(async (newMode: SessionMode) => {
    if (!activeSessionId || newMode === activeMode) return;

    setError(null);

    try {
      let result = await switchSessionMode(activeSessionBaseUrl, activeSessionId, newMode);
      if (result.needsConfirmation) {
        const ok = window.confirm(result.warning ?? "Switching modes will end the current conversation. Continue?");
        if (!ok) return;
        result = await switchSessionMode(activeSessionBaseUrl, activeSessionId, newMode, true);
      }

      await onRefreshSessions();
      if (result.session?.id) {
        onSelectSession(result.session.id);
      }

      setActiveMode(newMode);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to switch mode");
    }
  }, [activeMode, activeSessionBaseUrl, activeSessionId, onRefreshSessions, onSelectSession]);

  const handleOpenCompanionTerminal = useCallback(async () => {
    if (!activeSessionId) return;

    setError(null);

    try {
      const session = await createCompanionTerminal(activeSessionBaseUrl, activeSessionId);
      await onRefreshSessions();
      onSelectSession(session.id);
      setActiveMode("terminal");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to open companion terminal");
    }
  }, [activeSessionBaseUrl, activeSessionId, onRefreshSessions, onSelectSession]);

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

      const reopened = await reopenWorkSession(activeSessionBaseUrl, activeSessionId);
      await onRefreshSessions();
      onSelectSession(reopened.id);
      setActiveMode(reopened.mode === "chat" ? "chat" : "terminal");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to reopen session");
    }
  }, [
    activeSession,
    activeSessionBaseUrl,
    activeSessionId,
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
        await api<TerminalSession>(activeSessionBaseUrl, `/api/terminal/sessions/${activeSessionId}/terminate`, {
          method: "POST",
        });
        await onRefreshSessions();
      }

      onSelectSession(null);
      setActiveMode("terminal");
    } catch {
      // ignore for now
    }
  }, [activeSessionBaseUrl, activeSessionId, isLocalSession, onRefreshSessions, onSelectSession, onTerminateLocalSession]);

  if (!visible) return null;

  const workdirPlaceholder = projects[0]?.workdir ?? "~/projects/my-app";

  return (
    <div className="agents-view">
      <div className="agents-topbar">
        <select
          aria-label="Target machine"
          value={selectedTargetId}
          onChange={(e) => setSelectedTargetId(e.target.value)}
        >
          {targetOptions.map((target) => (
            <option key={target.id} value={target.id}>
              {target.label}
            </option>
          ))}
        </select>

        <select
          aria-label="Agent"
          value={selectedAgentId ?? ""}
          onChange={(e) => setSelectedAgentId(e.target.value || null)}
        >
          <option value="shell">{selectedTarget?.isLocal ? "Shell (local)" : "Shell"}</option>
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
                  baseUrl={activeSessionBaseUrl}
                  sessionId={activeSessionId}
                  isLocal={isLocalSession}
                  isActive={visible}
                  interactive={activeSession.status === "running" || activeSession.status === "created"}
                  onSessionStatusChange={(session) => {
                    if (isLocalSession) {
                      onLocalSessionStatusChange(session as LocalTerminalSession);
                    } else {
                      void onRefreshSessions();
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

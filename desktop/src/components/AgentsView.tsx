import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  adoptCodeServer,
  createChatSession,
  createCodeServerSession,
  createCompanionTerminal,
  listAgents,
  listDetectedCodeServers,
  listProjects,
  reopenWorkSession,
  setupClaude,
  switchSessionMode,
} from "../api";
import { useChatSocket } from "../hooks/useChatSocket";
import { isTauri } from "../lib/platform";
import { SessionSidebar } from "./SessionSidebar";
import { SessionHeader } from "./SessionHeader";
import { ChatRenderer } from "./ChatRenderer";
import { CodeServerPanel } from "./CodeServerPanel";
import { PathAutocomplete } from "./PathAutocomplete";
import { TerminalRenderer } from "./TerminalRenderer";
import type {
  AgentInfo,
  CodeServerInfo,
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
  const [ideLoading, setIdeLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [detectedCodeServers, setDetectedCodeServers] = useState<CodeServerInfo[]>([]);
  const ideMenuRef = useRef<HTMLDetailsElement>(null);
  const launchableAgents = useMemo(
    () => agents.filter((agent) => agent.launchSupported !== false),
    [agents],
  );
  const manualOnlyAgents = useMemo(
    () => agents.filter((agent) => agent.launchSupported === false),
    [agents],
  );
  const claudeManualOnlyAgent = manualOnlyAgents.find((agent) => agent.id === "claude-code") ?? null;
  const [copiedSetupTargetId, setCopiedSetupTargetId] = useState<string | null>(null);
  const [dismissedSetupTargets, setDismissedSetupTargets] = useState<Record<string, boolean>>({});
  const [claudeApiKey, setClaudeApiKey] = useState("");
  const [claudeSetupSaving, setClaudeSetupSaving] = useState(false);
  const [claudeSetupError, setClaudeSetupError] = useState<string | null>(null);

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
      sshTarget: null,
    },
    ...connections
      .filter((connection) => connection.state === "connected")
      .map((connection) => ({
        id: connection.host.id,
        name: connection.host.name,
        baseUrl: connection.host.url,
        isLocal: false,
        ip: connection.machineInfo?.tailscaleIp ?? connection.host.url.replace(/^https?:\/\//, "").replace(/:\d+$/, ""),
        sshTarget: connection.machineInfo?.tailscaleIp && connection.machineInfo.tools.sshUser
          ? `${connection.machineInfo.tools.sshUser}@${connection.machineInfo.tailscaleIp}`
          : null,
      })),
  ].map((target) => ({
    ...target,
    label: formatTargetLabel(target.name, target.ip, target.isLocal),
  })), [connections, daemonUrl, formatTargetLabel, localHostName, localMachineIp]);

  const selectedTarget = targetOptions.find((target) => target.id === selectedTargetId) ?? targetOptions[0];
  const launchDaemonUrl = selectedTarget?.baseUrl ?? daemonUrl;
  const claudeSetupCommand = "ghost setup claude";
  const claudeSetupTargetLabel = selectedTarget?.isLocal
    ? "this computer"
    : (selectedTarget?.name ?? "this host");
  const claudeSetupScopeNote = selectedTarget?.isLocal
    ? "Claude setup is machine-local. If you later use another host, run this there too."
    : `${selectedTarget?.name ?? "This host"} needs its own Claude setup even if your local machine is already configured.`;
  const showClaudeSetupBanner = !!claudeManualOnlyAgent && !dismissedSetupTargets[selectedTargetId];

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
        const launchable = items.filter((agent) => agent.launchSupported !== false);
        setSelectedAgentId((current) => {
          if (current === "shell") return current;
          if (current && launchable.some((agent) => agent.id === current)) return current;
          return launchable[0]?.id ?? "shell";
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

  useEffect(() => {
    if (!visible) return;
    const poll = async () => {
      try {
        const detected = await listDetectedCodeServers(activeSessionBaseUrl);
        setDetectedCodeServers(detected);
      } catch { /* ignore */ }
    };
    poll();
    const interval = setInterval(poll, 10000);
    return () => clearInterval(interval);
  }, [activeSessionBaseUrl, visible]);

  useEffect(() => {
    if (!copiedSetupTargetId) return;
    const timer = window.setTimeout(() => {
      setCopiedSetupTargetId((current) => (current === copiedSetupTargetId ? null : current));
    }, 1800);
    return () => window.clearTimeout(timer);
  }, [copiedSetupTargetId]);

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

  const closeIdeMenu = useCallback(() => {
    ideMenuRef.current?.removeAttribute("open");
  }, []);

  const launchShellSession = useCallback(async (target: typeof selectedTarget) => {
    if (!target) return;

    const workdir = launchWorkdir.trim() || undefined;
    const projectId = selectedProjectId || undefined;

    if (target.isLocal) {
      const session = await onCreateLocalSession(workdir);
      if (session?.id) {
        onSelectSession(session.id);
        setActiveMode("terminal");
      }
      return;
    }

    const session = await api<TerminalSession>(target.baseUrl, "/api/terminal/sessions", {
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
  }, [
    launchWorkdir,
    onCreateLocalSession,
    onRefreshSessions,
    onSelectSession,
    selectedProjectId,
  ]);

  const handleNewSession = useCallback(async () => {
    if (!selectedAgentId || !selectedTarget) return;

    setError(null);
    setLoading(true);

    try {
      const workdir = launchWorkdir.trim() || undefined;
      const projectId = selectedProjectId || undefined;

      if (selectedAgentId === "shell") {
        await launchShellSession(selectedTarget);
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
    launchShellSession,
  ]);

  const handleCopyClaudeSetup = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(claudeSetupCommand);
      setCopiedSetupTargetId(selectedTargetId);
    } catch {
      setError("Failed to copy `ghost setup claude`.");
    }
  }, [claudeSetupCommand, selectedTargetId]);

  const handleOpenClaudeSetupShell = useCallback(async () => {
    if (!selectedTarget) return;

    setError(null);
    setSelectedAgentId("shell");
    setSelectedMode("terminal");
    setLoading(true);

    try {
      await launchShellSession(selectedTarget);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to open a setup shell");
    } finally {
      setLoading(false);
    }
  }, [launchShellSession, selectedTarget]);

  const handleDismissClaudeSetup = useCallback(() => {
    setDismissedSetupTargets((current) => ({
      ...current,
      [selectedTargetId]: true,
    }));
  }, [selectedTargetId]);

  const handleClaudeInlineSetup = useCallback(async () => {
    if (!claudeApiKey.trim()) return;
    setClaudeSetupSaving(true);
    setClaudeSetupError(null);
    try {
      await setupClaude(daemonUrl, { apiKey: claudeApiKey.trim() });
      setClaudeApiKey("");
      setDismissedSetupTargets((current) => ({
        ...current,
        [selectedTargetId]: true,
      }));
      // Refresh agents list so Claude shows as launchable now
      const items = await listAgents(launchDaemonUrl);
      setAgents(items);
      const launchable = items.filter((agent) => agent.launchSupported !== false);
      setSelectedAgentId((current) => {
        if (current && launchable.some((agent) => agent.id === current)) return current;
        return launchable[0]?.id ?? "shell";
      });
    } catch (e) {
      setClaudeSetupError(e instanceof Error ? e.message : "Setup failed");
    } finally {
      setClaudeSetupSaving(false);
    }
  }, [claudeApiKey, daemonUrl, launchDaemonUrl, selectedTargetId]);

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

  const handleOpenBrowserIde = useCallback(async () => {
    if (!selectedTarget) return;

    closeIdeMenu();
    setError(null);
    setIdeLoading(true);

    try {
      const session = await createCodeServerSession(
        launchDaemonUrl,
        launchWorkdir.trim() || "~",
        selectedProjectId || undefined,
      );
      await onRefreshSessions();
      onSelectSession(session.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to start VS Code in browser");
    } finally {
      setIdeLoading(false);
    }
  }, [closeIdeMenu, launchDaemonUrl, launchWorkdir, onRefreshSessions, onSelectSession, selectedProjectId, selectedTarget]);

  const handleOpenLocalIde = useCallback(async () => {
    if (!selectedTarget) return;

    closeIdeMenu();
    setError(null);

    if (!isTauri()) {
      setError("Open IDE locally is only available in the desktop app.");
      return;
    }

    if (!selectedTarget.isLocal && !selectedTarget.sshTarget) {
      setError("Remote IDE launch needs SSH details from the selected host.");
      return;
    }

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("open_in_vscode", {
        workdir: launchWorkdir.trim() || "~",
        sshTarget: selectedTarget.sshTarget ?? null,
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(`Failed to open local VS Code: ${message}`);
    }
  }, [closeIdeMenu, launchWorkdir, selectedTarget]);

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
          {launchableAgents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name} {agent.version ? `v${agent.version}` : ""} ({agent.agentType})
            </option>
          ))}
          {manualOnlyAgents.map((agent) => (
            <option key={agent.id} value={agent.id} disabled>
              {agent.name} (manual only)
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

        <details className="ide-launch-menu" ref={ideMenuRef}>
          <summary className="btn-secondary ide-launch-trigger" style={{ fontSize: "0.78rem" }}>
            Open IDE
            <span className="ide-launch-caret">▾</span>
          </summary>
          <div className="ide-launch-popover">
            <button
              type="button"
              className="ide-launch-option"
              onClick={() => void handleOpenBrowserIde()}
              disabled={ideLoading}
            >
              <span className="ide-launch-option-title">VS Code in browser</span>
              <span className="ide-launch-option-meta">Start a `code-server` session for the selected folder.</span>
            </button>
            <button
              type="button"
              className="ide-launch-option"
              onClick={() => void handleOpenLocalIde()}
              disabled={!selectedTarget?.isLocal && !selectedTarget?.sshTarget}
            >
              <span className="ide-launch-option-title">VS Code locally</span>
              <span className="ide-launch-option-meta">
                {selectedTarget?.isLocal
                  ? "Open this folder in your local VS Code app."
                  : "Open a Remote SSH window from local VS Code."}
              </span>
            </button>
          </div>
        </details>

        {error && <span style={{ color: "var(--accent-red)", fontSize: "0.78rem" }}>{error}</span>}
      </div>

      {showClaudeSetupBanner && selectedTarget && (
        <div className="agent-setup-banner">
          <div className="agent-setup-copy">
            <div className="agent-setup-title">
              Set up managed Claude Code on {claudeSetupTargetLabel}
            </div>
            <div className="agent-setup-message">
              Ghost can see Claude Code on this host, but it stays manual-only until the daemon host has API or cloud auth.
              Run <code>{claudeSetupCommand}</code> once on {claudeSetupTargetLabel} so Ghost can launch managed Claude sessions there.
            </div>
            <div className="agent-setup-note">
              You can still run Claude Code directly and attach Ghost through MCP in the meantime.
            </div>
            <div className="agent-setup-note">{claudeSetupScopeNote}</div>
            {selectedTarget?.isLocal && (
              <div style={{
                marginTop: "8px",
                padding: "8px 0",
                borderTop: "1px solid var(--border, #333)",
              }}>
                <div style={{ fontSize: "12px", color: "var(--text-muted, #888)", marginBottom: "6px" }}>
                  Or configure directly:
                </div>
                <div style={{ display: "flex", gap: "6px", alignItems: "center" }}>
                  <input
                    type="password"
                    placeholder="Anthropic API key"
                    value={claudeApiKey}
                    onChange={(e) => setClaudeApiKey(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleClaudeInlineSetup()}
                    style={{
                      flex: 1,
                      padding: "5px 8px",
                      background: "var(--bg-input, #1e1e1e)",
                      border: "1px solid var(--border, #333)",
                      borderRadius: "4px",
                      color: "var(--text, #ccc)",
                      fontSize: "13px",
                    }}
                  />
                  <button
                    className="btn-secondary"
                    onClick={() => void handleClaudeInlineSetup()}
                    disabled={claudeSetupSaving || !claudeApiKey.trim()}
                    style={{ opacity: claudeSetupSaving || !claudeApiKey.trim() ? 0.5 : 1 }}
                  >
                    {claudeSetupSaving ? "Saving..." : "Save"}
                  </button>
                </div>
                {claudeSetupError && (
                  <div style={{ marginTop: "4px", color: "var(--text-error, #f87171)", fontSize: "12px" }}>
                    {claudeSetupError}
                  </div>
                )}
              </div>
            )}
          </div>
          <div className="agent-setup-command">
            <code>{claudeSetupCommand}</code>
            <div className="agent-setup-actions">
              <button className="btn-secondary" onClick={() => void handleCopyClaudeSetup()}>
                {copiedSetupTargetId === selectedTargetId ? "Copied" : "Copy command"}
              </button>
              <button
                className="btn-secondary"
                onClick={() => void handleOpenClaudeSetupShell()}
                disabled={loading}
              >
                {selectedTarget.isLocal ? "Open local shell" : "Open shell on host"}
              </button>
              <button className="agent-setup-dismiss" onClick={handleDismissClaudeSetup}>
                Dismiss
              </button>
            </div>
          </div>
        </div>
      )}

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
          <PathAutocomplete
            value={launchWorkdir}
            onChange={handleWorkdirChange}
            baseUrl={launchDaemonUrl}
            placeholder={workdirPlaceholder}
          />
        </label>
      </div>

      <div className="agents-main">
        {detectedCodeServers.length > 0 && (
          <div className="code-server-detection-banner">
            {detectedCodeServers.map((cs) => (
              <div key={cs.pid} className="code-server-detection-item">
                <span>code-server detected at <strong>{cs.workdir}</strong> (port {cs.port})</span>
                <button
                  className="btn-secondary"
                  style={{ fontSize: "0.78rem", padding: "2px 10px" }}
                  onClick={async () => {
                    try {
                      await adoptCodeServer(activeSessionBaseUrl, cs.pid);
                      setDetectedCodeServers((prev) => prev.filter((d) => d.pid !== cs.pid));
                      void onRefreshSessions();
                    } catch (err) {
                      console.error("Failed to adopt:", err);
                    }
                  }}
                >
                  Adopt
                </button>
                <button
                  className="btn-ghost"
                  style={{ fontSize: "0.78rem", padding: "2px 8px" }}
                  onClick={() => setDetectedCodeServers((prev) => prev.filter((d) => d.pid !== cs.pid))}
                >
                  Dismiss
                </button>
              </div>
            ))}
          </div>
        )}

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

              {activeSession.driverKind === "code_server_driver" ? (
                <CodeServerPanel
                  session={activeSession}
                  baseUrl={activeSessionBaseUrl}
                  onRefresh={onRefreshSessions}
                />
              ) : activeMode === "chat" && !isLocalSession ? (
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

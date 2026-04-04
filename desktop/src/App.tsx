import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, wsUrlFromHttp } from "./api";
import { loadHosts, addHost as persistAddHost, removeHost as persistRemoveHost } from "./hosts";
import { appLog } from "./log";
import type {
  Conversation,
  ConversationDetail,
  EventEnvelope,
  HostConnection,
  LocalTerminalSession,
  Message,
  RunDetail,
  RunRecord,
  SavedHost,
  SystemStatus,
  TerminalSession,
} from "./types";
import { Sidebar } from "./components/Sidebar";
import { ChatView } from "./components/ChatView";
import { InspectorPanel } from "./components/InspectorPanel";
import { TerminalWorkspace } from "./components/TerminalWorkspace";
import { LogViewer } from "./components/LogViewer";
import "./App.css";

type MainView = "chat" | "terminal" | "logs" | "settings";

function App() {
  // Multi-host state
  const [hosts, setHosts] = useState<SavedHost[]>(() => loadHosts());
  const [connections, setConnections] = useState<Map<string, HostConnection>>(new Map());

  // Shared state (unchanged)
  const [mainView, setMainView] = useState<MainView>("terminal");
  const [activeTerminalSessionId, setActiveTerminalSessionId] = useState<string | null>(null);
  const [localSessions, setLocalSessions] = useState<LocalTerminalSession[]>([]);
  const [actionError, setActionError] = useState("");

  // Per-active-host UI state
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [messageInput, setMessageInput] = useState("");
  const [events, setEvents] = useState<EventEnvelope[]>([]);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [runDetail, setRunDetail] = useState<RunDetail | null>(null);

  const activeRunIdRef = useRef(activeRunId);
  useEffect(() => { activeRunIdRef.current = activeRunId; }, [activeRunId]);

  // --- Derived state (activeHostId) ---

  const activeHostId: string | null = useMemo(() => {
    if (!activeTerminalSessionId) return null;
    if (localSessions.some((s) => s.id === activeTerminalSessionId)) return null;
    for (const [hostId, conn] of connections) {
      if (conn.sessions?.some((s) => s.id === activeTerminalSessionId)) return hostId;
    }
    return null;
  }, [activeTerminalSessionId, connections, localSessions]);

  const activeConnection = activeHostId ? connections.get(activeHostId) ?? null : null;
  const activeHost = activeHostId ? hosts.find((h) => h.id === activeHostId) ?? null : null;
  const activeHostUrl = activeHost?.url ?? null;
  const activeRuns = activeConnection?.runs ?? [];
  const activeSystemStatus = activeConnection?.systemStatus ?? null;
  const activeConversations = activeConnection?.conversations ?? [];
  const activeTerminalSessions = activeConnection?.sessions ?? [];

  const selectedConversation = useMemo(
    () => activeConversations.find((item) => item.id === selectedConversationId) ?? null,
    [activeConversations, selectedConversationId],
  );
  const activeRun = useMemo(
    () => activeRuns.find((item) => item.id === activeRunId) ?? null,
    [activeRuns, activeRunId],
  );

  // --- Connection helpers ---

  const updateConnection = useCallback((hostId: string, update: Partial<HostConnection>) => {
    setConnections((prev) => {
      const next = new Map(prev);
      const existing = next.get(hostId);
      if (existing) {
        next.set(hostId, { ...existing, ...update });
      }
      return next;
    });
  }, []);

  const checkHostHealth = useCallback(async (host: SavedHost) => {
    try {
      const health = await api<{ ok: boolean; telegramEnabled?: boolean }>(host.url, "/health");
      const msg = health.telegramEnabled ? "Connected · Telegram on" : "Connected";
      updateConnection(host.id, { state: "connected", message: msg });
    } catch (error) {
      const msg = error instanceof Error ? error.message : "Connection failed";
      updateConnection(host.id, { state: "error", message: msg });
    }
  }, [updateConnection]);

  const initializeHosts = useCallback((hostList: SavedHost[]) => {
    const initial = new Map<string, HostConnection>();
    for (const host of hostList) {
      initial.set(host.id, {
        host,
        state: "connecting",
        message: "Connecting...",
        sessions: null,
        runs: null,
        conversations: null,
        systemStatus: null,
      });
    }
    setConnections(initial);
    for (const host of hostList) {
      void checkHostHealth(host);
    }
  }, [checkHostHealth]);

  const loadHostData = useCallback(async (hostId: string, url: string) => {
    try {
      const [sessions, runs, conversations, systemStatus] = await Promise.all([
        api<TerminalSession[]>(url, "/api/terminal/sessions"),
        api<RunRecord[]>(url, "/api/runs"),
        api<Conversation[]>(url, "/api/conversations"),
        api<SystemStatus>(url, "/api/system/status"),
      ]);
      updateConnection(hostId, { sessions, runs, conversations, systemStatus });
    } catch (error) {
      appLog.error("app", `Failed to load data for host ${hostId}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [updateConnection]);

  // --- Effects ---

  // Initialize hosts on mount
  useEffect(() => {
    initializeHosts(hosts);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Auto-spawn a local terminal on first mount
  const localSpawnedRef = useRef(false);
  useEffect(() => {
    if (localSpawnedRef.current) return;
    localSpawnedRef.current = true;
    void handleCreateLocalSession();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Lazy-load host data when a host becomes active for the first time
  useEffect(() => {
    if (!activeHostId || !activeHost) return;
    const conn = connections.get(activeHostId);
    if (!conn || conn.state !== "connected" || conn.sessions !== null) return;
    void loadHostData(activeHostId, activeHost.url);
  }, [activeHostId, activeHost, connections, loadHostData]);

  // Health polling every 30s
  useEffect(() => {
    if (hosts.length === 0) return;
    const interval = setInterval(() => {
      for (const host of hosts) {
        void checkHostHealth(host);
      }
    }, 30000);
    return () => clearInterval(interval);
  }, [hosts, checkHostHealth]);

  // Reset per-host UI state when active host changes
  useEffect(() => {
    setSelectedConversationId(null);
    setMessages([]);
    setActiveRunId(null);
    setRunDetail(null);
    setEvents([]);
    if (activeConnection?.conversations?.length) {
      setSelectedConversationId(activeConnection.conversations[0].id);
    }
    if (activeConnection?.runs?.length) {
      setActiveRunId(activeConnection.runs[0].id);
    }
  }, [activeHostId]); // eslint-disable-line react-hooks/exhaustive-deps

  // Load conversation detail
  useEffect(() => {
    if (!selectedConversationId || !activeHostUrl) return;
    const url = activeHostUrl;
    api<ConversationDetail>(url, `/api/conversations/${selectedConversationId}`)
      .then((data) => { setMessages(data.messages); })
      .catch((error) => {
        appLog.error("app", `Failed to load conversation: ${error instanceof Error ? error.message : String(error)}`);
      });
  }, [selectedConversationId, activeHostUrl]);

  // Load run detail
  useEffect(() => {
    if (!activeRunId || !activeHostUrl) return;
    const url = activeHostUrl;
    api<RunDetail>(url, `/api/runs/${activeRunId}`)
      .then((data) => { setRunDetail(data); })
      .catch((error) => {
        appLog.error("app", `Failed to load run: ${error instanceof Error ? error.message : String(error)}`);
      });
  }, [activeRunId, activeHostUrl]);

  // Conversation WebSocket — scoped to active host
  useEffect(() => {
    if (!selectedConversationId || !activeHostUrl) return;

    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const ws = new WebSocket(wsUrlFromHttp(activeHostUrl));

    ws.onopen = () => {
      appLog.info("conv-ws", "Connected");
      ws.send(JSON.stringify({ op: "subscribe", conversationId: selectedConversationId, afterSeq: 0 }));
    };

    ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.op === "event") {
        const envelope = data.event as EventEnvelope;
        setEvents((current) => [...current.slice(-299), envelope]);
        if (envelope.type === "message_created") {
          const payload = envelope.payload as { messageId?: string; role?: "user" | "assistant" | "system"; content?: string };
          if (
            typeof payload.messageId === "string"
            && typeof payload.role === "string"
            && typeof payload.content === "string"
            && envelope.conversationId === selectedConversationId
          ) {
            const nextMessage: Message = {
              id: payload.messageId,
              conversationId: selectedConversationId,
              role: payload.role,
              content: payload.content,
              createdAt: envelope.ts,
              runId: envelope.runId,
            };
            setMessages((current) => current.some((item) => item.id === nextMessage.id) ? current : [...current, nextMessage]);
          }
        }
        if (!refreshTimer && activeHostId) {
          const hostId = activeHostId;
          const hostUrl = activeHostUrl;
          refreshTimer = setTimeout(() => {
            refreshTimer = null;
            void Promise.all([
              api<RunRecord[]>(hostUrl, "/api/runs"),
              api<SystemStatus>(hostUrl, "/api/system/status"),
            ]).then(([runs, systemStatus]) => {
              updateConnection(hostId, { runs, systemStatus });
            });
          }, 500);
        }
        const currentRunId = activeRunIdRef.current;
        if (envelope.runId && currentRunId === envelope.runId && activeHostUrl) {
          api<RunDetail>(activeHostUrl, `/api/runs/${envelope.runId}`)
            .then((data) => setRunDetail(data))
            .catch(() => {});
        }
      } else if (data.op === "error") {
        appLog.error("conv-ws", `Server error: ${data.message ?? "unknown"}`);
      }
    };

    ws.onerror = () => { appLog.error("conv-ws", "WebSocket error event"); };
    ws.onclose = (event) => {
      appLog.warn("conv-ws", `Disconnected: code=${event.code} reason=${event.reason || "none"}`);
    };

    return () => {
      if (refreshTimer) clearTimeout(refreshTimer);
      ws.close();
    };
  }, [activeHostUrl, selectedConversationId]); // eslint-disable-line react-hooks/exhaustive-deps

  // --- Action handlers ---

  const handleSendMessage = useCallback(async (event: FormEvent) => {
    event.preventDefault();
    if (!selectedConversationId || !messageInput.trim() || !activeHostUrl) return;
    const content = messageInput.trim();
    const url = activeHostUrl;
    setMessageInput("");
    setActionError("");
    await api(url, `/api/conversations/${selectedConversationId}/messages`, {
      method: "POST",
      body: JSON.stringify({ content }),
    });
    const run = await api<{ runId: string }>(url, "/api/runs", {
      method: "POST",
      body: JSON.stringify({ conversationId: selectedConversationId, content }),
    });
    setActiveRunId(run.runId);
    if (activeHostId) {
      const [runs, systemStatus] = await Promise.all([
        api<RunRecord[]>(url, "/api/runs"),
        api<SystemStatus>(url, "/api/system/status"),
      ]);
      updateConnection(activeHostId, { runs, systemStatus });
    }
  }, [activeHostUrl, activeHostId, selectedConversationId, messageInput, updateConnection]);

  const handleRetryRun = useCallback(async () => {
    if (!activeRunId || !activeHostUrl) return;
    try {
      const data = await api<{ runId: string }>(activeHostUrl, `/api/runs/${activeRunId}/retry`, { method: "POST" });
      setActiveRunId(data.runId);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Retry failed");
    }
  }, [activeHostUrl, activeRunId]);

  const handleCancelRun = useCallback(async () => {
    if (!activeRunId || !activeHostUrl) return;
    try {
      await api(activeHostUrl, `/api/runs/${activeRunId}/cancel`, { method: "POST" });
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Cancel failed");
    }
  }, [activeHostUrl, activeRunId]);

  const handleCreateRemoteSession = useCallback(async (hostId: string, mode: "agent" | "rescue_shell" | "project") => {
    const host = hosts.find((h) => h.id === hostId);
    if (!host) return;
    try {
      const session = await api<TerminalSession>(host.url, "/api/terminal/sessions", {
        method: "POST",
        body: JSON.stringify({ mode }),
      });
      updateConnection(hostId, {
        sessions: [...(connections.get(hostId)?.sessions ?? []), session],
      });
      setActiveTerminalSessionId(session.id);
      setMainView("terminal");
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Create session failed");
    }
  }, [hosts, connections, updateConnection]);

  const handleRemoteSessionStatusChange = useCallback((_session: TerminalSession) => {
    if (!activeHostId || !activeHostUrl) return;
    const hostId = activeHostId;
    const url = activeHostUrl;
    void api<TerminalSession[]>(url, "/api/terminal/sessions").then((sessions) => {
      updateConnection(hostId, { sessions });
    });
  }, [activeHostId, activeHostUrl, updateConnection]);

  const handleKillRemoteSession = useCallback(async (sessionId: string) => {
    if (!activeHostId || !activeHostUrl) return;
    const hostId = activeHostId;
    const url = activeHostUrl;
    try {
      await api(url, `/api/terminal/sessions/${sessionId}/terminate`, { method: "POST" });
      const sessions = await api<TerminalSession[]>(url, "/api/terminal/sessions");
      updateConnection(hostId, { sessions });
      if (activeTerminalSessionId === sessionId) {
        const nextActive = sessions.find((s) => s.id !== sessionId && (s.status === "created" || s.status === "running"));
        setActiveTerminalSessionId(nextActive?.id ?? null);
      }
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Terminate session failed");
    }
  }, [activeHostId, activeHostUrl, activeTerminalSessionId, updateConnection]);

  const handleCreateLocalSession = useCallback(async () => {
    try {
      const cols = 120;
      const rows = 30;
      const sessionId = await invoke<string>("pty_spawn", { cols, rows, workdir: null });
      const session: LocalTerminalSession = {
        id: sessionId,
        status: "running",
        createdAt: new Date().toISOString(),
      };
      setLocalSessions((prev) => [...prev, session]);
      setActiveTerminalSessionId(sessionId);
      setMainView("terminal");
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      appLog.error("app", `Failed to spawn local terminal: ${msg}`);
      setActionError(`Failed to spawn local terminal: ${msg}`);
    }
  }, []);

  const handleLocalSessionStatusChange = useCallback((session: LocalTerminalSession) => {
    setLocalSessions((prev) =>
      prev.map((s) => (s.id === session.id ? session : s)),
    );
  }, []);

  const handleKillLocalSession = useCallback(async (sessionId: string) => {
    const existing = localSessions.find((s) => s.id === sessionId);
    if (!existing || existing.status !== "running") return;
    try {
      await invoke("pty_kill", { sessionId });
      setLocalSessions((prev) =>
        prev.map((s) => (s.id === sessionId ? { ...s, status: "terminated" as const } : s)),
      );
      if (activeTerminalSessionId === sessionId) {
        const remaining = localSessions.filter((s) => s.id !== sessionId && s.status === "running");
        setActiveTerminalSessionId(remaining[0]?.id ?? null);
      }
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Kill local session failed");
    }
  }, [activeTerminalSessionId, localSessions]);

  const handleResolveApproval = useCallback(async (approvalId: string, status: "approved" | "rejected") => {
    if (!activeHostUrl || !activeHostId) return;
    try {
      await api(activeHostUrl, `/api/approvals/${approvalId}/resolve`, {
        method: "POST",
        body: JSON.stringify({ status, resolvedBy: "ghost-protocol-app" }),
      });
      const systemStatus = await api<SystemStatus>(activeHostUrl, "/api/system/status");
      updateConnection(activeHostId, { systemStatus });
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Approval resolution failed");
    }
  }, [activeHostUrl, activeHostId, updateConnection]);

  const handleAddHost = useCallback((name: string, url: string) => {
    const updated = persistAddHost(hosts, name, url);
    setHosts(updated);
    const newHost = updated[updated.length - 1];
    setConnections((prev) => {
      const next = new Map(prev);
      next.set(newHost.id, {
        host: newHost,
        state: "connecting",
        message: "Connecting...",
        sessions: null,
        runs: null,
        conversations: null,
        systemStatus: null,
      });
      return next;
    });
    void checkHostHealth(newHost);
  }, [hosts, checkHostHealth]);

  const handleRemoveHost = useCallback((hostId: string) => {
    const updated = persistRemoveHost(hosts, hostId);
    setHosts(updated);
    setConnections((prev) => {
      const next = new Map(prev);
      next.delete(hostId);
      return next;
    });
    const conn = connections.get(hostId);
    if (conn?.sessions?.some((s) => s.id === activeTerminalSessionId)) {
      setActiveTerminalSessionId(null);
    }
  }, [hosts, connections, activeTerminalSessionId]);

  // --- Render ---

  // Build connections array for Sidebar
  const hostConnections = useMemo(
    () => hosts.map((h) => connections.get(h.id)).filter((c): c is HostConnection => c != null),
    [hosts, connections],
  );

  // Gather all remote sessions across all hosts for TerminalWorkspace
  const allRemoteSessions = useMemo(() => {
    const result: Array<{ hostId: string; hostName: string; session: TerminalSession }> = [];
    for (const host of hosts) {
      const conn = connections.get(host.id);
      if (!conn?.sessions) continue;
      for (const session of conn.sessions) {
        result.push({ hostId: host.id, hostName: host.name, session });
      }
    }
    return result;
  }, [hosts, connections]);

  return (
    <main className="shell">
      <Sidebar
        hosts={hostConnections}
        mainView={mainView}
        onChangeView={setMainView}
        onAddHost={handleAddHost}
        onRemoveHost={handleRemoveHost}
      />

      <section className="main-panel">
        <div style={{ display: mainView === "chat" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="terminal-header">
            <div>
              <h2>{selectedConversation?.title || "Chat"}</h2>
              <p className="muted">{activeHost ? `${activeHost.name} — Conversation` : "Select a remote host to chat"}</p>
            </div>
          </div>
          {activeHostUrl ? (
            <ChatView
              messages={messages}
              messageInput={messageInput}
              selectedConversationId={selectedConversationId}
              activeRunId={activeRunId}
              actionError={actionError}
              onChangeMessageInput={setMessageInput}
              onSendMessage={(e) => void handleSendMessage(e)}
              onRetryRun={() => void handleRetryRun()}
              onCancelRun={() => void handleCancelRun()}
            />
          ) : (
            <div className="empty-state" style={{ padding: "2rem" }}>
              Select a remote terminal tab to access chat
            </div>
          )}
        </div>
        <div style={{ display: mainView === "terminal" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <TerminalWorkspace
            hosts={hosts}
            hostConnections={connections}
            localSessions={localSessions}
            allRemoteSessions={allRemoteSessions}
            activeSessionId={activeTerminalSessionId}
            visible={mainView === "terminal"}
            onSelect={setActiveTerminalSessionId}
            onCreateRemoteSession={(hostId, mode) => void handleCreateRemoteSession(hostId, mode)}
            onCreateLocalSession={() => void handleCreateLocalSession()}
            onRemoteSessionStatusChange={handleRemoteSessionStatusChange}
            onLocalSessionStatusChange={handleLocalSessionStatusChange}
            onKillRemoteSession={(id) => void handleKillRemoteSession(id)}
            onKillLocalSession={(id) => void handleKillLocalSession(id)}
          />
        </div>
        <div style={{ display: mainView === "logs" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <LogViewer baseUrl={activeHostUrl} />
        </div>
        <div style={{ display: mainView === "settings" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="settings-view">
            <div className="settings-header">
              <h2>Settings</h2>
              <p className="muted">Host connections and configuration</p>
            </div>
            <div className="settings-content">
              <div className="settings-section">
                <h3>Active Host: {activeHost?.name ?? "None (Local)"}</h3>
                {activeSystemStatus && (
                  <div className="settings-info-grid">
                    <div className="settings-info-item">
                      <span className="settings-info-label">Bind address</span>
                      <span className="settings-info-value">{activeSystemStatus.connection?.bindHost ?? "—"}:{activeSystemStatus.connection?.bindPort ?? "—"}</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Telegram</span>
                      <span className="settings-info-value">{activeSystemStatus.connection?.telegramEnabled ? "Enabled" : "Disabled"}</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Active agents</span>
                      <span className="settings-info-value">{activeSystemStatus.activeAgents?.length ?? 0}</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Terminal sessions</span>
                      <span className="settings-info-value">{activeTerminalSessions.length} total</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Allowed CIDRs</span>
                      <span className="settings-info-value">{activeSystemStatus.connection?.allowedCidrs?.join(", ") ?? "—"}</span>
                    </div>
                  </div>
                )}
                {!activeSystemStatus && (
                  <p className="muted">Select a remote terminal tab to view host settings</p>
                )}
              </div>
            </div>
          </div>
        </div>
      </section>

      <InspectorPanel
        activeHostName={activeHost?.name ?? null}
        activeRun={activeRun}
        runs={activeRuns}
        runDetail={runDetail}
        systemStatus={activeSystemStatus}
        terminalSessionCount={
          activeTerminalSessions.filter((s) => s.status === "created" || s.status === "running").length
          + localSessions.filter((s) => s.status === "running").length
        }
        events={events}
        activeRunId={activeRunId}
        onSelectRun={setActiveRunId}
        onResolveApproval={(id, status) => void handleResolveApproval(id, status)}
      />
    </main>
  );
}

export default App;

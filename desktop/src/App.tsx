import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, defaultBaseUrl } from "./api";
import { wsUrlFromHttp } from "./api";
import { appLog } from "./log";
import type {
  Conversation,
  ConversationDetail,
  EventEnvelope,
  Message,
  RunDetail,
  RunRecord,
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
  const [baseUrl, setBaseUrl] = useState(defaultBaseUrl);
  const [draftBaseUrl, setDraftBaseUrl] = useState(defaultBaseUrl);
  const [mainView, setMainView] = useState<MainView>("terminal");
  const [connectionState, setConnectionState] = useState<"idle" | "connecting" | "connected" | "error">("idle");
  const [connectionMessage, setConnectionMessage] = useState("Not connected");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [messageInput, setMessageInput] = useState("");
  const [events, setEvents] = useState<EventEnvelope[]>([]);
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [runDetail, setRunDetail] = useState<RunDetail | null>(null);
  const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
  const [terminalSessions, setTerminalSessions] = useState<TerminalSession[]>([]);
  const [activeTerminalSessionId, setActiveTerminalSessionId] = useState<string | null>(null);
  const [actionError, setActionError] = useState("");

  // Refs to avoid stale closures in WebSocket handlers
  const activeRunIdRef = useRef(activeRunId);
  useEffect(() => { activeRunIdRef.current = activeRunId; }, [activeRunId]);

  const selectedConversation = useMemo(
    () => conversations.find((item) => item.id === selectedConversationId) ?? null,
    [conversations, selectedConversationId],
  );
  const activeRun = useMemo(
    () => runs.find((item) => item.id === activeRunId) ?? null,
    [runs, activeRunId],
  );

  // --- Data fetching ---

  async function refreshConversations(currentBaseUrl = baseUrl) {
    const data = await api<Conversation[]>(currentBaseUrl, "/api/conversations");
    setConversations(data);
    if (!selectedConversationId && data.length > 0) {
      setSelectedConversationId(data[0].id);
    }
  }

  async function refreshRuns(currentBaseUrl = baseUrl) {
    const data = await api<RunRecord[]>(currentBaseUrl, "/api/runs");
    setRuns(data);
    if (!activeRunId && data.length > 0) {
      setActiveRunId(data[0].id);
    }
  }

  async function refreshSystemStatus(currentBaseUrl = baseUrl) {
    const data = await api<SystemStatus>(currentBaseUrl, "/api/system/status");
    setSystemStatus(data);
  }

  async function refreshTerminalSessions(currentBaseUrl = baseUrl) {
    const data = await api<TerminalSession[]>(currentBaseUrl, "/api/terminal/sessions");
    setTerminalSessions(data);
    if (!activeTerminalSessionId && data.length > 0) {
      setActiveTerminalSessionId(data[0].id);
    }
  }

  async function loadConversation(conversationId: string, currentBaseUrl = baseUrl) {
    const data = await api<ConversationDetail>(currentBaseUrl, `/api/conversations/${conversationId}`);
    setSelectedConversationId(conversationId);
    setMessages(data.messages);
  }

  async function loadRun(runId: string, currentBaseUrl = baseUrl) {
    const data = await api<RunDetail>(currentBaseUrl, `/api/runs/${runId}`);
    setActiveRunId(runId);
    setRunDetail(data);
  }

  async function initialize(currentBaseUrl = baseUrl) {
    try {
      const health = await api<{ ok: boolean; telegramEnabled?: boolean }>(currentBaseUrl, "/health");
      setConnectionState("connected");
      setConnectionMessage(health.telegramEnabled ? "Daemon reachable · Telegram bridge on" : "Daemon reachable");
      await Promise.all([
        refreshConversations(currentBaseUrl),
        refreshRuns(currentBaseUrl),
        refreshSystemStatus(currentBaseUrl),
        refreshTerminalSessions(currentBaseUrl),
      ]);
    } catch (error) {
      const msg = error instanceof Error ? error.message : "Connection failed";
      appLog.error("app", `Initialize failed: ${msg}`);
      setConnectionState("error");
      setConnectionMessage(msg);
    }
  }

  // --- Effects ---

  useEffect(() => {
    void initialize(baseUrl);
  }, []);

  useEffect(() => {
    if (!selectedConversationId) return;
    loadConversation(selectedConversationId).catch((error) => {
      setConnectionMessage(error instanceof Error ? error.message : "Failed to load conversation");
    });
  }, [selectedConversationId]);

  useEffect(() => {
    if (!activeRunId) return;
    loadRun(activeRunId).catch((error) => {
      setConnectionMessage(error instanceof Error ? error.message : "Failed to load run");
    });
  }, [activeRunId]);

  // Conversation WebSocket — debounced refreshes, ref-based activeRunId
  useEffect(() => {
    if (!selectedConversationId) return;
    setConnectionState("connecting");
    setConnectionMessage("Connecting WebSocket…");
    appLog.info("conv-ws", `Connecting for conversation ${selectedConversationId.slice(0, 8)}`);

    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const ws = new WebSocket(wsUrlFromHttp(baseUrl));

    ws.onopen = () => {
      appLog.info("conv-ws", "Connected");
      setConnectionState("connected");
      setConnectionMessage("Realtime connected");
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
        // Debounce run/status refreshes to avoid hammering the API
        if (!refreshTimer) {
          refreshTimer = setTimeout(() => {
            refreshTimer = null;
            void refreshRuns();
            void refreshSystemStatus();
          }, 500);
        }
        const currentRunId = activeRunIdRef.current;
        if (envelope.runId && currentRunId === envelope.runId) {
          void loadRun(envelope.runId);
        }
      } else if (data.op === "error") {
        const msg = data.message ?? "WebSocket error";
        appLog.error("conv-ws", `Server error: ${msg}`);
        setConnectionState("error");
        setConnectionMessage(msg);
      }
    };

    ws.onerror = () => {
      appLog.error("conv-ws", "WebSocket error event");
      setConnectionState("error");
      setConnectionMessage("WebSocket error");
    };
    ws.onclose = (event) => {
      appLog.warn("conv-ws", `Disconnected: code=${event.code} reason=${event.reason || "none"}`);
      setConnectionState((current) => (current === "error" ? current : "idle"));
      setConnectionMessage("Realtime disconnected");
    };

    return () => {
      if (refreshTimer) clearTimeout(refreshTimer);
      ws.close();
    };
  }, [baseUrl, selectedConversationId]);

  // --- Action handlers ---

  const handleSendMessage = useCallback(async (event: FormEvent) => {
    event.preventDefault();
    if (!selectedConversationId || !messageInput.trim()) return;
    const content = messageInput.trim();
    setMessageInput("");
    setActionError("");
    await api(baseUrl, `/api/conversations/${selectedConversationId}/messages`, {
      method: "POST",
      body: JSON.stringify({ content }),
    });
    const run = await api<{ runId: string }>(baseUrl, "/api/runs", {
      method: "POST",
      body: JSON.stringify({ conversationId: selectedConversationId, content }),
    });
    setActiveRunId(run.runId);
    await Promise.all([refreshRuns(), refreshSystemStatus()]);
  }, [baseUrl, selectedConversationId, messageInput]);

  const handleApplyBaseUrl = useCallback(async (event: FormEvent) => {
    event.preventDefault();
    localStorage.setItem("ghost-protocol.baseUrl", draftBaseUrl);
    setBaseUrl(draftBaseUrl);
    setConnectionState("connecting");
    setConnectionMessage("Reconnecting…");
    await initialize(draftBaseUrl);
  }, [draftBaseUrl]);

  const handleRetryRun = useCallback(async () => {
    if (!activeRunId) return;
    try {
      const data = await api<{ runId: string }>(baseUrl, `/api/runs/${activeRunId}/retry`, { method: "POST" });
      setActiveRunId(data.runId);
      await Promise.all([refreshRuns(), refreshSystemStatus()]);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Retry failed");
    }
  }, [baseUrl, activeRunId]);

  const handleCancelRun = useCallback(async () => {
    if (!activeRunId) return;
    try {
      await api(baseUrl, `/api/runs/${activeRunId}/cancel`, { method: "POST" });
      await Promise.all([refreshRuns(), refreshSystemStatus(), loadRun(activeRunId)]);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Cancel failed");
    }
  }, [baseUrl, activeRunId]);

  const handleCreateTerminalSession = useCallback(async (mode: "agent" | "rescue_shell") => {
    try {
      const session = await api<TerminalSession>(baseUrl, "/api/terminal/sessions", {
        method: "POST",
        body: JSON.stringify({ mode }),
      });
      setActiveTerminalSessionId(session.id);
      await refreshTerminalSessions();
      setMainView("terminal");
    } catch (error) {
      // TerminalWorkspace handles its own errors via the hook
    }
  }, [baseUrl]);

  const handleTerminalSessionStatusChange = useCallback((_session: TerminalSession) => {
    void refreshTerminalSessions();
  }, []);

  const handleKillTerminalSession = useCallback(async (sessionId: string) => {
    try {
      await api(baseUrl, `/api/terminal/sessions/${sessionId}/terminate`, { method: "POST" });
      // Refresh list so the terminated session moves out of active tabs
      const updated = await api<TerminalSession[]>(baseUrl, "/api/terminal/sessions");
      setTerminalSessions(updated);
      if (activeTerminalSessionId === sessionId) {
        // Switch to another active session, or null
        const nextActive = updated.find((s) => s.id !== sessionId && (s.status === "created" || s.status === "running"));
        setActiveTerminalSessionId(nextActive?.id ?? null);
      }
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Terminate session failed");
    }
  }, [baseUrl, activeTerminalSessionId]);

  const handleResolveApproval = useCallback(async (approvalId: string, status: "approved" | "rejected") => {
    try {
      await api(baseUrl, `/api/approvals/${approvalId}/resolve`, {
        method: "POST",
        body: JSON.stringify({ status, resolvedBy: "ghost-protocol-app" }),
      });
      await Promise.all([refreshSystemStatus(), activeRunId ? loadRun(activeRunId) : Promise.resolve()]);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Approval resolution failed");
    }
  }, [baseUrl, activeRunId]);

  // --- Render ---

  return (
    <main className="shell">
      <Sidebar
        connectionState={connectionState}
        connectionMessage={connectionMessage}
        mainView={mainView}
        onChangeView={setMainView}
      />

      <section className="main-panel">
        {/* CSS visibility toggle — both views stay mounted to preserve terminal state */}
        <div style={{ display: mainView === "chat" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="terminal-header">
            <div>
              <h2>{selectedConversation?.title || "Chat"}</h2>
              <p className="muted">Conversation with Hermes agent</p>
            </div>
          </div>
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
        </div>
        <div style={{ display: mainView === "terminal" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <TerminalWorkspace
            baseUrl={baseUrl}
            sessions={terminalSessions}
            activeSessionId={activeTerminalSessionId}
            visible={mainView === "terminal"}
            onSelect={setActiveTerminalSessionId}
            onCreateSession={(mode) => void handleCreateTerminalSession(mode)}
            onSessionStatusChange={handleTerminalSessionStatusChange}
            onRefreshSessions={() => void refreshTerminalSessions()}
            onKillSession={(id) => void handleKillTerminalSession(id)}
          />
        </div>
        <div style={{ display: mainView === "logs" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <LogViewer baseUrl={baseUrl} />
        </div>
        <div style={{ display: mainView === "settings" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="settings-view">
            <div className="settings-header">
              <h2>Settings</h2>
              <p className="muted">Daemon connection and configuration</p>
            </div>
            <div className="settings-content">
              <form className="settings-form" onSubmit={(e) => void handleApplyBaseUrl(e)}>
                <label className="label">
                  Daemon URL
                  <input value={draftBaseUrl} onChange={(e) => setDraftBaseUrl(e.currentTarget.value)} />
                </label>
                <div className="settings-connection-row">
                  <span className={`status-dot ${connectionState}`} />
                  <span className="settings-connection-text">{connectionMessage}</span>
                </div>
                <button type="submit" className="btn-primary">Connect</button>
              </form>

              <div className="settings-section">
                <h3>System Info</h3>
                <div className="settings-info-grid">
                  <div className="settings-info-item">
                    <span className="settings-info-label">Bind address</span>
                    <span className="settings-info-value">{systemStatus?.connection?.bindHost ?? "—"}:{systemStatus?.connection?.bindPort ?? "—"}</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Telegram</span>
                    <span className="settings-info-value">{systemStatus?.connection?.telegramEnabled ? "Enabled" : "Disabled"}</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Active agents</span>
                    <span className="settings-info-value">{systemStatus?.activeAgents?.length ?? 0}</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Terminal sessions</span>
                    <span className="settings-info-value">{terminalSessions.length} total, {terminalSessions.filter((s) => s.status === "running" || s.status === "created").length} active</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Allowed CIDRs</span>
                    <span className="settings-info-value">{systemStatus?.connection?.allowedCidrs?.join(", ") ?? "—"}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      <InspectorPanel
        activeRun={activeRun}
        runs={runs}
        runDetail={runDetail}
        systemStatus={systemStatus}
        terminalSessionCount={terminalSessions.filter((s) => s.status === "created" || s.status === "running").length}
        events={events}
        activeRunId={activeRunId}
        onSelectRun={setActiveRunId}
        onResolveApproval={(id, status) => void handleResolveApproval(id, status)}
      />
    </main>
  );
}

export default App;

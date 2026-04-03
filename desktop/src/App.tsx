import { FormEvent, useEffect, useMemo, useState } from "react";
import "./App.css";

type Conversation = {
  id: string;
  title?: string | null;
  createdAt: string;
  updatedAt: string;
};

type Message = {
  id: string;
  conversationId: string;
  role: "user" | "assistant" | "system";
  content: string;
  createdAt: string;
  runId?: string | null;
};

type RunRecord = {
  id: string;
  conversationId: string;
  status: string;
  waitingReason?: string | null;
  currentStep?: string | null;
  model?: string | null;
  tokenUsage: number;
  costEstimate: number;
  startedAt: string;
  finishedAt?: string | null;
  heartbeatAt?: string | null;
};

type AgentRecord = {
  id: string;
  runId: string;
  lineage: string;
  name: string;
  type: string;
  status: string;
  currentTask?: string | null;
  model?: string | null;
  tokenUsage: number;
  startedAt: string;
  finishedAt?: string | null;
  heartbeatAt?: string | null;
};

type ApprovalRecord = {
  id: string;
  runId: string;
  type: string;
  status: string;
  payload: Record<string, unknown>;
  requestedAt: string;
};

type UsageRecord = {
  id: string;
  entityType: string;
  entityId: string;
  model?: string | null;
  tokensIn: number;
  tokensOut: number;
  cost: number;
  createdAt: string;
};

type EventEnvelope = {
  eventId: string;
  type: string;
  ts: string;
  seq: number;
  conversationId?: string | null;
  runId?: string | null;
  summary: string;
  payload: Record<string, unknown>;
};

type RunTimelineItem = {
  id: number;
  run_id: string;
  seq: number;
  event_type: string;
  summary: string;
  created_at: string;
  payload: Record<string, unknown>;
};

type RunDetail = {
  run: RunRecord;
  live?: Record<string, unknown> | null;
  timeline: RunTimelineItem[];
  attempts: Array<Record<string, unknown>>;
  agents: AgentRecord[];
  approvals: ApprovalRecord[];
  artifacts: Array<Record<string, unknown>>;
  usage: UsageRecord[];
};

type SystemStatus = {
  activeAgents: AgentRecord[];
  pendingApprovals: ApprovalRecord[];
  recentRuns: RunRecord[];
  connection: {
    bindHost: string;
    bindPort: number;
    telegramEnabled: boolean;
    allowedCidrs: string[];
  };
};

type ConversationDetail = {
  conversation: Conversation;
  messages: Message[];
  runs: RunRecord[];
};

const defaultBaseUrl = localStorage.getItem("hermes.desktop.baseUrl") ?? "http://127.0.0.1:8787";

function wsUrlFromHttp(baseUrl: string) {
  if (baseUrl.startsWith("https://")) return baseUrl.replace("https://", "wss://") + "/ws";
  if (baseUrl.startsWith("http://")) return baseUrl.replace("http://", "ws://") + "/ws";
  return `ws://${baseUrl}/ws`;
}

function fmt(ts?: string | null) {
  if (!ts) return "—";
  try {
    return new Date(ts).toLocaleString();
  } catch {
    return ts;
  }
}

async function api<T>(baseUrl: string, path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${baseUrl}${path}`, {
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    ...init,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `Request failed: ${res.status}`);
  }
  return res.json() as Promise<T>;
}

function App() {
  const [baseUrl, setBaseUrl] = useState(defaultBaseUrl);
  const [draftBaseUrl, setDraftBaseUrl] = useState(defaultBaseUrl);
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
  const [actionError, setActionError] = useState("");

  const selectedConversation = useMemo(
    () => conversations.find((item) => item.id === selectedConversationId) ?? null,
    [conversations, selectedConversationId],
  );
  const activeRun = useMemo(
    () => runs.find((item) => item.id === activeRunId) ?? null,
    [runs, activeRunId],
  );

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
      await Promise.all([refreshConversations(currentBaseUrl), refreshRuns(currentBaseUrl), refreshSystemStatus(currentBaseUrl)]);
    } catch (error) {
      setConnectionState("error");
      setConnectionMessage(error instanceof Error ? error.message : "Connection failed");
    }
  }

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

  useEffect(() => {
    if (!selectedConversationId) return;
    setConnectionState("connecting");
    setConnectionMessage("Connecting WebSocket…");
    const ws = new WebSocket(wsUrlFromHttp(baseUrl));
    ws.onopen = () => {
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
        void refreshRuns();
        void refreshSystemStatus();
        if (envelope.runId && activeRunId === envelope.runId) {
          void loadRun(envelope.runId);
        }
      } else if (data.op === "error") {
        setConnectionState("error");
        setConnectionMessage(data.message ?? "WebSocket error");
      }
    };
    ws.onerror = () => {
      setConnectionState("error");
      setConnectionMessage("WebSocket error");
    };
    ws.onclose = () => {
      setConnectionState((current) => (current === "error" ? current : "idle"));
      setConnectionMessage("Realtime disconnected");
    };
    return () => ws.close();
  }, [baseUrl, selectedConversationId, activeRunId]);

  async function handleCreateConversation() {
    const conversation = await api<Conversation>(baseUrl, "/api/conversations", { method: "POST", body: JSON.stringify({ title: "New conversation" }) });
    await refreshConversations();
    setSelectedConversationId(conversation.id);
    setEvents([]);
  }

  async function handleSendMessage(event: FormEvent) {
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
  }

  async function handleApplyBaseUrl(event: FormEvent) {
    event.preventDefault();
    localStorage.setItem("hermes.desktop.baseUrl", draftBaseUrl);
    setBaseUrl(draftBaseUrl);
    setConnectionState("connecting");
    setConnectionMessage("Reconnecting…");
    await initialize(draftBaseUrl);
  }

  async function handleRetryRun() {
    if (!activeRunId) return;
    try {
      const data = await api<{ runId: string }>(baseUrl, `/api/runs/${activeRunId}/retry`, { method: "POST" });
      setActiveRunId(data.runId);
      await Promise.all([refreshRuns(), refreshSystemStatus()]);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Retry failed");
    }
  }

  async function handleCancelRun() {
    if (!activeRunId) return;
    try {
      await api(baseUrl, `/api/runs/${activeRunId}/cancel`, { method: "POST" });
      await Promise.all([refreshRuns(), refreshSystemStatus(), loadRun(activeRunId)]);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Cancel failed");
    }
  }

  async function handleResolveApproval(approvalId: string, status: "approved" | "rejected") {
    try {
      await api(baseUrl, `/api/approvals/${approvalId}/resolve`, {
        method: "POST",
        body: JSON.stringify({ status, resolvedBy: "desktop-app" }),
      });
      await Promise.all([refreshSystemStatus(), activeRunId ? loadRun(activeRunId) : Promise.resolve()]);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Approval resolution failed");
    }
  }

  return (
    <main className="shell">
      <aside className="sidebar panel">
        <div className="panel-header">
          <h2>Conversations</h2>
          <button onClick={() => void handleCreateConversation()}>New</button>
        </div>
        <form className="stack-sm" onSubmit={(event) => void handleApplyBaseUrl(event)}>
          <label className="label">
            Daemon URL
            <input value={draftBaseUrl} onChange={(event) => setDraftBaseUrl(event.currentTarget.value)} />
          </label>
          <button type="submit">Connect</button>
        </form>
        <div className="status-block">
          <strong>Connection</strong>
          <span>{connectionMessage}</span>
          <span className={`status-pill ${connectionState}`}>{connectionState}</span>
        </div>
        <div className="conversation-list">
          {conversations.map((conversation) => (
            <button
              key={conversation.id}
              className={`conversation-item ${conversation.id === selectedConversationId ? "selected" : ""}`}
              onClick={() => setSelectedConversationId(conversation.id)}
            >
              <strong>{conversation.title || "Untitled conversation"}</strong>
              <span>{fmt(conversation.updatedAt)}</span>
            </button>
          ))}
        </div>
      </aside>

      <section className="chat panel">
        <div className="panel-header">
          <div>
            <h2>{selectedConversation?.title || "Chat"}</h2>
            <p className="muted">Primary desktop interface over the shared daemon API and event stream</p>
          </div>
          <div className="toolbar">
            <button onClick={() => void handleRetryRun()} disabled={!activeRunId}>Retry run</button>
            <button onClick={() => void handleCancelRun()} disabled={!activeRunId}>Cancel run</button>
          </div>
        </div>

        {actionError ? <div className="error-banner">{actionError}</div> : null}

        <div className="messages">
          {messages.map((message) => (
            <article key={message.id} className={`message ${message.role}`}>
              <header>
                <strong>{message.role}</strong>
                <span>{fmt(message.createdAt)}</span>
              </header>
              <pre>{message.content}</pre>
            </article>
          ))}
        </div>

        <form className="composer" onSubmit={(event) => void handleSendMessage(event)}>
          <textarea
            value={messageInput}
            onChange={(event) => setMessageInput(event.currentTarget.value)}
            placeholder="Send a message to Hermes…"
            rows={4}
          />
          <button type="submit" disabled={!selectedConversationId || !messageInput.trim()}>
            Send and run
          </button>
        </form>
      </section>

      <aside className="inspector panel">
        <div className="panel-header">
          <div>
            <h2>Run live</h2>
            <p className="muted">Desktop-first observability</p>
          </div>
        </div>

        <div className="stats-grid">
          <div><span>Status</span><strong>{activeRun?.status ?? "—"}</strong></div>
          <div><span>Current step</span><strong>{activeRun?.currentStep ?? "—"}</strong></div>
          <div><span>Tokens</span><strong>{activeRun?.tokenUsage ?? 0}</strong></div>
          <div><span>Cost</span><strong>{activeRun?.costEstimate?.toFixed?.(4) ?? "0.0000"}</strong></div>
          <div><span>Approvals</span><strong>{systemStatus?.pendingApprovals.length ?? 0}</strong></div>
          <div><span>Telegram bridge</span><strong>{systemStatus?.connection.telegramEnabled ? "enabled" : "off"}</strong></div>
        </div>

        <section className="stack-md">
          <h3>Runs</h3>
          <div className="run-list">
            {runs.map((run) => (
              <button key={run.id} className={`run-item ${run.id === activeRunId ? "selected" : ""}`} onClick={() => setActiveRunId(run.id)}>
                <strong>{run.status}</strong>
                <span>{run.currentStep || run.id.slice(0, 8)}</span>
              </button>
            ))}
          </div>
        </section>

        <section className="stack-md">
          <h3>Agents</h3>
          <div className="timeline compact">
            {(runDetail?.agents ?? systemStatus?.activeAgents ?? []).map((agent) => (
              <article key={agent.id} className="timeline-item">
                <strong>{agent.name} · {agent.status}</strong>
                <span>{agent.currentTask || agent.type}</span>
              </article>
            ))}
          </div>
        </section>

        <section className="stack-md">
          <h3>Approvals</h3>
          <div className="timeline compact">
            {(systemStatus?.pendingApprovals ?? []).length === 0 ? <div className="empty-state">No pending approvals</div> : null}
            {(systemStatus?.pendingApprovals ?? []).map((approval) => (
              <article key={approval.id} className="timeline-item approval-item">
                <strong>{approval.type}</strong>
                <span>{JSON.stringify(approval.payload)}</span>
                <div className="toolbar">
                  <button onClick={() => void handleResolveApproval(approval.id, "approved")}>Approve</button>
                  <button onClick={() => void handleResolveApproval(approval.id, "rejected")}>Reject</button>
                </div>
              </article>
            ))}
          </div>
        </section>

        <section className="stack-md">
          <h3>Attempts & usage</h3>
          <div className="timeline compact">
            {(runDetail?.attempts ?? []).map((attempt) => (
              <article key={String(attempt.id)} className="timeline-item">
                <strong>Attempt {String(attempt.attemptNumber ?? "?")}</strong>
                <span>{String(attempt.status ?? "unknown")}</span>
              </article>
            ))}
            {(runDetail?.usage ?? []).map((usage) => (
              <article key={usage.id} className="timeline-item">
                <strong>{usage.model || "model"}</strong>
                <span>{usage.tokensIn + usage.tokensOut} tokens · ${usage.cost.toFixed(4)}</span>
              </article>
            ))}
          </div>
        </section>

        <section className="stack-md">
          <h3>Timeline</h3>
          <div className="timeline">
            {(runDetail?.timeline ?? []).map((item) => (
              <article key={`${item.seq}`} className="timeline-item">
                <strong>{item.event_type}</strong>
                <span>{item.summary}</span>
              </article>
            ))}
          </div>
        </section>

        <section className="stack-md">
          <h3>Live events</h3>
          <div className="timeline compact">
            {events.slice(-20).reverse().map((event) => (
              <article key={event.eventId} className="timeline-item">
                <strong>{event.type}</strong>
                <span>{event.summary}</span>
              </article>
            ))}
          </div>
        </section>
      </aside>
    </main>
  );
}

export default App;

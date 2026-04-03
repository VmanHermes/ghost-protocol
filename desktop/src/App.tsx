import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
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

type RunDetail = {
  run: RunRecord;
  live?: Record<string, unknown> | null;
  timeline: Array<Record<string, unknown>>;
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
  const wsRef = useRef<WebSocket | null>(null);
  const selectedConversation = useMemo(
    () => conversations.find((item) => item.id === selectedConversationId) ?? null,
    [conversations, selectedConversationId],
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

  async function loadConversation(conversationId: string, currentBaseUrl = baseUrl) {
    const data = await api<{ conversation: Conversation; messages: Message[] }>(currentBaseUrl, `/api/conversations/${conversationId}`);
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
      await api<{ ok: boolean }>(currentBaseUrl, "/health");
      setConnectionState("connected");
      setConnectionMessage("Daemon reachable");
      await Promise.all([refreshConversations(currentBaseUrl), refreshRuns(currentBaseUrl)]);
    } catch (error) {
      setConnectionState("error");
      setConnectionMessage(error instanceof Error ? error.message : "Connection failed");
    }
  }

  useEffect(() => {
    initialize(baseUrl);
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
    wsRef.current = ws;
    ws.onopen = () => {
      setConnectionState("connected");
      setConnectionMessage("Realtime connected");
      ws.send(JSON.stringify({ op: "subscribe", conversationId: selectedConversationId, afterSeq: 0 }));
    };
    ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.op === "event") {
        const envelope = data.event as EventEnvelope;
        setEvents((current) => [...current.slice(-199), envelope]);
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
            setMessages((current) => {
              if (current.some((item) => item.id === nextMessage.id)) return current;
              return [...current, nextMessage];
            });
          }
        }
        if (envelope.runId) {
          refreshRuns().catch(() => undefined);
          if (activeRunId === envelope.runId) {
            loadRun(envelope.runId).catch(() => undefined);
          }
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
    return () => {
      ws.close();
      wsRef.current = null;
    };
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
    await api(baseUrl, `/api/conversations/${selectedConversationId}/messages`, {
      method: "POST",
      body: JSON.stringify({ content }),
    });
    const run = await api<{ runId: string }>(baseUrl, "/api/runs", {
      method: "POST",
      body: JSON.stringify({ conversationId: selectedConversationId, content }),
    });
    setActiveRunId(run.runId);
    await refreshRuns();
  }

  async function handleApplyBaseUrl(event: FormEvent) {
    event.preventDefault();
    localStorage.setItem("hermes.desktop.baseUrl", draftBaseUrl);
    setBaseUrl(draftBaseUrl);
    setConnectionState("connecting");
    setConnectionMessage("Reconnecting…");
    await initialize(draftBaseUrl);
  }

  const pendingApprovals = runs.filter((run) => run.status === "waiting").length;
  const activeRun = runs.find((run) => run.id === activeRunId) ?? null;

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
            <p className="muted">Primary desktop interface over the shared daemon API</p>
          </div>
          <div className={`status-pill ${connectionState}`}>{connectionMessage}</div>
        </div>

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
          <h2>Run live</h2>
        </div>
        <div className="stats-grid">
          <div>
            <span>Status</span>
            <strong>{activeRun?.status ?? "—"}</strong>
          </div>
          <div>
            <span>Current step</span>
            <strong>{activeRun?.currentStep ?? "—"}</strong>
          </div>
          <div>
            <span>Tokens</span>
            <strong>{activeRun?.tokenUsage ?? 0}</strong>
          </div>
          <div>
            <span>Cost</span>
            <strong>{activeRun?.costEstimate?.toFixed?.(4) ?? "0.0000"}</strong>
          </div>
          <div>
            <span>Pending approvals</span>
            <strong>{pendingApprovals}</strong>
          </div>
          <div>
            <span>Model</span>
            <strong>{activeRun?.model ?? "default"}</strong>
          </div>
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
          <h3>Timeline</h3>
          <div className="timeline">
            {(runDetail?.timeline ?? []).map((item) => (
              <article key={`${item.seq}`} className="timeline-item">
                <strong>{String(item.event_type)}</strong>
                <span>{String(item.summary)}</span>
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

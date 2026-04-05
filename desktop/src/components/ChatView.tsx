import { useCallback, useEffect, useRef, useState } from "react";
import { listAgents, createChatSession, listChatMessages, sendChatMessage } from "../api";
import type { AgentInfo, ChatMessage, SavedHost } from "../types";

type Props = {
  daemonUrl: string;
  hosts: SavedHost[];
};

export function ChatView({ daemonUrl }: Props) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [activeChatSessionId, setActiveChatSessionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const pollIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Fetch agents on mount
  useEffect(() => {
    listAgents(daemonUrl)
      .then((a) => {
        setAgents(a);
        if (a.length > 0) setSelectedAgent(a[0].id);
      })
      .catch(() => {
        // ignore — daemon may not have agents endpoint yet
      });
  }, [daemonUrl]);

  // Scroll to bottom when messages change
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  // Poll for messages when session is active
  const pollMessages = useCallback(async (sessionId: string) => {
    try {
      const msgs = await listChatMessages(daemonUrl, sessionId);
      setMessages(msgs);
    } catch {
      // ignore poll errors
    }
  }, [daemonUrl]);

  useEffect(() => {
    if (!activeChatSessionId) return;
    const id = setInterval(() => {
      void pollMessages(activeChatSessionId);
    }, 2000);
    pollIntervalRef.current = id;
    return () => clearInterval(id);
  }, [activeChatSessionId, pollMessages]);

  const handleStartChat = useCallback(async () => {
    if (!selectedAgent) return;
    setError(null);
    setLoading(true);
    try {
      const result = await createChatSession(daemonUrl, selectedAgent);
      const sessionId: string = result.session?.id ?? result.session;
      setActiveChatSessionId(sessionId);
      setMessages([]);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create chat session");
    } finally {
      setLoading(false);
    }
  }, [daemonUrl, selectedAgent]);

  const handleSend = useCallback(async () => {
    if (!activeChatSessionId || !input.trim()) return;
    const content = input.trim();
    setInput("");
    setLoading(true);
    try {
      await sendChatMessage(daemonUrl, activeChatSessionId, content);
      // Immediately fetch to show user message
      await pollMessages(activeChatSessionId);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to send message");
    } finally {
      setLoading(false);
    }
  }, [daemonUrl, activeChatSessionId, input, pollMessages]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  }, [handleSend]);

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0, padding: "12px", gap: "8px" }}>
      {/* Agent selector / session starter */}
      {!activeChatSessionId && (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", flex: 1, gap: "16px" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: "12px", minWidth: "300px" }}>
            <h2 style={{ margin: 0, fontSize: "16px" }}>Start a Chat Session</h2>
            {agents.length === 0 ? (
              <p className="muted" style={{ margin: 0, fontSize: "13px" }}>
                No agents detected. Make sure the daemon has agents registered.
              </p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
                <label style={{ fontSize: "13px" }}>Agent</label>
                <select
                  value={selectedAgent ?? ""}
                  onChange={(e) => setSelectedAgent(e.target.value || null)}
                  style={{ padding: "6px 8px", fontSize: "13px", borderRadius: "4px", border: "1px solid var(--border, #333)" }}
                >
                  {agents.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.name} ({a.agentType})
                    </option>
                  ))}
                </select>
              </div>
            )}
            <button
              onClick={() => void handleStartChat()}
              disabled={!selectedAgent || loading}
              style={{ padding: "8px 16px", fontSize: "13px", cursor: "pointer" }}
            >
              {loading ? "Starting…" : "Start Chat"}
            </button>
            {error && <p style={{ color: "var(--error, #f87171)", margin: 0, fontSize: "12px" }}>{error}</p>}
          </div>
        </div>
      )}

      {/* Active chat session */}
      {activeChatSessionId && (
        <>
          {/* Messages */}
          <div
            className="chat-messages"
            style={{
              flex: 1,
              overflowY: "auto",
              display: "flex",
              flexDirection: "column",
              gap: "8px",
              paddingBottom: "8px",
            }}
          >
            {messages.length === 0 && (
              <p className="muted" style={{ textAlign: "center", fontSize: "13px", marginTop: "24px" }}>
                Session started. Send a message to begin.
              </p>
            )}
            {messages.map((msg) => {
              if (msg.role === "system") {
                return (
                  <div
                    key={msg.id}
                    className="chat-message-system"
                    style={{
                      textAlign: "center",
                      fontSize: "11px",
                      color: "var(--muted, #666)",
                      padding: "2px 0",
                    }}
                  >
                    {msg.content}
                  </div>
                );
              }
              const isUser = msg.role === "user";
              return (
                <div
                  key={msg.id}
                  className={isUser ? "chat-message-user" : "chat-message-assistant"}
                  style={{
                    display: "flex",
                    justifyContent: isUser ? "flex-end" : "flex-start",
                  }}
                >
                  <div
                    style={{
                      maxWidth: "75%",
                      padding: "8px 12px",
                      borderRadius: "8px",
                      fontSize: "13px",
                      lineHeight: "1.5",
                      background: isUser ? "var(--accent, #3b82f6)" : "var(--surface2, #1e1e2e)",
                      color: isUser ? "#fff" : "inherit",
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                    }}
                  >
                    {msg.content}
                  </div>
                </div>
              );
            })}
            <div ref={messagesEndRef} />
          </div>

          {/* Error */}
          {error && (
            <div className="error-banner" style={{ fontSize: "12px" }}>{error}</div>
          )}

          {/* Input */}
          <div
            className="chat-input"
            style={{ display: "flex", gap: "8px", alignItems: "flex-end" }}
          >
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Send a message… (Shift+Enter for newline)"
              rows={3}
              disabled={loading}
              style={{ flex: 1, resize: "none", padding: "8px", fontSize: "13px", borderRadius: "4px", border: "1px solid var(--border, #333)", fontFamily: "inherit" }}
            />
            <button
              onClick={() => void handleSend()}
              disabled={!input.trim() || loading}
              style={{ padding: "8px 16px", fontSize: "13px", cursor: "pointer", alignSelf: "flex-end" }}
            >
              {loading ? "…" : "Send"}
            </button>
          </div>
        </>
      )}
    </div>
  );
}

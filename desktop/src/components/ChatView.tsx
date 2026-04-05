import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listAgents, createChatSession } from "../api";
import { useTerminalSocket } from "../hooks/useTerminalSocket";
import type { AgentInfo, SavedHost } from "../types";

type Props = {
  daemonUrl: string;
  hosts: SavedHost[];
};

const LOCAL_DAEMON = "http://127.0.0.1:8787";

export function ChatView({ daemonUrl }: Props) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [activeChatSessionId, setActiveChatSessionId] = useState<string | null>(null);
  const [activeAgentName, setActiveAgentName] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Terminal refs for embedded agent TUI
  const termContainerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  // Fetch agents on mount
  useEffect(() => {
    listAgents(daemonUrl)
      .then((a) => {
        setAgents(a);
        if (a.length > 0) setSelectedAgent(a[0].id);
      })
      .catch(() => {});
  }, [daemonUrl]);

  // Initialize xterm.js when chat session starts
  useEffect(() => {
    if (!activeChatSessionId || !termContainerRef.current) return;

    const terminal = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      theme: {
        background: "#0d1117",
        foreground: "#e6edf3",
        cursor: "#e6edf3",
        selectionBackground: "#264f78",
      },
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(termContainerRef.current);
    fitAddon.fit();

    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    // Handle resize
    const observer = new ResizeObserver(() => {
      try { fitAddon.fit(); } catch {}
    });
    observer.observe(termContainerRef.current);

    return () => {
      observer.disconnect();
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [activeChatSessionId]);

  // Connect to daemon WebSocket for the chat session's terminal output
  const { sendInput, resize } = useTerminalSocket({
    baseUrl: LOCAL_DAEMON,
    sessionId: activeChatSessionId,
    terminalRef,
    isActive: !!activeChatSessionId,
  });

  // Forward xterm input to the daemon
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || !activeChatSessionId) return;

    const disposable = terminal.onData((data) => {
      sendInput(data, false);
    });

    return () => disposable.dispose();
  }, [activeChatSessionId, sendInput]);

  // Forward resize to daemon
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || !activeChatSessionId) return;

    const disposable = terminal.onResize(({ cols, rows }) => {
      resize(cols, rows);
    });

    return () => disposable.dispose();
  }, [activeChatSessionId, resize]);

  const handleStartChat = useCallback(async () => {
    if (!selectedAgent) return;
    setError(null);
    setLoading(true);
    try {
      const result = await createChatSession(daemonUrl, selectedAgent);
      const sessionId: string = result.session?.id ?? result.session;
      const agentName: string = result.agent?.name ?? selectedAgent;
      setActiveChatSessionId(sessionId);
      setActiveAgentName(agentName);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create chat session");
    } finally {
      setLoading(false);
    }
  }, [daemonUrl, selectedAgent]);

  const handleEndChat = useCallback(() => {
    setActiveChatSessionId(null);
    setActiveAgentName(null);
  }, []);

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0 }}>
      {/* Agent selector — shown when no active session */}
      {!activeChatSessionId && (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", flex: 1, gap: "16px" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: "12px", minWidth: "300px" }}>
            <h2 style={{ margin: 0, fontSize: "16px" }}>Start a Chat Session</h2>
            {agents.length === 0 ? (
              <p className="muted" style={{ margin: 0, fontSize: "13px" }}>
                No agents detected. Make sure the daemon is running.
              </p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
                <label style={{ fontSize: "13px" }}>Agent</label>
                <select
                  value={selectedAgent ?? ""}
                  onChange={(e) => setSelectedAgent(e.target.value || null)}
                  style={{ padding: "6px 8px", fontSize: "13px", borderRadius: "4px", border: "1px solid var(--border, #333)", background: "var(--surface, #1a1a2e)", color: "inherit" }}
                >
                  {agents.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.name} {a.version ? `v${a.version}` : ""} ({a.agentType})
                    </option>
                  ))}
                </select>
              </div>
            )}
            <button
              onClick={() => void handleStartChat()}
              disabled={!selectedAgent || loading}
              className="btn-primary"
              style={{ padding: "8px 16px", fontSize: "13px", cursor: "pointer" }}
            >
              {loading ? "Starting…" : "Start Chat"}
            </button>
            {error && <p style={{ color: "var(--error, #f87171)", margin: 0, fontSize: "12px" }}>{error}</p>}
          </div>
        </div>
      )}

      {/* Active chat — embedded terminal showing the agent's TUI */}
      {activeChatSessionId && (
        <>
          <div style={{ display: "flex", alignItems: "center", gap: "8px", padding: "8px 12px", borderBottom: "1px solid var(--border, rgba(255,255,255,0.06))" }}>
            <span style={{ fontSize: "13px", fontWeight: 500 }}>{activeAgentName}</span>
            <span className="muted" style={{ fontSize: "12px" }}>chat session</span>
            <div style={{ flex: 1 }} />
            <button
              onClick={handleEndChat}
              style={{ padding: "4px 12px", fontSize: "12px", cursor: "pointer", background: "transparent", border: "1px solid var(--border, #333)", borderRadius: "4px", color: "var(--text-muted, #94a3b8)" }}
            >
              End Chat
            </button>
          </div>
          <div
            ref={termContainerRef}
            style={{ flex: 1, minHeight: 0, padding: "4px" }}
          />
        </>
      )}
    </div>
  );
}

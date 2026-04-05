import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, listHosts, addHostApi, removeHostApi, listDiscoveries, acceptDiscovery, dismissDiscovery, type ApiHost } from "./api";
import { appLog } from "./log";
import type {
  DiscoveredPeer,
  HostConnection,
  LocalTerminalSession,
  SavedHost,
  TerminalSession,
} from "./types";
import { Sidebar } from "./components/Sidebar";
import { TerminalWorkspace } from "./components/TerminalWorkspace";
import { LogViewer } from "./components/LogViewer";
import { RightPanel } from "./components/RightPanel";
import { PermissionsTab } from "./components/PermissionsTab";
import "./App.css";

import { ChatView } from "./components/ChatView";

// "chat" is kept in the union so Sidebar nav items still type-check,
// but no panel renders for it until the Rust daemon adds chat support.
type MainView = "chat" | "terminal" | "logs" | "settings";

const LOCAL_DAEMON = "http://127.0.0.1:8787";

function App() {
  // Multi-host state
  const [hosts, setHosts] = useState<SavedHost[]>([]);

  const refreshHosts = useCallback(async () => {
    try {
      const apiHosts = await listHosts(LOCAL_DAEMON);
      setHosts(
        apiHosts.map((h: ApiHost) => ({ id: h.id, name: h.name, url: h.url })),
      );
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    refreshHosts();
  }, [refreshHosts]);

  const [connections, setConnections] = useState<Map<string, HostConnection>>(new Map());

  // Shared state (unchanged)
  const [mainView, setMainView] = useState<MainView>("terminal");
  const [activeTerminalSessionId, setActiveTerminalSessionId] = useState<string | null>(null);
  const [localSessions, setLocalSessions] = useState<LocalTerminalSession[]>([]);
  const [, setActionError] = useState("");

  // Discovery state
  const [discoveries, setDiscoveries] = useState<DiscoveredPeer[]>([]);

  // --- Derived state (activeHostId) ---

  const activeHostId: string | null = useMemo(() => {
    if (!activeTerminalSessionId) return null;
    if (localSessions.some((s) => s.id === activeTerminalSessionId)) return null;
    for (const [hostId, conn] of connections) {
      if (conn.sessions?.some((s) => s.id === activeTerminalSessionId)) return hostId;
    }
    return null;
  }, [activeTerminalSessionId, connections, localSessions]);

  const activeHost = activeHostId ? hosts.find((h) => h.id === activeHostId) ?? null : null;
  const activeHostUrl = activeHost?.url ?? null;

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
      appLog.info("health", `${host.name} (${host.url}): ${msg}`);
      updateConnection(host.id, { state: "connected", message: msg });
    } catch (error) {
      const msg = error instanceof Error ? error.message : "Connection failed";
      appLog.warn("health", `${host.name} (${host.url}): ${msg}`);
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
      // Only load terminal sessions — conversations, runs, and system/status
      // are not served by the Rust daemon yet.
      const sessions = await api<TerminalSession[]>(url, "/api/terminal/sessions");
      updateConnection(hostId, { sessions });
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

  // Health polling every 30s (ref keeps interval stable across host list changes)
  const hostsRef = useRef(hosts);
  useEffect(() => { hostsRef.current = hosts; }, [hosts]);
  useEffect(() => {
    const interval = setInterval(() => {
      for (const host of hostsRef.current) {
        void checkHostHealth(host);
      }
    }, 30000);
    return () => clearInterval(interval);
  }, [checkHostHealth]);

  // Discovery polling every 30s
  const refreshDiscoveries = useCallback(async () => {
    try {
      const disc = await listDiscoveries(LOCAL_DAEMON);
      setDiscoveries(disc);
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    refreshDiscoveries();
    const interval = setInterval(refreshDiscoveries, 30000);
    return () => clearInterval(interval);
  }, [refreshDiscoveries]);

  // --- Action handlers ---

  const handleCreateRemoteSession = useCallback(async (hostId: string, mode: "agent" | "rescue_shell" | "project") => {
    const host = hosts.find((h) => h.id === hostId);
    if (!host) {
      appLog.error("session", `No host found for id=${hostId}`);
      return;
    }
    appLog.info("session", `Creating ${mode} session on ${host.name} (${host.url})...`);
    try {
      const session = await api<TerminalSession>(host.url, "/api/terminal/sessions", {
        method: "POST",
        body: JSON.stringify({ mode }),
      });
      appLog.info("session", `Session created: ${session.id} (${mode}) on ${host.name}`);
      setConnections((prev) => {
        const next = new Map(prev);
        const existing = prev.get(hostId);
        if (existing) {
          next.set(hostId, { ...existing, sessions: [...(existing.sessions ?? []), session] });
        }
        return next;
      });
      setActiveTerminalSessionId(session.id);
      setMainView("terminal");
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      appLog.error("session", `Failed to create ${mode} session on ${host.name}: ${msg}`);
      setActionError(msg);
    }
  }, [hosts]);

  const handleRemoteSessionStatusChange = useCallback((session: TerminalSession) => {
    if (!activeHostId) return;
    const hostId = activeHostId;
    setConnections((prev) => {
      const conn = prev.get(hostId);
      if (!conn?.sessions) return prev;
      const updatedSessions = conn.sessions.map((s) =>
        s.id === session.id ? session : s,
      );
      const next = new Map(prev);
      next.set(hostId, { ...conn, sessions: updatedSessions });
      return next;
    });
  }, [activeHostId]);

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

  const handleAddHost = useCallback(async (name: string, url: string) => {
    try {
      const ip = new URL(url).hostname;
      await addHostApi(LOCAL_DAEMON, name, ip);
      await refreshHosts();
    } catch (error) {
      appLog.error("app", `Failed to add host: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [refreshHosts]);

  const handleRemoveHost = useCallback(async (hostId: string) => {
    const host = hosts.find((h) => h.id === hostId);
    const conn = connections.get(hostId);
    if (host && conn?.sessions) {
      const active = conn.sessions.filter((s) => s.status === "created" || s.status === "running");
      for (const session of active) {
        void api(host.url, `/api/terminal/sessions/${session.id}/terminate`, { method: "POST" }).catch(() => {});
      }
    }
    try {
      await removeHostApi(LOCAL_DAEMON, hostId);
      await refreshHosts();
    } catch (error) {
      appLog.error("app", `Failed to remove host: ${error instanceof Error ? error.message : String(error)}`);
    }
    setConnections((prev) => {
      const next = new Map(prev);
      next.delete(hostId);
      return next;
    });
    if (conn?.sessions?.some((s) => s.id === activeTerminalSessionId)) {
      setActiveTerminalSessionId(null);
    }
  }, [hosts, connections, activeTerminalSessionId, refreshHosts]);

  const handleAcceptDiscovery = useCallback(async (ip: string) => {
    try {
      await acceptDiscovery(LOCAL_DAEMON, ip);
      await refreshHosts();
      await refreshDiscoveries();
    } catch (error) {
      appLog.error("discovery", `Failed to accept: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [refreshHosts, refreshDiscoveries]);

  const handleDismissDiscovery = useCallback(async (ip: string) => {
    try {
      await dismissDiscovery(LOCAL_DAEMON, ip);
      await refreshDiscoveries();
    } catch (error) {
      appLog.error("discovery", `Failed to dismiss: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [refreshDiscoveries]);

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
        discoveries={discoveries}
        mainView={mainView}
        onChangeView={setMainView}
        onAddHost={handleAddHost}
        onRemoveHost={handleRemoveHost}
        onAcceptDiscovery={handleAcceptDiscovery}
        onDismissDiscovery={handleDismissDiscovery}
      />

      <section className="main-panel">
        <div style={{ display: mainView === "chat" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <ChatView daemonUrl={LOCAL_DAEMON} hosts={hosts} />
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
              <p className="muted">Configuration & permissions</p>
            </div>
            <div className="settings-content">
              <div className="settings-section">
                <h3>Permissions</h3>
                <PermissionsTab daemonUrl={LOCAL_DAEMON} />
              </div>
              <div className="settings-section">
                <h3>Network</h3>
                <div className="settings-info-grid">
                  <div className="settings-info-item">
                    <span className="settings-info-label">Daemon</span>
                    <span className="settings-info-value">{LOCAL_DAEMON}</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Known hosts</span>
                    <span className="settings-info-value">{hosts.length}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      <RightPanel daemonUrl={LOCAL_DAEMON} />
    </main>
  );
}

export default App;

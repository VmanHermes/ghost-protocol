import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isTauri } from "./lib/platform";
import { api, listHosts, addHostApi, removeHostApi, listDiscoveries, acceptDiscovery, dismissDiscovery, type ApiHost } from "./api";
import { appLog } from "./log";
import type {
  DiscoveredPeer,
  HostConnection,
  LocalTerminalSession,
  MainView,
  SavedHost,
  TerminalSession,
} from "./types";
import { Sidebar } from "./components/Sidebar";
import { LogViewer } from "./components/LogViewer";
import { RightPanel } from "./components/RightPanel";
import { PermissionsTab } from "./components/PermissionsTab";
import "./App.css";

import { AgentsView } from "./components/AgentsView";

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
  const [mainView, setMainView] = useState<MainView>("agents");
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
    if (localSpawnedRef.current || !isTauri()) return;
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

  const handleCreateLocalSession = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
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
      setMainView("agents");
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      appLog.error("app", `Failed to spawn local terminal: ${msg}`);
      setActionError(`Failed to spawn local terminal: ${msg}`);
    }
  }, []);

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

  // Flat list of all remote sessions for AgentsView
  const allFlatSessions: TerminalSession[] = useMemo(() => {
    const result: TerminalSession[] = [];
    connections.forEach((conn) => {
      if (conn.sessions) {
        result.push(...conn.sessions);
      }
    });
    return result;
  }, [connections]);

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
        <AgentsView
          daemonUrl={LOCAL_DAEMON}
          sessions={allFlatSessions}
          localSessions={localSessions}
          visible={mainView === "agents"}
          onCreateLocalSession={() => void handleCreateLocalSession()}
          onRefreshSessions={() => {
            hosts.forEach((h) => {
              const conn = connections.get(h.id);
              if (conn && conn.state === "connected") {
                loadHostData(h.id, h.url);
              }
            });
          }}
        />



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

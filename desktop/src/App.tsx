import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isTauri } from "./lib/platform";
import {
  api,
  listHosts,
  addHostApi,
  removeHostApi,
  listDiscoveries,
  acceptDiscovery,
  dismissDiscovery,
  getMachineInfo,
  getMachineStatus,
  listSystemLogs,
  listPermissions,
  type ApiHost,
} from "./api";
import { appLog } from "./log";
import type {
  DiscoveredPeer,
  HostConnection,
  LocalTerminalSession,
  MainView,
  MachineInfo,
  MachineStatus,
  SavedHost,
  TerminalSession,
} from "./types";
import { Sidebar } from "./components/Sidebar";
import { LogViewer } from "./components/LogViewer";
import { RightPanel } from "./components/RightPanel";
import { PermissionsTab } from "./components/PermissionsTab";
import "./App.css";

import { AgentsView } from "./components/AgentsView";
import packageJson from "../package.json";

const LOCAL_DAEMON = "http://127.0.0.1:8787";
const APP_VERSION = packageJson.version;
const HOST_POLL_MS = 10_000;
const DISCOVERY_POLL_MS = 10_000;

const LOCAL_TERMINAL_CAPABILITIES = ["supports_resume", "supports_terminal_view"];

function asLocalTerminalSession(session: LocalTerminalSession): TerminalSession {
  return {
    id: session.id,
    mode: "terminal",
    status: session.status,
    name: session.name ?? "Shell",
    workdir: session.workdir ?? "~",
    command: [],
    createdAt: session.createdAt,
    startedAt: session.createdAt,
    finishedAt: session.finishedAt ?? null,
    lastChunkAt: null,
    pid: null,
    exitCode: session.exitCode ?? null,
    projectId: null,
    parentSessionId: null,
    rootSessionId: null,
    hostId: null,
    hostName: "local",
    agentId: null,
    driverKind: "terminal_driver",
    capabilities: LOCAL_TERMINAL_CAPABILITIES,
  };
}

function App() {
  const [localMachineInfo, setLocalMachineInfo] = useState<MachineInfo | null>(null);
  const [localMachineStatus, setLocalMachineStatus] = useState<MachineStatus | null>(null);
  const [isExportingDiagnostics, setIsExportingDiagnostics] = useState(false);
  const [diagnosticsStatus, setDiagnosticsStatus] = useState("");

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

  // Local daemon sessions (fetched directly, independent of hosts)
  const [daemonSessions, setDaemonSessions] = useState<TerminalSession[]>([]);

  const refreshDaemonSessions = useCallback(async () => {
    try {
      const sessions = await api<TerminalSession[]>(LOCAL_DAEMON, "/api/terminal/sessions");
      setDaemonSessions(sessions);
    } catch {
      // ignore
    }
  }, []);

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
        machineInfo: null,
        machineStatus: null,
      });
    }
    setConnections(initial);
    for (const host of hostList) {
      void checkHostHealth(host);
    }
  }, [checkHostHealth]);

  const loadHostData = useCallback(async (host: SavedHost) => {
    try {
      const [sessionsResult, machineInfoResult, machineStatusResult] = await Promise.allSettled([
        api<TerminalSession[]>(host.url, "/api/terminal/sessions"),
        getMachineInfo(host.url),
        getMachineStatus(host.url),
      ]);

      updateConnection(host.id, {
        sessions: sessionsResult.status === "fulfilled"
          ? sessionsResult.value.map((session) => ({
            ...session,
            hostId: host.id,
            hostName: host.name,
          }))
          : null,
        machineInfo: machineInfoResult.status === "fulfilled" ? machineInfoResult.value : null,
        machineStatus: machineStatusResult.status === "fulfilled" ? machineStatusResult.value : null,
      });
    } catch (error) {
      appLog.error("app", `Failed to load data for host ${host.id}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [updateConnection]);

  const refreshAllSessions = useCallback(async () => {
    await Promise.all([
      refreshDaemonSessions(),
      ...hosts
        .filter((host) => connections.get(host.id)?.state === "connected")
        .map((host) => loadHostData(host)),
    ]);
  }, [connections, hosts, loadHostData, refreshDaemonSessions]);

  // --- Effects ---

  // Initialize hosts on mount
  useEffect(() => {
    if (hosts.length > 0) {
      initializeHosts(hosts);
    }
  }, [hosts, initializeHosts]);

  // Load local daemon sessions on mount
  useEffect(() => {
    refreshDaemonSessions();
  }, [refreshDaemonSessions]);

  const refreshLocalMachineMeta = useCallback(async () => {
    try {
      const [info, status] = await Promise.all([
        getMachineInfo(LOCAL_DAEMON),
        getMachineStatus(LOCAL_DAEMON),
      ]);
      setLocalMachineInfo(info);
      setLocalMachineStatus(status);
    } catch (error) {
      appLog.warn("app", `Failed to load local machine metadata: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, []);

  useEffect(() => {
    void refreshLocalMachineMeta();
    const interval = setInterval(() => void refreshLocalMachineMeta(), 30000);
    return () => clearInterval(interval);
  }, [refreshLocalMachineMeta]);

  // Auto-spawn a local terminal on first mount
  const localSpawnedRef = useRef(false);
  useEffect(() => {
    if (localSpawnedRef.current || !isTauri()) return;
    localSpawnedRef.current = true;
    void handleCreateLocalSession();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Load session data for hosts once they become connected
  useEffect(() => {
    for (const host of hosts) {
      const conn = connections.get(host.id);
      if (conn && conn.state === "connected" && conn.sessions === null) {
        void loadHostData(host);
      }
    }
  }, [hosts, connections, loadHostData]);

  // Health polling every 30s (ref keeps interval stable across host list changes)
  const hostsRef = useRef(hosts);
  useEffect(() => { hostsRef.current = hosts; }, [hosts]);
  useEffect(() => {
    const interval = setInterval(() => {
      for (const host of hostsRef.current) {
        void checkHostHealth(host);
      }
    }, HOST_POLL_MS);
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
    const interval = setInterval(refreshDiscoveries, DISCOVERY_POLL_MS);
    return () => clearInterval(interval);
  }, [refreshDiscoveries]);

  useEffect(() => {
    if (mainView !== "agents") return undefined;

    const refreshSessions = () => {
      void refreshAllSessions();
    };

    refreshSessions();
    const interval = setInterval(refreshSessions, 5000);
    return () => clearInterval(interval);
  }, [mainView, refreshAllSessions]);

  // --- Action handlers ---

  const handleCreateLocalSession = useCallback(async (workdir?: string | null) => {
    if (!isTauri()) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const cols = 120;
      const rows = 30;
      const requestedWorkdir = workdir?.trim() ? workdir.trim() : null;
      const sessionId = await invoke<string>("pty_spawn", { cols, rows, workdir: requestedWorkdir });
      const session: LocalTerminalSession = {
        id: sessionId,
        status: "running",
        createdAt: new Date().toISOString(),
        name: "Shell",
        workdir: requestedWorkdir ?? "~",
      };
      setLocalSessions((prev) => [...prev, session]);
      setActiveTerminalSessionId(sessionId);
      setMainView("agents");
      return session;
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      appLog.error("app", `Failed to spawn local terminal: ${msg}`);
      setActionError(`Failed to spawn local terminal: ${msg}`);
      return null;
    }
  }, []);

  const handleLocalSessionStatusChange = useCallback((session: LocalTerminalSession) => {
    setLocalSessions((prev) => {
      const existing = prev.find((entry) => entry.id === session.id);
      if (!existing) return [...prev, session];
      return prev.map((entry) => (
        entry.id === session.id
          ? {
            ...entry,
            ...session,
            name: session.name ?? entry.name,
            workdir: session.workdir ?? entry.workdir,
            finishedAt: session.finishedAt ?? entry.finishedAt,
          }
          : entry
      ));
    });
  }, []);

  const handleTerminateLocalSession = useCallback(async (sessionId: string) => {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("pty_kill", { sessionId });
    setLocalSessions((prev) => prev.map((session) => (
      session.id === sessionId
        ? { ...session, status: "terminated", finishedAt: new Date().toISOString() }
        : session
    )));
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

  const fetchBackendDiagnostics = useCallback(async (baseUrl: string) => {
    const [machineInfoResult, machineStatusResult, serverLogsResult, sessionsResult] = await Promise.allSettled([
      getMachineInfo(baseUrl),
      getMachineStatus(baseUrl),
      listSystemLogs(baseUrl, 400),
      api<TerminalSession[]>(baseUrl, "/api/terminal/sessions"),
    ]);

    return {
      machineInfo: machineInfoResult.status === "fulfilled" ? machineInfoResult.value : null,
      machineStatus: machineStatusResult.status === "fulfilled" ? machineStatusResult.value : null,
      serverLogs: serverLogsResult.status === "fulfilled" ? serverLogsResult.value : null,
      sessions: sessionsResult.status === "fulfilled" ? sessionsResult.value : null,
      errors: [
        machineInfoResult,
        machineStatusResult,
        serverLogsResult,
        sessionsResult,
      ]
        .filter((result): result is PromiseRejectedResult => result.status === "rejected")
        .map((result) => String(result.reason)),
    };
  }, []);

  const handleExportDiagnostics = useCallback(async () => {
    setIsExportingDiagnostics(true);
    setDiagnosticsStatus("");

    try {
      const [localSnapshot, remoteSnapshots] = await Promise.all([
        fetchBackendDiagnostics(LOCAL_DAEMON),
        Promise.all(hosts.map(async (host) => ({
          host,
          connection: connections.get(host.id) ?? null,
          snapshot: await fetchBackendDiagnostics(host.url),
        }))),
      ]);
      const permissions = await listPermissions(LOCAL_DAEMON).catch(() => []);

      const payload = {
        collectedAt: new Date().toISOString(),
        appVersion: APP_VERSION,
        mainView,
        activeSessionId: activeTerminalSessionId,
        local: {
          daemonUrl: LOCAL_DAEMON,
          machineInfo: localMachineInfo ?? localSnapshot.machineInfo,
          machineStatus: localMachineStatus ?? localSnapshot.machineStatus,
          daemonSessions,
          localSessions,
          discoveries,
          permissions,
          serverLogs: localSnapshot.serverLogs,
          errors: localSnapshot.errors,
        },
        hosts: remoteSnapshots.map(({ host, connection, snapshot }) => ({
          host,
          state: connection?.state ?? "idle",
          message: connection?.message ?? "",
          machineInfo: connection?.machineInfo ?? snapshot.machineInfo,
          machineStatus: connection?.machineStatus ?? snapshot.machineStatus,
          sessions: connection?.sessions ?? snapshot.sessions,
          serverLogs: snapshot.serverLogs,
          errors: snapshot.errors,
        })),
        clientLogs: [...appLog.entries],
      };

      const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `ghost-protocol-diagnostics-${new Date().toISOString().slice(0, 19)}.json`;
      anchor.click();
      URL.revokeObjectURL(url);

      setDiagnosticsStatus("Diagnostics exported.");
      appLog.info("diagnostics", "Exported diagnostics snapshot");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setDiagnosticsStatus(`Export failed: ${message}`);
      appLog.error("diagnostics", `Failed to export diagnostics: ${message}`);
    } finally {
      setIsExportingDiagnostics(false);
    }
  }, [
    activeTerminalSessionId,
    connections,
    daemonSessions,
    discoveries,
    fetchBackendDiagnostics,
    hosts,
    localMachineInfo,
    localMachineStatus,
    localSessions,
    mainView,
  ]);

  // --- Render ---

  // Build connections array for Sidebar
  const hostConnections = useMemo(
    () => hosts.map((h) => connections.get(h.id)).filter((c): c is HostConnection => c != null),
    [hosts, connections],
  );

  // Flat list of all sessions for AgentsView (local daemon + remote hosts)
  const allFlatSessions: TerminalSession[] = useMemo(() => {
    const result: TerminalSession[] = localSessions.map(asLocalTerminalSession);
    result.push(...daemonSessions);
    const daemonIds = new Set(daemonSessions.map((s) => s.id));
    connections.forEach((conn) => {
      if (conn.sessions) {
        for (const s of conn.sessions) {
          if (!daemonIds.has(s.id)) result.push(s);
        }
      }
    });
    return result;
  }, [daemonSessions, connections, localSessions]);

  const activeSession = useMemo(
    () => allFlatSessions.find((session) => session.id === activeTerminalSessionId) ?? null,
    [allFlatSessions, activeTerminalSessionId],
  );
  const activeSessionBaseUrl = activeHostUrl ?? LOCAL_DAEMON;

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
          connections={hostConnections}
          activeSessionBaseUrl={activeSessionBaseUrl}
          localHostName={localMachineInfo?.hostname ?? null}
          localMachineIp={localMachineInfo?.tailscaleIp ?? null}
          sessions={allFlatSessions}
          localSessions={localSessions}
          activeSessionId={activeTerminalSessionId}
          visible={mainView === "agents"}
          onSelectSession={setActiveTerminalSessionId}
          onCreateLocalSession={handleCreateLocalSession}
          onTerminateLocalSession={handleTerminateLocalSession}
          onLocalSessionStatusChange={handleLocalSessionStatusChange}
          onRefreshSessions={refreshAllSessions}
        />
        {mainView === "logs" ? (
          <div style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0 }}>
            <LogViewer baseUrl={activeHostUrl} />
          </div>
        ) : null}
        <div style={{ display: mainView === "settings" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="settings-view">
            <div className="settings-header">
              <h2>Settings</h2>
              <p className="muted">Configuration & permissions</p>
            </div>
            <div className="settings-content">
              <div className="settings-section">
                <h3>Versions</h3>
                <div className="settings-info-grid">
                  <div className="settings-info-item">
                    <span className="settings-info-label">Desktop app</span>
                    <span className="settings-info-value">v{APP_VERSION}</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Local daemon</span>
                    <span className="settings-info-value">{localMachineInfo ? `v${localMachineInfo.daemonVersion}` : "Loading..."}</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Local host</span>
                    <span className="settings-info-value">{localMachineInfo?.hostname ?? "Loading..."}</span>
                  </div>
                </div>
                {hostConnections.length > 0 && (
                  <div className="settings-host-list">
                    {hostConnections.map((connection) => (
                      <div key={connection.host.id} className="settings-host-item">
                        <div>
                          <div className="settings-host-name">{connection.host.name}</div>
                          <div className="settings-host-meta">{connection.machineInfo?.os ?? connection.message}</div>
                        </div>
                        <div className="settings-host-version">
                          {connection.machineInfo ? `v${connection.machineInfo.daemonVersion}` : "—"}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
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
                  <div className="settings-info-item">
                    <span className="settings-info-label">Connected hosts</span>
                    <span className="settings-info-value">{hostConnections.filter((connection) => connection.state === "connected").length}</span>
                  </div>
                  <div className="settings-info-item">
                    <span className="settings-info-label">Local active sessions</span>
                    <span className="settings-info-value">{localMachineStatus?.activeSessions ?? daemonSessions.filter((session) => session.status === "running").length}</span>
                  </div>
                </div>
              </div>
              <div className="settings-section">
                <h3>Diagnostics</h3>
                <p className="muted settings-help">
                  Export a JSON snapshot with app logs, daemon logs, machine versions, sessions, and connection state for release testing.
                </p>
                <div className="settings-actions">
                  <button
                    className="btn-secondary"
                    onClick={() => void handleExportDiagnostics()}
                    disabled={isExportingDiagnostics}
                  >
                    {isExportingDiagnostics ? "Exporting..." : "Export Diagnostics"}
                  </button>
                </div>
                {diagnosticsStatus && <div className="settings-status-note">{diagnosticsStatus}</div>}
              </div>
            </div>
          </div>
        </div>
      </section>

      <RightPanel
        daemonUrl={LOCAL_DAEMON}
        activeSession={activeSession}
        localMachineInfo={localMachineInfo}
        localMachineStatus={localMachineStatus}
        hostConnections={hostConnections}
        sessions={allFlatSessions}
        onSelectSession={setActiveTerminalSessionId}
      />
    </main>
  );
}

export default App;

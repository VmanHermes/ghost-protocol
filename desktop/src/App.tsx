import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, listHosts, addHostApi, removeHostApi, type ApiHost } from "./api";
import { loadHosts, addHost as persistAddHost, removeHost as persistRemoveHost } from "./hosts";
import { appLog } from "./log";
import type {
  HostConnection,
  LocalTerminalSession,
  SavedHost,
  TerminalSession,
} from "./types";
import { Sidebar } from "./components/Sidebar";
import { TerminalWorkspace } from "./components/TerminalWorkspace";
import { LogViewer } from "./components/LogViewer";
import "./App.css";

// NOTE: "chat" view is hidden until the Rust daemon adds conversation/agent support.
// ChatView, InspectorPanel, and related imports are commented out for now.
// import { ChatView } from "./components/ChatView";
// import { InspectorPanel } from "./components/InspectorPanel";

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
      // Fall back to localStorage if daemon isn't running
      setHosts(loadHosts());
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
  const [showSetupChecklist, setShowSetupChecklist] = useState(() => loadHosts().length === 0);
  const [hostingStatus, setHostingStatus] = useState<"idle" | "starting" | "active" | "error">("idle");
  const [hostingError, setHostingError] = useState<string | null>(null);
  const [hostingAddress, setHostingAddress] = useState<string | null>(null);

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
  const activeSystemStatus = activeConnection?.systemStatus ?? null;
  const activeTerminalSessions = activeConnection?.sessions ?? [];

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

  // Restore hosting state on launch
  useEffect(() => {
    (async () => {
      try {
        await invoke<string>("detect_daemon");
        // Daemon is running — check if Tailscale is connected
        try {
          const ip = await invoke<string>("detect_tailscale_ip");
          setHostingStatus("active");
          setHostingAddress(`${ip}:8787`);
        } catch {
          // Daemon running but no Tailscale — started manually, stay idle
        }
      } catch {
        // No daemon running — stay idle
      }
    })();
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
    } catch {
      // Fall back to localStorage
      setHosts((prev) => persistAddHost(prev, name, url));
    }
  }, [refreshHosts]);

  const handleRemoveHost = useCallback(async (hostId: string) => {
    const host = hosts.find((h) => h.id === hostId);
    const conn = connections.get(hostId);
    // Terminate active sessions on this host
    if (host && conn?.sessions) {
      const active = conn.sessions.filter((s) => s.status === "created" || s.status === "running");
      for (const session of active) {
        void api(host.url, `/api/terminal/sessions/${session.id}/terminate`, { method: "POST" }).catch(() => {});
      }
    }
    try {
      await removeHostApi(LOCAL_DAEMON, hostId);
      await refreshHosts();
    } catch {
      setHosts((prev) => persistRemoveHost(prev, hostId));
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

  const handleHostDetected = useCallback((name: string, url: string) => {
    const alreadyExists = hosts.some((h) => h.url === url);
    if (!alreadyExists) {
      void handleAddHost(name, url);
    }
    setShowSetupChecklist(false);
  }, [hosts, handleAddHost]);

  const handleStartHosting = useCallback(async () => {
    appLog.info("hosting", "Starting host flow...");
    setHostingStatus("starting");
    setHostingError(null);

    // 1. Check Tailscale
    let tailscaleIp: string;
    try {
      tailscaleIp = await invoke<string>("detect_tailscale_ip");
      appLog.info("hosting", `Tailscale IP: ${tailscaleIp}`);
    } catch {
      appLog.error("hosting", "Tailscale not connected to a mesh");
      setHostingStatus("error");
      setHostingError("Tailscale not connected to a mesh");
      return;
    }

    // 2. Install daemon if needed, then start
    try {
      appLog.info("hosting", "Checking daemon installation...");
      const installResult = await invoke<string>("install_daemon");
      appLog.info("hosting", `Daemon install: ${installResult}`);
    } catch (err) {
      const msg = String(err ?? "");
      appLog.error("hosting", `Failed to install daemon: ${msg}`);
      setHostingStatus("error");
      setHostingError(`Failed to install daemon: ${msg}`);
      return;
    }

    try {
      await invoke<string>("start_daemon", { bindHost: tailscaleIp, port: 8787 });
      appLog.info("hosting", `Daemon spawned, binding to ${tailscaleIp}:8787`);
    } catch (err) {
      const msg = String(err ?? "");
      appLog.error("hosting", `Failed to start daemon: ${msg}`);
      setHostingStatus("error");
      setHostingError(`Failed to start daemon: ${msg}`);
      return;
    }

    // 3. Poll for health (up to 10 seconds)
    for (let i = 0; i < 10; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      try {
        await invoke<string>("detect_daemon");
        appLog.info("hosting", `Daemon healthy after ${i + 1}s — now hosting on ${tailscaleIp}:8787`);
        setHostingStatus("active");
        setHostingAddress(`${tailscaleIp}:8787`);
        const alreadyExists = hosts.some((h) => h.url === "http://127.0.0.1:8787");
        if (!alreadyExists) {
          void handleAddHost("This Computer", "http://127.0.0.1:8787");
        }
        return;
      } catch {
        appLog.debug("hosting", `Health poll ${i + 1}/10 — not ready`);
      }
    }

    // Timeout — daemon failed to start
    appLog.error("hosting", "Daemon failed to start (timed out after 10s)");
    setHostingStatus("error");
    setHostingError("Daemon failed to start (timed out)");
    try { await invoke("stop_daemon"); } catch { /* ignore */ }
  }, [hosts, handleAddHost]);

  const handleStopHosting = useCallback(async () => {
    appLog.info("hosting", "Stopping daemon...");
    try {
      await invoke("stop_daemon");
      appLog.info("hosting", "Daemon stopped");
    } catch {
      // Ignore — may already be stopped
    }
    setHostingStatus("idle");
    setHostingAddress(null);
    setHostingError(null);
  }, []);

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
        showSetupChecklist={showSetupChecklist}
        onShowSetupChecklist={() => setShowSetupChecklist(true)}
        hostingStatus={hostingStatus}
        hostingError={hostingError}
        hostingAddress={hostingAddress}
        onStartHosting={() => void handleStartHosting()}
        onStopHosting={() => void handleStopHosting()}
      />

      <section className="main-panel">
        {/* Chat view hidden — Rust daemon does not serve conversation/agent endpoints yet */}
        {/* Will be restored when the Rust daemon adds chat support */}

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
            setupChecklist={{
              visible: showSetupChecklist,
              onDismiss: () => setShowSetupChecklist(false),
              onHostDetected: handleHostDetected,
            }}
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

      {/* InspectorPanel hidden — agent/run/approval observability not available in Rust daemon yet */}
      {/* Will be restored when the Rust daemon adds agent support */}
    </main>
  );
}

export default App;

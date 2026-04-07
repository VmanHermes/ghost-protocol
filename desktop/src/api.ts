import type {
  PeerPermissionRecord,
  PendingApprovalRecord,
  PermissionTier,
  DiscoveredPeer,
  AgentInfo,
  ProjectRecord,
  ChatMessage,
  MachineInfo,
  MachineStatus,
  TerminalSession,
  WorkSessionViews,
  CodeServerInfo,
} from "./types";

export function wsUrlFromHttp(baseUrl: string) {
  if (baseUrl.startsWith("https://")) return baseUrl.replace("https://", "wss://") + "/ws";
  if (baseUrl.startsWith("http://")) return baseUrl.replace("http://", "ws://") + "/ws";
  return `ws://${baseUrl}/ws`;
}

export function fmt(ts?: string | null) {
  if (!ts) return "—";
  try {
    return new Date(ts).toLocaleString();
  } catch {
    return ts;
  }
}

export async function api<T>(baseUrl: string, path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${baseUrl}${path}`, {
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    ...init,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(parseApiError(text, res.status));
  }
  return res.json() as Promise<T>;
}

function parseApiError(text: string, status: number): string {
  if (!text) return `Request failed: ${status}`;

  try {
    const data = JSON.parse(text) as { error?: string; message?: string };
    if (data.error === "forbidden" && data.message) {
      return `Permission denied: ${data.message}`;
    }
    if (data.message) return data.message;
    if (typeof data.error === "string") return data.error;
  } catch {
    // Fall back to the raw body for non-JSON responses.
  }

  return text;
}

export type BackendLogEntry = {
  ts: string | null;
  level: string;
  logger: string;
  message: string;
};

type RawBackendLogEntry = {
  ts?: string | null;
  timestamp?: string | null;
  level?: string | null;
  logger?: string | null;
  source?: string | null;
  message?: string | null;
};

function normalizeBackendLogEntry(entry: RawBackendLogEntry): BackendLogEntry {
  return {
    ts: entry.ts ?? entry.timestamp ?? null,
    level: entry.level ?? "INFO",
    logger: entry.logger ?? entry.source ?? "server",
    message: entry.message ?? "",
  };
}

export type ApiHost = {
  id: string;
  name: string;
  tailscaleIp: string;
  url: string;
  status: string;
  lastSeen: string | null;
  capabilities: {
    gpu: string | null;
    ramGb: number | null;
    hermes: boolean;
    ollama: boolean;
  } | null;
};

export async function listHosts(daemonUrl: string): Promise<ApiHost[]> {
  return api<ApiHost[]>(daemonUrl, "/api/hosts");
}

export async function addHostApi(
  daemonUrl: string,
  name: string,
  tailscaleIp: string,
): Promise<ApiHost> {
  return api<ApiHost>(daemonUrl, "/api/hosts", {
    method: "POST",
    body: JSON.stringify({ name, tailscaleIp }),
  });
}

export async function removeHostApi(
  daemonUrl: string,
  hostId: string,
): Promise<void> {
  await fetch(`${daemonUrl}/api/hosts/${hostId}`, { method: "DELETE" });
}

export async function listPermissions(daemonUrl: string): Promise<PeerPermissionRecord[]> {
  return api<PeerPermissionRecord[]>(daemonUrl, "/api/permissions");
}

export async function setPermission(
  daemonUrl: string,
  hostId: string,
  tier: PermissionTier,
): Promise<{ hostId: string; tier: string }> {
  return api(daemonUrl, `/api/hosts/${hostId}/permissions`, {
    method: "PUT",
    body: JSON.stringify({ tier }),
  });
}

export async function listApprovals(
  daemonUrl: string,
  status?: string,
): Promise<PendingApprovalRecord[]> {
  const query = status ? `?status=${status}` : "";
  return api<PendingApprovalRecord[]>(daemonUrl, `/api/approvals${query}`);
}

export async function getApproval(
  daemonUrl: string,
  approvalId: string,
): Promise<PendingApprovalRecord> {
  return api<PendingApprovalRecord>(daemonUrl, `/api/approvals/${approvalId}`);
}

export async function resolveApproval(
  daemonUrl: string,
  approvalId: string,
  action: "approve" | "deny",
): Promise<{ status: string }> {
  return api(daemonUrl, `/api/approvals/${approvalId}/${action}`, {
    method: "PUT",
  });
}

export async function listDiscoveries(daemonUrl: string): Promise<DiscoveredPeer[]> {
  return api<DiscoveredPeer[]>(daemonUrl, "/api/discoveries");
}

export async function acceptDiscovery(
  daemonUrl: string,
  ip: string,
): Promise<ApiHost> {
  return api<ApiHost>(daemonUrl, `/api/discoveries/${ip}/accept`, {
    method: "PUT",
  });
}

export async function dismissDiscovery(
  daemonUrl: string,
  ip: string,
): Promise<void> {
  await fetch(`${daemonUrl}/api/discoveries/${ip}/dismiss`, { method: "PUT" });
}

export async function listAgents(daemonUrl: string): Promise<AgentInfo[]> {
  return api<AgentInfo[]>(daemonUrl, "/api/agents");
}

export async function getMachineInfo(daemonUrl: string): Promise<MachineInfo> {
  return api<MachineInfo>(daemonUrl, "/api/system/hardware");
}

export async function getMachineStatus(daemonUrl: string): Promise<MachineStatus> {
  return api<MachineStatus>(daemonUrl, "/api/system/hardware/status");
}

export async function listSystemLogs(
  daemonUrl: string,
  limit = 300,
): Promise<BackendLogEntry[]> {
  const entries = await api<RawBackendLogEntry[]>(daemonUrl, `/api/system/logs?limit=${limit}`);
  return entries.map(normalizeBackendLogEntry);
}

export async function listProjects(daemonUrl: string): Promise<ProjectRecord[]> {
  return api<ProjectRecord[]>(daemonUrl, "/api/projects");
}

export async function createChatSession(
  daemonUrl: string,
  agentId: string,
  projectId?: string,
  workdir?: string,
): Promise<{ session: any; agent: AgentInfo }> {
  return api(daemonUrl, "/api/chat/sessions", {
    method: "POST",
    body: JSON.stringify({ agentId, projectId, workdir }),
  });
}

export async function listChatMessages(
  daemonUrl: string,
  sessionId: string,
): Promise<ChatMessage[]> {
  return api<ChatMessage[]>(daemonUrl, `/api/chat/sessions/${sessionId}/messages`);
}

export async function sendChatMessage(
  daemonUrl: string,
  sessionId: string,
  content: string,
): Promise<ChatMessage> {
  return api(daemonUrl, `/api/chat/sessions/${sessionId}/message`, {
    method: "POST",
    body: JSON.stringify({ content }),
  });
}

export async function switchSessionMode(
  daemonUrl: string,
  sessionId: string,
  mode: "chat" | "terminal",
  confirmed = false,
): Promise<{
  session?: TerminalSession;
  warning?: string;
  needsConfirmation?: boolean;
  companionAvailable?: boolean;
  reopenSupported?: boolean;
  openedCompanion?: boolean;
  replacedSessionId?: string;
}> {
  return api(daemonUrl, `/api/sessions/${sessionId}/switch-mode`, {
    method: "POST",
    body: JSON.stringify({ mode, confirmed }),
  });
}

export async function getWorkSessionViews(
  daemonUrl: string,
  sessionId: string,
): Promise<{ session: TerminalSession; views: WorkSessionViews }> {
  return api(daemonUrl, `/api/work-sessions/${sessionId}/views`);
}

export async function createCompanionTerminal(
  daemonUrl: string,
  sessionId: string,
): Promise<TerminalSession> {
  return api(daemonUrl, `/api/work-sessions/${sessionId}/companion-terminal`, {
    method: "POST",
  });
}

export async function reopenWorkSession(
  daemonUrl: string,
  sessionId: string,
): Promise<TerminalSession> {
  return api(daemonUrl, `/api/work-sessions/${sessionId}/reopen`, {
    method: "POST",
  });
}

// ---------------------------------------------------------------------------
// code-server
// ---------------------------------------------------------------------------

export async function createCodeServerSession(
  daemonUrl: string,
  workdir: string,
  projectId?: string,
): Promise<TerminalSession> {
  return api<TerminalSession>(daemonUrl, "/api/code-server/sessions", {
    method: "POST",
    body: JSON.stringify({ workdir, projectId }),
  });
}

export async function terminateCodeServerSession(
  daemonUrl: string,
  sessionId: string,
): Promise<{ status: string }> {
  return api(daemonUrl, `/api/code-server/sessions/${sessionId}/terminate`, {
    method: "POST",
  });
}

export async function listDetectedCodeServers(
  daemonUrl: string,
): Promise<CodeServerInfo[]> {
  return api<CodeServerInfo[]>(daemonUrl, "/api/code-server/detected");
}

export async function adoptCodeServer(
  daemonUrl: string,
  pid: number,
): Promise<TerminalSession> {
  return api<TerminalSession>(daemonUrl, "/api/code-server/adopt", {
    method: "POST",
    body: JSON.stringify({ pid }),
  });
}

export async function listDirs(
  baseUrl: string,
  path: string,
): Promise<{ parent: string; dirs: string[] }> {
  return api<{ parent: string; dirs: string[] }>(
    baseUrl,
    `/api/fs/list-dirs?path=${encodeURIComponent(path)}`,
  );
}

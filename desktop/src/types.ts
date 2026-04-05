export type Conversation = {
  id: string;
  title?: string | null;
  createdAt: string;
  updatedAt: string;
};

export type Message = {
  id: string;
  conversationId: string;
  role: "user" | "assistant" | "system";
  content: string;
  createdAt: string;
  runId?: string | null;
};

export type RunRecord = {
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

export type AgentRecord = {
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

export type ApprovalRecord = {
  id: string;
  runId: string;
  type: string;
  status: string;
  payload: Record<string, unknown>;
  requestedAt: string;
};

export type UsageRecord = {
  id: string;
  entityType: string;
  entityId: string;
  model?: string | null;
  tokensIn: number;
  tokensOut: number;
  cost: number;
  createdAt: string;
};

export type EventEnvelope = {
  eventId: string;
  type: string;
  ts: string;
  seq: number;
  conversationId?: string | null;
  runId?: string | null;
  summary: string;
  payload: Record<string, unknown>;
};

export type RunTimelineItem = {
  id: number;
  run_id: string;
  seq: number;
  event_type: string;
  summary: string;
  created_at: string;
  payload: Record<string, unknown>;
};

export type RunDetail = {
  run: RunRecord;
  live?: Record<string, unknown> | null;
  timeline: RunTimelineItem[];
  attempts: Array<Record<string, unknown>>;
  agents: AgentRecord[];
  approvals: ApprovalRecord[];
  artifacts: Array<Record<string, unknown>>;
  usage: UsageRecord[];
};

export type TerminalSession = {
  id: string;
  mode: "agent" | "project" | "rescue_shell";
  status: "created" | "running" | "exited" | "terminated" | "error";
  name?: string | null;
  workdir: string;
  command: string[];
  createdAt: string;
  startedAt?: string | null;
  finishedAt?: string | null;
  lastChunkAt?: string | null;
  pid?: number | null;
  exitCode?: number | null;
};

export type TerminalChunk = {
  id: number;
  sessionId: string;
  stream: "stdout" | "stderr" | "system";
  chunk: string;
  createdAt: string;
};

export type TerminalSessionDetail = {
  session: TerminalSession;
  chunks: TerminalChunk[];
};

export type SystemStatus = {
  activeAgents: AgentRecord[];
  pendingApprovals: ApprovalRecord[];
  recentRuns: RunRecord[];
  activeTerminalSessions: TerminalSession[];
  connection: {
    bindHost: string;
    bindPort: number;
    telegramEnabled: boolean;
    allowedCidrs: string[];
  };
};

export type ConversationDetail = {
  conversation: Conversation;
  messages: Message[];
  runs: RunRecord[];
};

export type MainView = "chat" | "terminal" | "logs" | "settings";

// --- Local terminal types (Tauri PTY) ---

export type LocalTerminalSession = {
  id: string;
  status: "running" | "exited" | "terminated" | "error";
  createdAt: string;
  exitCode?: number | null;
};

export type TerminalSource =
  | { type: "local"; sessionId: string }
  | { type: "remote"; hostId: string; sessionId: string };

export type TerminalTab = {
  source: TerminalSource;
  label: string;
  status: "running" | "exited" | "terminated" | "error" | "created";
};

// --- Multi-host types (Phase 2) ---

export type SavedHost = {
  id: string;
  name: string;
  url: string;
};

export type HostConnectionState = "idle" | "connecting" | "connected" | "error";

export type HostConnection = {
  host: SavedHost;
  state: HostConnectionState;
  message: string;
  sessions: TerminalSession[] | null;
  runs: RunRecord[] | null;
  conversations: Conversation[] | null;
  systemStatus: SystemStatus | null;
};

// --- Peer permissions types (Phase 2d) ---

export type PermissionTier = "full-access" | "approval-required" | "read-only" | "no-access";

export type PeerPermissionRecord = {
  hostId: string;
  hostName: string;
  tailscaleIp: string;
  tier: PermissionTier;
  updatedAt: string;
};

export type PendingApprovalRecord = {
  id: string;
  hostId: string;
  method: string;
  path: string;
  bodyJson: string | null;
  status: "pending" | "approved" | "denied" | "expired";
  createdAt: string;
  resolvedAt: string | null;
  expiresAt: string;
  resultJson: string | null;
};

// --- Discovery types (Phase 2e) ---

export type DiscoveredPeer = {
  tailscaleIp: string;
  name: string;
  discoveredAt: string;
};

// --- Agent & project types (Phase 3a) ---

export type AgentInfo = {
  id: string;
  name: string;
  agentType: "cli" | "api";
  command: string;
  version: string | null;
};

export type ProjectRecord = {
  id: string;
  name: string;
  workdir: string;
  configJson: string;
  registeredAt: string;
  updatedAt: string;
};

export type ChatMessage = {
  id: string;
  sessionId: string;
  role: "user" | "assistant" | "system";
  content: string;
  createdAt: string;
};

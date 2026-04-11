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
  mode: "agent" | "project" | "rescue_shell" | "chat" | "terminal";
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
  projectId?: string | null;
  parentSessionId?: string | null;
  rootSessionId?: string | null;
  hostId?: string | null;
  hostName?: string | null;
  agentId?: string | null;
  driverKind?: "terminal_driver" | "structured_chat_driver" | "api_driver" | "ide_driver" | "code_server_driver";
  capabilities?: string[];
  sessionType?: string;
  port?: number | null;
  url?: string | null;
  adopted?: boolean;
};

export type CodeServerInfo = {
  pid: number;
  port: number;
  workdir: string;
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
  peer?: {
    currentTier: PermissionTier;
  };
};

export type MachineInfo = {
  hostname: string;
  tailscaleIp: string | null;
  daemonVersion: string;
  os: string;
  cpu: {
    cores: number;
    model: string;
  };
  ramGb: number;
  gpu: {
    model: string;
    vramGb: number;
    driver: string;
    utilizationPercent: number | null;
    vramUsedGb: number | null;
  } | null;
  tools: {
    tmux: string | null;
    hermes: string | null;
    ollama: string | null;
    sshUser: string;
    agents: AgentInfo[];
  };
};

export type MachineStatus = {
  cpuPercent: number | null;
  ramUsedGb: number;
  ramTotalGb: number;
  gpuPercent: number | null;
  gpuVramUsedGb: number | null;
  activeSessions: number;
  uptimeHours: number;
  notableProcesses: Array<Record<string, unknown>>;
};

export type ConversationDetail = {
  conversation: Conversation;
  messages: Message[];
  runs: RunRecord[];
};

export type MainView = "agents" | "logs" | "settings";

// --- Local terminal types (Tauri PTY) ---

export type LocalTerminalSession = {
  id: string;
  status: "running" | "exited" | "terminated" | "error";
  createdAt: string;
  finishedAt?: string | null;
  exitCode?: number | null;
  name?: string | null;
  workdir?: string | null;
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
  machineInfo: MachineInfo | null;
  machineStatus: MachineStatus | null;
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
  persistent: boolean;
  launchSupported?: boolean;
  launchNote?: string | null;
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

export type ChatDeltaEvent = {
  op: "chat_delta";
  sessionId: string;
  messageId: string;
  delta: string;
};

export type ChatMessageEvent = {
  op: "chat_message";
  message: ChatMessage;
};

export type WorkSessionViews = {
  chat: boolean;
  terminal: boolean;
  logs: boolean;
  artifacts: boolean;
  approvals: boolean;
  delegation: boolean;
  openCompanionTerminal: boolean;
  reopenAsTerminal: boolean;
  safeModeSwitch: boolean;
};

export type DelegationContract = {
  id: string;
  parentSessionId: string;
  requesterAgentId: string | null;
  targetHostId: string | null;
  targetAgentId: string;
  task: string;
  allowedSkillsJson: string;
  toolAllowlistJson: string;
  artifactInputsJson: string;
  budgetTokens: number | null;
  budgetSecs: number | null;
  approvalMode: string;
  experimentalCommEnabled: boolean;
  status: string;
  createdAt: string;
  updatedAt: string;
};

export type AgentMailboxMessage = {
  id: string;
  contractId: string;
  fromSessionId: string;
  toSessionId: string;
  kind: string;
  content: string;
  visibility: string;
  correlationId: string | null;
  createdAt: string;
};

export type SkillCandidate = {
  id: string;
  sourceSessionId: string;
  traceRefsJson: string;
  proposedChange: string;
  riskLevel: string;
  status: string;
  reviewer: string | null;
  promotedSkillVersion: string | null;
  createdAt: string;
  reviewedAt: string | null;
};

export type ChatStatusEvent = {
  op: "chat_status";
  sessionId: string;
  status: "thinking" | "tool_use" | "idle" | "error" | "exited" | "terminated";
};

export type SessionMetaEvent = {
  op: "session_meta";
  sessionId: string;
  tokens?: number | null;
  contextPct?: number | null;
};

export type ChatWsEvent =
  | ChatDeltaEvent
  | ChatMessageEvent
  | ChatStatusEvent
  | SessionMetaEvent;

export type SessionMode = "chat" | "terminal";

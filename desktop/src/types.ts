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
  status: "running" | "exited" | "terminated";
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

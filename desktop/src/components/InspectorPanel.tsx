import type {
  AgentRecord,
  ApprovalRecord,
  EventEnvelope,
  RunDetail,
  RunRecord,
  SystemStatus,
} from "../types";

type Props = {
  activeHostName: string | null;
  activeRun: RunRecord | null;
  runs: RunRecord[];
  runDetail: RunDetail | null;
  systemStatus: SystemStatus | null;
  terminalSessionCount: number;
  events: EventEnvelope[];
  activeRunId: string | null;
  onSelectRun: (runId: string) => void;
  onResolveApproval: (approvalId: string, status: "approved" | "rejected") => void;
};

function formatTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return String(n);
}

function timeAgo(ts: string): string {
  const ms = Date.now() - new Date(ts).getTime();
  const minutes = Math.floor(ms / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export function InspectorPanel({
  activeHostName,
  activeRun: _activeRun,
  runs,
  runDetail,
  systemStatus,
  terminalSessionCount,
  events,
  activeRunId: _activeRunId,
  onSelectRun: _onSelectRun,
  onResolveApproval,
}: Props) {
  const agents: AgentRecord[] = runDetail?.agents ?? systemStatus?.activeAgents ?? [];
  const approvals: ApprovalRecord[] = systemStatus?.pendingApprovals ?? [];
  const totalTokens = runs.reduce((sum, r) => sum + r.tokenUsage, 0);
  const activeSessions = terminalSessionCount;

  // Build alert items from approvals + recent notable events
  const alertItems: Array<{
    id: string;
    title: string;
    description: string;
    type: "approval" | "error" | "success" | "info";
    time: string;
    approvalId?: string;
  }> = [];

  for (const approval of approvals) {
    alertItems.push({
      id: approval.id,
      title: `${approval.type.replace(/_/g, " ")}`,
      description: JSON.stringify(approval.payload).slice(0, 60),
      type: "approval",
      time: timeAgo(approval.requestedAt),
      approvalId: approval.id,
    });
  }

  // Add recent run events as alerts
  for (const run of runs.slice(0, 3)) {
    if (run.status === "error" || run.status === "failed") {
      alertItems.push({
        id: `run-${run.id}`,
        title: "Run Failed",
        description: run.currentStep || run.id.slice(0, 8),
        type: "error",
        time: run.finishedAt ? timeAgo(run.finishedAt) : "—",
      });
    } else if (run.status === "completed") {
      alertItems.push({
        id: `run-${run.id}`,
        title: "Run Complete",
        description: run.currentStep || run.id.slice(0, 8),
        type: "success",
        time: run.finishedAt ? timeAgo(run.finishedAt) : "—",
      });
    }
  }

  return (
    <aside className="observability">
      <div className="observability-header">
        <h2>{activeHostName ? `${activeHostName} — Observability` : "Local — Observability"}</h2>
        <p className="muted">Real-time metrics & alerts</p>
      </div>

      {/* Metric cards */}
      <div className="metric-cards">
        <div className="metric-card">
          <div className="metric-card-header">
            <span className="metric-label">Token Usage</span>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#10b981" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="23 6 13.5 15.5 8.5 10.5 1 18" />
              <polyline points="17 6 23 6 23 12" />
            </svg>
          </div>
          <div className="metric-value">{formatTokens(totalTokens)}</div>
          <div className="metric-change positive">+{runs.length > 0 ? Math.round((runs[0]?.tokenUsage / Math.max(totalTokens, 1)) * 100) : 0}%</div>
        </div>

        <div className="metric-card">
          <div className="metric-card-header">
            <span className="metric-label">Active Sessions</span>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#60a5fa" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
              <circle cx="9" cy="7" r="4" />
              <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
              <path d="M16 3.13a4 4 0 0 1 0 7.75" />
            </svg>
          </div>
          <div className="metric-value">{activeSessions}</div>
          <div className="metric-change positive">+{agents.length}</div>
        </div>

        <div className="metric-card">
          <div className="metric-card-header">
            <span className="metric-label">Requests/min</span>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#10b981" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="23 6 13.5 15.5 8.5 10.5 1 18" />
              <polyline points="17 6 23 6 23 12" />
            </svg>
          </div>
          <div className="metric-value">{events.length}</div>
          <div className="metric-change neutral">stable</div>
        </div>
      </div>

      {/* Needs attention */}
      <div className="attention-section">
        <div className="attention-header">
          <h3>Needs Attention</h3>
          <span className="attention-count">{alertItems.length}</span>
        </div>

        <div className="alert-list">
          {alertItems.length === 0 && (
            <div className="empty-state">All clear — no items need attention</div>
          )}
          {alertItems.map((item) => (
            <div key={item.id} className={`alert-card alert-${item.type}`}>
              <div className="alert-card-icon">
                {item.type === "error" && (
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#ef4444" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="12" cy="12" r="10" /><line x1="15" y1="9" x2="9" y2="15" /><line x1="9" y1="9" x2="15" y2="15" />
                  </svg>
                )}
                {item.type === "approval" && (
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#f59e0b" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="12" cy="12" r="10" /><line x1="12" y1="8" x2="12" y2="12" /><line x1="12" y1="16" x2="12.01" y2="16" />
                  </svg>
                )}
                {item.type === "success" && (
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#10b981" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="12" cy="12" r="10" /><polyline points="16 10 11 15 8 12" />
                  </svg>
                )}
                {item.type === "info" && (
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#3b82f6" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="12" cy="12" r="10" /><line x1="12" y1="16" x2="12" y2="12" /><line x1="12" y1="8" x2="12.01" y2="8" />
                  </svg>
                )}
              </div>
              <div className="alert-card-body">
                <div className="alert-card-title">{item.title}</div>
                <div className="alert-card-desc">{item.description}</div>
                <div className="alert-card-meta">
                  <span className={`alert-badge alert-badge-${item.type}`}>{item.type}</span>
                  <span className="alert-time">{item.time}</span>
                </div>
              </div>
              {item.approvalId && (
                <div className="alert-card-actions">
                  <button className="btn-approve" onClick={() => onResolveApproval(item.approvalId!, "approved")}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="20 6 9 17 4 12" /></svg>
                  </button>
                  <button className="btn-reject" onClick={() => onResolveApproval(item.approvalId!, "rejected")}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
                  </button>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>

      <button className="btn-view-all">View All Alerts</button>
    </aside>
  );
}

import type { MachineInfo, MachineStatus, OutcomeRecord, TerminalSession } from "../types";

type MachineCardData = {
  hostname: string;
  ip: string | null;
  online: boolean;
  machineInfo: MachineInfo | null;
  machineStatus: MachineStatus | null;
  activeSessions: number;
};

type Props = {
  localMachine: MachineCardData;
  remoteMachines: MachineCardData[];
  sessions: TerminalSession[];
  outcomes: OutcomeRecord[];
  onSelectSession: (sessionId: string) => void;
};

function shortenPath(path: string): string {
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= 2) return path;
  return parts.slice(-2).join("/");
}

function formatDuration(startedAt: string): string {
  const elapsed = Math.floor((Date.now() - new Date(startedAt).getTime()) / 1000);
  if (elapsed < 60) return `${elapsed}s`;
  const minutes = Math.floor(elapsed / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return `${hours}h ${remainingMinutes}m`;
}

function formatRelativeTime(isoDate: string): string {
  const elapsed = Math.floor((Date.now() - new Date(isoDate).getTime()) / 1000);
  if (elapsed < 60) return "just now";
  const minutes = Math.floor(elapsed / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function formatOutcomeAction(outcome: OutcomeRecord): string {
  const desc = outcome.description ?? `${outcome.category}/${outcome.action}`;
  const parts: string[] = [desc];
  if (outcome.exitCode != null) parts.push(`(code ${outcome.exitCode})`);
  if (outcome.durationSecs != null) {
    const secs = Math.round(outcome.durationSecs);
    parts.push(secs < 60 ? `${secs}s` : `${Math.floor(secs / 60)}m`);
  }
  return parts.join(" · ");
}

function MachineCard({ machine }: { machine: MachineCardData }) {
  const statusClass = machine.online ? "status-dot-online" : "status-dot-offline";
  const ram = machine.machineStatus
    ? `RAM ${machine.machineStatus.ramUsedGb.toFixed(0)}/${machine.machineStatus.ramTotalGb.toFixed(0)} GB`
    : null;
  const gpu = machine.machineInfo?.gpu
    ? `GPU ${machine.machineInfo.gpu.model}`
    : null;

  return (
    <div className="machine-card">
      <div className="machine-card-header">
        <span className={`status-dot ${statusClass}`} />
        <span className="machine-card-name">{machine.hostname}</span>
      </div>
      <div className="machine-card-stats">
        {ram && <span>{ram}</span>}
        {gpu && <span>{gpu}</span>}
        <span>{machine.activeSessions} active</span>
      </div>
    </div>
  );
}

function AgentEntry({
  session,
  onSelect,
}: {
  session: TerminalSession;
  onSelect: () => void;
}) {
  const statusClass = session.status === "running" ? "status-dot-online" : "status-dot-error";
  const agentName = session.agentId ?? "Agent";
  const machine = session.hostName ?? "local";
  const duration = session.startedAt ? formatDuration(session.startedAt) : "";

  return (
    <div className="agent-entry" onClick={onSelect} role="button" tabIndex={0} onKeyDown={(e) => { if (e.key === "Enter") onSelect(); }}>
      <div className="agent-entry-header">
        <span className={`status-dot ${statusClass}`} />
        <span className="agent-entry-name">{agentName}</span>
        <span className="agent-entry-machine">{machine}</span>
      </div>
      <div className="agent-entry-detail">
        <span className="muted">{shortenPath(session.workdir)}</span>
        <span className="muted">{duration}</span>
      </div>
    </div>
  );
}

export function MeshOverview({ localMachine, remoteMachines, sessions, outcomes, onSelectSession }: Props) {
  const activeAgents = sessions.filter(
    (s) => s.status === "running" && s.agentId,
  );

  const allMachines = [localMachine, ...remoteMachines];

  return (
    <div className="mesh-overview">
      <div className="mesh-section">
        <div className="mesh-section-header">Machines</div>
        <div className="mesh-section-content">
          {allMachines.map((m) => (
            <MachineCard key={m.hostname + (m.ip ?? "")} machine={m} />
          ))}
        </div>
      </div>

      <div className="mesh-section">
        <div className="mesh-section-header">
          Active Agents
          {activeAgents.length > 0 && <span className="tab-badge">{activeAgents.length}</span>}
        </div>
        <div className="mesh-section-content">
          {activeAgents.length === 0 ? (
            <div className="muted mesh-empty">No active agents</div>
          ) : (
            activeAgents.map((s) => (
              <AgentEntry key={s.id} session={s} onSelect={() => onSelectSession(s.id)} />
            ))
          )}
        </div>
      </div>

      <div className="mesh-section">
        <div className="mesh-section-header">Recent Activity</div>
        <div className="mesh-section-content">
          {outcomes.length === 0 ? (
            <div className="muted mesh-empty">No recent activity</div>
          ) : (
            outcomes.map((o) => (
              <div key={o.id} className="activity-entry">
                <span className="activity-entry-action">{formatOutcomeAction(o)}</span>
                <span className="activity-entry-time muted">{formatRelativeTime(o.createdAt)}</span>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

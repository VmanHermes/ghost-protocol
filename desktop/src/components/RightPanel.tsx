import { useState } from "react";
import { ApprovalsTab } from "./ApprovalsTab";
import { MeshOverview } from "./MeshOverview";
import { useOutcomes } from "../hooks/useOutcomes";
import type { HostConnection, MachineInfo, MachineStatus, TerminalSession } from "../types";

type Props = {
  daemonUrl: string;
  activeSession: TerminalSession | null;
  localMachineInfo: MachineInfo | null;
  localMachineStatus: MachineStatus | null;
  hostConnections: HostConnection[];
  sessions: TerminalSession[];
  onSelectSession: (sessionId: string) => void;
};

export function RightPanel({
  daemonUrl,
  activeSession,
  localMachineInfo,
  localMachineStatus,
  hostConnections,
  sessions,
  onSelectSession,
}: Props) {
  const [pendingCount, setPendingCount] = useState(0);
  const outcomes = useOutcomes({ daemonUrl, limit: 10 });

  const localMachine = {
    hostname: localMachineInfo?.hostname ?? "this machine",
    ip: localMachineInfo?.tailscaleIp ?? null,
    online: true,
    machineInfo: localMachineInfo,
    machineStatus: localMachineStatus,
    activeSessions: sessions.filter(
      (s) => s.status === "running" && !s.hostId,
    ).length,
  };

  const remoteMachines = hostConnections.map((conn) => ({
    hostname: conn.host.name,
    ip: conn.machineInfo?.tailscaleIp ?? null,
    online: conn.state === "connected",
    machineInfo: conn.machineInfo,
    machineStatus: conn.machineStatus,
    activeSessions: conn.sessions?.filter((s) => s.status === "running").length ?? 0,
  }));

  return (
    <aside className="right-panel">
      {pendingCount > 0 && (
        <div className="right-panel-section">
          <div className="right-panel-header">
            <h3>Approvals</h3>
            <span className="tab-badge">{pendingCount}</span>
          </div>
          <div className="right-panel-content">
            <ApprovalsTab
              daemonUrl={daemonUrl}
              activeSession={activeSession}
              onPendingCountChange={setPendingCount}
            />
          </div>
        </div>
      )}

      {pendingCount === 0 && (
        <ApprovalsTab
          daemonUrl={daemonUrl}
          activeSession={activeSession}
          onPendingCountChange={setPendingCount}
        />
      )}

      <div className="right-panel-section right-panel-section-grow">
        <div className="right-panel-header">
          <h3>Mesh Overview</h3>
        </div>
        <div className="right-panel-content">
          <MeshOverview
            localMachine={localMachine}
            remoteMachines={remoteMachines}
            sessions={sessions}
            outcomes={outcomes}
            onSelectSession={onSelectSession}
          />
        </div>
      </div>
    </aside>
  );
}

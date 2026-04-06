import { useState } from "react";
import { ApprovalsTab } from "./ApprovalsTab";
import type { TerminalSession } from "../types";

type Props = {
  daemonUrl: string;
  activeSession: TerminalSession | null;
};

export function RightPanel({ daemonUrl, activeSession }: Props) {
  const [pendingCount, setPendingCount] = useState(0);

  return (
    <aside className="right-panel">
      <div className="right-panel-header">
        <h3>Approvals</h3>
        {pendingCount > 0 && (
          <span className="tab-badge">{pendingCount}</span>
        )}
      </div>
      <div className="right-panel-content">
        <ApprovalsTab
          daemonUrl={daemonUrl}
          activeSession={activeSession}
          onPendingCountChange={setPendingCount}
        />
      </div>
    </aside>
  );
}

import { useState } from "react";
import { ApprovalsTab } from "./ApprovalsTab";

type Props = {
  daemonUrl: string;
};

export function RightPanel({ daemonUrl }: Props) {
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
          onPendingCountChange={setPendingCount}
        />
      </div>
    </aside>
  );
}

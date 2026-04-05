import { useState } from "react";
import { ApprovalsTab } from "./ApprovalsTab";
import { PermissionsTab } from "./PermissionsTab";

type Tab = "approvals" | "permissions";

type Props = {
  daemonUrl: string;
};

export function RightPanel({ daemonUrl }: Props) {
  const [activeTab, setActiveTab] = useState<Tab>("approvals");
  const [pendingCount, setPendingCount] = useState(0);

  return (
    <aside className="right-panel">
      <div className="right-panel-tabs">
        <button
          className={`right-panel-tab ${activeTab === "approvals" ? "active" : ""}`}
          onClick={() => setActiveTab("approvals")}
        >
          Approvals
          {pendingCount > 0 && (
            <span className="tab-badge">{pendingCount}</span>
          )}
        </button>
        <button
          className={`right-panel-tab ${activeTab === "permissions" ? "active" : ""}`}
          onClick={() => setActiveTab("permissions")}
        >
          Permissions
        </button>
      </div>

      <div className="right-panel-content">
        {activeTab === "approvals" ? (
          <ApprovalsTab
            daemonUrl={daemonUrl}
            onPendingCountChange={setPendingCount}
          />
        ) : (
          <PermissionsTab daemonUrl={daemonUrl} />
        )}
      </div>
    </aside>
  );
}

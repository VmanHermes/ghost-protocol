import { ReactNode, useState } from "react";
import type { HostConnection, MainView } from "../types";

type Props = {
  hosts: HostConnection[];
  mainView: MainView;
  onChangeView: (view: MainView) => void;
  onAddHost: (name: string, url: string) => void;
  onRemoveHost: (hostId: string) => void;
  showSetupChecklist: boolean;
  onShowSetupChecklist: () => void;
  hostingStatus: "idle" | "starting" | "active" | "error";
  hostingError: string | null;
  hostingAddress: string | null;
  onStartHosting: () => void;
  onStopHosting: () => void;
};

const NAV_ITEMS: { view: MainView; label: string; icon: ReactNode }[] = [
  {
    view: "terminal",
    label: "Terminal",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <polyline points="4 17 10 11 4 5" />
        <line x1="12" y1="19" x2="20" y2="19" />
      </svg>
    ),
  },
  {
    view: "chat",
    label: "Chat",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      </svg>
    ),
  },
  {
    view: "logs",
    label: "Logs",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="16" y1="13" x2="8" y2="13" />
        <line x1="16" y1="17" x2="8" y2="17" />
        <polyline points="10 9 9 9 8 9" />
      </svg>
    ),
  },
  {
    view: "settings",
    label: "Settings",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    ),
  },
];

export function Sidebar({
  hosts,
  mainView,
  onChangeView,
  onAddHost,
  onRemoveHost,
  showSetupChecklist,
  onShowSetupChecklist,
  hostingStatus,
  hostingError,
  hostingAddress,
  onStartHosting,
  onStopHosting,
}: Props) {
  const [showAddForm, setShowAddForm] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftUrl, setDraftUrl] = useState("http://");

  const handleSubmitHost = () => {
    const name = draftName.trim();
    const url = draftUrl.trim();
    if (!name || !url) return;
    if (!url.startsWith("http://") && !url.startsWith("https://")) return;
    onAddHost(name, url);
    setDraftName("");
    setDraftUrl("http://");
    setShowAddForm(false);
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <div className="sidebar-brand-icon">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 2L2 7l10 5 10-5-10-5z" />
            <path d="M2 17l10 5 10-5" />
            <path d="M2 12l10 5 10-5" />
          </svg>
        </div>
        <div>
          <div className="sidebar-brand-title">Ghost Protocol</div>
          <div className="sidebar-brand-subtitle">Developer Console</div>
        </div>
      </div>

      <nav className="sidebar-nav">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.view}
            className={`sidebar-nav-item ${mainView === item.view ? "active" : ""}`}
            onClick={() => onChangeView(item.view)}
          >
            {item.icon}
            <span>{item.label}</span>
          </button>
        ))}
      </nav>

      <div className="sidebar-spacer" />

      <div className="sidebar-hosts">
        <div className="sidebar-hosts-header">Hosts</div>
        {hosts.length === 0 && !showAddForm && (
          <div className="sidebar-hosts-empty">Add a remote host to connect</div>
        )}
        {hosts.map((conn) => (
          <div key={conn.host.id} className="sidebar-host-row">
            <span className={`status-dot ${conn.state}`} />
            <span className="sidebar-host-name">{conn.host.name}</span>
            <span className="sidebar-host-status">
              {conn.state === "connected" ? "connected" : conn.state === "connecting" ? "connecting" : "unreachable"}
            </span>
            <button
              className="sidebar-host-remove"
              onClick={() => onRemoveHost(conn.host.id)}
              title="Remove host"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        ))}

        {showAddForm ? (
          <div className="sidebar-add-host-form">
            <input
              className="sidebar-add-host-input"
              placeholder="Host name"
              value={draftName}
              onChange={(e) => setDraftName(e.currentTarget.value)}
              autoFocus
            />
            <input
              className="sidebar-add-host-input"
              placeholder="http://host:port"
              value={draftUrl}
              onChange={(e) => setDraftUrl(e.currentTarget.value)}
              onKeyDown={(e) => { if (e.key === "Enter") handleSubmitHost(); }}
            />
            <div className="sidebar-add-host-actions">
              <button className="btn-primary sidebar-add-host-btn" onClick={handleSubmitHost}>Connect</button>
              <button className="btn-secondary sidebar-add-host-btn" onClick={() => setShowAddForm(false)}>Cancel</button>
            </div>
          </div>
        ) : (
          <button className="sidebar-add-host-toggle" onClick={() => setShowAddForm(true)}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            Add host
          </button>
        )}
        {/* Hosting toggle */}
        <div className="sidebar-hosting">
          {hostingStatus === "idle" || hostingStatus === "error" ? (
            <button className="sidebar-hosting-btn" onClick={onStartHosting}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="5 3 19 12 5 21 5 3" />
              </svg>
              Host a connection
            </button>
          ) : hostingStatus === "starting" ? (
            <button className="sidebar-hosting-btn disabled" disabled>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" />
              </svg>
              Starting...
            </button>
          ) : (
            <div className="sidebar-hosting-active">
              <div className="sidebar-hosting-header">
                <span className="sidebar-hosting-label">
                  <span className="status-dot connected" />
                  Hosting
                </span>
                <button className="sidebar-hosting-stop" onClick={onStopHosting} title="Stop hosting">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
                  </svg>
                </button>
              </div>
              {hostingAddress && (
                <div className="sidebar-hosting-address">
                  <code>{hostingAddress}</code>
                  <button
                    className="sidebar-hosting-copy"
                    onClick={() => void navigator.clipboard.writeText(hostingAddress)}
                    title="Copy address"
                  >
                    Copy
                  </button>
                </div>
              )}
            </div>
          )}
          {hostingStatus === "error" && hostingError && (
            <div className="sidebar-hosting-error">
              {hostingError}
              {hostingError.includes("not installed") && (
                <>
                  {". "}
                  <button className="sidebar-setup-link-inline" onClick={onShowSetupChecklist}>
                    Set up this computer
                  </button>
                </>
              )}
            </div>
          )}
        </div>
        {!showSetupChecklist && (
          <button
            className="sidebar-setup-link"
            onClick={onShowSetupChecklist}
          >
            Set up this computer
          </button>
        )}
      </div>

      <div className="sidebar-user">
        <div className="sidebar-user-avatar">D</div>
        <div>
          <div className="sidebar-user-name">Developer</div>
          <div className="sidebar-user-email">dev@ghost-protocol</div>
        </div>
      </div>
    </aside>
  );
}

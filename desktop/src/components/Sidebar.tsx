import { ReactNode, useState } from "react";
import type { DiscoveredPeer, HostConnection, MainView } from "../types";

type Props = {
  hosts: HostConnection[];
  discoveries: DiscoveredPeer[];
  mainView: MainView;
  onChangeView: (view: MainView) => void;
  onAddHost: (name: string, url: string) => void;
  onRemoveHost: (hostId: string) => void;
  onAcceptDiscovery: (ip: string) => void;
  onDismissDiscovery: (ip: string) => void;
};

const NAV_ITEMS: { view: MainView; label: string; icon: ReactNode }[] = [
  {
    view: "agents",
    label: "Agents",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 8V4H8" /><rect width="16" height="12" x="4" y="8" rx="2" /><path d="M2 14h2" /><path d="M20 14h2" /><path d="M15 13v2" /><path d="M9 13v2" />
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
  discoveries,
  mainView,
  onChangeView,
  onAddHost,
  onRemoveHost,
  onAcceptDiscovery,
  onDismissDiscovery,
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

  const sortedHosts = [...hosts].sort((a, b) => {
    const order: Record<string, number> = { connected: 0, connecting: 1, error: 2, idle: 3 };
    return (order[a.state] ?? 3) - (order[b.state] ?? 3);
  });

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
        <div className="sidebar-hosts-header">
          Connections
          <button className="sidebar-add-btn" onClick={() => setShowAddForm(!showAddForm)} title="Add manually">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
            </svg>
          </button>
        </div>

        {discoveries.map((peer) => (
          <div key={peer.tailscaleIp} className="sidebar-discovery-card">
            <div className="sidebar-discovery-info">
              <span className="sidebar-discovery-name">{peer.name}</span>
              <span className="sidebar-discovery-ip muted">{peer.tailscaleIp}</span>
            </div>
            <div className="sidebar-discovery-actions">
              <button className="btn-discovery-add" onClick={() => onAcceptDiscovery(peer.tailscaleIp)}>Add</button>
              <button className="btn-discovery-dismiss" onClick={() => onDismissDiscovery(peer.tailscaleIp)}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            </div>
          </div>
        ))}

        {sortedHosts.length === 0 && discoveries.length === 0 && !showAddForm && (
          <div className="sidebar-hosts-empty">Add a remote host to connect</div>
        )}
        {sortedHosts.map((conn) => (
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

        {showAddForm && (
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

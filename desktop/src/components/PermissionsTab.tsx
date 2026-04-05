import { useCallback, useEffect, useState } from "react";
import type { PeerPermissionRecord, PermissionTier } from "../types";
import { listPermissions, setPermission } from "../api";

type Props = {
  daemonUrl: string;
};

const TIER_COLORS: Record<PermissionTier, string> = {
  "full-access": "#10b981",
  "approval-required": "#f59e0b",
  "read-only": "#60a5fa",
  "no-access": "#ef4444",
};

const TIER_OPTIONS: PermissionTier[] = [
  "full-access",
  "approval-required",
  "read-only",
  "no-access",
];

export function PermissionsTab({ daemonUrl }: Props) {
  const [permissions, setPermissions] = useState<PeerPermissionRecord[] | null>(null);
  const [saving, setSaving] = useState<Set<string>>(new Set());

  const fetchPermissions = useCallback(async () => {
    try {
      const data = await listPermissions(daemonUrl);
      setPermissions(data);
    } catch {
      // Silently ignore fetch errors — daemon may be temporarily unreachable
    }
  }, [daemonUrl]);

  useEffect(() => {
    void fetchPermissions();
    const timer = setInterval(() => void fetchPermissions(), 10_000);
    return () => clearInterval(timer);
  }, [fetchPermissions]);

  const handleTierChange = async (hostId: string, tier: PermissionTier) => {
    setSaving((prev) => new Set(prev).add(hostId));
    try {
      await setPermission(daemonUrl, hostId, tier);
      setPermissions((prev) =>
        prev
          ? prev.map((p) => (p.hostId === hostId ? { ...p, tier } : p))
          : prev
      );
    } catch {
      // Silently ignore — UI will revert on next poll
    } finally {
      setSaving((prev) => {
        const next = new Set(prev);
        next.delete(hostId);
        return next;
      });
    }
  };

  if (permissions === null) {
    return <div className="muted">Loading permissions...</div>;
  }

  if (permissions.length === 0) {
    return (
      <div className="empty-state">
        No known hosts. Add a host to configure permissions.
      </div>
    );
  }

  return (
    <div className="permissions-list">
      {permissions.map((p) => (
        <div key={p.hostId} className="permission-row">
          <div className="permission-host">
            <div className="permission-host-name">{p.hostName}</div>
            <div className="permission-host-ip">{p.tailscaleIp}</div>
          </div>
          <div className="permission-tier">
            <span
              className="tier-badge"
              style={{ color: TIER_COLORS[p.tier] }}
            >
              {p.tier}
            </span>
            <select
              value={p.tier}
              disabled={saving.has(p.hostId)}
              onChange={(e) =>
                void handleTierChange(p.hostId, e.currentTarget.value as PermissionTier)
              }
            >
              {TIER_OPTIONS.map((tier) => (
                <option key={tier} value={tier}>
                  {tier}
                </option>
              ))}
            </select>
          </div>
        </div>
      ))}
    </div>
  );
}

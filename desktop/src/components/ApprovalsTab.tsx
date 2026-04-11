import { useCallback, useEffect, useRef, useState } from "react";
import type { PendingApprovalRecord, TerminalSession } from "../types";
import { listApprovals, listChatMessages, resolveApproval, sendChatMessage } from "../api";

type Props = {
  daemonUrl: string;
  activeSession: TerminalSession | null;
  onPendingCountChange: (count: number) => void;
};

const APPROVAL_HINT_RE = /\b(needs your approval|need your approval|approval|approve|prompt to allow|allow it)\b/i;

function formatAction(method: string, path: string): string {
  if (method === "POST" && path === "/api/terminal/sessions") return "Create terminal session";
  if (method === "POST" && path.endsWith("/input")) return "Send terminal input";
  if (method === "POST" && path.endsWith("/resize")) return "Resize terminal";
  if (method === "POST" && path.endsWith("/terminate")) return "Terminate session";
  if (method === "POST" && path === "/api/hosts") return "Add host";
  if (method === "DELETE" && path.startsWith("/api/hosts/")) return "Remove host";
  return `${method} ${path}`;
}

function formatCountdown(expiresAt: string): string {
  const remaining = Math.max(0, new Date(expiresAt).getTime() - Date.now());
  const totalSeconds = Math.floor(remaining / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function CountdownTimer({ expiresAt }: { expiresAt: string }) {
  const [display, setDisplay] = useState(() => formatCountdown(expiresAt));

  useEffect(() => {
    const timer = setInterval(() => {
      setDisplay(formatCountdown(expiresAt));
    }, 1000);
    return () => clearInterval(timer);
  }, [expiresAt]);

  return <span className="countdown">{display}</span>;
}

export function ApprovalsTab({ daemonUrl, activeSession, onPendingCountChange }: Props) {
  const [approvals, setApprovals] = useState<PendingApprovalRecord[] | null>(null);
  const [agentApprovalHint, setAgentApprovalHint] = useState<string | null>(null);
  const [resolving, setResolving] = useState<Set<string>>(new Set());
  const [sendingHintResponse, setSendingHintResponse] = useState(false);
  const prevPendingCount = useRef<number>(-1);

  const fetchApprovals = useCallback(async () => {
    try {
      const data = await listApprovals(daemonUrl);
      setApprovals(data);
      const pendingCount = data.filter((a) => a.status === "pending").length;
      if (pendingCount !== prevPendingCount.current) {
        prevPendingCount.current = pendingCount;
        onPendingCountChange(pendingCount);
      }
    } catch {
      // Silently ignore fetch errors
    }
  }, [daemonUrl, onPendingCountChange]);

  useEffect(() => {
    void fetchApprovals();
    const timer = setInterval(() => void fetchApprovals(), 3_000);
    return () => clearInterval(timer);
  }, [fetchApprovals]);

  useEffect(() => {
    const isChatLike = activeSession && (
      activeSession.mode === "chat"
      || activeSession.driverKind === "structured_chat_driver"
      || activeSession.driverKind === "api_driver"
    );

    if (!activeSession?.id || !isChatLike) {
      setAgentApprovalHint(null);
      return undefined;
    }

    let cancelled = false;

    const fetchHint = async () => {
      try {
        const messages = await listChatMessages(daemonUrl, activeSession.id);
        if (cancelled) return;
        const match = [...messages]
          .reverse()
          .find((message) => (
            message.role === "assistant"
            && APPROVAL_HINT_RE.test(message.content)
          ));

        if (!match) {
          setAgentApprovalHint(null);
          return;
        }

        const compact = match.content.replace(/\s+/g, " ").trim();
        setAgentApprovalHint(compact.length > 180 ? `${compact.slice(0, 179)}…` : compact);
      } catch {
        if (!cancelled) {
          setAgentApprovalHint(null);
        }
      }
    };

    void fetchHint();
    const timer = setInterval(() => void fetchHint(), 3_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [activeSession, daemonUrl]);

  const handleHintRespond = async (message: string) => {
    if (!activeSession?.id) return;
    setSendingHintResponse(true);
    try {
      await sendChatMessage(daemonUrl, activeSession.id, message);
      setAgentApprovalHint(null);
    } catch {
      // ignore
    } finally {
      setSendingHintResponse(false);
    }
  };

  const handleResolve = async (id: string, action: "approve" | "deny") => {
    setResolving((prev) => new Set(prev).add(id));
    try {
      await resolveApproval(daemonUrl, id, action);
      await fetchApprovals();
    } catch {
      // Silently ignore
    } finally {
      setResolving((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
    }
  };

  if (approvals === null) {
    return <div className="muted">Loading approvals...</div>;
  }

  const pending = approvals.filter((a) => a.status === "pending");
  const resolved = approvals
    .filter((a) => a.status !== "pending")
    .slice(0, 10);

  if (approvals.length === 0 && !agentApprovalHint) {
    return null;
  }

  return (
    <div className="approvals-list">
      {pending.length > 0 && (
        <div className="approvals-section">
          <div className="muted">Pending</div>
          {pending.map((approval) => (
            <div key={approval.id} className="approval-card approval-pending">
              <div className="approval-info">
                <div className="approval-action">
                  {formatAction(approval.method, approval.path)}
                </div>
                <div className="approval-meta">
                  From: {approval.hostId} &middot;{" "}
                  Expires: <CountdownTimer expiresAt={approval.expiresAt} />
                </div>
              </div>
              <div className="approval-actions">
                <button
                  className="btn-approve"
                  disabled={resolving.has(approval.id)}
                  onClick={() => void handleResolve(approval.id, "approve")}
                >
                  Approve
                </button>
                <button
                  className="btn-reject"
                  disabled={resolving.has(approval.id)}
                  onClick={() => void handleResolve(approval.id, "deny")}
                >
                  Deny
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {resolved.length > 0 && (
        <div className="approvals-section">
          <div className="muted">Recent</div>
          {resolved.map((approval) => (
            <div
              key={approval.id}
              className={`approval-card approval-${approval.status}`}
            >
              <div className="approval-info">
                <div className="approval-action">
                  {formatAction(approval.method, approval.path)}
                </div>
                <div className="approval-meta">From: {approval.hostId}</div>
              </div>
              <span
                className={`approval-status-badge status-${approval.status}`}
              >
                {approval.status}
              </span>
            </div>
          ))}
        </div>
      )}

      {agentApprovalHint && (
        <div className="approvals-section">
          <div className="muted">Session hint</div>
          <div className="approval-card approval-pending">
            <div className="approval-info">
              <div className="approval-action">Agent reported an approval step</div>
              <div className="approval-meta">{agentApprovalHint}</div>
            </div>
            {activeSession?.id && (
              <div className="approval-actions">
                <button
                  className="btn-approve"
                  disabled={sendingHintResponse}
                  onClick={() => void handleHintRespond("Yes, proceed. You have approval to run the commands.")}
                >
                  Approve
                </button>
                <button
                  className="btn-reject"
                  disabled={sendingHintResponse}
                  onClick={() => void handleHintRespond("No, do not proceed with that action.")}
                >
                  Deny
                </button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

import { FormEvent, useMemo } from "react";

export type TerminalSession = {
  id: string;
  mode: "agent" | "project" | "rescue_shell";
  status: "created" | "running" | "exited" | "terminated" | "error";
  name?: string | null;
  workdir: string;
  command: string[];
  createdAt: string;
  startedAt?: string | null;
  finishedAt?: string | null;
  lastChunkAt?: string | null;
  pid?: number | null;
  exitCode?: number | null;
};

export type TerminalChunk = {
  id: number;
  sessionId: string;
  stream: "stdout" | "stderr" | "system";
  chunk: string;
  createdAt: string;
};

export type TerminalSessionDetail = {
  session: TerminalSession;
  chunks: TerminalChunk[];
};

type Props = {
  sessions: TerminalSession[];
  activeSessionId: string | null;
  sessionDetail: TerminalSessionDetail | null;
  sessionInput: string;
  error: string;
  onSelect: (sessionId: string) => void;
  onChangeInput: (value: string) => void;
  onCreateSession: (mode: "agent" | "rescue_shell") => void;
  onSendInput: () => void;
  onInterrupt: () => void;
  onTerminate: () => void;
  onRefresh: () => void;
};

export function RemoteSessionPanel({
  sessions,
  activeSessionId,
  sessionDetail,
  sessionInput,
  error,
  onSelect,
  onChangeInput,
  onCreateSession,
  onSendInput,
  onInterrupt,
  onTerminate,
  onRefresh,
}: Props) {
  const output = useMemo(
    () => (sessionDetail?.chunks ?? []).map((item) => item.chunk).join(""),
    [sessionDetail],
  );

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    onSendInput();
  }

  return (
    <section className="stack-md remote-session-panel">
      <div className="panel-header remote-session-header">
        <div>
          <h3>Shared terminal</h3>
          <p className="muted">tmux-backed remote terminals for Hermes and rescue access</p>
        </div>
        <div className="toolbar">
          <button onClick={() => onCreateSession("agent")}>New shared Hermes</button>
          <button onClick={() => onCreateSession("rescue_shell")}>New rescue shell</button>
          <button onClick={onRefresh}>Refresh</button>
        </div>
      </div>

      {error ? <div className="error-banner">{error}</div> : null}

      <div className="remote-session-list">
        {sessions.length === 0 ? <div className="empty-state">No remote sessions yet</div> : null}
        {sessions.map((session) => (
          <button
            key={session.id}
            className={`run-item ${session.id === activeSessionId ? "selected" : ""}`}
            onClick={() => onSelect(session.id)}
          >
            <strong>{session.name || session.mode.replace("_", " ")}</strong>
            <span>
              {session.status} · {session.workdir}
            </span>
          </button>
        ))}
      </div>

      {sessionDetail ? (
        <>
          <div className="stats-grid terminal-stats-grid">
            <div><span>Status</span><strong>{sessionDetail.session.status}</strong></div>
            <div><span>PID</span><strong>{sessionDetail.session.pid ?? "—"}</strong></div>
            <div><span>Mode</span><strong>{sessionDetail.session.mode}</strong></div>
            <div><span>Exit code</span><strong>{sessionDetail.session.exitCode ?? "—"}</strong></div>
          </div>

          <pre className="terminal-output">{output || "(session output will appear here)"}</pre>

          <form className="composer" onSubmit={handleSubmit}>
            <textarea
              value={sessionInput}
              onChange={(event) => onChangeInput(event.currentTarget.value)}
              placeholder="Send input to the remote Hermes session…"
              rows={3}
            />
            <div className="toolbar">
              <button type="submit" disabled={!sessionInput.trim()}>Send</button>
              <button type="button" onClick={onInterrupt} disabled={!activeSessionId}>Ctrl+C</button>
              <button type="button" onClick={onTerminate} disabled={!activeSessionId}>Terminate</button>
            </div>
          </form>
        </>
      ) : (
        <div className="empty-state">Select a remote session to inspect or control it.</div>
      )}
    </section>
  );
}

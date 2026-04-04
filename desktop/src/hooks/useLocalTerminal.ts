import { useCallback, useEffect, useRef, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { appLog } from "../log";
import type { LocalTerminalSession } from "../types";

const SRC = "local-pty";

export type UseLocalTerminalOptions = {
  sessionId: string | null;
  terminalRef: React.RefObject<Terminal | null>;
  onSessionStatusChange?: (session: LocalTerminalSession) => void;
  onError?: (message: string) => void;
};

export type UseLocalTerminalReturn = {
  sendInput: (data: string) => void;
  resize: (cols: number, rows: number) => void;
  kill: () => void;
  sessionMeta: LocalTerminalSession | null;
  isConnected: boolean;
};

type PtyChunkPayload = {
  session_id: string;
  data: string;
};

type PtyStatusPayload = {
  session_id: string;
  status: string;
  exit_code: number | null;
};

export function useLocalTerminal({
  sessionId,
  terminalRef,
  onSessionStatusChange,
  onError,
}: UseLocalTerminalOptions): UseLocalTerminalReturn {
  const sessionIdRef = useRef(sessionId);
  const onStatusChangeRef = useRef(onSessionStatusChange);
  const onErrorRef = useRef(onError);
  const chunkBufferRef = useRef<string[]>([]);

  const [sessionMeta, setSessionMeta] = useState<LocalTerminalSession | null>(null);
  const [isConnected, setIsConnected] = useState(false);

  useEffect(() => { sessionIdRef.current = sessionId; }, [sessionId]);
  useEffect(() => { onStatusChangeRef.current = onSessionStatusChange; }, [onSessionStatusChange]);
  useEffect(() => { onErrorRef.current = onError; }, [onError]);

  // Flush buffered chunks when terminal becomes available
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || chunkBufferRef.current.length === 0) return;
    for (const data of chunkBufferRef.current) {
      terminal.write(data);
    }
    chunkBufferRef.current = [];
  });

  // Main event listener lifecycle
  useEffect(() => {
    if (!sessionId) {
      setSessionMeta(null);
      setIsConnected(false);
      return;
    }

    let cancelled = false;
    const currentSessionId = sessionId;

    appLog.info(SRC, `Attaching to PTY session ${currentSessionId.slice(0, 8)}`);

    // Reset terminal for fresh session
    const terminal = terminalRef.current;
    if (terminal) terminal.reset();
    chunkBufferRef.current = [];

    setSessionMeta({
      id: currentSessionId,
      status: "running",
      createdAt: new Date().toISOString(),
    });
    setIsConnected(true);

    // Listen for pty:chunk events
    const chunkUnlisten = listen<PtyChunkPayload>("pty:chunk", (event) => {
      if (cancelled || event.payload.session_id !== currentSessionId) return;
      const term = terminalRef.current;
      if (term) {
        term.write(event.payload.data);
      } else {
        chunkBufferRef.current.push(event.payload.data);
      }
    });

    // Listen for pty:status events
    const statusUnlisten = listen<PtyStatusPayload>("pty:status", (event) => {
      if (cancelled || event.payload.session_id !== currentSessionId) return;
      const status = event.payload.status as LocalTerminalSession["status"];
      appLog.info(SRC, `Session ${currentSessionId.slice(0, 8)} status: ${status} (exit_code=${event.payload.exit_code})`);
      const updated: LocalTerminalSession = {
        id: currentSessionId,
        status,
        createdAt: sessionMeta?.createdAt ?? new Date().toISOString(),
        exitCode: event.payload.exit_code,
      };
      setSessionMeta(updated);
      setIsConnected(status === "running");
      onStatusChangeRef.current?.(updated);
    });

    return () => {
      cancelled = true;
      setIsConnected(false);
      chunkUnlisten.then((unlisten) => unlisten());
      statusUnlisten.then((unlisten) => unlisten());
      appLog.info(SRC, `Detached from PTY session ${currentSessionId.slice(0, 8)}`);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, terminalRef]);

  const sendInput = useCallback((data: string) => {
    const sid = sessionIdRef.current;
    if (!sid) return;
    invoke("pty_write", { sessionId: sid, data }).catch((err: unknown) => {
      appLog.error(SRC, `pty_write failed: ${err}`);
      onErrorRef.current?.(`Failed to send input: ${err}`);
    });
  }, []);

  const resize = useCallback((cols: number, rows: number) => {
    const sid = sessionIdRef.current;
    if (!sid) return;
    invoke("pty_resize", { sessionId: sid, cols, rows }).catch((err: unknown) => {
      appLog.error(SRC, `pty_resize failed: ${err}`);
    });
  }, []);

  const kill = useCallback(() => {
    const sid = sessionIdRef.current;
    if (!sid) {
      onErrorRef.current?.("No active PTY session");
      return;
    }
    appLog.info(SRC, `Killing PTY session ${sid.slice(0, 8)}`);
    invoke("pty_kill", { sessionId: sid }).catch((err: unknown) => {
      appLog.error(SRC, `pty_kill failed: ${err}`);
      onErrorRef.current?.(`Failed to kill session: ${err}`);
    });
  }, []);

  return { sendInput, resize, kill, sessionMeta, isConnected };
}

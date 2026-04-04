import { useCallback, useEffect, useRef, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import { wsUrlFromHttp } from "../api";
import { appLog } from "../log";
import type { TerminalChunk, TerminalSession } from "../types";

const SRC = "terminal-ws";
const RECONNECT_BASE_MS = 500;
const RECONNECT_MAX_MS = 5000;

export type SessionChunkCache = {
  chunks: string[];
  lastChunkId: number;
};

export type UseTerminalSocketOptions = {
  baseUrl: string;
  sessionId: string | null;
  terminalRef: React.RefObject<Terminal | null>;
  initialCache?: SessionChunkCache | null;
  onSessionStatusChange?: (session: TerminalSession) => void;
  onError?: (message: string) => void;
};

export type UseTerminalSocketReturn = {
  sendInput: (data: string, appendNewline?: boolean) => void;
  resize: (cols: number, rows: number) => void;
  interrupt: () => void;
  terminate: () => void;
  sessionMeta: TerminalSession | null;
  isConnected: boolean;
  getChunkCache: () => SessionChunkCache;
};

export function useTerminalSocket({
  baseUrl,
  sessionId,
  terminalRef,
  initialCache,
  onSessionStatusChange,
  onError,
}: UseTerminalSocketOptions): UseTerminalSocketReturn {
  const wsRef = useRef<WebSocket | null>(null);
  const lastChunkIdRef = useRef<number>(0);
  const chunkBufferRef = useRef<string[]>([]);
  const chunkCacheRef = useRef<string[]>([]);
  const sessionIdRef = useRef(sessionId);
  const initialCacheRef = useRef(initialCache);
  const onStatusChangeRef = useRef(onSessionStatusChange);
  const onErrorRef = useRef(onError);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [sessionMeta, setSessionMeta] = useState<TerminalSession | null>(null);
  const [isConnected, setIsConnected] = useState(false);

  useEffect(() => { sessionIdRef.current = sessionId; }, [sessionId]);
  useEffect(() => { initialCacheRef.current = initialCache; }, [initialCache]);
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

  // Main WebSocket lifecycle with auto-reconnect
  useEffect(() => {
    if (!sessionId) {
      setSessionMeta(null);
      setIsConnected(false);
      return;
    }

    // Each effect invocation gets its own "cancelled" flag.
    // When cleanup runs (session change or unmount), cancelled becomes true
    // and no reconnect can fire for this stale connection.
    let cancelled = false;
    let reconnectAttempt = 0;

    const currentSessionId = sessionId;

    function connect(isReconnect: boolean) {
      if (cancelled) return;

      // Reset terminal only on fresh connect (not reconnect)
      if (!isReconnect) {
        const terminal = terminalRef.current;
        if (terminal) terminal.reset();

        // Restore from cache if available — write cached content locally
        // and subscribe only for new chunks from the server
        const cache = initialCacheRef.current;
        if (cache && cache.chunks.length > 0) {
          chunkCacheRef.current = [...cache.chunks];
          lastChunkIdRef.current = cache.lastChunkId;
          if (terminal) {
            for (const data of cache.chunks) {
              terminal.write(data);
            }
          }
        } else {
          chunkCacheRef.current = [];
          lastChunkIdRef.current = 0;
        }
        chunkBufferRef.current = [];
      }

      const url = wsUrlFromHttp(baseUrl);
      appLog.info(SRC, `${isReconnect ? "Reconnecting" : "Connecting"} to ${url} for session ${currentSessionId.slice(0, 8)}`);

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.onopen = () => {
        if (cancelled) { ws.close(); return; }
        appLog.info(SRC, `Connected, subscribing to session ${currentSessionId.slice(0, 8)} afterChunkId=${lastChunkIdRef.current}`);
        reconnectAttempt = 0;
        setIsConnected(true);
        ws.send(JSON.stringify({
          op: "subscribe_terminal",
          sessionId: currentSessionId,
          afterChunkId: lastChunkIdRef.current,
        }));
      };

      ws.onmessage = (event) => {
        if (cancelled) return;

        let data: Record<string, unknown>;
        try {
          data = JSON.parse(event.data as string);
        } catch {
          appLog.error(SRC, `Invalid JSON from server: ${(event.data as string).slice(0, 100)}`);
          return;
        }

        if (data.op === "subscribed_terminal") {
          appLog.info(SRC, `Subscribed, replaying ${data.replayed ?? 0} chunks`);
          if (data.session) {
            setSessionMeta(data.session as TerminalSession);
          }
        } else if (data.op === "terminal_chunk") {
          const chunk = data.chunk as TerminalChunk;
          if (chunk.id <= lastChunkIdRef.current) return;
          lastChunkIdRef.current = chunk.id;
          chunkCacheRef.current.push(chunk.chunk);
          const term = terminalRef.current;
          if (term) {
            term.write(chunk.chunk);
          } else {
            chunkBufferRef.current.push(chunk.chunk);
          }
        } else if (data.op === "terminal_status") {
          const session = data.session as TerminalSession;
          appLog.info(SRC, `Session status: ${session.status}`);
          setSessionMeta(session);
          onStatusChangeRef.current?.(session);
        } else if (data.op === "error") {
          const msg = (data.message as string) ?? "Terminal websocket error";
          appLog.error(SRC, msg);
          onErrorRef.current?.(msg);
        }
      };

      ws.onerror = () => {
        appLog.error(SRC, "WebSocket error event");
      };

      ws.onclose = (event) => {
        if (wsRef.current === ws) {
          wsRef.current = null;
          setIsConnected(false);
        }

        // If this effect has been cleaned up, don't reconnect — the session changed or we unmounted.
        if (cancelled) {
          appLog.info(SRC, "WebSocket closed (session changed, no reconnect)");
          return;
        }

        appLog.warn(SRC, `WebSocket closed: code=${event.code} reason=${event.reason || "none"}`);

        // Auto-reconnect with exponential backoff
        const delay = Math.min(RECONNECT_BASE_MS * Math.pow(2, reconnectAttempt), RECONNECT_MAX_MS);
        appLog.info(SRC, `Reconnecting in ${delay}ms (attempt ${reconnectAttempt + 1})`);
        reconnectAttempt += 1;
        reconnectTimerRef.current = setTimeout(() => {
          reconnectTimerRef.current = null;
          connect(true);
        }, delay);
      };
    }

    connect(false);

    return () => {
      cancelled = true;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      const ws = wsRef.current;
      if (ws) {
        wsRef.current = null;
        ws.close();
      }
      setIsConnected(false);
    };
  }, [baseUrl, sessionId, terminalRef]);

  const sendInput = useCallback((input: string, appendNewline = false) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN || !sessionIdRef.current) return;
    ws.send(JSON.stringify({
      op: "terminal_input",
      sessionId: sessionIdRef.current,
      input,
      appendNewline,
    }));
  }, []);

  const resize = useCallback((cols: number, rows: number) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN || !sessionIdRef.current) return;
    ws.send(JSON.stringify({
      op: "resize_terminal",
      sessionId: sessionIdRef.current,
      cols,
      rows,
    }));
  }, []);

  const interrupt = useCallback(() => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN || !sessionIdRef.current) {
      onErrorRef.current?.("Terminal connection is not ready");
      return;
    }
    ws.send(JSON.stringify({ op: "interrupt_terminal", sessionId: sessionIdRef.current }));
  }, []);

  const terminate = useCallback(() => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN || !sessionIdRef.current) {
      onErrorRef.current?.("Terminal connection is not ready");
      return;
    }
    ws.send(JSON.stringify({ op: "terminate_terminal", sessionId: sessionIdRef.current }));
  }, []);

  const getChunkCache = useCallback((): SessionChunkCache => ({
    chunks: chunkCacheRef.current,
    lastChunkId: lastChunkIdRef.current,
  }), []);

  return { sendInput, resize, interrupt, terminate, sessionMeta, isConnected, getChunkCache };
}

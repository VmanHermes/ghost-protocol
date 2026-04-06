import { useCallback, useEffect, useRef, useState } from "react";
import { wsUrlFromHttp } from "../api";
import type { ChatMessage } from "../types";

export type UseChatSocketOptions = {
  baseUrl: string;
  sessionId: string | null;
  isActive: boolean;
  onError?: (message: string) => void;
  onSessionRenamed?: (sessionId: string, name: string) => void;
};

export type ChatSessionMeta = {
  tokens: number | null;
  contextPct: number | null;
  status: string;
};

export type UseChatSocketReturn = {
  messages: ChatMessage[];
  streamingDelta: string;
  streamingMessageId: string | null;
  meta: ChatSessionMeta;
  isConnected: boolean;
  sendMessage: (content: string) => void;
};

function mergeStreamingText(current: string, incoming: string): string {
  if (!incoming) return current;
  if (!current) return incoming;
  if (incoming.startsWith(current)) return incoming;
  if (current.endsWith(incoming)) return current;

  const maxOverlap = Math.min(current.length, incoming.length);
  for (let overlap = maxOverlap; overlap > 0; overlap -= 1) {
    if (current.slice(-overlap) === incoming.slice(0, overlap)) {
      return current + incoming.slice(overlap);
    }
  }

  return current + incoming;
}

export function useChatSocket({
  baseUrl,
  sessionId,
  isActive,
  onError,
  onSessionRenamed,
}: UseChatSocketOptions): UseChatSocketReturn {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [streamingDelta, setStreamingDelta] = useState("");
  const [streamingMessageId, setStreamingMessageId] = useState<string | null>(null);
  const [meta, setMeta] = useState<ChatSessionMeta>({
    tokens: null, contextPct: null, status: "idle",
  });
  const [isConnected, setIsConnected] = useState(false);

  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const backoffRef = useRef(500);
  const streamingMessageIdRef = useRef<string | null>(null);

  // Reset state when session changes
  useEffect(() => {
    setMessages([]);
    setStreamingDelta("");
    setStreamingMessageId(null);
    streamingMessageIdRef.current = null;
    setMeta({ tokens: null, contextPct: null, status: "idle" });
  }, [sessionId]);

  useEffect(() => {
    if (!isActive || !sessionId) return;

    let disposed = false;

    function connect() {
      if (disposed) return;

      const wsUrl = wsUrlFromHttp(baseUrl);
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => {
        setIsConnected(true);
        backoffRef.current = 500;
        ws.send(JSON.stringify({ op: "subscribe_chat", sessionId }));
      };

      ws.onmessage = (event) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        let data: any;
        try { data = JSON.parse(event.data); } catch { return; }

        switch (data.op) {
          case "chat_message":
            if (data.message) {
              setMessages((prev) => {
                if (prev.some((m) => m.id === data.message.id)) return prev;
                return [...prev, data.message];
              });
              setStreamingDelta("");
              setStreamingMessageId(null);
              streamingMessageIdRef.current = null;
            }
            break;
          case "chat_delta":
            if (data.delta && data.messageId) {
              if (streamingMessageIdRef.current && streamingMessageIdRef.current !== data.messageId) {
                setStreamingDelta(data.delta);
              } else {
                setStreamingDelta((prev) => mergeStreamingText(prev, data.delta));
              }
              streamingMessageIdRef.current = data.messageId;
              setStreamingMessageId(data.messageId);
            }
            break;
          case "chat_status":
            if (data.status) {
              setMeta((prev) => ({ ...prev, status: data.status }));
            }
            break;
          case "session_meta":
            setMeta((prev) => ({
              ...prev,
              tokens: data.tokens ?? prev.tokens,
              contextPct: data.contextPct ?? prev.contextPct,
            }));
            break;
          case "session_renamed":
            if (data.sessionId && data.name) {
              onSessionRenamed?.(data.sessionId, data.name);
            }
            break;
          case "subscribed_chat":
            break;
          case "error":
            onError?.(data.message ?? "WebSocket error");
            break;
        }
      };

      ws.onclose = () => {
        setIsConnected(false);
        wsRef.current = null;
        if (!disposed) {
          reconnectTimerRef.current = setTimeout(() => {
            backoffRef.current = Math.min(backoffRef.current * 2, 5000);
            connect();
          }, backoffRef.current);
        }
      };

      ws.onerror = () => { ws.close(); };
    }

    connect();

    return () => {
      disposed = true;
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
      wsRef.current?.close();
      wsRef.current = null;
      setIsConnected(false);
    };
  }, [baseUrl, sessionId, isActive, onError, onSessionRenamed]);

  const sendMessage = useCallback(
    (content: string) => {
      if (!sessionId) return;
      fetch(`${baseUrl}/api/chat/sessions/${sessionId}/message`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content }),
      })
        .then((res) => res.json())
        .then((msg: ChatMessage) => {
          setMessages((prev) => {
            if (prev.some((m) => m.id === msg.id)) return prev;
            return [...prev, msg];
          });
        })
        .catch((e) => onError?.(e.message));
    },
    [baseUrl, sessionId, onError],
  );

  return { messages, streamingDelta, streamingMessageId, meta, isConnected, sendMessage };
}

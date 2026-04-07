import { useCallback, useEffect, useRef, useState } from "react";
import { listSystemLogs, type BackendLogEntry } from "../api";
import { appLog, LogEntry, LogLevel } from "../log";

type Props = {
  baseUrl: string | null;
};

const LEVEL_COLORS: Record<LogLevel, string> = {
  debug: "#94a3b8",
  info: "#60a5fa",
  warn: "#fbbf24",
  error: "#f87171",
};

export function LogViewer({ baseUrl }: Props) {
  const [filter, setFilter] = useState<"all" | "client" | "server">("all");
  const [levelFilter, setLevelFilter] = useState<LogLevel | "all">("all");
  const [clientLogs, setClientLogs] = useState<LogEntry[]>([...appLog.entries]);
  const [serverLogs, setServerLogs] = useState<BackendLogEntry[]>([]);
  const [autoScroll, setAutoScroll] = useState(true);
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Subscribe to live client log updates
  useEffect(() => {
    return appLog.subscribe((entry) => {
      setClientLogs((prev) => [...prev.slice(-999), entry]);
    });
  }, []);

  // Fetch server logs on mount and periodically
  const fetchServerLogs = useCallback(async () => {
    if (!baseUrl) return;
    try {
      const data = await listSystemLogs(baseUrl, 300);
      setServerLogs(data);
    } catch {
      // Server might be down — that's exactly when we need client logs
    }
  }, [baseUrl]);

  useEffect(() => {
    if (!baseUrl) return;
    void fetchServerLogs();
    const timer = setInterval(() => void fetchServerLogs(), 5000);
    return () => clearInterval(timer);
  }, [fetchServerLogs, baseUrl]);

  // Auto-scroll
  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [clientLogs, serverLogs, autoScroll]);

  const merged = (() => {
    const items: { ts: string | null; level: string; source: string; message: string; origin: "client" | "server" }[] = [];
    if (filter !== "server") {
      for (const e of clientLogs) {
        items.push({ ts: e.ts, level: e.level, source: e.source, message: e.message, origin: "client" });
      }
    }
    if (filter !== "client") {
      for (const e of serverLogs) {
        items.push({ ts: e.ts, level: e.level.toLowerCase(), source: e.logger, message: e.message, origin: "server" });
      }
    }
    items.sort((a, b) => (a.ts ?? "").localeCompare(b.ts ?? ""));
    if (levelFilter !== "all") {
      return items.filter((e) => e.level === levelFilter);
    }
    return items;
  })();

  function handleExport() {
    const text = merged
      .map((e) => `${e.ts ?? "unknown"} [${e.origin}] [${e.level.toUpperCase()}] ${e.source}: ${e.message}`)
      .join("\n");
    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `ghost-protocol-logs-${new Date().toISOString().slice(0, 19)}.txt`;
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <section className="log-viewer">
      <div className="panel-header">
        <h3>Logs</h3>
        <div className="toolbar">
          <select value={filter} onChange={(e) => setFilter(e.currentTarget.value as typeof filter)}>
            <option value="all">All</option>
            <option value="client">Client</option>
            {baseUrl && <option value="server">Server</option>}
          </select>
          <select value={levelFilter} onChange={(e) => setLevelFilter(e.currentTarget.value as typeof levelFilter)}>
            <option value="all">All levels</option>
            <option value="error">Error</option>
            <option value="warn">Warn</option>
            <option value="info">Info</option>
            <option value="debug">Debug</option>
          </select>
          <button onClick={() => setAutoScroll(!autoScroll)}>{autoScroll ? "Pinned" : "Scroll"}</button>
          <button onClick={handleExport}>Export</button>
          <button onClick={() => void fetchServerLogs()}>Refresh</button>
        </div>
      </div>
      <div ref={containerRef} className="log-entries">
        {merged.length === 0 ? <div className="empty-state">No log entries</div> : null}
        {merged.map((entry, i) => (
          <div key={i} className="log-entry">
            <span className="log-ts">{entry.ts ? entry.ts.slice(11, 23) : "unknown"}</span>
            <span className="log-origin">{entry.origin === "server" ? "S" : "C"}</span>
            <span className="log-level" style={{ color: LEVEL_COLORS[entry.level as LogLevel] ?? "#94a3b8" }}>
              {entry.level.slice(0, 4).toUpperCase()}
            </span>
            <span className="log-source">{entry.source}</span>
            <span className="log-message">{entry.message}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

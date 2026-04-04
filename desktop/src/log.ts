export type LogLevel = "debug" | "info" | "warn" | "error";

export type LogEntry = {
  ts: string;
  level: LogLevel;
  source: string;
  message: string;
};

type LogListener = (entry: LogEntry) => void;

const MAX_ENTRIES = 1000;

class LogService {
  private _entries: LogEntry[] = [];
  private _listeners = new Set<LogListener>();

  get entries(): readonly LogEntry[] {
    return this._entries;
  }

  log(level: LogLevel, source: string, message: string) {
    const entry: LogEntry = {
      ts: new Date().toISOString(),
      level,
      source,
      message,
    };
    this._entries.push(entry);
    if (this._entries.length > MAX_ENTRIES) {
      this._entries = this._entries.slice(-MAX_ENTRIES);
    }
    for (const listener of this._listeners) {
      listener(entry);
    }
  }

  debug(source: string, message: string) { this.log("debug", source, message); }
  info(source: string, message: string) { this.log("info", source, message); }
  warn(source: string, message: string) { this.log("warn", source, message); }
  error(source: string, message: string) { this.log("error", source, message); }

  subscribe(listener: LogListener): () => void {
    this._listeners.add(listener);
    return () => this._listeners.delete(listener);
  }

  clear() {
    this._entries = [];
  }

  export(): string {
    return this._entries.map((e) => `${e.ts} [${e.level.toUpperCase()}] ${e.source}: ${e.message}`).join("\n");
  }
}

export const appLog = new LogService();

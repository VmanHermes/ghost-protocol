import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { useTerminalSocket } from "../hooks/useTerminalSocket";
import { useLocalTerminal } from "../hooks/useLocalTerminal";
import type { TerminalSession, LocalTerminalSession } from "../types";

type Props = {
  baseUrl: string;
  sessionId: string | null;
  isLocal: boolean;
  isActive: boolean;
  onSessionStatusChange?: (session: TerminalSession | LocalTerminalSession) => void;
  onError?: (message: string) => void;
};

const TERMINAL_THEME = {
  background: "#1a1f36",
  foreground: "#e2e8f0",
  cursor: "#93c5fd",
  green: "#10b981",
  blue: "#60a5fa",
  yellow: "#fbbf24",
  red: "#f87171",
  cyan: "#22d3ee",
  magenta: "#c084fc",
};

export function TerminalRenderer({ baseUrl, sessionId, isLocal, isActive, onSessionStatusChange, onError }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (!containerRef.current || !isActive) return;
    const terminal = new Terminal({
      cursorBlink: true, fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      theme: TERMINAL_THEME, allowProposedApi: true,
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(containerRef.current);
    fitAddon.fit();
    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;
    const observer = new ResizeObserver(() => { try { fitAddon.fit(); } catch {} });
    observer.observe(containerRef.current);
    return () => { observer.disconnect(); terminal.dispose(); terminalRef.current = null; fitAddonRef.current = null; };
  }, [isActive, sessionId]);

  const remote = useTerminalSocket({
    baseUrl, sessionId: !isLocal ? sessionId : null, terminalRef,
    isActive: isActive && !isLocal,
    onSessionStatusChange: onSessionStatusChange as (s: TerminalSession) => void,
    onError,
  });

  const local = useLocalTerminal({
    sessionId: isLocal ? sessionId : null, terminalRef,
    isActive: isActive && isLocal,
    onSessionStatusChange: onSessionStatusChange as (s: LocalTerminalSession) => void,
    onError,
  });

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || !isActive || !sessionId) return;
    const sendInput = isLocal ? local.sendInput : remote.sendInput;
    const disposable = terminal.onData((data) => {
      if (isLocal) sendInput(data);
      else sendInput(data, false);
    });
    return () => disposable.dispose();
  }, [isActive, sessionId, isLocal, local.sendInput, remote.sendInput]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || !isActive || !sessionId) return;
    const resize = isLocal ? local.resize : remote.resize;
    const disposable = terminal.onResize(({ cols, rows }) => resize(cols, rows));
    return () => disposable.dispose();
  }, [isActive, sessionId, isLocal, local.resize, remote.resize]);

  return <div ref={containerRef} className="terminal-host" style={{ flex: 1, minHeight: 0 }} />;
}

# Terminal Feedback Fixes Design

**Date:** 2026-04-04
**Scope:** Frontend fixes for terminal UX feedback (4 items)
**Files affected:** `desktop/src/components/TerminalWorkspace.tsx`, `desktop/src/hooks/useTerminalSocket.ts`, `desktop/src/hooks/useLocalTerminal.ts`, `desktop/src/App.css`

---

## 1. Remove `agent` mode from session creation UI

The `agent` session mode (Hermes AI agent) is not wired to any backend functionality. Exposing it confuses users about the difference between terminal types.

**Change:** Remove the `agent` button from the add-session dropdown menu in `TerminalWorkspace.tsx` (lines 336-339). Keep `rescue_shell` and `project` as the two available remote session modes.

No backend changes. The daemon still supports the `agent` mode — it's just hidden from the UI until Hermes integration is ready.

---

## 2. Terminal character spacing

Users report characters appear too spread out compared to other terminal emulators.

**Change:** Adjust xterm.js Terminal config in `TerminalWorkspace.tsx` (lines 186-204):
- Set `letterSpacing: -1` (tighten character spacing)
- Add `lineHeight: 1.0` (prevent browser default expansion)

**Validation:** Visual comparison with a native terminal. If `-1` is too tight or `0` with `lineHeight: 1.0` is sufficient, adjust during implementation. The goal is to match the look of a standard terminal emulator.

---

## 3. Mouse wheel scrolling doesn't scroll terminal history

Users report mouse wheel scrolling doesn't scroll through terminal output history — it may navigate inputs or do nothing useful.

**Investigation during implementation:**
- Check xterm.js version for known scroll bugs
- Test whether `overflow: hidden` on `.terminal-host` (App.css line 612) is intercepting wheel events before xterm.js can handle them
- Check if FitAddon or container sizing prevents xterm's viewport from receiving wheel events
- Test in isolation (minimal xterm.js setup) to rule out config issues

**Fix:** Determined during investigation. Likely a CSS `overflow` interaction or xterm.js viewport sizing issue.

---

## 4. Text bleeding between remote sessions (Critical)

Terminal output from one session appears in another session's terminal. This is the most serious bug.

### Root Cause

Both `useTerminalSocket` (remote sessions) and `useLocalTerminal` (local sessions) share a single xterm.js `Terminal` instance via `terminalRef`. There is no coordination over which hook is allowed to write to the terminal at any given time.

Three specific bugs:

1. **Unguarded buffer flush in `useTerminalSocket` (lines 62-69):** The buffer flush effect runs on every render with no dependency array. It writes buffered chunks to the terminal even when a local session is the active display target.

2. **No active-writer coordination:** When chunks arrive via WebSocket (`onmessage`, line 147-157) or Tauri events (`pty:chunk`, `useLocalTerminal.ts` line 91-98), they write directly to `terminalRef.current` without checking whether their session type is currently displayed.

3. **Stale refs across session switches:** `chunkBufferRef` and `chunkCacheRef` in `useTerminalSocket` are `useRef` values that persist across renders. Chunks buffered for session A can flush into session B's terminal after a switch.

### Fix: Active Writer Guard

Pass an `isActive` boolean prop to each hook, derived from `isLocalSession` in `TerminalWorkspace`:
- `useTerminalSocket` receives `isActive: !isLocalSession` (active when a remote session is displayed)
- `useLocalTerminal` receives `isActive: isLocalSession` (active when a local session is displayed)

Each hook's behavior when `isActive` is false:
- **Do not write to the terminal** — chunks arriving while inactive are buffered/cached but not written
- **Buffer flush is skipped** — the unguarded flush effect checks `isActive` before writing
- WebSocket/Tauri event listeners still run (to maintain connection and cache state), but `terminal.write()` calls are gated

Each hook's behavior when `isActive` transitions from false to true:
- Terminal is reset and content is replayed from cache (existing behavior, already happens on session switch)

### Interface Changes

```typescript
// useTerminalSocket
export type UseTerminalSocketOptions = {
  baseUrl: string;
  sessionId: string | null;
  terminalRef: React.RefObject<Terminal | null>;
  isActive: boolean;              // NEW
  initialCache?: SessionChunkCache | null;
  onSessionStatusChange?: (session: TerminalSession) => void;
  onError?: (message: string) => void;
};

// useLocalTerminal
export type UseLocalTerminalOptions = {
  sessionId: string | null;
  terminalRef: React.RefObject<Terminal | null>;
  isActive: boolean;              // NEW
  onSessionStatusChange?: (session: LocalTerminalSession) => void;
  onError?: (message: string) => void;
};
```

### Callsite in TerminalWorkspace

```typescript
const { ... } = useTerminalSocket({
  baseUrl: activeBaseUrl,
  sessionId: remoteSessionId,
  terminalRef,
  isActive: !isLocalSession,       // NEW
  initialCache,
  onSessionStatusChange: onRemoteSessionStatusChange,
  onError: setError,
});

const { ... } = useLocalTerminal({
  sessionId: isLocalSession ? activeSessionId : null,
  terminalRef,
  isActive: isLocalSession,        // NEW
  onSessionStatusChange: onLocalSessionStatusChange,
  onError: setError,
});
```

---

## Items Not Fixed

- **Shift+Enter for new row (feedback #3):** Standard terminal behavior. Shells handle multi-line input via `\` continuation. Not a bug.
- **Hermes-specific scroll behavior (feedback #4 partial):** The mouse wheel issue in Hermes specifically is out of scope since Hermes is not wired up. The general terminal scroll issue is addressed in section 3.

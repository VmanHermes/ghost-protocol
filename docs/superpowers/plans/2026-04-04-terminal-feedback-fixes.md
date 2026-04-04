# Terminal Feedback Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 4 terminal UX issues from user feedback: remove unused agent mode, fix character spacing, fix mouse wheel scrolling, and fix text bleeding between sessions.

**Architecture:** All changes are frontend-only in the Tauri desktop app. The critical fix (session bleeding) adds an `isActive` guard to both terminal hooks so only the currently-displayed session writes to the shared xterm.js Terminal instance. The other fixes are config/CSS adjustments.

**Tech Stack:** React 19, xterm.js v6 (`@xterm/xterm`), TypeScript, Vite, Tauri v2

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `desktop/src/components/TerminalWorkspace.tsx` | Modify | Remove agent button, adjust xterm config, pass `isActive` to hooks |
| `desktop/src/hooks/useTerminalSocket.ts` | Modify | Add `isActive` guard to all `terminal.write()` paths |
| `desktop/src/hooks/useLocalTerminal.ts` | Modify | Add `isActive` guard to all `terminal.write()` paths |
| `desktop/src/App.css` | Modify | Fix terminal-host CSS for scroll issues |

---

### Task 1: Remove `agent` mode button from session creation menu

**Files:**
- Modify: `desktop/src/components/TerminalWorkspace.tsx:337`

- [ ] **Step 1: Remove the agent button**

In `desktop/src/components/TerminalWorkspace.tsx`, delete line 337:

```tsx
// DELETE this line:
<button className="terminal-add-menu-item terminal-add-menu-sub" title="Hermes AI agent with tool access and approval flow" onClick={() => { onCreateRemoteSession(host.id, "agent"); setShowAddMenu(false); }}>agent</button>
```

The `rescue_shell` and `project` buttons on lines 336 and 338 remain unchanged.

- [ ] **Step 2: Verify the build compiles**

Run:
```bash
cd desktop && npm run build
```

Expected: Build succeeds. The `onCreateRemoteSession` prop type still accepts `"agent"` — we're only hiding it from the UI, not removing it from the type system.

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/TerminalWorkspace.tsx
git commit -m "fix(desktop): remove agent mode from session creation menu

Agent mode (Hermes AI) is not wired to backend yet. Hiding it
reduces confusion about terminal type differences."
```

---

### Task 2: Fix terminal character spacing

**Files:**
- Modify: `desktop/src/components/TerminalWorkspace.tsx:186-204`

- [ ] **Step 1: Adjust xterm.js Terminal config**

In `desktop/src/components/TerminalWorkspace.tsx`, update the Terminal constructor options (lines 186-204). Change `letterSpacing: 0` to `letterSpacing: -1` and add `lineHeight: 1.0`:

```tsx
const terminal = new Terminal({
  cursorBlink: true,
  convertEol: false,
  fontFamily: 'SFMono-Regular, Consolas, "Liberation Mono", Menlo, monospace',
  fontSize: 14,
  letterSpacing: -1,
  lineHeight: 1.0,
  theme: {
    background: "#1a1f36",
    foreground: "#e2e8f0",
    cursor: "#93c5fd",
    green: "#10b981",
    blue: "#60a5fa",
    yellow: "#fbbf24",
    red: "#f87171",
    cyan: "#22d3ee",
    magenta: "#c084fc",
  },
  scrollback: 5000,
});
```

- [ ] **Step 2: Visual test**

Run the desktop app:
```bash
cd desktop && npm run tauri dev
```

Open a local terminal session. Compare character spacing with a native terminal (e.g., Alacritty, Ghostty, or GNOME Terminal) by running the same command in both (e.g., `ls -la`).

If `letterSpacing: -1` is too tight (characters overlap), change to `letterSpacing: 0` and keep only the `lineHeight: 1.0` addition. The goal is matching the feel of a standard terminal — not pixel-perfect, just "not weirdly spaced."

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/TerminalWorkspace.tsx
git commit -m "fix(desktop): tighten terminal character spacing

Set letterSpacing: -1 and lineHeight: 1.0 on xterm.js to match
the density of standard terminal emulators."
```

---

### Task 3: Fix mouse wheel scrolling in terminal

**Files:**
- Modify: `desktop/src/App.css:608-616`
- Modify: `desktop/src/components/TerminalWorkspace.tsx:186-204` (possibly)

- [ ] **Step 1: Investigate the scroll issue**

Run the desktop app and open a local terminal. Generate enough output to scroll (e.g., `seq 1 500`). Then try scrolling up with the mouse wheel.

Check in browser DevTools (right-click the terminal area → Inspect):
1. Does `.xterm-viewport` have `overflow-y: auto`? (xterm.js sets this internally)
2. Is `.xterm-viewport` the same height as `.xterm-screen`? If they're the same height, xterm thinks there's nothing to scroll.
3. Does `.terminal-host` or `.terminal-main` intercept wheel events?

The likely issue: `.terminal-host` has `padding: 8px 12px` which means the xterm element doesn't fill the full container. Wheel events landing on the padding area don't reach xterm's viewport. Additionally, `.terminal-host .xterm { height: 100% }` may not account for the padding, causing xterm's viewport to miscalculate its scrollable area.

- [ ] **Step 2: Fix the CSS**

In `desktop/src/App.css`, update `.terminal-host` (lines 608-616) to use `box-sizing: border-box` and ensure xterm fills the container correctly:

```css
.terminal-host {
  flex: 1;
  min-height: 0;
  padding: 8px 12px;
  overflow: hidden;
  box-sizing: border-box;
}
.terminal-host .xterm { height: 100%; }
.terminal-host .xterm-viewport,
.terminal-host .xterm-screen { border-radius: 0; }
```

If that alone doesn't fix it, the padding may be the problem. Move the padding to a background visual effect instead:

```css
.terminal-host {
  flex: 1;
  min-height: 0;
  padding: 0;
  overflow: hidden;
}
.terminal-host .xterm {
  height: 100%;
  padding: 8px 12px;
  box-sizing: border-box;
}
.terminal-host .xterm-viewport,
.terminal-host .xterm-screen { border-radius: 0; }
```

- [ ] **Step 3: Visual test**

Run the desktop app again. Open a local terminal. Run `seq 1 500`. Scroll up with the mouse wheel. Verify:
1. Mouse wheel scrolls through terminal output history (you can see line 1)
2. Scrolling down returns to the bottom
3. New output auto-scrolls to the bottom when at the bottom position

- [ ] **Step 4: Commit**

```bash
git add desktop/src/App.css
git commit -m "fix(desktop): enable mouse wheel scrolling in terminal

Ensure xterm.js viewport properly receives wheel events by fixing
container sizing and padding interaction."
```

---

### Task 4: Add `isActive` guard to `useTerminalSocket`

**Files:**
- Modify: `desktop/src/hooks/useTerminalSocket.ts`

- [ ] **Step 1: Add `isActive` to the options type**

In `desktop/src/hooks/useTerminalSocket.ts`, add `isActive` to `UseTerminalSocketOptions` (line 16-23):

```typescript
export type UseTerminalSocketOptions = {
  baseUrl: string;
  sessionId: string | null;
  terminalRef: React.RefObject<Terminal | null>;
  isActive: boolean;
  initialCache?: SessionChunkCache | null;
  onSessionStatusChange?: (session: TerminalSession) => void;
  onError?: (message: string) => void;
};
```

- [ ] **Step 2: Destructure `isActive` and create a ref for it**

Update the function signature destructuring (line 35-42) and add a ref to track it in callbacks:

```typescript
export function useTerminalSocket({
  baseUrl,
  sessionId,
  terminalRef,
  isActive,
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
  const isActiveRef = useRef(isActive);
  const onStatusChangeRef = useRef(onSessionStatusChange);
  const onErrorRef = useRef(onError);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
```

Add a sync effect alongside the existing ref syncs (after line 59):

```typescript
useEffect(() => { isActiveRef.current = isActive; }, [isActive]);
```

- [ ] **Step 3: Guard the buffer flush effect**

Update the buffer flush effect (lines 62-69) to check `isActive` before writing:

```typescript
// Flush buffered chunks when terminal becomes available
useEffect(() => {
  if (!isActive) return;
  const terminal = terminalRef.current;
  if (!terminal || chunkBufferRef.current.length === 0) return;
  for (const data of chunkBufferRef.current) {
    terminal.write(data);
  }
  chunkBufferRef.current = [];
});
```

- [ ] **Step 4: Guard the `terminal_chunk` handler**

In the `ws.onmessage` handler (line 147-157), gate the `terminal.write()` call on `isActiveRef.current`:

```typescript
} else if (data.op === "terminal_chunk") {
  const chunk = data.chunk as TerminalChunk;
  if (chunk.id <= lastChunkIdRef.current) return;
  lastChunkIdRef.current = chunk.id;
  chunkCacheRef.current.push(chunk.chunk);
  if (isActiveRef.current) {
    const term = terminalRef.current;
    if (term) {
      term.write(chunk.chunk);
    } else {
      chunkBufferRef.current.push(chunk.chunk);
    }
  }
}
```

When `isActive` is false, chunks are still added to `chunkCacheRef` (so they're available when switching back) but not written to the terminal or buffered for writing.

- [ ] **Step 5: Guard the cache restore in `connect()`**

In the `connect()` function's fresh-connect path (lines 91-111), gate the terminal writes on `isActiveRef.current`:

```typescript
if (!isReconnect) {
  const terminal = terminalRef.current;
  if (isActiveRef.current && terminal) terminal.reset();

  const cache = initialCacheRef.current;
  if (cache && cache.chunks.length > 0) {
    chunkCacheRef.current = [...cache.chunks];
    lastChunkIdRef.current = cache.lastChunkId;
    if (isActiveRef.current && terminal) {
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
```

- [ ] **Step 6: Verify the build compiles**

Run:
```bash
cd desktop && npm run build
```

Expected: Build fails because `TerminalWorkspace.tsx` doesn't pass `isActive` yet. That's expected — it'll be wired in Task 6.

- [ ] **Step 7: Commit**

```bash
git add desktop/src/hooks/useTerminalSocket.ts
git commit -m "fix(desktop): add isActive guard to useTerminalSocket

Gate all terminal.write() calls on isActive flag to prevent
chunks from one session bleeding into another session's display.
Chunks are still cached when inactive for replay on reactivation."
```

---

### Task 5: Add `isActive` guard to `useLocalTerminal`

**Files:**
- Modify: `desktop/src/hooks/useLocalTerminal.ts`

- [ ] **Step 1: Add `isActive` to the options type**

In `desktop/src/hooks/useLocalTerminal.ts`, add `isActive` to `UseLocalTerminalOptions` (lines 10-15):

```typescript
export type UseLocalTerminalOptions = {
  sessionId: string | null;
  terminalRef: React.RefObject<Terminal | null>;
  isActive: boolean;
  onSessionStatusChange?: (session: LocalTerminalSession) => void;
  onError?: (message: string) => void;
};
```

- [ ] **Step 2: Destructure `isActive` and create a ref for it**

Update the function signature (line 36-41) and add the ref:

```typescript
export function useLocalTerminal({
  sessionId,
  terminalRef,
  isActive,
  onSessionStatusChange,
  onError,
}: UseLocalTerminalOptions): UseLocalTerminalReturn {
  const sessionIdRef = useRef(sessionId);
  const onStatusChangeRef = useRef(onSessionStatusChange);
  const onErrorRef = useRef(onError);
  const isActiveRef = useRef(isActive);
  const chunkBufferRef = useRef<string[]>([]);
```

Add sync effect alongside existing ref syncs (after line 52):

```typescript
useEffect(() => { isActiveRef.current = isActive; }, [isActive]);
```

- [ ] **Step 3: Guard the buffer flush effect**

Update the buffer flush effect (lines 55-62) to check `isActive`:

```typescript
// Flush buffered chunks when terminal becomes available
useEffect(() => {
  if (!isActive) return;
  const terminal = terminalRef.current;
  if (!terminal || chunkBufferRef.current.length === 0) return;
  for (const data of chunkBufferRef.current) {
    terminal.write(data);
  }
  chunkBufferRef.current = [];
}, [terminalRef, isActive]);
```

- [ ] **Step 4: Guard the `pty:chunk` event listener**

In the Tauri event listener (lines 91-98), gate the `terminal.write()` call:

```typescript
const chunkUnlisten = listen<PtyChunkPayload>("pty:chunk", (event) => {
  if (cancelled || event.payload.session_id !== currentSessionId) return;
  if (isActiveRef.current) {
    const term = terminalRef.current;
    if (term) {
      term.write(event.payload.data);
    } else {
      chunkBufferRef.current.push(event.payload.data);
    }
  }
});
```

- [ ] **Step 5: Guard the terminal reset on fresh session**

In the session lifecycle effect (lines 77-80), gate the terminal reset:

```typescript
if (isActiveRef.current) {
  const terminal = terminalRef.current;
  if (terminal) terminal.reset();
}
chunkBufferRef.current = [];
```

- [ ] **Step 6: Verify the build compiles**

Run:
```bash
cd desktop && npm run build
```

Expected: Build fails because `TerminalWorkspace.tsx` doesn't pass `isActive` yet. Expected — wired in Task 6.

- [ ] **Step 7: Commit**

```bash
git add desktop/src/hooks/useLocalTerminal.ts
git commit -m "fix(desktop): add isActive guard to useLocalTerminal

Gate all terminal.write() calls on isActive flag to prevent
local PTY output from writing to terminal when a remote session
is displayed."
```

---

### Task 6: Wire `isActive` into TerminalWorkspace and final integration test

**Files:**
- Modify: `desktop/src/components/TerminalWorkspace.tsx:127-153`

- [ ] **Step 1: Pass `isActive` to `useTerminalSocket`**

In `desktop/src/components/TerminalWorkspace.tsx`, update the `useTerminalSocket` call (lines 127-140) to include `isActive`:

```tsx
const {
  sendInput: remoteSendInput,
  resize: remoteResize,
  terminate,
  sessionMeta: remoteSessionMeta,
  getChunkCache,
} = useTerminalSocket({
  baseUrl: activeBaseUrl,
  sessionId: remoteSessionId,
  terminalRef,
  isActive: !isLocalSession,
  initialCache,
  onSessionStatusChange: onRemoteSessionStatusChange,
  onError: setError,
});
```

- [ ] **Step 2: Pass `isActive` to `useLocalTerminal`**

Update the `useLocalTerminal` call (lines 143-153) to include `isActive`:

```tsx
const {
  sendInput: localSendInput,
  resize: localResize,
  kill: localKill,
  sessionMeta: localSessionMeta,
} = useLocalTerminal({
  sessionId: isLocalSession ? activeSessionId : null,
  terminalRef,
  isActive: isLocalSession,
  onSessionStatusChange: onLocalSessionStatusChange,
  onError: setError,
});
```

- [ ] **Step 3: Verify the build compiles**

Run:
```bash
cd desktop && npm run build
```

Expected: Build succeeds with no type errors.

- [ ] **Step 4: Integration test — session bleeding**

Run the desktop app:
```bash
cd desktop && npm run tauri dev
```

Test procedure:
1. Connect to a remote host
2. Open two remote sessions (e.g., two `project` shells)
3. In session A, run: `while true; do echo "SESSION-A $(date)"; sleep 0.5; done`
4. Switch to session B's tab
5. In session B, run: `while true; do echo "SESSION-B $(date)"; sleep 0.5; done`
6. Rapidly switch between tabs A and B for 10-15 seconds

Verify: Session A's tab only shows "SESSION-A" lines. Session B's tab only shows "SESSION-B" lines. No text from one session appears in the other.

7. Also test local ↔ remote switching: open a local shell and a remote session, run continuous output in both, switch between them rapidly.

- [ ] **Step 5: Commit**

```bash
git add desktop/src/components/TerminalWorkspace.tsx
git commit -m "fix(desktop): wire isActive guards to prevent session bleeding

Pass isActive boolean to both useTerminalSocket and useLocalTerminal
hooks based on isLocalSession state. Only the currently-displayed
session type can write to the shared xterm.js Terminal instance."
```

---

## Task Dependency Order

```
Task 1 (remove agent) ─────────────────── can run independently
Task 2 (character spacing) ────────────── can run independently
Task 3 (scroll fix) ──────────────────── can run independently
Task 4 (guard useTerminalSocket) ──┐
Task 5 (guard useLocalTerminal) ───┤── must complete before Task 6
Task 6 (wire isActive + test) ────────── depends on Tasks 4 & 5
```

Tasks 1, 2, and 3 are independent of each other and of Tasks 4-6.

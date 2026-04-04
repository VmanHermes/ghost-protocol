# Phase 1: Local Terminal (Tauri PTY) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a local terminal capability to Ghost Protocol so the app is usable immediately on first launch without any daemon connection — spawning shell sessions directly via Tauri's Rust backend.

**Architecture:** The Rust backend spawns PTY processes and streams I/O to the frontend via Tauri events and commands. A new `useLocalTerminal` hook mirrors the `useTerminalSocket` interface shape so `TerminalWorkspace` can consume both local and remote sessions uniformly through a `TerminalSource` abstraction.

**Tech Stack:** Rust (portable-pty crate for PTY), Tauri 2 commands/events, React 19, TypeScript, xterm.js v6

---

## File Structure

### Rust (new files)

| File | Responsibility |
|---|---|
| `desktop/src-tauri/src/pty.rs` | PTY session spawning, I/O multiplexing, resize, kill. All PTY logic lives here. |
| `desktop/src-tauri/src/lib.rs` | Modified — registers PTY commands and event setup |

### Frontend (new files)

| File | Responsibility |
|---|---|
| `desktop/src/hooks/useLocalTerminal.ts` | Hook that connects xterm.js to Tauri PTY commands/events. Same return shape as `useTerminalSocket`. |

### Frontend (modified files)

| File | Change |
|---|---|
| `desktop/src/types.ts` | Add `TerminalSource` type, `LocalTerminalSession` type |
| `desktop/src/components/TerminalWorkspace.tsx` | Support both local and remote sessions via source abstraction |
| `desktop/src/App.tsx` | Manage local sessions alongside remote sessions |

### Config (modified files)

| File | Change |
|---|---|
| `desktop/src-tauri/Cargo.toml` | Add `portable-pty`, `uuid`, `tokio` dependencies |
| `desktop/src-tauri/capabilities/default.json` | Add shell execution permissions |

---

## Task 1: Add Rust PTY Dependencies

**Files:**
- Modify: `desktop/src-tauri/Cargo.toml`
- Modify: `desktop/src-tauri/capabilities/default.json`

- [ ] **Step 1: Add PTY and async dependencies to Cargo.toml**

Add under `[dependencies]`:

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-opener = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
portable-pty = "0.8"
uuid = { version = "1", features = ["v4"] }
tokio = { version = "1", features = ["sync", "rt"] }
```

- [ ] **Step 2: Add shell execution permission to capabilities**

Update `desktop/src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default",
    "core:event:default"
  ]
}
```

- [ ] **Step 3: Verify it compiles**

Run:
```bash
cd desktop && npm run tauri build -- --debug 2>&1 | tail -5
```

If `portable-pty` has issues on the current platform, fall back to the `nix` + `libc` crates with raw PTY via `forkpty`. But `portable-pty` should work on Linux and macOS.

- [ ] **Step 4: Commit**

```bash
git add desktop/src-tauri/Cargo.toml desktop/src-tauri/capabilities/default.json
git commit -m "chore: add portable-pty and tokio dependencies for local terminal"
```

---

## Task 2: Implement Rust PTY Module

**Files:**
- Create: `desktop/src-tauri/src/pty.rs`

This is the core module. It manages multiple PTY sessions identified by UUID strings, streams output via Tauri events, and accepts input via Tauri commands.

- [ ] **Step 1: Create the PTY session manager**

Create `desktop/src-tauri/src/pty.rs`:

```rust
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

/// Output chunk sent to the frontend via Tauri event.
#[derive(Clone, Serialize)]
pub struct PtyChunk {
    pub session_id: String,
    pub data: String,
}

/// Status change sent to the frontend via Tauri event.
#[derive(Clone, Serialize)]
pub struct PtyStatus {
    pub session_id: String,
    pub status: String, // "running" | "exited"
    pub exit_code: Option<i32>,
}

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Spawn a new PTY session. Returns the session ID.
    /// Starts a reader thread that emits `pty:chunk` events and
    /// a waiter thread that emits `pty:status` on exit.
    pub fn spawn(
        &self,
        app: &AppHandle,
        cols: u16,
        rows: u16,
        workdir: Option<String>,
    ) -> Result<String, String> {
        let session_id = Uuid::new_v4().to_string();
        let pty_system = native_pty_system();

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system
            .openpty(size)
            .map_err(|e| format!("Failed to open PTY: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l"); // login shell
        if let Some(ref dir) = workdir {
            cmd.cwd(dir);
        } else if let Ok(home) = std::env::var("HOME") {
            cmd.cwd(home);
        }
        // Pass through environment
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("GHOST_PROTOCOL_LOCAL", "1");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn shell: {e}"))?;

        // Get a writer handle for sending input
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to take PTY writer: {e}"))?;

        // Get a reader handle for the output thread
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY reader: {e}"))?;

        let session = PtySession {
            master: pair.master,
            writer,
            child,
        };

        self.sessions
            .lock()
            .map_err(|e| format!("Lock poisoned: {e}"))?
            .insert(session_id.clone(), session);

        // Reader thread — streams PTY output to frontend
        let reader_id = session_id.clone();
        let reader_app = app.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = reader_app.emit("pty:chunk", PtyChunk {
                            session_id: reader_id.clone(),
                            data: text,
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        // Waiter thread — notifies frontend when process exits
        let waiter_id = session_id.clone();
        let waiter_app = app.clone();
        let sessions_ref = Arc::new(Mutex::new(())); // just for the event
        thread::spawn(move || {
            // portable-pty Child doesn't have a blocking wait on all platforms,
            // so we poll with a short sleep
            let exit_code = loop {
                // Try to get the exit status
                // We need to access the child through the manager, but since we moved it,
                // we'll just wait for the reader to finish (EOF on PTY = process exited)
                thread::sleep(std::time::Duration::from_millis(200));
                // The reader thread will exit when the process dies (read returns 0/error).
                // We can't easily get exit code from here without the child handle.
                // So we do a simpler approach: just emit status after reader dies.
                break None::<i32>;
            };
            drop(sessions_ref);
            let _ = waiter_app.emit("pty:status", PtyStatus {
                session_id: waiter_id,
                status: "exited".to_string(),
                exit_code,
            });
        });

        Ok(session_id)
    }

    /// Write input data to a PTY session.
    pub fn write_input(&self, session_id: &str, data: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| format!("Lock poisoned: {e}"))?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {session_id} not found"))?;
        session
            .writer
            .write_all(data.as_bytes())
            .map_err(|e| format!("Write failed: {e}"))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("Flush failed: {e}"))?;
        Ok(())
    }

    /// Resize a PTY session.
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock().map_err(|e| format!("Lock poisoned: {e}"))?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("Session {session_id} not found"))?;
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Resize failed: {e}"))?;
        Ok(())
    }

    /// Kill a PTY session and clean up.
    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| format!("Lock poisoned: {e}"))?;
        if let Some(mut session) = sessions.remove(session_id) {
            let _ = session.child.kill();
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cd desktop && cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -10
```

Expected: compiles with possible warnings. Fix any errors before proceeding.

- [ ] **Step 3: Commit**

```bash
git add desktop/src-tauri/src/pty.rs
git commit -m "feat: add Rust PTY session manager module"
```

---

## Task 3: Register Tauri Commands and Wire Up PTY Manager

**Files:**
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add Tauri commands and register the PTY manager as app state**

Replace `desktop/src-tauri/src/lib.rs` with:

```rust
mod pty;

use pty::PtyManager;
use tauri::State;

#[tauri::command]
fn pty_spawn(
    app: tauri::AppHandle,
    state: State<'_, PtyManager>,
    cols: u16,
    rows: u16,
    workdir: Option<String>,
) -> Result<String, String> {
    state.spawn(&app, cols, rows, workdir)
}

#[tauri::command]
fn pty_write(
    state: State<'_, PtyManager>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    state.write_input(&session_id, &data)
}

#[tauri::command]
fn pty_resize(
    state: State<'_, PtyManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.resize(&session_id, cols, rows)
}

#[tauri::command]
fn pty_kill(
    state: State<'_, PtyManager>,
    session_id: String,
) -> Result<(), String> {
    state.kill(&session_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(PtyManager::new())
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cd desktop && cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -10
```

Expected: clean compile.

- [ ] **Step 3: Commit**

```bash
git add desktop/src-tauri/src/lib.rs
git commit -m "feat: register PTY Tauri commands (spawn, write, resize, kill)"
```

---

## Task 4: Fix PTY Exit Detection

**Files:**
- Modify: `desktop/src-tauri/src/pty.rs`

The waiter thread in Task 2 is a placeholder — it can't access the child handle because it was moved into the session map. Fix this by keeping the child handle accessible for waiting.

- [ ] **Step 1: Refactor to move child waiting into the spawn method**

Replace the waiter thread section in `spawn()` (everything after the reader thread spawn) with:

```rust
        // Waiter thread — polls child exit status and emits pty:status
        let waiter_id = session_id.clone();
        let waiter_app = app.clone();
        let waiter_sessions = Arc::clone(&self.sessions_arc);
        thread::spawn(move || {
            // Wait for the reader thread to finish (PTY EOF = shell exited).
            // The reader thread exits when read() returns 0 or errors.
            // We poll the child status through the session map.
            loop {
                thread::sleep(std::time::Duration::from_millis(500));
                let mut sessions = match waiter_sessions.lock() {
                    Ok(s) => s,
                    Err(_) => break,
                };
                if let Some(session) = sessions.get_mut(&waiter_id) {
                    if let Ok(Some(status)) = session.child.try_wait() {
                        let code = status.exit_code() as i32;
                        sessions.remove(&waiter_id);
                        drop(sessions);
                        let _ = waiter_app.emit("pty:status", PtyStatus {
                            session_id: waiter_id,
                            status: "exited".to_string(),
                            exit_code: Some(code),
                        });
                        return;
                    }
                } else {
                    // Session was already removed (killed)
                    let _ = waiter_app.emit("pty:status", PtyStatus {
                        session_id: waiter_id,
                        status: "terminated".to_string(),
                        exit_code: None,
                    });
                    return;
                }
            }
        });
```

This requires changing the `PtyManager` struct to use `Arc<Mutex<>>` so the waiter thread can access sessions. Update the struct:

```rust
pub struct PtyManager {
    sessions_arc: Arc<Mutex<HashMap<String, PtySession>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions_arc: Arc::new(Mutex::new(HashMap::new())),
        }
    }
```

And update all methods that access `self.sessions` to use `self.sessions_arc` instead.

- [ ] **Step 2: Verify it compiles**

```bash
cd desktop && cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tail -10
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src-tauri/src/pty.rs
git commit -m "fix: proper PTY child exit detection via polling"
```

---

## Task 5: Add Frontend Types for Local Sessions

**Files:**
- Modify: `desktop/src/types.ts`

- [ ] **Step 1: Add local terminal and source types**

Add at the end of `desktop/src/types.ts`:

```typescript
// --- Local terminal types (Tauri PTY) ---

export type LocalTerminalSession = {
  id: string;
  status: "running" | "exited" | "terminated";
  createdAt: string;
  exitCode?: number | null;
};

export type TerminalSource =
  | { type: "local"; sessionId: string }
  | { type: "remote"; hostId: string; sessionId: string };

// Unified tab entry for the terminal workspace
export type TerminalTab = {
  source: TerminalSource;
  label: string;       // e.g. "Local · shell" or "Desktop · rescue shell"
  status: "running" | "exited" | "terminated" | "error" | "created";
};
```

- [ ] **Step 2: Verify types compile**

```bash
cd desktop && npx tsc --noEmit 2>&1 | head -20
```

Expected: no errors (new types are additive).

- [ ] **Step 3: Commit**

```bash
git add desktop/src/types.ts
git commit -m "feat: add LocalTerminalSession and TerminalSource types"
```

---

## Task 6: Implement useLocalTerminal Hook

**Files:**
- Create: `desktop/src/hooks/useLocalTerminal.ts`

This hook mirrors `useTerminalSocket`'s return shape but uses Tauri commands/events instead of WebSocket.

- [ ] **Step 1: Create the hook**

Create `desktop/src/hooks/useLocalTerminal.ts`:

```typescript
import { useCallback, useEffect, useRef, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { appLog } from "../log";
import type { LocalTerminalSession } from "../types";

const SRC = "local-pty";

type PtyChunk = { session_id: string; data: string };
type PtyStatus = { session_id: string; status: string; exit_code: number | null };

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

export function useLocalTerminal({
  sessionId,
  terminalRef,
  onSessionStatusChange,
  onError,
}: UseLocalTerminalOptions): UseLocalTerminalReturn {
  const sessionIdRef = useRef(sessionId);
  const onStatusChangeRef = useRef(onSessionStatusChange);
  const onErrorRef = useRef(onError);
  const [sessionMeta, setSessionMeta] = useState<LocalTerminalSession | null>(null);
  const [isConnected, setIsConnected] = useState(false);

  useEffect(() => { sessionIdRef.current = sessionId; }, [sessionId]);
  useEffect(() => { onStatusChangeRef.current = onSessionStatusChange; }, [onSessionStatusChange]);
  useEffect(() => { onErrorRef.current = onError; }, [onError]);

  // Listen for PTY output and status events
  useEffect(() => {
    if (!sessionId) {
      setSessionMeta(null);
      setIsConnected(false);
      return;
    }

    const currentSessionId = sessionId;
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    setSessionMeta({
      id: currentSessionId,
      status: "running",
      createdAt: new Date().toISOString(),
    });
    setIsConnected(true);

    // Listen for output chunks
    listen<PtyChunk>("pty:chunk", (event) => {
      if (cancelled || event.payload.session_id !== currentSessionId) return;
      const term = terminalRef.current;
      if (term) {
        term.write(event.payload.data);
      }
    }).then((unlisten) => {
      if (cancelled) { unlisten(); return; }
      unlisteners.push(unlisten);
    });

    // Listen for status changes (exit)
    listen<PtyStatus>("pty:status", (event) => {
      if (cancelled || event.payload.session_id !== currentSessionId) return;
      const status = event.payload.status as "exited" | "terminated";
      const updated: LocalTerminalSession = {
        id: currentSessionId,
        status,
        createdAt: sessionMeta?.createdAt ?? new Date().toISOString(),
        exitCode: event.payload.exit_code,
      };
      appLog.info(SRC, `Session ${currentSessionId.slice(0, 8)} ${status} (code=${event.payload.exit_code})`);
      setSessionMeta(updated);
      setIsConnected(false);
      onStatusChangeRef.current?.(updated);
    }).then((unlisten) => {
      if (cancelled) { unlisten(); return; }
      unlisteners.push(unlisten);
    });

    appLog.info(SRC, `Attached to local session ${currentSessionId.slice(0, 8)}`);

    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) {
        unlisten();
      }
      setIsConnected(false);
    };
  }, [sessionId, terminalRef]);

  const sendInput = useCallback((data: string) => {
    const id = sessionIdRef.current;
    if (!id) return;
    invoke("pty_write", { sessionId: id, data }).catch((err) => {
      appLog.error(SRC, `Write failed: ${err}`);
      onErrorRef.current?.(String(err));
    });
  }, []);

  const resize = useCallback((cols: number, rows: number) => {
    const id = sessionIdRef.current;
    if (!id) return;
    invoke("pty_resize", { sessionId: id, cols, rows }).catch((err) => {
      appLog.error(SRC, `Resize failed: ${err}`);
    });
  }, []);

  const kill = useCallback(() => {
    const id = sessionIdRef.current;
    if (!id) return;
    invoke("pty_kill", { sessionId: id }).catch((err) => {
      appLog.error(SRC, `Kill failed: ${err}`);
      onErrorRef.current?.(String(err));
    });
  }, []);

  return { sendInput, resize, kill, sessionMeta, isConnected };
}
```

- [ ] **Step 2: Verify types compile**

```bash
cd desktop && npx tsc --noEmit 2>&1 | head -20
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add desktop/src/hooks/useLocalTerminal.ts
git commit -m "feat: add useLocalTerminal hook for Tauri PTY sessions"
```

---

## Task 7: Add Local Session Management to App.tsx

**Files:**
- Modify: `desktop/src/App.tsx`

Add state and handlers for local terminal sessions alongside the existing remote session management.

- [ ] **Step 1: Add local session state and handlers**

Add these imports at the top of `App.tsx`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import type { LocalTerminalSession } from "./types";
```

Add state after the existing `activeTerminalSessionId` state:

```typescript
  const [localSessions, setLocalSessions] = useState<LocalTerminalSession[]>([]);
```

Add handler functions after the existing terminal handlers:

```typescript
  const handleCreateLocalSession = useCallback(async () => {
    try {
      const terminal = terminalRef?.current;
      const cols = terminal?.cols ?? 120;
      const rows = terminal?.rows ?? 32;
      const sessionId = await invoke<string>("pty_spawn", { cols, rows, workdir: null });
      const session: LocalTerminalSession = {
        id: sessionId,
        status: "running",
        createdAt: new Date().toISOString(),
      };
      setLocalSessions((prev) => [...prev, session]);
      setActiveTerminalSessionId(sessionId);
      setMainView("terminal");
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    }
  }, []);

  const handleLocalSessionStatusChange = useCallback((session: LocalTerminalSession) => {
    setLocalSessions((prev) =>
      prev.map((s) => (s.id === session.id ? session : s)),
    );
  }, []);

  const handleKillLocalSession = useCallback(async (sessionId: string) => {
    try {
      await invoke("pty_kill", { sessionId });
      setLocalSessions((prev) =>
        prev.map((s) => (s.id === sessionId ? { ...s, status: "terminated" as const } : s)),
      );
      if (activeTerminalSessionId === sessionId) {
        const remaining = localSessions.filter((s) => s.id !== sessionId && s.status === "running");
        setActiveTerminalSessionId(remaining[0]?.id ?? null);
      }
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    }
  }, [activeTerminalSessionId, localSessions]);
```

- [ ] **Step 2: Pass local session props to TerminalWorkspace**

Update the `TerminalWorkspace` usage in the JSX to include local session props:

```tsx
          <TerminalWorkspace
            baseUrl={baseUrl}
            sessions={terminalSessions}
            localSessions={localSessions}
            activeSessionId={activeTerminalSessionId}
            visible={mainView === "terminal"}
            onSelect={setActiveTerminalSessionId}
            onCreateSession={(mode) => void handleCreateTerminalSession(mode)}
            onCreateLocalSession={() => void handleCreateLocalSession()}
            onSessionStatusChange={handleTerminalSessionStatusChange}
            onLocalSessionStatusChange={handleLocalSessionStatusChange}
            onRefreshSessions={() => void refreshTerminalSessions()}
            onKillSession={(id) => void handleKillTerminalSession(id)}
            onKillLocalSession={(id) => void handleKillLocalSession(id)}
          />
```

Also, auto-spawn a local session on app launch so the terminal is immediately usable. Add an effect after the existing `useEffect(() => { void initialize(baseUrl); }, []);`:

```typescript
  // Auto-spawn a local terminal on first launch
  useEffect(() => {
    void handleCreateLocalSession();
  }, []);
```

Note: `App.tsx` will have type errors until Task 8 updates `TerminalWorkspace`. That's expected.

- [ ] **Step 3: Commit**

```bash
git add desktop/src/App.tsx
git commit -m "feat: add local session state and handlers to App"
```

---

## Task 8: Update TerminalWorkspace to Support Both Session Types

**Files:**
- Modify: `desktop/src/components/TerminalWorkspace.tsx`

This is the largest frontend change. The workspace needs to:
1. Accept local sessions as a prop
2. Detect whether the active session is local or remote
3. Use the right hook (`useLocalTerminal` vs `useTerminalSocket`)
4. Label tabs with their source
5. Update the "+" button to offer local and remote options

- [ ] **Step 1: Update Props and imports**

Add imports:

```typescript
import { useLocalTerminal } from "../hooks/useLocalTerminal";
import type { LocalTerminalSession } from "../types";
```

Update `Props`:

```typescript
type Props = {
  baseUrl: string;
  sessions: TerminalSession[];
  localSessions: LocalTerminalSession[];
  activeSessionId: string | null;
  visible: boolean;
  onSelect: (sessionId: string) => void;
  onCreateSession: (mode: "agent" | "rescue_shell") => void;
  onCreateLocalSession: () => void;
  onSessionStatusChange: (session: TerminalSession) => void;
  onLocalSessionStatusChange: (session: LocalTerminalSession) => void;
  onRefreshSessions: () => void;
  onKillSession: (sessionId: string) => void;
  onKillLocalSession: (sessionId: string) => void;
};
```

- [ ] **Step 2: Add session source detection and dual hooks**

Inside the component, determine if the active session is local or remote:

```typescript
  const isLocalSession = useMemo(
    () => localSessions.some((s) => s.id === activeSessionId),
    [localSessions, activeSessionId],
  );

  const activeLocalSessions = useMemo(
    () => localSessions.filter((s) => s.status === "running"),
    [localSessions],
  );
```

Use both hooks, but only the relevant one will be active (the other gets `sessionId: null`):

```typescript
  const {
    sendInput: remoteSendInput,
    resize: remoteResize,
    terminate,
    sessionMeta: remoteSessionMeta,
  } = useTerminalSocket({
    baseUrl,
    sessionId: isLocalSession ? null : activeSessionId,
    terminalRef,
    onSessionStatusChange,
    onError: setError,
  });

  const {
    sendInput: localSendInput,
    resize: localResize,
    kill: localKill,
    sessionMeta: localSessionMeta,
  } = useLocalTerminal({
    sessionId: isLocalSession ? activeSessionId : null,
    terminalRef,
    onSessionStatusChange: onLocalSessionStatusChange,
    onError: setError,
  });

  // Unified interface
  const sendInput = isLocalSession ? localSendInput : remoteSendInput;
  const resize = isLocalSession ? localResize : remoteResize;
  const sessionMeta = isLocalSession ? localSessionMeta : remoteSessionMeta;
```

- [ ] **Step 3: Update the terminal.onData binding**

The xterm `onData` handler needs to use the current `sendInput`. Since the terminal is created once but `sendInput` changes based on active session type, use a ref:

```typescript
  const sendInputRef = useRef(sendInput);
  useEffect(() => { sendInputRef.current = sendInput; }, [sendInput]);
```

And in the terminal init effect, change:

```typescript
  terminal.onData((data) => sendInputRef.current(data, false));
```

to:

```typescript
  terminal.onData((data) => sendInputRef.current(data));
```

Note: `localSendInput` only takes one argument (no `appendNewline`). Update the remote version's call to match. The simplest fix: the `onData` handler calls `sendInputRef.current(data)` and the remote `sendInput` defaults `appendNewline` to `false` (which it already does in `useTerminalSocket`).

- [ ] **Step 4: Update tab rendering with source labels**

Replace the active sessions tab rendering to include both local and remote sessions:

```typescript
      {/* Session tabs — active only */}
      <div className="terminal-tabs">
        {/* Local sessions */}
        {activeLocalSessions.map((session) => (
          <button
            key={session.id}
            className={`terminal-tab ${session.id === activeSessionId ? "active" : ""}`}
            onClick={() => onSelect(session.id)}
          >
            <span className="terminal-tab-dot" style={{ background: "#10b981" }} />
            <span className="terminal-tab-source">Local</span>
            {" · shell"}
            <span
              className="terminal-tab-close"
              onClick={(e) => { e.stopPropagation(); onKillLocalSession(session.id); }}
              title="Kill session"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </span>
          </button>
        ))}

        {/* Remote sessions */}
        {activeSessions.map((session) => (
          <button
            key={session.id}
            className={`terminal-tab ${session.id === activeSessionId ? "active" : ""}`}
            onClick={() => onSelect(session.id)}
          >
            <span
              className="terminal-tab-dot"
              style={{ background: SESSION_DOT_COLORS[session.status] ?? "#6b7280" }}
            />
            <span className="terminal-tab-source">Remote</span>
            {" · "}{session.name || session.mode.replace("_", " ")}
            <span
              className="terminal-tab-close"
              onClick={(e) => handleCloseTab(e, session.id)}
              title="Kill session"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </span>
          </button>
        ))}

        {/* Add session button */}
        <button className="terminal-tab-add" onClick={onCreateLocalSession} title="New local shell">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </button>
      </div>
```

- [ ] **Step 5: Update status bar and kill button**

The stop button at the bottom should call the right kill function:

```typescript
        <div className="terminal-statusbar-actions">
          {sessionMeta && sessionMeta.status === "running" && activeSessionId && (
            <button
              className="terminal-stop-btn"
              onClick={isLocalSession ? localKill : terminate}
              title="Terminate session"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
              </svg>
            </button>
          )}
        </div>
```

- [ ] **Step 6: Update the status bar info to show source**

```typescript
        <div className="terminal-statusbar-info">
          <span>{isLocalSession ? "Local" : "Remote"} · {activeSession?.name || activeSession?.mode || localSessionMeta?.status || "none"}</span>
          <span className="terminal-statusbar-sep" />
          <span>Uptime: {formatUptime(activeSession?.startedAt || localSessionMeta?.createdAt)}</span>
        </div>
```

- [ ] **Step 7: Verify everything compiles**

```bash
cd desktop && npx tsc --noEmit 2>&1 | head -20
```

Fix any type errors. Common issues:
- `sendInput` signature mismatch (local takes 1 arg, remote takes 2). Fix by wrapping: `const sendInput = isLocalSession ? (data: string) => localSendInput(data) : (data: string) => remoteSendInput(data, false);`
- Missing props on `TerminalWorkspace` — check that `App.tsx` passes all new props.

- [ ] **Step 8: Commit**

```bash
git add desktop/src/components/TerminalWorkspace.tsx
git commit -m "feat: TerminalWorkspace supports both local and remote sessions"
```

---

## Task 9: Add CSS for Source Labels

**Files:**
- Modify: `desktop/src/App.css`

- [ ] **Step 1: Add source label styling**

Add to `desktop/src/App.css`:

```css
.terminal-tab-source {
  color: var(--text-tertiary);
  font-size: 11px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}
```

- [ ] **Step 2: Commit**

```bash
git add desktop/src/App.css
git commit -m "style: add terminal tab source label styling"
```

---

## Task 10: End-to-End Smoke Test

**Files:** None (manual testing)

- [ ] **Step 1: Build and launch the app**

```bash
cd desktop && npm run tauri dev
```

- [ ] **Step 2: Verify local terminal spawns on "+" click**

1. Click the "+" button in the terminal tab bar
2. A new tab labeled "Local · shell" should appear
3. A shell prompt should be visible in the terminal area
4. Type `echo hello` — should echo back immediately with no lag

- [ ] **Step 3: Verify terminal input/output**

1. Run `ls -la` — should show directory listing
2. Run `pwd` — should show home directory
3. Try Ctrl+C — should interrupt
4. Try tab completion — should work

- [ ] **Step 4: Verify resize**

1. Resize the window — terminal should reflow
2. Run `tput cols && tput lines` — should match visible area

- [ ] **Step 5: Verify session kill**

1. Click the "x" on a local tab — session should terminate
2. Tab should disappear from active tabs
3. Create another local session — should work fine

- [ ] **Step 6: Verify remote sessions still work**

1. Start the daemon: `cd backend && PYTHONPATH=src .venv/bin/python -m ghost_protocol_daemon`
2. Create a remote session via "Run Agent" or the "+" menu
3. Verify remote session shows "Remote · rescue shell" label
4. Verify input/output works on the remote session
5. Switch between local and remote tabs — no garbled output

- [ ] **Step 7: Commit any fixes from testing**

```bash
git add -A && git commit -m "fix: smoke test corrections for local terminal"
```

---

## Summary of Deliverables

After completing all tasks, the app will:

1. **Launch instantly** with a working local terminal — no daemon needed
2. **Spawn local shells** via Tauri's Rust PTY backend with zero network overhead
3. **Label all tabs** with their source (Local vs Remote)
4. **Support both local and remote sessions** simultaneously in the same workspace
5. **Kill local sessions** cleanly via the tab close button or stop button

This lays the foundation for Phase 2 (multi-host connections) and Phase 3 (onboarding with setup checklist in the local terminal).

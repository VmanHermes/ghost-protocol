# Unified Agents Page — Design Spec

## Overview

Merge the separate Chat and Terminal views into a single **Agents** page. One page to manage all agent sessions across the mesh — start new sessions, switch between chat and terminal modes, monitor session metadata, and view delegated sub-sessions in a tree.

## Terminology

- **Daemon** — one per machine, manages all processes and API on that host
- **Agent** — a detected capability/binary that can be spawned (Claude Code, Hermes, Ollama model, etc.)
- **Session** — a running instance of an agent. Has its own PID, workdir, stdout, chat history. One agent can have many sessions.

## Page Layout

The Agents page has three zones within the existing app shell (sidebar nav + main content area):

### Top bar

Horizontal strip at the top of the Agents page content area:

- **Agent picker** — dropdown listing all detected agents from the daemon (`GET /api/agents`). Shows agent name, version (if available), and type (cli/api).
- **Mode selector** — segmented toggle: Chat | Terminal. Determines how the new session is created.
- **"+ New Session" button** — creates a session with the selected agent + mode.
- When no agents are detected, the dropdown is disabled and a "+ Set up an agent" link appears, opening a terminal with `ghost init` pre-typed.

### Session sidebar (left panel)

Resizable panel with a drag handle. Has a minimum width (e.g. 200px) but no fixed pixel width — user can resize the boundary between sidebar and main area.

Split into two sections with a visual separator:

**Active Sessions:**
- Section header: "ACTIVE SESSIONS" (uppercase label, muted)
- Each session is a card with:
  - Status dot: green (running), amber (needs-approval), red (error)
  - Border color matches status (uniform card background, border changes)
  - Agent name + type badge (cli/api)
  - Host machine name (e.g. "omarchy", "localhost")
  - Workdir (truncated)
  - Last output preview (monospace, single line, ellipsis overflow). Source depends on mode: chat mode shows last `chat_messages` content, terminal mode shows last `terminal_chunks` text.
  - Approval badge when applicable: amber background, "⚠ Approval needed"
- Clicking a card selects it and loads it into the main area
- Delegated (child) sessions render indented under their parent with a tree connector line

**Previous Sessions:**
- Section header: "PREVIOUS SESSIONS" (uppercase label, muted)
- Same card structure but dimmed (reduced opacity)
- Shows relative timestamp ("2h ago", "yesterday")
- Gray status dot, gray border
- Clicking loads the session's history (read-only, messages/chunks from DB)

**Empty state:**
- Dashed border card: "No agents detected" + "+ Set up an agent" link

### Main content area (right)

Takes remaining width. Contains:

**Session header bar:**
- Status dot + agent name + workdir + host
- **Chat / Terminal toggle** — segmented control for mode switching on the active session
- Session metadata row (only populated fields shown):
  - Duration: live timer ("12m 34s"), updated every second in the frontend
  - Tokens: cumulative for the session ("2.4k tokens"), updated on each message completion
  - Context %: progress bar + percentage, updated on each message. Yellow warning badge at >80%: "Context filling"
  - These fields are agent-dependent — hidden entirely when agent doesn't expose them
- "Open IDE" button — no-op for now, grayed with tooltip "code-server coming soon"
- "End Session" button
- Delegated session badge: "Delegated from: {agent} on {host}" when `parent_session_id` is set

**Content area:**
- **Chat mode** → ChatRenderer: message bubbles, tool call indicators, streaming deltas, composer input
- **Terminal mode** → TerminalRenderer: xterm.js embed (existing terminal code reused)

## Session Lifecycle

### Creating a session

1. User picks agent + mode from top bar, clicks "+ New Session"
2. Frontend calls `POST /api/chat/sessions` (chat mode) or `POST /api/terminal/sessions` (terminal mode)
3. Daemon creates `terminal_sessions` record with appropriate `mode`
4. **Chat mode:** ChatProcessManager spawns agent as a direct child process with piped stdin/stdout (no tmux, no PTY)
5. **Terminal mode:** TerminalManager spawns agent in tmux as a full TUI (existing behavior)
6. Session appears in the sidebar as active

### Mode switching (session handoff)

1. User clicks Chat/Terminal toggle in session header
2. Frontend calls `POST /api/sessions/{id}/switch-mode` with `{mode: "chat" | "terminal"}`
3. Daemon checks agent's persistence capability
4. **If persistent (Claude Code):** Kill current process, spawn other mode with `--session-id X` / `--resume`. Return updated session. Brief loading indicator in frontend (~1-2s).
5. **If not persistent:** Return `{warning: "Switching modes will end the current conversation"}`. Frontend shows confirmation dialog. If confirmed, daemon kills and spawns fresh. If declined, no change.
6. Session record's `mode` field updates
7. Frontend swaps renderer (ChatRenderer ↔ TerminalRenderer) and switches WebSocket subscription

### Ending a session

- "End Session" kills the process, marks session as `exited` in DB
- Session moves from Active to Previous in the sidebar
- Chat messages and terminal chunks remain in DB for history

### Agent persistence matrix

| Agent | Chat command | Terminal command | Persistence | Handoff |
|---|---|---|---|---|
| Claude Code | `claude -p --session-id X --input-format stream-json --output-format stream-json` | `claude --resume --session-id X` | Yes (~/.claude/ JSONL) | Seamless |
| Hermes | `hermes` (stdin/stdout) | `hermes` (tmux TUI) | No | Warn + fresh |
| OpenClaw | `openclaw` (stdin/stdout) | `openclaw` (tmux TUI) | No | Warn + fresh |
| Ollama | `ollama run {model}` (stdin/stdout) | `ollama run {model}` (tmux) | No | Warn + fresh |
| Aider | `aider` (stdin/stdout) | `aider` (tmux TUI) | No | Warn + fresh |

## ChatProcessManager (new daemon component)

New module in `daemon/src/chat/manager.rs`, parallel to the existing `TerminalManager`.

### Responsibilities

- Spawn agent process as a direct child with `Command::new().stdin(Stdio::piped()).stdout(Stdio::piped())`
- Agent-specific launch commands (see persistence matrix)
- Read stdout in a loop, push `chat_delta` WebSocket events for real-time streaming
- Pass stdout through the appropriate `ChatAdapter` to parse into structured messages
- Store complete messages in `chat_messages` table
- Broadcast `chat_message` events when a complete message is parsed
- Forward user input: receive via HTTP POST, format per agent (NDJSON for Claude, plain text for Ollama), write to stdin pipe
- Track process lifecycle (running → exited/error)
- Handle process crashes gracefully (update session status, surface error)

### Does NOT replace TerminalManager

Terminal mode sessions continue using tmux via the existing `TerminalManager`. The session record's `mode` field determines which manager owns a given session. Mode switching transfers ownership between managers.

## WebSocket Protocol Additions

Existing terminal protocol (`subscribe_terminal`, `terminal_chunk`, `terminal_status`) remains unchanged.

New operations for chat mode:

```
// Client → Daemon: subscribe to a chat session
{op: "subscribe_chat", sessionId: "..."}

// Daemon → Client: streaming token delta (arrives every few ms during generation)
{op: "chat_delta", sessionId: "...", messageId: "...", delta: "partial text"}

// Daemon → Client: complete parsed message (emitted when full response is done)
{op: "chat_message", message: {id, sessionId, role, content, createdAt}}

// Daemon → Client: agent status indicator
{op: "chat_status", sessionId: "...", status: "thinking" | "tool_use" | "idle" | "error"}

// Daemon → Client: session metadata updates
{op: "session_meta", sessionId: "...", tokens: 1234, contextPct: 45, duration: 360}
```

### Streaming flow

1. User sends message → `POST /api/chat/sessions/{id}/message`
2. Daemon formats and writes to agent's stdin pipe
3. Agent generates response, writing to stdout
4. ChatProcessManager reads stdout, emits `chat_delta` events in real-time
5. User sees tokens appearing in the ChatRenderer as they arrive
6. When response completes, ChatAdapter produces final `chat_message`
7. Message stored in DB, `chat_message` event sent to all subscribers

## HTTP API Changes

### New endpoints

```
POST /api/sessions/{id}/switch-mode
Body: {mode: "chat" | "terminal", confirmed?: boolean}
Returns: {session: TerminalSessionRecord}
  or:   {warning: "Switching modes will end the current conversation", needsConfirmation: true}
```

Two-step flow for non-persistent agents:

1. Frontend calls without `confirmed`. Daemon checks persistence — if agent doesn't support it, returns `{warning, needsConfirmation: true}` without killing anything.
2. Frontend shows confirmation dialog. If user confirms, calls again with `confirmed: true`. Daemon kills and spawns fresh.
3. For persistent agents (Claude Code), the first call succeeds immediately — no confirmation needed.

### Existing endpoints unchanged

- `POST /api/chat/sessions` — create chat session (already exists)
- `POST /api/terminal/sessions` — create terminal session (already exists)
- `POST /api/chat/sessions/{id}/message` — send message (already exists, but now writes to subprocess stdin instead of tmux)
- `GET /api/chat/sessions/{id}/messages` — list messages (already exists)
- `GET /api/agents` — list detected agents (already exists)

## Database Changes

### terminal_sessions table additions

```sql
ALTER TABLE terminal_sessions ADD COLUMN parent_session_id TEXT REFERENCES terminal_sessions(id) ON DELETE SET NULL;
ALTER TABLE terminal_sessions ADD COLUMN host_id TEXT;
ALTER TABLE terminal_sessions ADD COLUMN host_name TEXT;
```

- `parent_session_id` — nullable, set when a session is spawned by another agent via delegation
- `host_id` / `host_name` — which machine runs this session. For local sessions, matches the local daemon's identity. For remote sessions, matches the remote host.

### No changes to chat_messages

Existing schema is sufficient: `id, session_id, role, content, created_at`.

## Frontend Components

### Removed

- `ChatView.tsx` — replaced by AgentsView
- `TerminalWorkspace.tsx` — absorbed into AgentsView
- Sidebar nav items "Chat" and "Terminal" — replaced by single "Agents" item

### New / refactored

- **`AgentsView.tsx`** — top-level page component. Owns agent picker, session list, main area. Manages `activeSessionId` state.
- **`SessionSidebar.tsx`** — resizable left panel. Renders active + previous session cards. Drag handle for width adjustment with min-width constraint.
- **`SessionHeader.tsx`** — agent name, status, mode toggle, metadata row (duration, tokens, context %), Open IDE button, End Session button.
- **`ChatRenderer.tsx`** — message bubble list + composer. Handles `chat_delta` (streaming bubble) and `chat_message` (final message). Reuses existing CSS: `.message.user` (blue border, #f0f4ff bg), `.message.assistant` (green border, #f0fdf4 bg).
- **`TerminalRenderer.tsx`** — xterm.js embed extracted from current TerminalWorkspace. Reuses existing terminal theme and hooks.
- **`useChatSocket.ts`** — new hook parallel to `useTerminalSocket`. Subscribes to `subscribe_chat`, handles `chat_delta`, `chat_message`, `chat_status`, `session_meta` events.

### Local PTY sessions (non-agent shells)

The current TerminalWorkspace allows creating plain shell sessions via Tauri PTY (no daemon, no agent). These remain available in the Agents page as a special case:

- The agent picker includes a "Shell" option (not a detected agent — always present)
- Selecting Shell + Terminal creates a local PTY session via Tauri, same as today
- Shell + Chat is disabled (no agent to chat with)
- Local shell sessions appear in the session sidebar like any other session, with host shown as "local"
- `useLocalTerminal.ts` hook continues to handle these sessions

### Existing hooks reused

- `useTerminalSocket.ts` — unchanged, used by TerminalRenderer for remote terminal mode sessions
- `useLocalTerminal.ts` — unchanged, used for local PTY sessions (shell and local agent terminals)

### Styling

All new CSS uses existing design tokens from App.css:
- `--bg-base`, `--bg-surface`, `--bg-elevated`, `--bg-input`
- `--text-primary`, `--text-secondary`, `--text-muted`
- `--accent-blue`, `--accent-green`, `--accent-red`, `--accent-yellow`
- `--border`, `--border-hover`
- `--radius-sm`, `--radius-md`
- `--shadow-sm`, `--shadow-md`

Session card border colors by status:
- Running: `var(--accent-green)` (#10b981)
- Needs approval: `var(--accent-yellow)` (#f59e0b)
- Error: `var(--accent-red)` (#ef4444)
- Exited/previous: `var(--border)` (#e2e5ea)

## Chat Adapters

The existing `ChatAdapter` trait (`daemon/src/chat/adapters/`) gets a real implementation:

### Claude adapter

- Parses NDJSON lines from `claude -p --output-format stream-json`
- Extracts `content_block_delta` for streaming deltas
- Extracts `assistant` messages for complete responses
- Extracts `tool_use` blocks for tool call indicators
- Reports token usage from `result` events

### Ollama adapter

- Reads stdout character-by-character (Ollama streams tokens directly)
- Detects response boundaries (prompt marker `>>> `)
- No token reporting beyond what Ollama exposes

### Generic adapter

- Treats all stdout as assistant text
- Line-buffered delta streaming
- No metadata extraction
- Fallback for Hermes, OpenClaw, Aider, and unknown agents

## Delegated Sessions

### Data model

Sessions with `parent_session_id` set are delegated — spawned by another agent, not by the user directly.

### Creation flow

1. Agent A on machine 1 calls `ghost_spawn_remote_session` MCP tool (new tool to be added)
2. Local daemon calls `POST {remote_host}/api/chat/sessions` with `parent_session_id` and task description
3. Remote daemon creates session, permission check applies (approval-required peers trigger approval flow)
4. Remote agent starts working autonomously (fire-and-forget for now)
5. Parent agent can poll status via `ghost_check_mesh` or the session API

### UI representation

- Child sessions appear indented under parent in the session sidebar
- Tree connector lines show the relationship
- Session header shows "Delegated from: {agent} on {host}" badge
- Clicking a child session navigates to it normally

### Future: supervised delegation

Fire-and-forget is the initial implementation. Supervised orchestration (Agent A monitors and directs Agent B in real-time) is tracked in `docs/project-plan.md` under Experimental/TBD.

## Empty States

### No agents detected

- Agent picker disabled
- Session sidebar: dashed-border empty state card with "+ Set up an agent" link
- Link opens a terminal session with `ghost init` wizard
- Main area: onboarding message explaining the flow

### No active sessions (agents exist)

- Session sidebar's Active section: "No active sessions"
- Main area: centered prompt "Select an agent and mode to get started"

### Session process crash

- Status → "error", border turns red
- Last error output in sidebar preview
- Main area: error state with last output lines + "Restart Session" button

### Handoff failure

- Daemon returns error from switch-mode endpoint
- Frontend stays in current mode, shows toast notification
- No data loss

### Remote host goes offline

- WebSocket disconnects, reconnect with exponential backoff
- Session card status dot → gray
- Main area: "Connection lost — reconnecting..." overlay
- Auto-recovery when host returns

### Context window full

- Context % bar → 100%, turns red
- Claude Code handles compaction internally
- Ghost Protocol surfaces the status but does not intervene

## Scope boundaries

### In scope

- Unified Agents page (replacing Chat + Terminal views)
- ChatProcessManager for subprocess-based chat sessions
- Session handoff (chat ↔ terminal) with persistence detection
- Streaming chat deltas via WebSocket
- Session metadata display (duration, tokens, context %)
- Resizable session sidebar
- Delegated session data model + tree view
- `ghost_spawn_remote_session` MCP tool
- `POST /api/sessions/{id}/switch-mode` endpoint

### Out of scope (future work)

- Supervised agent delegation (Experimental/TBD)
- Open IDE / code-server integration (button is no-op)
- Context compaction controls (informational only)
- Mobile responsive layout for Agents page
- Agent-to-agent communication protocol beyond fire-and-forget

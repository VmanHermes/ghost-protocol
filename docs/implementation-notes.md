# Implementation notes

## Phase 1 summary
- inspected `/home/vmandesk/.hermes/hermes-agent/run_agent.py`, `gateway/run.py`, `gateway/platforms/telegram.py`, and `gateway/platforms/api_server.py`
- confirmed the existing Hermes runtime already exposes callbacks for lifecycle, status, tool start/complete, and streaming
- built a sidecar daemon rather than rewriting Hermes orchestration in place
- added a persistent event log, HTTP API, WebSocket replay/subscribe path, and an initial Tauri Ghost Protocol shell

## Phase 2 summary
- extended the backend schema with:
  - run attempts
  - agents
  - approvals
  - artifacts
  - usage records
- made run live projections derive from emitted events in the store layer
- added richer APIs for:
  - system status
  - run detail
  - retry/cancel
  - approvals resolution
  - agents listing
- added a root-agent projection path using `agent_spawned` and `agent_updated`
- made usage emission persist to `usage_records`
- added an outbound Telegram bridge that consumes the shared daemon event stream and sends concise progress/final summaries
- updated the Ghost Protocol app to surface:
  - system status
  - active agents
  - approvals queue
  - attempts and usage
  - retry/cancel actions

## Phase 3-in-progress summary
- wired Hermes dangerous-command approval callback into the daemon runtime using `tools.approval.register_gateway_notify`
- daemon now emits `approval_requested` when the runtime blocks for approval
- desktop/HTTP approval resolution now unblocks the underlying Hermes approval queue via `resolve_gateway_approval`
- Telegram bridge now supports:
  - outbound concise progress/final updates from the shared event stream
  - inbound polling from the configured Telegram chat
  - starting runs from Telegram messages through the daemon pipeline
  - `/approve` and `/deny` handling for pending approvals

## Validation completed
- backend modules compile with `python3 -m py_compile backend/src/ghost_protocol_daemon/*.py`
- frontend build succeeds with `npm run build`
- daemon `/health` and `/api/system/status` return expected data
- conversation create/message append/run start succeed
- run detail includes timeline, agent, attempts, and usage data
- retry endpoint works
- approval resolution endpoint works end-to-end against daemon state
- Telegram bridge is configured from `.env` and enabled at runtime

## Still deferred
- richer approval payloads directly from Hermes command safety internals beyond the current command-oriented bridge
- subagent tree reconstruction from delegated runs
- artifact persistence linked to originating events
- stronger Tailscale-native service identity / ACL integration beyond CIDR allowlisting
- production-grade Telegram inbound dedupe/history persistence beyond the current lightweight polling bridge

# Implementation notes

## Phase 1 summary
- inspected `/home/vmandesk/.hermes/hermes-agent/run_agent.py`, `gateway/run.py`, `gateway/platforms/telegram.py`, and `gateway/platforms/api_server.py`
- confirmed the existing Hermes runtime already exposes callbacks for lifecycle, status, tool start/complete, and streaming
- built a sidecar daemon rather than rewriting Hermes orchestration in place
- added a persistent event log, HTTP API, WebSocket replay/subscribe path, and an initial Tauri desktop shell

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
- updated the desktop app to surface:
  - system status
  - active agents
  - approvals queue
  - attempts and usage
  - retry/cancel actions

## Validation completed
- backend modules compile with `python3 -m py_compile backend/src/hermes_desktop_daemon/*.py`
- frontend build succeeds with `npm run build`
- daemon `/health` and `/api/system/status` return expected data
- conversation create/message append/run start succeed
- run detail includes timeline, agent, attempts, and usage data
- retry endpoint works
- Telegram bridge is configured from `.env` and enabled at runtime

## Still deferred after phase 2
- inbound Telegram -> daemon conversation bridging
- approval requests emitted directly from Hermes approvals into the daemon approvals table
- subagent tree reconstruction from delegated runs
- artifact persistence linked to originating events
- stronger Tailscale-native service identity / ACL integration beyond CIDR allowlisting

# Hermes Desktop v1 project plan

## Goal
Build a Tauri 2 desktop application that becomes the primary interface for Hermes, backed by a headless daemon that exposes explicit HTTP and WebSocket APIs over Tailscale.

## Phase 1 focus
1. Inspect current Hermes runtime, gateway, and Telegram integration.
2. Define a stable event envelope and resumable WebSocket protocol.
3. Implement a smallest useful end-to-end backend slice.
4. Scaffold the desktop app around that transport.

## Existing architecture summary
- The current Hermes runtime lives at `/home/vmandesk/.hermes/hermes-agent`.
- `run_agent.py` contains the `AIAgent` orchestration loop.
- `gateway/run.py` adapts messaging platforms to the runtime and already bridges callbacks like `status_callback`, `step_callback`, `tool_start_callback`, and `tool_complete_callback`.
- `gateway/platforms/telegram.py` is the current Telegram adapter.
- `gateway/platforms/api_server.py` already uses `aiohttp`, which is a good fit for a boring explicit daemon transport.

## Key design choice
Phase 1 uses a thin sidecar daemon in this project rather than invasive refactors inside the existing Hermes source tree. The daemon imports the existing runtime and turns its lifecycle into a durable event stream.

## Smallest end-to-end slice
- create/list conversations
- append user messages
- start a run that invokes the existing `AIAgent`
- emit and persist run/message/tool/status events
- stream events over WebSocket with resume by sequence
- show conversation list, chat, run live state, and event timeline in the desktop app

## Deferred after Phase 1
- Telegram fully routed through the new event pipeline
- approval queue UX and rich diff viewers
- artifact downloads/previews beyond basic linkage
- stronger Tailscale service identity integration and deployment automation

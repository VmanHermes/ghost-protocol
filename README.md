# Hermes Desktop v1

New primary interface for Hermes using a headless Linux daemon plus a Tauri 2 desktop client.

Phase 1 goals implemented here:
- inspect and preserve the existing Hermes runtime instead of rebuilding orchestration
- add an explicit persistent event log with a stable event envelope
- expose HTTP + WebSocket transport for conversations, runs, and live events
- scaffold a Tauri 2 + React + Vite desktop app against the same backend API

Workspace layout:
- `backend/` — Python daemon, event store, projections, HTTP and WebSocket transport
- `desktop/` — Tauri 2 desktop client (React + TypeScript)
- `docs/` — architecture notes, implementation notes, and phase plan

Current architecture decision:
- keep Hermes runtime headless and outside the desktop shell
- use the existing `AIAgent` runtime from `/home/vmandesk/.hermes/hermes-agent`
- add a thin adapter layer that emits persistent events and exposes explicit APIs
- use WebSocket for primary realtime delivery and HTTP for request-response APIs

Quick start:
1. `cd backend && python -m venv .venv && source .venv/bin/activate && pip install -e .`
2. `python -m hermes_desktop_daemon.server`
3. `cd ../desktop && npm install && npm run tauri dev`

Important note:
Telegram is not replaced in Hermes itself. This project builds the new primary interface and daemon event pipeline while preserving the current runtime path for later Telegram adapter unification.

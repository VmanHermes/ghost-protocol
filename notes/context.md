# Context

This workspace builds Ghost Protocol as the primary interface for Hermes.

Phase 1 implementation strategy:
- preserve the existing Hermes runtime in /home/vmandesk/.hermes/hermes-agent
- add a thin sidecar daemon with persistent events and WebSocket transport
- scaffold a Tauri 2 Ghost Protocol client against that transport

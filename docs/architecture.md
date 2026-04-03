# Hermes Desktop v1 architecture notes

## Event envelope
Every event uses:
- eventId
- type
- ts
- seq
- conversationId
- runId
- agentId
- stepId
- toolCallId
- artifactId
- approvalId
- causationId
- correlationId
- visibility
- payloadVersion
- summary
- payload

## WebSocket protocol
Client -> server messages:
- `subscribe`: `{ "op": "subscribe", "conversationId"?, "runId"?, "afterSeq"?, "lastEventId"? }`
- `ping`: `{ "op": "ping", "ts": "..." }`
- `approve`: reserved for later phases
- `reject`: reserved for later phases
- `cancel_run`: reserved for later phases

Server -> client messages:
- `hello`
- `subscribed`
- `event`
- `heartbeat`
- `error`

## Persistence model in phase 1
SQLite tables:
- conversations
- messages
- runs
- events
- run_live_projection
- run_timeline_projection

## Tailscale access stance in phase 1
- daemon is intended to bind on a private/Tailscale interface only
- middleware only accepts clients from configured Tailscale/private CIDRs
- no public exposure is assumed or supported

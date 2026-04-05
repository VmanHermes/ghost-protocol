# MCP Tools for Agent Interaction

**Date:** 2026-04-05
**Status:** Draft
**Phase:** 2g (extends Phase 2: The Context Layer)

## Context

The Ghost Protocol MCP server currently exposes read-only resources. Agents can read mesh state but have no way to take action through MCP — and more importantly, agents don't proactively read resources unless instructed. Tools are far more visible to agents: they appear in the tool list and agents naturally consider using them. We need to convert key interactions into MCP tools and add instructions that nudge agents to use them.

## Goals

1. Add MCP tool support to the JSON-RPC transport (tools/list, tools/call)
2. Implement three tools: ghost_report_outcome, ghost_check_mesh, ghost_list_machines
3. Add usage instructions to the context briefing so agents know to report outcomes

## Non-Goals

- Changing the existing resource system (resources stay as-is)
- Adding write tools beyond outcome reporting (no terminal creation via MCP yet)
- Authentication for MCP tools (MCP runs on localhost stdio, already trusted)

---

## MCP Protocol Extension

### Initialize Capabilities

The `initialize` response adds `"tools": {}` alongside existing `"resources": {}`:

```json
{
  "capabilities": {
    "resources": {},
    "tools": {}
  }
}
```

### New JSON-RPC Methods

**`tools/list`** — returns tool definitions:

```json
{
  "tools": [
    {
      "name": "ghost_report_outcome",
      "description": "Report the outcome of work you performed. Call this after completing builds, deployments, inference, or other significant tasks. Helps the mesh learn which machines are best for which work.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "category": { "type": "string", "description": "Type of work: build, inference, deploy, test, custom" },
          "action": { "type": "string", "description": "What you did: 'cargo build --release', 'ollama run llama3', etc." },
          "status": { "type": "string", "enum": ["success", "failure", "timeout", "cancelled"], "description": "Outcome" },
          "description": { "type": "string", "description": "Optional context about what you were trying to accomplish" },
          "targetMachine": { "type": "string", "description": "Which machine the work ran on (hostname or IP)" },
          "exitCode": { "type": "integer", "description": "Process exit code if applicable" },
          "durationSecs": { "type": "number", "description": "How long the work took in seconds" },
          "metadata": { "type": "object", "description": "Any additional structured data" }
        },
        "required": ["category", "action", "status"]
      }
    },
    {
      "name": "ghost_check_mesh",
      "description": "Get current mesh state: machines, active sessions, recent activity, and permission levels. Use this to understand what's available before routing work.",
      "inputSchema": {
        "type": "object",
        "properties": {},
        "required": []
      }
    },
    {
      "name": "ghost_list_machines",
      "description": "Get structured machine data for routing decisions: name, IP, online status, GPU, RAM, capabilities, and your permission tier on each machine.",
      "inputSchema": {
        "type": "object",
        "properties": {},
        "required": []
      }
    }
  ]
}
```

**`tools/call`** — dispatches to the right handler based on tool name. Returns:

```json
{
  "content": [
    {
      "type": "text",
      "text": "..."
    }
  ]
}
```

---

## Tool Implementations

### ghost_report_outcome

Calls `POST /api/outcomes` on the daemon's HTTP API (localhost):

```
Input: { category, action, status, description?, targetMachine?, exitCode?, durationSecs?, metadata? }
Output: { "id": "uuid", "createdAt": "2026-04-05T14:30:00Z", "message": "Outcome recorded" }
```

### ghost_check_mesh

Calls `context_briefing()` on the ResourceBuilder:

```
Input: {} (no params)
Output: The full plain-text context briefing (machines, sessions, activity, permissions, hints)
```

### ghost_list_machines

Combines data from multiple HTTP endpoints into one structured response:
- `GET /api/hosts` — host list with status and capabilities
- `GET /api/permissions` — permission tiers
- `GET /api/system/hardware` — local machine info

Returns:
```json
{
  "local": {
    "hostname": "laptop",
    "ip": "100.64.1.1",
    "cpu": "AMD Ryzen 9",
    "ramGb": 64,
    "gpu": null
  },
  "peers": [
    {
      "name": "shared-host",
      "ip": "100.64.1.3",
      "status": "online",
      "gpu": "RTX 4090",
      "ramGb": 64,
      "capabilities": { "hermes": true, "ollama": true },
      "permissionTier": "full-access"
    }
  ]
}
```

---

## Context Briefing Instructions

Append to the end of `context_briefing()`:

```
Available Ghost Protocol tools:
  - ghost_report_outcome: Report what you did and the result after completing work
  - ghost_check_mesh: Check current mesh state (machines, sessions, activity)
  - ghost_list_machines: Get machine capabilities and permissions for routing decisions

After completing significant work (builds, deployments, inference, file operations), 
use ghost_report_outcome to log the result. This helps the mesh learn which machines 
are best for which tasks.
```

This section appears in the MCP `initialize` instructions, so agents see it at the start of every session.

---

## Files to Modify

### Daemon (Rust)

| File | Change |
|---|---|
| `daemon/src/mcp/transport.rs` | Add tools/list and tools/call handlers, add tools to initialize capabilities |
| `daemon/src/mcp/resources.rs` | Add `list_machines()` method, add tool instructions to `context_briefing()` |

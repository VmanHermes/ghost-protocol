# Shared terminal / remote sessions

Ghost Protocol now targets a shared-terminal model with Zellij rather than a simple side-panel remote shell.

Current direction:
- Tailscale/private-network access to the daemon
- Zellij-backed terminal sessions for durability and re-attachment
- Ghost Protocol UI as the shared control surface
- Hermes sessions and rescue shells as named remote sessions
- Chat and terminal as interchangeable views in the main workspace

What the current slice adds:
- durable remote sessions that survive client disconnects better than raw child shells
- Zellij-backed attach / re-attach semantics in the daemon
- xterm-based terminal UI in the main app area instead of a textarea-like side panel
- top-level Chat / Terminal toggle in the main workspace
- raw terminal keystroke forwarding, resize handling, interrupt, and terminate controls

Planned next steps:
- project/workspace-scoped session launch
- multiple Zellij panes/tabs surfaced in the UI
- file tree, diff, logs, and process views around the shared terminal
- session permissions / safer rescue shell controls

# Ghost Protocol — Vision

## The problem

AI coding agents are powerful but isolated. Each one runs in a single terminal, on a single machine, with no awareness of what happened before or what's running elsewhere. If you have a desktop, a laptop, and a home server — each with different GPUs, models, and tools — you're left manually SSH-ing between them, copy-pasting context, and mentally tracking which agent did what, where.

There's no shared memory. No unified view. No way to say "run this on the machine with the GPU" from your laptop. Every session starts cold.

## The idea

Ghost Protocol turns your machines into a mesh where AI agents are first-class citizens. Instead of managing terminals and agents per-machine, you get a single control plane that spans all your devices over Tailscale.

The core insight: **the network is the computer, and agents should be able to work across it as naturally as you do.** You already move between your machines throughout the day. Your agents should too — picking the right hardware for the job, remembering what was tried before, and reporting back what they did.

## The metaphor

The name isn't just a name — it maps to how the system works:

- **Ghost** — the agent. It lives in the machine, does the work, and moves between vessels.
- **Vessel** — the host. Your desktop, laptop, server — the hardware that houses the ghosts.
- **Ghost Sight** — the observer. Your Tauri app, the single pane of glass where you watch, direct, and intervene across every vessel in the mesh.

## What this looks like in practice

You open Ghost Protocol on your laptop. The sidebar shows your desktop and home server are online. You pick Hermes on the desktop (it has the GPU) and ask it to refactor a module. While it works, you start Claude Code on your laptop for a different task with Ghost's MCP server attached. Both sessions are visible, both report outcomes, and both contribute to a shared memory that the next session draws from.

When you open a new session tomorrow, the agent already knows: "Last time we worked on the auth module, the team decided to use session tokens stored server-side for compliance reasons." That context came from the intelligence layer, which quietly extracted it from yesterday's session transcript.

No manual context sharing. No "let me catch you up." The mesh remembers.

## Design principles

- **Agent-agnostic.** Ghost Protocol doesn't replace your agents — it discovers and supervises whatever's installed: Claude Code, Hermes, Ollama, Aider, or anything you register. The value is in the orchestration layer, not in being yet another AI runtime.

- **Daemon is the source of truth.** Every machine runs a lightweight Rust daemon that owns its local state — sessions, outcomes, agent detection, permissions. The desktop app is a thin client. The CLI is a thin client. Agents interact through MCP tools. Everything flows through the daemon.

- **Tailscale for networking.** WireGuard-encrypted mesh with no certificates to manage, no ports to forward, no cloud dependency. If two machines are on the same Tailnet, they can find each other.

- **Memory without effort.** The intelligence layer is opt-in but zero-friction once enabled. It processes session transcripts after they end, extracts lessons and context, and injects relevant memories into future sessions. You don't tag, organize, or search — the system does it. And because the provider abstraction supports local models (Ollama) alongside cloud APIs, memory extraction can run entirely within your tailnet — session data never has to leave your hardware.

- **Supervised by design.** Agents run autonomously, but you're always the final authority. Every cross-machine write operation goes through an approval flow. You can watch any agent's terminal in real-time, pause it, or kill it from any device. Ghost Protocol gives agents freedom to work without giving up your ability to intervene.

- **Permissions by default.** Every cross-machine operation respects a 4-tier permission model (full-access, approval-required, read-only, no-access). You control exactly what each machine can do to yours.

## Where it's going

The current system handles terminals, chat, code-server instances, and cross-machine orchestration. The next frontiers are agent observability (real-time view of all agents across the mesh), supervised delegation (Agent A spawns Agent B on a remote machine, monitors its progress, and course-corrects), and mobile access.

**Mobile as a feed, not a terminal.** On a phone, a raw terminal is the wrong interface. The mobile experience should be a timeline of agent actions — "Hermes committed 3 files", "Claude Code ran tests — 2 failed", "Awaiting your approval" — that you tap to expand into detail when needed. The outcome log and intelligence layer already capture this data; the mobile client just needs to present it as a feed. The terminal is there if you want it, but the feed is how you stay aware on the go.

**Credential isolation across the mesh.** When running agents on shared or less-trusted machines, you shouldn't need to put your API keys there. A machine you trust (your laptop) can act as a credential vault — the remote agent performs the work, but inference requests route through your trusted machine for signing. Your keys never leave your physical hardware, but every machine in the mesh gets access to the intelligence. This extends the permission model from "what can this machine do" to "what secrets does this machine need."

The long-term vision: a personal AI operations center that runs on your own hardware, learns from every interaction, and makes your entire fleet of machines and agents work together as one.

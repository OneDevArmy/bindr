# Bindr

[![Crates.io](https://img.shields.io/crates/v/bindr.svg?label=crates.io)](https://crates.io/crates/bindr)
[![Docs](https://img.shields.io/badge/docs-bindrcli.com-blue)](https://docs.bindrcli.com)
[![CI](https://github.com/OneDevArmy/bindr/actions/workflows/ci.yml/badge.svg)](https://github.com/OneDevArmy/bindr/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Built with Tokio](https://img.shields.io/badge/runtime-tokio-purple.svg)](https://tokio.rs)

Bindr is a terminal-native micro-kernel for multi-agent software creation. It orchestrates specialized Brainstorm → Plan → Execute → Document agents that coordinate over an event-driven kernel, retain project context, and stream responses live while staying within guardrails you control.

## Highlights
- **Kernel-driven agents** – every mode runs as an isolated Tokio task with automatic restarts, lifecycle telemetry, and broadcast events for UI or automation consumers.
- **Context that sticks** – conversation history, model choices, and project metadata persist through `SessionManager`, making it effortless to resume work or hand off between agents.
- **Tool governance** – per-mode capability profiles keep risky operations gated; reviews and approvals surface through kernel notifications so humans stay in charge.
- **Production ergonomics** – streaming TUI, structured logging, and environment warnings (e.g., running from `~`) keep teams informed before things go sideways.

## Quick Start
```bash
# install
cargo install bindr

# launch the TUI from your project directory
bindr
```
Add an API key when prompted, then explore commands like `/mode`, `/model`, and `/help` inside the conversation pane. Bindr will automatically route intents between mode agents and emit notifications as they save context or request tools. For web search, export your Tavily key before launching:

```bash
export TAVILY_API_KEY=sk_tavily_xxx
```

## Architecture Notes
The kernel (`src/core/kernel.rs`) supervises agents via bounded queues and a broadcast event bus. `AgentRuntime` wires per-mode factories into the kernel, parses JSON tool calls emitted by the models, and either executes or queues the requests (with approvals surfaced in the TUI). Tool results, lifecycle events, and environment warnings flow back through kernel notifications so the UI can maintain an activity feed without interrupting the agents.

**Tool call contract:** when an agent needs a tool it emits a JSON envelope such as:

```json
{
  "tool": "web_search",
  "args": { "query": "tui framework ratatui guide", "max_results": 5 }
}
```

If the tool requires approval, it appears in the side panel (`/approve` to authorize); otherwise the kernel executes it immediately and streams the output as a system message.

## Contributing
We welcome issues and pull requests. Run `cargo fmt && cargo check && cargo test` before submitting, and see `AGENTS.md` plus `docs/architecture.md` for deeper architectural guidance.

## License
Licensed under the [Apache-2.0](./LICENSE) license.

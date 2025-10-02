# Bindr

[![Crates.io](https://img.shields.io/crates/v/bindr.svg)](https://crates.io/crates/bindr)
[![Docs](https://img.shields.io/badge/docs-latest-blue.svg)](https://docs.bindr.dev)
[![CI](https://github.com/OneDevArmy/bindr/actions/workflows/ci.yml/badge.svg)](https://github.com/OneDevArmy/bindr/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)

> Multi-mode AI workflow orchestration for builders shipping fast.

Bindr is a TUI-driven assistant that turns large language models into disciplined project collaborators. It guides you from idea to shipped product through structured modes (Brainstorm → Plan → Execute → Document) while enforcing tool permissions, context handoffs, and review steps tailored to each stage of delivery.

## Why Bindr?
- *Mode-aware agents* ensure the assistant behaves like a strategist in Brainstorm, a project manager in Plan, an engineer in Execute, and a technical writer in Document.
- *Tooling guardrails* automatically enforce per-mode capabilities (read-only, scaffolding, patching, command execution) so work stays safe and auditable.
- *Context continuity* delivers rich handoff summaries that keep every mode aligned on decisions and pending tasks.
- *Professional UX* includes approval prompts, diff previews, and command confirmations so humans stay in control while moving fast.

## Getting Started

### Prerequisites
- Rust 1.76+ and Cargo
- API key for a supported LLM provider (OpenAI, Anthropic, OpenRouter, etc.)

### Install from crates.io
```bash
cargo install bindr
```

### Build from source
```bash
git clone https://github.com/OneDevArmy/bindr.git
cd bindr
cargo install --path .
```

## Usage
Launch the TUI:
```bash
bindr
```

Inside the conversation view you can:
- **Enter** to send prompts to the active mode agent.
- **/mode** to cycle modes or `/mode <b|p|e|d>` to jump directly to Brainstorm, Plan, Execute, or Document.
- **/model** to switch providers/models.
- **/help** to list commands.

### Mode capabilities
- **Brainstorm** – read-only discovery, clarifying questions, requirement capture.
- **Plan** – scaffold directories/files and implementation roadmaps (with approval).
- **Execute** – propose patches, run gated commands, and update code under supervision.
- **Document** – generate README content, changelogs, and inline comments without touching code.

## Roadmap
- [x] Unified prompt refactor and centralized mode instructions
- [x] Mode-aware tool dispatcher with approvals
- [ ] Tool execution pipeline with human-in-the-loop approvals
- [ ] Project indexing and diff summarization
- [ ] Cloud session sync and team collaboration

Contributions are welcome! A `CONTRIBUTING.md` guide will be published soon. In the meantime, open an issue or pull request on GitHub.

## License

Licensed under the [Apache-2.0](./LICENSE) license.
# Contributing to Bindr

Thanks for helping evolve Bindr into a production-grade multi-agent runtime. This guide captures the workflow and quality bar we expect for every change.

## Development Environment
- **Rust toolchain**: Install the latest stable toolchain via [rustup](https://rustup.rs). Bindr targets Rust 2021.
- **Components**: Ensure `rustfmt`, `clippy`, and `cargo-nextest` are installed.
- **Optional tooling**: We recommend [`just`](https://github.com/casey/just) or simple shell aliases for running the task bundle below.

```bash
rustup component add rustfmt clippy
cargo install cargo-nextest
```

## Repository Bootstrap
- **Clone**: `git clone git@github.com:OneDevArmy/bindr.git && cd bindr`
- **Secrets**: Export API keys (e.g. `TAVILY_API_KEY`) before running the TUI so tool calls can execute.
- **Editor config**: Enable format-on-save and diagnostics for Rust.

## Branching & Commits
- Use feature branches named `feat/<slug>`, `fix/<slug>`, or similar.
- Follow the conventional commit style, e.g. `feat(kernel): add restart backoff telemetry`.
- Keep commits focused. Squash before merging when the history is noisy.

## Pre-commit Checklist
Run the full validation suite locally before pushing:

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features
cargo nextest run --all-features
```

- Add or update unit/integration tests for new behavior.
- Document significant architectural changes in `docs/architecture.md`.
- Update `README.md`, `PROMPT.md`, or `AGENTS.md` when user-facing flows or agent responsibilities change.

## Pull Request Expectations
- **Description**: Summarize the change, highlight testing, and note risk mitigation.
- **Screenshots / logs**: Include relevant TUI captures or `tracing` output when altering UX flows.
- **Review scope**: Call out any intentional follow-ups to avoid hidden TODOs.
- **CI green**: Ensure the `ci.yml` workflow passes before requesting review.

## Code Style & Architecture
- Favor small, cohesive modules in `src/core/`, `src/agents/`, and `src/ui/`.
- Public functions must include concise doc comments.
- Prefer `anyhow` for application errors and `thiserror` for domain-specific enumerations.
- Use `tracing` for structured logging; avoid `println!` except in CLI entry points.
- Respect the guardrails in `AGENTS.md`: no speculative rewrites and ensure kernel stability.

## Security & Secrets
- Never commit API keys or secrets. Use environment variables and `.env` files ignored by Git.
- Validate file paths before performing write operations to avoid accidental destructive actions.

## Getting Help
- Open discussions for architectural proposals before large refactors.
- Tag `@OneDevArmy/bindr-maintainers` on PRs requiring additional review.

Thanks again for helping keep Bindr robust, maintainable, and transparent.

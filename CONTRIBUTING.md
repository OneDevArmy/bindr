# Contributing to Bindr

We’re excited that you’re interested in improving Bindr. This guide walks you through how we ship changes together while keeping the project stable and friendly.

## Getting Set Up
- **Toolchain**: Install Rust using [rustup](https://rustup.rs). Bindr targets Rust 2021.
- **Components**: Add `rustfmt`, `clippy`, and `cargo-nextest` so your environment matches CI.
- **Nice-to-haves**: [`just`](https://github.com/casey/just) or your favorite task runner can simplify command bundles.

```bash
rustup component add rustfmt clippy
cargo install cargo-nextest
```

## Project Basics
- **Clone**: `git clone git@github.com:OneDevArmy/bindr.git && cd bindr`
- **Configuration**: Set any provider API keys as environment variables before using the TUI.
- **Editor**: Enable format-on-save and diagnostics for Rust so issues surface early.

## Branching & Commits
- Start work on a feature branch such as `feat/<slug>` or `fix/<slug>`.
- Follow conventional commits, e.g. `feat(kernel): add restart backoff telemetry`.
- Keep commits focused. Squash only if intermediate history is noisy.

## Before You Push
Run the full validation suite locally:

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features
cargo nextest run --all-features
```

- Add or update unit/integration tests for new behavior.
- Capture architecture changes in `docs/architecture.md`.
- Refresh `README.md`, `PROMPT.md`, or `AGENTS.md` when user-facing flows evolve.

## Opening a Pull Request
- **Tell the story**: Summarize the change, note testing, and call out risks or follow-ups.
- **Show the impact**: Include `tracing` output or TUI screenshots when UX or streaming behavior changes.
- **Respect reviewers**: Avoid hidden TODOs and make sure CI is green before requesting review.

## Code Style & Architecture
- Prefer small, cohesive modules under `src/core/`, `src/agents/`, and `src/ui/`.
- Document all public functions with concise doc comments.
- Use `anyhow` for application errors and `thiserror` for domain-specific error enums.
- Leverage `tracing` for logging; reserve `println!` for CLI entry points.
- Follow the guardrails in `AGENTS.md`—no speculative rewrites and keep the runtime stable.

## Security
- Never commit credentials or secrets. Use environment variables and ignore local `.env` files.
- Validate file paths before writing to disk to prevent destructive operations.
- Report security vulnerabilities to [security@bindrcli.com](mailto:security@bindrcli.com).

## Need a Hand?
- Open a discussion for architectural proposals before large refactors.
- Tag `@OneDevArmy/bindr-maintainers` when you need extra eyes on a PR.

Thanks for contributing! Every improvement helps build a more capable Bindr.

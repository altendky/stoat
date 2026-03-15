# AI Agent Context

For project requirements, architecture, and conventions, see:

- [Project Documentation](docs/src/project/index.md)

## Sans-IO Architecture

This project uses a sans-IO crate layout:

- `stoat-core` — Pure logic, no I/O dependencies. All new logic should go here when possible.
- `stoat-io` — I/O layer (axum, reqwest, tokio). Interprets effects from core.
- `stoat` — Binary crate, wires core + I/O together.

Core crates must not depend on tokio, async runtimes, network, or filesystem access.

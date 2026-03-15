# Decisions

Resolved design decisions and their rationale.

## Name

**Decision:** stoat -- **St**reaming **OA**uth **T**ransformer.

The name is a backronym for the tool's three defining properties: it streams data through without buffering, it manages OAuth tokens, and it transforms requests.
The crate name `stoat` is available on crates.io.
The logo concept is a stoat (the animal) with a transformer theme.

## Language

**Decision:** Rust.

Produces a single static binary with no runtime dependencies.
Good async HTTP ecosystem (`axum` for the local server, `reqwest` for the upstream client).
Consistent with the author's other projects.

## Sans-IO Crate Layout

**Decision:** Three crates following sans-IO principles.

- `stoat-core` -- Pure logic, no I/O dependencies.
  Config types, PKCE generation, token expiry checking, request transformation logic.
- `stoat-io` -- I/O layer.
  axum server, reqwest client, browser launch, file I/O, token exchange.
- `stoat` -- Binary crate, wires core + I/O together with CLI.

Core crates must not depend on tokio, async runtimes, network, or filesystem access.
This maximizes testability: core logic is tested with pure functions and deterministic inputs, no mocking required.

## Config Format

**Decision:** TOML.

Supports comments (unlike JSON), native to the Rust ecosystem (`toml` crate), well-maintained, and familiar to Rust developers.
The config file is the only place where provider-specific details live, so readability matters.

## Generic Design

**Decision:** stoat contains no provider-specific code.

All OAuth endpoints, client IDs, header rewrites, query param additions, and user-agent strings are specified in the user-supplied config file.
This makes the tool generally useful and avoids embedding any particular provider's credentials or conventions in the codebase.

## OAuth Code Receipt

**Decision:** Support both paste-from-terminal and local HTTP listener.

Paste mode (default): after the browser redirect, the user pastes the authorization code into the terminal.
This works when the OAuth provider's redirect URI points to the provider's own server, which displays the code for the user to copy.

Local listener mode: stoat starts a temporary HTTP server on localhost to receive the redirect callback directly.
This works when the redirect URI can be set to a localhost address.

Both modes are supported because different OAuth providers have different redirect URI requirements.

## No Request Body Transforms

**Decision:** v1 does not parse or modify request or response bodies.

Request bodies are forwarded as-is.
Response bodies are streamed through unmodified.
This keeps the proxy simple and avoids the need to understand provider-specific body formats.

If body transforms are needed in the future (e.g., tool name prefixing for specific providers), they can be added as an optional config section.

## Port Reporting

**Decision:** Print `port=<N>` to stdout when the server starts listening, then all subsequent output goes to stderr via `tracing`.

This enables scripting: a wrapper can capture the port from stdout and pass it to a child process as an environment variable.
Separating the port from log output (stderr) avoids parsing issues.

## Token Refresh Strategy

**Decision:** Refresh proactively before forwarding a request when the token is expired or near expiry.

The proxy checks the stored `expires_at` timestamp before each request.
If the token is expired or within a configurable margin of expiry, it refreshes first.
This avoids forwarding requests that will fail with 401 and needing to retry.

## Dual License

**Decision:** MIT / Apache-2.0 dual license.

Standard for the Rust ecosystem.

## Test Runner

**Decision:** `cargo-nextest`.

Faster test execution, better output formatting, JUnit XML support for CI reporting.
The `ci` profile is configured with `fail-fast = false` and JUnit output.

## Coverage

**Decision:** `cargo-llvm-cov` with Codecov.

LLVM-based coverage instrumentation via `cargo-llvm-cov` with `nextest` integration.
Coverage is uploaded to Codecov via OIDC (tokenless).
Project ratchet: 2% threshold.
Patch target: 100%.

## Dev Tool Management

**Decision:** mise for reproducible dev tool versions.

All non-Rust dev tools (pre-commit, cargo-deny, cargo-llvm-cov, nextest, lychee) are pinned in `mise.toml` with checksums locked in `mise.lock`.
This ensures consistent tool versions across developers and CI.

## Dependency Updates

**Decision:** Renovate with self-hosted bot.

Automated dependency updates for Rust crates, GitHub Actions (SHA-pinned), pre-commit hooks, mise tools, and the Rust toolchain itself.
Custom regex managers track `rust-toolchain.toml` channel and `Cargo.toml` `rust-version` as a grouped "Rust toolchain" update.

## Merge Queue

**Decision:** Mergify with label-gated queue.

Merge queue is triggered by the `enqueue` label, requires at least one approved review, and uses merge commits.
Renovate bot PRs are auto-approved.

## Static Linking

**Decision:** musl targets with `+crt-static` for portable Linux binaries.

`.cargo/config.toml` sets `target-feature=+crt-static` for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`.
This is technically redundant (musl defaults to static linking) but makes the intent explicit.

## Toolchain

**Decision:** Edition 2024, Rust 1.89 minimum.

Edition 2024 for the latest language features.
`rust-toolchain.toml` pins channel `1.89` for local development consistency.
CI tests on latest stable.

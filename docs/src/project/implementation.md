# Implementation

## Phase 1: Project Setup

- [x] Initialize Cargo workspace with sans-IO crate layout (`stoat-core`, `stoat-io`, `stoat`)
- [x] Create `rust-toolchain.toml`
- [x] Configure linting (`rustfmt.toml`, workspace clippy lints)
- [x] Set up pre-commit hooks (`.pre-commit-config.yaml`, `typos.toml`)
- [x] Add license files (`LICENSE-MIT`, `LICENSE-APACHE`)
- [x] Set up GitHub Actions CI
- [x] Create `.gitignore`, `.gitattributes`
- [x] Configure `cargo-deny` (`deny.toml`)
- [x] Configure markdownlint (`.markdownlint-cli2.yaml`)
- [x] Configure lychee link checker (`.lychee.toml`)
- [x] Write project documentation (`docs/src/project/`)

## Phase 2: Config and CLI

- [ ] Define config file types in `stoat-core` (serde deserialization)
- [ ] Implement tilde expansion for `token_file` in `stoat-core`
- [ ] Implement config validation in `stoat-core` (required fields, URL parsing)
- [ ] Implement CLI with `clap` in `stoat` binary (`login` and `serve` subcommands)
- [ ] Implement config file loading in `stoat-io` (file reading)

## Phase 3: OAuth Flow

- [ ] Implement PKCE code verifier and S256 challenge generation in `stoat-core`
- [ ] Implement authorization URL construction in `stoat-core`
- [ ] Implement browser launch via `open` crate in `stoat-io`
- [ ] Implement authorization code receipt -- paste mode in `stoat-io`
- [ ] Implement authorization code receipt -- local HTTP listener mode in `stoat-io`
- [ ] Implement token exchange (POST to `token_url`) in `stoat-io`
- [ ] Implement token storage (write JSON with `0600` permissions) in `stoat-io`

## Phase 4: Token Management

- [ ] Define token types and expiry checking in `stoat-core`
- [ ] Implement token file loading in `stoat-io`
- [ ] Implement token refresh (POST to `token_url` with `grant_type=refresh_token`) in `stoat-io`
- [ ] Implement token file update after refresh in `stoat-io`

## Phase 5: Proxy Server

- [ ] Implement request transformation logic in `stoat-core` (header strip/set, query param append, template resolution)
- [ ] Implement `axum` HTTP server with configurable bind address in `stoat-io`
- [ ] Implement port 0 support and port reporting to stdout
- [ ] Implement upstream request forwarding via `reqwest` in `stoat-io` (preserving path and body)
- [ ] Implement streaming response forwarding (chunked/SSE pass-through)
- [ ] Integrate token refresh with proxy (refresh before forwarding if expired)
- [ ] Implement `tracing` logging to stderr

## Phase 6: Polish

- [ ] Error handling and user-facing error messages
- [ ] Add unit tests for `stoat-core` (pure logic, no mocking needed)
- [ ] Add integration tests for `stoat-io`
- [ ] Test with a real OAuth provider
- [ ] Verify streaming behavior with SSE responses

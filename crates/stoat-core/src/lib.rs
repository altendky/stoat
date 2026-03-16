//! stoat-core: Pure logic for stoat (sans-IO)
//!
//! This crate contains no I/O dependencies — no tokio, no async, no network,
//! no filesystem access. All logic is expressed as pure functions and types.
//!
//! Responsibilities:
//! - Config file types and deserialization
//! - PKCE code verifier and S256 challenge generation
//! - OAuth authorization URL construction
//! - Token types and expiry checking
//! - Request transformation (header stripping, header setting with template
//!   resolution, query parameter appending)

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod config;

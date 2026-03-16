//! stoat-io: I/O layer for stoat
//!
//! This crate interprets effects produced by stoat-core and executes them
//! against real I/O: HTTP servers, HTTP clients, filesystem, browser, terminal.
//!
//! Responsibilities:
//! - axum HTTP server (local proxy listener)
//! - reqwest HTTP client (upstream API forwarding, token exchange/refresh)
//! - Browser launch for OAuth authorization flow
//! - Token file reading and writing
//! - Terminal I/O for paste-mode code receipt
//! - Local HTTP listener for OAuth callback receipt

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::path::{Path, PathBuf};

pub mod browser;
pub mod callback;
pub mod paste;
pub mod token_exchange;
pub mod token_manager;
pub mod token_refresh;
pub mod token_store;

/// Read a file from disk and return its contents as a string.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the file cannot be read.
pub fn read_file(path: &Path) -> Result<String, std::io::Error> {
    std::fs::read_to_string(path)
}

/// Return the current user's home directory, if it can be determined.
///
/// This is a thin wrapper around [`dirs::home_dir`] to keep platform-specific
/// logic in the I/O layer.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

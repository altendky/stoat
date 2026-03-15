// stoat-io: I/O layer for stoat
//
// This crate interprets effects produced by stoat-core and executes them
// against real I/O: HTTP servers, HTTP clients, filesystem, browser, terminal.
//
// Responsibilities:
// - axum HTTP server (local proxy listener)
// - reqwest HTTP client (upstream API forwarding, token exchange/refresh)
// - Browser launch for OAuth authorization flow
// - Token file reading and writing
// - Terminal I/O for paste-mode code receipt
// - Local HTTP listener for OAuth callback receipt

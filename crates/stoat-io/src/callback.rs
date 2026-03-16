//! OAuth callback receipt via a temporary local HTTP listener.
//!
//! Starts a one-shot HTTP server on localhost that waits for the OAuth
//! provider to redirect the user's browser back with an authorization code.
//! The API is split into two phases so the caller can open the browser
//! between starting the listener and waiting for the callback.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use serde::Deserialize;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Query parameters received on the OAuth callback.
#[derive(Debug, Deserialize)]
struct CallbackParams {
    /// The authorization code from the provider.
    code: Option<String>,
    /// The state parameter for CSRF verification.
    state: Option<String>,
    /// Error code if the authorization was denied or failed.
    error: Option<String>,
    /// Human-readable error description.
    error_description: Option<String>,
}

/// Result of receiving the OAuth callback.
#[derive(Debug)]
pub struct CallbackResult {
    /// The authorization code.
    pub code: String,
    /// The state parameter (for CSRF verification by the caller).
    pub state: Option<String>,
}

/// Error from the callback listener.
#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    /// The OAuth provider returned an error instead of an authorization code.
    #[error("authorization denied: {error}: {description}")]
    AuthorizationDenied {
        /// The error code.
        error: String,
        /// Human-readable description.
        description: String,
    },

    /// The callback did not include an authorization code.
    #[error("callback did not include an authorization code")]
    MissingCode,

    /// Failed to bind the listener.
    #[error("failed to bind callback listener on {address}")]
    Bind {
        /// The address we tried to bind.
        address: SocketAddr,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The callback channel was closed unexpectedly.
    #[error("callback channel closed unexpectedly")]
    ChannelClosed,

    /// The server encountered an error.
    #[error("callback server error")]
    Server(#[source] std::io::Error),
}

/// A handle to a running callback listener.
///
/// Created by [`start_callback_listener`]. Call [`wait`](Self::wait) to
/// block until the OAuth callback is received.
pub struct CallbackListener {
    /// The port the server is listening on.
    port: u16,
    /// Receives the callback result from the handler.
    rx: oneshot::Receiver<Result<CallbackResult, CallbackError>>,
    /// Handle to the spawned server task.
    server_handle: JoinHandle<Result<(), std::io::Error>>,
}

impl CallbackListener {
    /// The actual port the server is listening on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Wait for the OAuth callback to be received.
    ///
    /// Consumes the listener and returns the callback result. The server
    /// is shut down after the callback is received.
    ///
    /// # Errors
    ///
    /// Returns a [`CallbackError`] if the authorization was denied, the
    /// callback was malformed, or the server encountered an error.
    pub async fn wait(self) -> Result<CallbackResult, CallbackError> {
        let result = self.rx.await.map_err(|_| CallbackError::ChannelClosed)??;

        // Wait for graceful server shutdown.
        if let Ok(server_result) = self.server_handle.await {
            server_result.map_err(CallbackError::Server)?;
        }

        Ok(result)
    }
}

/// Start a temporary HTTP server on localhost to receive the OAuth callback.
///
/// The server listens on `127.0.0.1:{port}` (use port 0 for automatic
/// assignment). It handles exactly one request and then shuts down.
///
/// Returns a [`CallbackListener`] that can be used to retrieve the actual
/// port and wait for the callback. Open the browser after calling this
/// function but before calling [`CallbackListener::wait`].
///
/// # Errors
///
/// Returns [`CallbackError::Bind`] if the listener cannot be bound.
pub async fn start_callback_listener(port: u16) -> Result<CallbackListener, CallbackError> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| CallbackError::Bind {
            address: addr,
            source,
        })?;
    let actual_port = listener
        .local_addr()
        .map_err(|source| CallbackError::Bind {
            address: addr,
            source,
        })?
        .port();

    let (tx, rx) = oneshot::channel::<Result<CallbackResult, CallbackError>>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

    let app = axum::Router::new().route(
        "/",
        get({
            let tx = Arc::clone(&tx);
            let shutdown_tx = Arc::clone(&shutdown_tx);
            move |Query(params): Query<CallbackParams>| async move {
                let result = parse_callback(params);
                let is_ok = result.is_ok();

                // Send the result back to the caller.
                let result_sender = tx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(sender) = result_sender {
                    let _sent = sender.send(result);
                }

                // Signal the server to shut down.
                let shutdown_sender = shutdown_tx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(sender) = shutdown_sender {
                    let _sent = sender.send(());
                }

                if is_ok {
                    Html(
                        "<html><body><h1>Authorization successful</h1>\
                         <p>You can close this tab and return to the terminal.</p>\
                         </body></html>"
                            .to_owned(),
                    )
                } else {
                    Html(
                        "<html><body><h1>Authorization failed</h1>\
                         <p>Check the terminal for details.</p>\
                         </body></html>"
                            .to_owned(),
                    )
                }
            }
        }),
    );

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
    });

    Ok(CallbackListener {
        port: actual_port,
        rx,
        server_handle,
    })
}

fn parse_callback(params: CallbackParams) -> Result<CallbackResult, CallbackError> {
    if let Some(error) = params.error {
        return Err(CallbackError::AuthorizationDenied {
            error,
            description: params.error_description.unwrap_or_default(),
        });
    }

    let code = params.code.ok_or(CallbackError::MissingCode)?;

    Ok(CallbackResult {
        code,
        state: params.state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_callback unit tests ---

    #[test]
    fn parse_callback_success_with_state() {
        let params = CallbackParams {
            code: Some("auth-code-123".into()),
            state: Some("state-xyz".into()),
            error: None,
            error_description: None,
        };

        let result = parse_callback(params).unwrap();
        assert_eq!(result.code, "auth-code-123");
        assert_eq!(result.state.as_deref(), Some("state-xyz"));
    }

    #[test]
    fn parse_callback_success_without_state() {
        let params = CallbackParams {
            code: Some("auth-code-456".into()),
            state: None,
            error: None,
            error_description: None,
        };

        let result = parse_callback(params).unwrap();
        assert_eq!(result.code, "auth-code-456");
        assert!(result.state.is_none());
    }

    #[test]
    fn parse_callback_error_with_description() {
        let params = CallbackParams {
            code: None,
            state: None,
            error: Some("access_denied".into()),
            error_description: Some("User denied access".into()),
        };

        let result = parse_callback(params);
        assert!(matches!(
            result,
            Err(CallbackError::AuthorizationDenied {
                ref error,
                ref description,
            }) if error == "access_denied" && description == "User denied access"
        ));
    }

    #[test]
    fn parse_callback_error_without_description() {
        let params = CallbackParams {
            code: None,
            state: None,
            error: Some("server_error".into()),
            error_description: None,
        };

        let result = parse_callback(params);
        assert!(matches!(
            result,
            Err(CallbackError::AuthorizationDenied {
                ref error,
                ref description,
            }) if error == "server_error" && description.is_empty()
        ));
    }

    #[test]
    fn parse_callback_error_takes_precedence_over_code() {
        let params = CallbackParams {
            code: Some("code".into()),
            state: None,
            error: Some("access_denied".into()),
            error_description: Some("denied".into()),
        };

        let result = parse_callback(params);
        assert!(matches!(
            result,
            Err(CallbackError::AuthorizationDenied { .. })
        ));
    }

    #[test]
    fn parse_callback_missing_code() {
        let params = CallbackParams {
            code: None,
            state: Some("state".into()),
            error: None,
            error_description: None,
        };

        let result = parse_callback(params);
        assert!(matches!(result, Err(CallbackError::MissingCode)));
    }

    // --- Integration tests for the full callback listener ---

    #[tokio::test]
    async fn listener_receives_authorization_code() {
        let listener = start_callback_listener(0).await.unwrap();
        let port = listener.port();

        // Send a callback request with a valid code and state.
        let client = reqwest::Client::new();
        let response = client
            .get(format!(
                "http://127.0.0.1:{port}/?code=abc123&state=test-state"
            ))
            .send()
            .await
            .unwrap();

        // The server should respond with a success page.
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = response.text().await.unwrap();
        assert!(body.contains("Authorization successful"));

        // The listener should return the code and state.
        let result = listener.wait().await.unwrap();
        assert_eq!(result.code, "abc123");
        assert_eq!(result.state.as_deref(), Some("test-state"));
    }

    #[tokio::test]
    async fn listener_receives_code_without_state() {
        let listener = start_callback_listener(0).await.unwrap();
        let port = listener.port();

        let client = reqwest::Client::new();
        client
            .get(format!("http://127.0.0.1:{port}/?code=abc123"))
            .send()
            .await
            .unwrap();

        let result = listener.wait().await.unwrap();
        assert_eq!(result.code, "abc123");
        assert!(result.state.is_none());
    }

    #[tokio::test]
    async fn listener_receives_error() {
        let listener = start_callback_listener(0).await.unwrap();
        let port = listener.port();

        let client = reqwest::Client::new();
        let response = client
            .get(format!(
                "http://127.0.0.1:{port}/?error=access_denied\
                 &error_description=User+denied+access"
            ))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = response.text().await.unwrap();
        assert!(body.contains("Authorization failed"));

        let result = listener.wait().await;
        assert!(matches!(
            result,
            Err(CallbackError::AuthorizationDenied { .. })
        ));
    }

    #[tokio::test]
    async fn listener_missing_code() {
        let listener = start_callback_listener(0).await.unwrap();
        let port = listener.port();

        let client = reqwest::Client::new();
        client
            .get(format!("http://127.0.0.1:{port}/?state=test-state"))
            .send()
            .await
            .unwrap();

        let result = listener.wait().await;
        assert!(matches!(result, Err(CallbackError::MissingCode)));
    }

    #[tokio::test]
    async fn listener_port_is_nonzero() {
        let listener = start_callback_listener(0).await.unwrap();
        assert_ne!(listener.port(), 0, "should have been assigned a real port");

        // Clean up: send a request so the server shuts down.
        let client = reqwest::Client::new();
        let _response = client
            .get(format!("http://127.0.0.1:{}/?code=x", listener.port()))
            .send()
            .await;
        let _result = listener.wait().await;
    }
}

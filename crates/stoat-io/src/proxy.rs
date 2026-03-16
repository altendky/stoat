//! HTTP proxy server.
//!
//! Implements the reverse proxy that accepts requests from downstream clients,
//! transforms them using the configured translation rules, and forwards them
//! to the upstream API. Responses are streamed back without buffering.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use http::HeaderMap;
use http::StatusCode;
use http::header::{CONNECTION, HOST, TRANSFER_ENCODING};
use reqwest::Client;
use stoat_core::config::{Config, Translation};
use stoat_core::transform;
use tokio::net::TcpListener;
use tracing::{debug, error, info};

/// Shared state for the proxy server.
pub struct ProxyState {
    /// Parsed configuration.
    config: Config,

    /// Resolved path to the token file.
    token_path: PathBuf,

    /// HTTP client for upstream requests.
    client: Client,
}

/// Error returned when the proxy server cannot start.
#[derive(Debug, thiserror::Error)]
pub enum ProxyStartError {
    /// Failed to bind the TCP listener.
    #[error("failed to bind to {address}")]
    Bind {
        address: std::net::SocketAddr,
        #[source]
        source: std::io::Error,
    },

    /// Failed to get the local address after binding.
    #[error("failed to get local address")]
    LocalAddr(#[source] std::io::Error),

    /// Failed to flush stdout after reporting the port.
    #[error("failed to flush stdout")]
    Flush(#[source] std::io::Error),

    /// Server error while serving requests.
    #[error("server error")]
    Serve(#[source] std::io::Error),
}

/// Start the proxy server.
///
/// Binds to the configured address, prints `port=<N>` to stdout, and
/// serves requests until the process is terminated.
///
/// # Errors
///
/// Returns a [`ProxyStartError`] if the server cannot bind to the configured
/// address or encounters a fatal error while serving.
pub async fn start(config: Config, token_path: PathBuf) -> Result<(), ProxyStartError> {
    let listen_addr = config.listen_address();

    let state = Arc::new(ProxyState {
        config,
        token_path,
        client: Client::new(),
    });

    let app = Router::new().fallback(proxy_handler).with_state(state);

    let listener =
        TcpListener::bind(listen_addr)
            .await
            .map_err(|source| ProxyStartError::Bind {
                address: listen_addr,
                source,
            })?;

    let local_addr = listener.local_addr().map_err(ProxyStartError::LocalAddr)?;

    // Print the port to stdout for scripting.  All subsequent output goes
    // to stderr via tracing.
    println!("port={}", local_addr.port());
    std::io::stdout().flush().map_err(ProxyStartError::Flush)?;

    info!(address = %local_addr, "proxy server listening");

    axum::serve(listener, app)
        .await
        .map_err(ProxyStartError::Serve)?;

    Ok(())
}

/// Handle an incoming proxy request.
///
/// Delegates to [`handle_proxy_request`] and converts any errors into
/// HTTP 502 Bad Gateway responses.
async fn proxy_handler(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    match handle_proxy_request(&state, request).await {
        Ok(response) => response,
        Err(e) => {
            error!(error = %e, "proxy request failed");
            (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response()
        }
    }
}

/// Internal handler that returns a `Result` for ergonomic error handling.
///
/// 1. Refreshes the OAuth token if needed.
/// 2. Applies configured request transformations.
/// 3. Forwards the request to the upstream API.
/// 4. Streams the response back to the client.
async fn handle_proxy_request(
    state: &ProxyState,
    request: Request,
) -> Result<Response, ProxyRequestError> {
    let now_unix = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    // Load (and refresh if needed) the OAuth token.
    let token = crate::token_manager::load_valid_token(
        &state.token_path,
        &state.config.oauth.token_url,
        &state.config.oauth.client_id,
        now_unix,
    )
    .await
    .map_err(ProxyRequestError::Token)?;

    let (parts, body) = request.into_parts();

    info!(method = %parts.method, path = parts.uri.path(), "incoming request");

    // Build the upstream URL.
    let query_params = state
        .config
        .translation
        .as_ref()
        .and_then(|t| t.query_params.as_ref());

    let upstream_url = transform::build_upstream_url(
        &state.config.upstream.base_url,
        parts.uri.path(),
        parts.uri.query(),
        query_params,
    );

    debug!(upstream_url = %upstream_url, "forwarding to upstream");

    // Build the outgoing request headers.
    let upstream_headers = build_upstream_headers(
        &parts.headers,
        state.config.translation.as_ref(),
        &token.access_token,
    );

    // Forward the request to the upstream API, streaming the body.
    let upstream_response = state
        .client
        .request(parts.method, upstream_url.as_str())
        .headers(upstream_headers)
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await
        .map_err(ProxyRequestError::Upstream)?;

    // Stream the response back to the client.
    let status = upstream_response.status();
    info!(status = %status, "upstream response");

    let response_headers = filter_response_headers(upstream_response.headers());
    let body_stream = upstream_response.bytes_stream();

    let mut response = Response::new(Body::from_stream(body_stream));
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;

    Ok(response)
}

/// Build the outgoing request headers by applying transformation rules.
///
/// 1. Copies headers from the incoming request, skipping hop-by-hop headers
///    and any headers in the `strip_headers` list.
/// 2. Adds headers from `set_headers` with template variables resolved.
fn build_upstream_headers(
    incoming: &HeaderMap,
    translation: Option<&Translation>,
    access_token: &str,
) -> reqwest::header::HeaderMap {
    let strip_headers = translation.and_then(|t| t.strip_headers.as_deref());

    let mut headers = reqwest::header::HeaderMap::new();

    // Copy incoming headers, filtering out hop-by-hop and stripped headers.
    for (name, value) in incoming {
        // Skip hop-by-hop headers that should not be forwarded.
        if name == HOST || name == CONNECTION || name == TRANSFER_ENCODING {
            continue;
        }

        // Skip headers configured for stripping.
        if let Some(strip) = strip_headers
            && transform::should_strip_header(name.as_str(), strip)
        {
            continue;
        }

        headers.append(name.clone(), value.clone());
    }

    // Add configured set-headers with template resolution.
    if let Some(set_headers) = translation.and_then(|t| t.set_headers.as_ref()) {
        let resolved = transform::resolve_set_headers(set_headers, access_token);
        for (name, value) in &resolved {
            if let (Ok(header_name), Ok(header_value)) = (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                headers.insert(header_name, header_value);
            }
        }
    }

    headers
}

/// Filter response headers, removing hop-by-hop headers.
fn filter_response_headers(incoming: &reqwest::header::HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in incoming {
        if name == CONNECTION || name == TRANSFER_ENCODING {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }
    headers
}

/// Error from a proxy request.
#[derive(Debug, thiserror::Error)]
enum ProxyRequestError {
    /// Failed to load or refresh the token.
    #[error("token error: {0}")]
    Token(#[source] crate::token_manager::TokenManagerError),

    /// Failed to forward the request to upstream.
    #[error("upstream request failed: {0}")]
    Upstream(#[source] reqwest::Error),
}

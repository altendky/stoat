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
struct ProxyState {
    /// Parsed configuration.
    config: Config,

    /// Resolved path to the token file.
    token_path: PathBuf,

    /// HTTP client for upstream requests.
    client: Client,
}

/// A proxy server that has been bound to a TCP listener but not yet started.
///
/// Created by [`bind`]. Call [`BoundProxy::serve`] to start serving, or
/// use the individual fields in tests to interact with the server directly.
pub struct BoundProxy {
    /// The axum application router.
    app: Router,
    /// The bound TCP listener.
    listener: TcpListener,
    /// The local address the server is bound to.
    local_addr: std::net::SocketAddr,
}

impl BoundProxy {
    /// The local address the server is bound to.
    #[must_use]
    pub const fn local_addr(&self) -> std::net::SocketAddr {
        self.local_addr
    }

    /// Serve requests until the server is shut down.
    ///
    /// # Errors
    ///
    /// Returns a [`ProxyStartError::Serve`] if the server encounters a fatal error.
    pub async fn serve(self) -> Result<(), ProxyStartError> {
        axum::serve(self.listener, self.app)
            .await
            .map_err(ProxyStartError::Serve)
    }
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

/// Bind the proxy server to its configured address.
///
/// Returns a [`BoundProxy`] that is ready to serve. This is separated from
/// [`start`] so that tests can inspect the bound address and interact with
/// the server without needing to capture stdout.
///
/// # Errors
///
/// Returns a [`ProxyStartError`] if the server cannot bind to the configured
/// address.
pub async fn bind(config: Config, token_path: PathBuf) -> Result<BoundProxy, ProxyStartError> {
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

    Ok(BoundProxy {
        app,
        listener,
        local_addr,
    })
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
    let bound = bind(config, token_path).await?;

    // Print the port to stdout for scripting.  All subsequent output goes
    // to stderr via tracing.
    println!("port={}", bound.local_addr.port());
    std::io::stdout().flush().map_err(ProxyStartError::Flush)?;

    info!(address = %bound.local_addr, "proxy server listening");

    bound.serve().await
}

/// Handle an incoming proxy request.
///
/// Delegates to [`handle_proxy_request`] and converts any errors into
/// HTTP 502 Bad Gateway responses.
async fn proxy_handler(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    match handle_proxy_request(&state, request).await {
        Ok(response) => response,
        Err(e) => {
            error!(error = %format_error_chain(&e), "proxy request failed");
            (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response()
        }
    }
}

/// Format an error and its entire source chain for logging.
fn format_error_chain(error: &dyn std::error::Error) -> String {
    use std::fmt::Write as _;
    let mut msg = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let _ = write!(msg, ": {cause}");
        source = cause.source();
    }
    msg
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
        state.config.oauth.token_format(),
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
pub(crate) fn build_upstream_headers(
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
pub(crate) fn filter_response_headers(incoming: &reqwest::header::HeaderMap) -> HeaderMap {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::extract::Request as AxumRequest;
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use stoat_core::config::Config;
    use stoat_core::token::StoredToken;
    use tokio::sync::Mutex;

    use super::*;
    use crate::token_store;

    fn test_config(upstream_port: u16, token_port: u16) -> Config {
        let toml = format!(
            r#"
listen = "127.0.0.1:0"

[upstream]
base_url = "http://127.0.0.1:{upstream_port}"

[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "http://127.0.0.1:{token_port}/token"
client_id = "test-client-id"
scopes = ["scope1"]
redirect_uri = "http://localhost:8080/callback"

[translation]
strip_headers = ["x-api-key"]

[translation.set_headers]
Authorization = "Bearer {{access_token}}"

[translation.query_params]
beta = "true"
"#
        );
        Config::from_toml(&toml).unwrap()
    }

    fn valid_token() -> StoredToken {
        StoredToken {
            access_token: "test-access-token".into(),
            refresh_token: "test-refresh-token".into(),
            expires_at: u64::MAX,
        }
    }

    /// Start a mock upstream server and return the port.
    async fn start_upstream(handler: axum::Router) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            axum::serve(listener, handler).await.unwrap();
        });
        (port, handle)
    }

    /// Set up a complete proxy test environment.
    ///
    /// Returns `(proxy_port, _handles)` where `proxy_port` is the port
    /// the proxy is listening on.
    async fn setup_proxy(
        upstream_handler: axum::Router,
    ) -> (
        u16,
        tempfile::TempDir,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<Result<(), ProxyStartError>>,
    ) {
        let (upstream_port, upstream_handle) = start_upstream(upstream_handler).await;

        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("tokens.json");
        token_store::write_token(&token_path, &valid_token()).unwrap();

        // Use a dummy token port since the token won't need refreshing.
        let config = test_config(upstream_port, 1);

        let bound = bind(config, token_path).await.unwrap();
        let proxy_port = bound.local_addr().port();

        let proxy_handle = tokio::spawn(async move { bound.serve().await });

        (proxy_port, dir, upstream_handle, proxy_handle)
    }

    // --- Unit tests for build_upstream_headers ---

    #[test]
    fn build_upstream_headers_strips_hop_by_hop() {
        let mut incoming = HeaderMap::new();
        incoming.insert(HOST, "example.com".parse().unwrap());
        incoming.insert(CONNECTION, "keep-alive".parse().unwrap());
        incoming.insert(TRANSFER_ENCODING, "chunked".parse().unwrap());
        incoming.insert("x-custom", "value".parse().unwrap());

        let result = build_upstream_headers(&incoming, None, "token");

        assert!(result.get(HOST.as_str()).is_none());
        assert!(result.get(CONNECTION.as_str()).is_none());
        assert!(result.get(TRANSFER_ENCODING.as_str()).is_none());
        assert_eq!(result.get("x-custom").unwrap(), "value");
    }

    #[test]
    fn build_upstream_headers_strips_configured_headers() {
        let mut incoming = HeaderMap::new();
        incoming.insert("x-api-key", "secret".parse().unwrap());
        incoming.insert("x-keep", "value".parse().unwrap());

        let translation = stoat_core::config::Translation {
            strip_headers: Some(vec!["x-api-key".into()]),
            set_headers: None,
            query_params: None,
        };

        let result = build_upstream_headers(&incoming, Some(&translation), "token");

        assert!(result.get("x-api-key").is_none());
        assert_eq!(result.get("x-keep").unwrap(), "value");
    }

    #[test]
    fn build_upstream_headers_sets_headers_with_template() {
        let incoming = HeaderMap::new();

        let mut set_headers = HashMap::new();
        set_headers.insert(
            "Authorization".to_owned(),
            "Bearer {access_token}".to_owned(),
        );

        let translation = stoat_core::config::Translation {
            strip_headers: None,
            set_headers: Some(set_headers),
            query_params: None,
        };

        let result = build_upstream_headers(&incoming, Some(&translation), "my-token");

        assert_eq!(result.get("authorization").unwrap(), "Bearer my-token");
    }

    #[test]
    fn filter_response_headers_removes_hop_by_hop() {
        let mut incoming = reqwest::header::HeaderMap::new();
        incoming.insert(CONNECTION, "keep-alive".parse().unwrap());
        incoming.insert(TRANSFER_ENCODING, "chunked".parse().unwrap());
        incoming.insert("content-type", "application/json".parse().unwrap());

        let result = filter_response_headers(&incoming);

        assert!(result.get(CONNECTION).is_none());
        assert!(result.get(TRANSFER_ENCODING).is_none());
        assert_eq!(result.get("content-type").unwrap(), "application/json");
    }

    // --- Integration tests for the full proxy ---

    #[tokio::test]
    async fn proxy_forwards_request_to_upstream() {
        let received = Arc::new(Mutex::new(None));
        let received_clone = Arc::clone(&received);

        let upstream = axum::Router::new().route(
            "/v1/chat",
            post(move |req: AxumRequest| {
                let received = Arc::clone(&received_clone);
                async move {
                    let method = req.method().to_string();
                    let path = req.uri().path().to_string();
                    let query = req.uri().query().map(String::from);
                    let auth = req
                        .headers()
                        .get("authorization")
                        .map(|v| v.to_str().unwrap().to_owned());
                    *received.lock().await = Some((method, path, query, auth));
                    (axum::http::StatusCode::OK, "upstream response body").into_response()
                }
            }),
        );

        let (proxy_port, _dir, _uh, _ph) = setup_proxy(upstream).await;

        let client = reqwest::Client::new();
        let response = client
            .post(format!("http://127.0.0.1:{proxy_port}/v1/chat"))
            .header("x-custom", "forwarded")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = response.text().await.unwrap();
        assert_eq!(body, "upstream response body");

        let (method, path, query, auth) = received.lock().await.take().unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/v1/chat");
        // The config adds beta=true as a query param.
        assert!(
            query.as_deref().unwrap_or("").contains("beta=true"),
            "expected beta=true in query, got: {query:?}"
        );
        assert_eq!(auth.as_deref(), Some("Bearer test-access-token"));
    }

    #[tokio::test]
    async fn proxy_strips_configured_headers() {
        let received_headers = Arc::new(Mutex::new(None));
        let received_clone = Arc::clone(&received_headers);

        let upstream = axum::Router::new().fallback(move |req: AxumRequest| {
            let received = Arc::clone(&received_clone);
            async move {
                let has_api_key = req.headers().contains_key("x-api-key");
                let has_custom = req.headers().contains_key("x-custom");
                *received.lock().await = Some((has_api_key, has_custom));
                "ok"
            }
        });

        let (proxy_port, _dir, _uh, _ph) = setup_proxy(upstream).await;

        let client = reqwest::Client::new();
        client
            .get(format!("http://127.0.0.1:{proxy_port}/test"))
            .header("x-api-key", "should-be-stripped")
            .header("x-custom", "should-be-forwarded")
            .send()
            .await
            .unwrap();

        let (has_api_key, has_custom) = received_headers.lock().await.take().unwrap();
        assert!(!has_api_key, "x-api-key should have been stripped");
        assert!(has_custom, "x-custom should have been forwarded");
    }

    #[tokio::test]
    async fn proxy_returns_upstream_status_code() {
        let upstream = axum::Router::new().fallback(|| async {
            (axum::http::StatusCode::NOT_FOUND, "not found").into_response()
        });

        let (proxy_port, _dir, _uh, _ph) = setup_proxy(upstream).await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{proxy_port}/missing"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn proxy_forwards_response_headers() {
        let upstream = axum::Router::new().fallback(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                "{\"ok\": true}",
            )
                .into_response()
        });

        let (proxy_port, _dir, _uh, _ph) = setup_proxy(upstream).await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{proxy_port}/test"))
            .send()
            .await
            .unwrap();

        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn proxy_returns_502_when_token_file_missing() {
        // Set up upstream (won't be reached).
        let upstream = axum::Router::new().fallback(|| async { "ok" });
        let (upstream_port, _handle) = start_upstream(upstream).await;

        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("nonexistent.json");

        let config = test_config(upstream_port, 1);
        let bound = bind(config, token_path).await.unwrap();
        let proxy_port = bound.local_addr().port();

        tokio::spawn(async move { bound.serve().await });

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{proxy_port}/test"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
        let body = response.text().await.unwrap();
        assert!(body.contains("proxy error"), "body: {body}");
    }

    #[tokio::test]
    async fn proxy_streams_response_body() {
        let upstream = axum::Router::new().route(
            "/stream",
            get(|| async {
                let body = "chunk1chunk2chunk3";
                body.into_response()
            }),
        );

        let (proxy_port, _dir, _uh, _ph) = setup_proxy(upstream).await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{proxy_port}/stream"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = response.text().await.unwrap();
        assert_eq!(body, "chunk1chunk2chunk3");
    }

    #[tokio::test]
    async fn proxy_appends_configured_query_params() {
        let received_query = Arc::new(Mutex::new(None));
        let received_clone = Arc::clone(&received_query);

        let upstream = axum::Router::new().fallback(move |req: AxumRequest| {
            let received = Arc::clone(&received_clone);
            async move {
                *received.lock().await = req.uri().query().map(String::from);
                "ok"
            }
        });

        let (proxy_port, _dir, _uh, _ph) = setup_proxy(upstream).await;

        let client = reqwest::Client::new();
        client
            .get(format!("http://127.0.0.1:{proxy_port}/test?existing=yes"))
            .send()
            .await
            .unwrap();

        let query = received_query.lock().await.take().unwrap();
        assert!(
            query.contains("existing=yes"),
            "should preserve original query: {query}"
        );
        assert!(
            query.contains("beta=true"),
            "should append configured params: {query}"
        );
    }

    #[tokio::test]
    async fn proxy_refreshes_expired_token() {
        // Mock upstream that captures the authorization header.
        let received_auth = Arc::new(Mutex::new(None));
        let received_clone = Arc::clone(&received_auth);

        let upstream = axum::Router::new().fallback(move |req: AxumRequest| {
            let received = Arc::clone(&received_clone);
            async move {
                let auth = req
                    .headers()
                    .get("authorization")
                    .map(|v| v.to_str().unwrap().to_owned());
                *received.lock().await = auth;
                "ok"
            }
        });

        let (upstream_port, _upstream_handle) = start_upstream(upstream).await;

        // Mock token refresh endpoint.
        let refresh_app = axum::Router::new().route(
            "/token",
            post(|| async {
                axum::Json(serde_json::json!({
                    "access_token": "refreshed-access-token",
                    "refresh_token": "refreshed-refresh-token",
                    "expires_in": 7200,
                    "token_type": "Bearer"
                }))
            }),
        );

        let refresh_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let token_port = refresh_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(refresh_listener, refresh_app).await.unwrap();
        });

        // Write an expired token.
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("tokens.json");
        let expired_token = StoredToken {
            access_token: "expired-access".into(),
            refresh_token: "old-refresh".into(),
            expires_at: 100, // Long expired.
        };
        token_store::write_token(&token_path, &expired_token).unwrap();

        let config = test_config(upstream_port, token_port);
        let bound = bind(config, token_path).await.unwrap();
        let proxy_port = bound.local_addr().port();
        tokio::spawn(async move { bound.serve().await });

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{proxy_port}/test"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);

        // The proxy should have used the refreshed token.
        let auth = received_auth.lock().await.take().unwrap();
        assert_eq!(auth, "Bearer refreshed-access-token");
    }
}

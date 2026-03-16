//! OAuth token refresh via HTTP POST.
//!
//! Sends the refresh token to the token endpoint with
//! `grant_type=refresh_token` and returns the token response.

use stoat_core::config::TokenFormat;
use stoat_core::oauth::TokenRefreshParams;
use stoat_core::token::TokenResponse;

/// Error from the token refresh request.
#[derive(Debug, thiserror::Error)]
pub enum TokenRefreshError {
    /// The HTTP request to the token endpoint failed.
    #[error("token refresh request failed")]
    Request(#[source] reqwest::Error),

    /// The token endpoint returned a non-success status.
    #[error("token endpoint returned status {status}: {body}")]
    Status {
        /// The HTTP status code.
        status: reqwest::StatusCode,
        /// The response body.
        body: String,
    },

    /// The response body could not be deserialized.
    #[error("failed to parse token refresh response")]
    Parse(#[source] reqwest::Error),
}

/// Refresh an access token by posting the refresh token to the token endpoint.
///
/// Sends a `grant_type=refresh_token` request and returns the raw
/// [`TokenResponse`]. The caller is responsible for converting this to a
/// [`StoredToken`] (using [`TokenResponse::into_refreshed_token`]) and
/// persisting it.
///
/// # Errors
///
/// Returns a [`TokenRefreshError`] if the request fails, the endpoint returns
/// a non-success status, or the response cannot be parsed.
pub async fn refresh_token(
    params: &TokenRefreshParams,
) -> Result<TokenResponse, TokenRefreshError> {
    let client = reqwest::Client::new();

    let request = client.post(params.token_url.as_str());
    let request = match params.token_format {
        TokenFormat::Form => request.form(&params.form_params()),
        TokenFormat::Json => request.json(&params.json_body()),
    };

    let response = request.send().await.map_err(TokenRefreshError::Request)?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<could not read body>".to_owned());
        return Err(TokenRefreshError::Status { status, body });
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(TokenRefreshError::Parse)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::extract::Form;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use stoat_core::config::TokenFormat;
    use stoat_core::oauth::TokenRefreshParams;
    use tokio::sync::Mutex;
    use url::Url;

    use super::*;

    async fn start_mock_server(handler: axum::Router) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            axum::serve(listener, handler).await.unwrap();
        });
        (port, handle)
    }

    fn test_refresh_params(port: u16) -> TokenRefreshParams {
        TokenRefreshParams {
            token_url: Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap(),
            refresh_token: "old-refresh-token".into(),
            client_id: "test-client".into(),
            token_format: TokenFormat::Form,
        }
    }

    #[tokio::test]
    async fn refresh_token_success() {
        let app = axum::Router::new().route(
            "/token",
            post(|| async {
                axum::Json(serde_json::json!({
                    "access_token": "new-access-token",
                    "refresh_token": "new-refresh-token",
                    "expires_in": 7200,
                    "token_type": "Bearer"
                }))
            }),
        );

        let (port, _handle) = start_mock_server(app).await;
        let params = test_refresh_params(port);

        let response = refresh_token(&params).await.unwrap();
        assert_eq!(response.access_token, "new-access-token");
        assert_eq!(response.refresh_token.as_deref(), Some("new-refresh-token"));
        assert_eq!(response.expires_in, Some(7200));
    }

    #[tokio::test]
    async fn refresh_token_sends_correct_form_params() {
        let received = Arc::new(Mutex::new(HashMap::new()));
        let received_clone = Arc::clone(&received);

        let app = axum::Router::new().route(
            "/token",
            post(move |Form(form): Form<HashMap<String, String>>| {
                let received = Arc::clone(&received_clone);
                async move {
                    *received.lock().await = form;
                    axum::Json(serde_json::json!({
                        "access_token": "tok",
                        "expires_in": 3600
                    }))
                }
            }),
        );

        let (port, _handle) = start_mock_server(app).await;
        let params = test_refresh_params(port);

        refresh_token(&params).await.unwrap();

        let form = received.lock().await.clone();
        assert_eq!(form.get("grant_type").unwrap(), "refresh_token");
        assert_eq!(form.get("refresh_token").unwrap(), "old-refresh-token");
        assert_eq!(form.get("client_id").unwrap(), "test-client");
    }

    #[tokio::test]
    async fn refresh_token_error_status() {
        let app = axum::Router::new().route(
            "/token",
            post(|| async {
                (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "{\"error\": \"invalid_grant\"}",
                )
                    .into_response()
            }),
        );

        let (port, _handle) = start_mock_server(app).await;
        let params = test_refresh_params(port);

        let result = refresh_token(&params).await;
        let err = result.unwrap_err();
        assert!(
            matches!(err, TokenRefreshError::Status { status, ref body }
                if status == reqwest::StatusCode::UNAUTHORIZED
                && body.contains("invalid_grant")),
            "expected Status error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn refresh_token_invalid_json() {
        let app = axum::Router::new().route("/token", post(|| async { "not valid json" }));

        let (port, _handle) = start_mock_server(app).await;
        let params = test_refresh_params(port);

        let result = refresh_token(&params).await;
        assert!(
            matches!(result, Err(TokenRefreshError::Parse(_))),
            "expected Parse error, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn refresh_token_connection_refused() {
        let params = TokenRefreshParams {
            token_url: Url::parse("http://127.0.0.1:1/token").unwrap(),
            refresh_token: "refresh".into(),
            client_id: "client".into(),
            token_format: TokenFormat::Form,
        };

        let result = refresh_token(&params).await;
        assert!(
            matches!(result, Err(TokenRefreshError::Request(_))),
            "expected Request error, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn refresh_token_without_new_refresh_token() {
        let app = axum::Router::new().route(
            "/token",
            post(|| async {
                axum::Json(serde_json::json!({
                    "access_token": "new-access",
                    "expires_in": 3600,
                    "token_type": "Bearer"
                }))
            }),
        );

        let (port, _handle) = start_mock_server(app).await;
        let params = test_refresh_params(port);

        let response = refresh_token(&params).await.unwrap();
        assert_eq!(response.access_token, "new-access");
        assert!(
            response.refresh_token.is_none(),
            "server did not return a new refresh token"
        );
    }

    #[tokio::test]
    async fn refresh_token_sends_json_body() {
        let received = Arc::new(Mutex::new(HashMap::new()));
        let received_clone = Arc::clone(&received);

        let app = axum::Router::new().route(
            "/token",
            post(
                move |axum::Json(json): axum::Json<HashMap<String, String>>| {
                    let received = Arc::clone(&received_clone);
                    async move {
                        *received.lock().await = json;
                        axum::Json(serde_json::json!({
                            "access_token": "tok",
                            "expires_in": 3600
                        }))
                    }
                },
            ),
        );

        let (port, _handle) = start_mock_server(app).await;
        let mut params = test_refresh_params(port);
        params.token_format = TokenFormat::Json;

        refresh_token(&params).await.unwrap();

        let json = received.lock().await.clone();
        assert_eq!(json.get("grant_type").unwrap(), "refresh_token");
        assert_eq!(json.get("refresh_token").unwrap(), "old-refresh-token");
        assert_eq!(json.get("client_id").unwrap(), "test-client");
    }
}

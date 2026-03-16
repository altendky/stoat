//! OAuth token exchange via HTTP POST.
//!
//! Sends the authorization code to the token endpoint and returns the
//! token response.

use stoat_core::oauth::TokenExchangeParams;
use stoat_core::token::TokenResponse;

/// Error from the token exchange.
#[derive(Debug, thiserror::Error)]
pub enum TokenExchangeError {
    /// The HTTP request to the token endpoint failed.
    #[error("token exchange request failed")]
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
    #[error("failed to parse token response")]
    Parse(#[source] reqwest::Error),
}

/// Exchange an authorization code for tokens by posting to the token endpoint.
///
/// # Errors
///
/// Returns a [`TokenExchangeError`] if the request fails, the endpoint returns
/// a non-success status, or the response cannot be parsed.
pub async fn exchange_code(
    params: &TokenExchangeParams,
) -> Result<TokenResponse, TokenExchangeError> {
    let client = reqwest::Client::new();

    let response = client
        .post(params.token_url.as_str())
        .form(&params.form_params())
        .send()
        .await
        .map_err(TokenExchangeError::Request)?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<could not read body>".to_owned());
        return Err(TokenExchangeError::Status { status, body });
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(TokenExchangeError::Parse)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::extract::Form;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use stoat_core::oauth::TokenExchangeParams;
    use tokio::sync::Mutex;
    use url::Url;

    use super::*;

    /// Start a mock token endpoint that returns a valid token response.
    async fn start_mock_token_server(handler: axum::Router) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            axum::serve(listener, handler).await.unwrap();
        });
        (port, handle)
    }

    fn test_exchange_params(port: u16) -> TokenExchangeParams {
        TokenExchangeParams {
            token_url: Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap(),
            code: "auth-code-123".into(),
            redirect_uri: Url::parse("http://localhost:8080/callback").unwrap(),
            client_id: "test-client".into(),
            code_verifier: Some("test-verifier".into()),
        }
    }

    #[tokio::test]
    async fn exchange_code_success() {
        let app = axum::Router::new().route(
            "/token",
            post(|| async {
                axum::Json(serde_json::json!({
                    "access_token": "test-access-token",
                    "refresh_token": "test-refresh-token",
                    "expires_in": 3600,
                    "token_type": "Bearer"
                }))
            }),
        );

        let (port, _handle) = start_mock_token_server(app).await;
        let params = test_exchange_params(port);

        let response = exchange_code(&params).await.unwrap();
        assert_eq!(response.access_token, "test-access-token");
        assert_eq!(
            response.refresh_token.as_deref(),
            Some("test-refresh-token")
        );
        assert_eq!(response.expires_in, Some(3600));
    }

    #[tokio::test]
    async fn exchange_code_receives_form_params() {
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
                        "refresh_token": "ref",
                        "expires_in": 3600,
                        "token_type": "Bearer"
                    }))
                }
            }),
        );

        let (port, _handle) = start_mock_token_server(app).await;
        let params = test_exchange_params(port);

        exchange_code(&params).await.unwrap();

        let form = received.lock().await.clone();
        assert_eq!(form.get("grant_type").unwrap(), "authorization_code");
        assert_eq!(form.get("code").unwrap(), "auth-code-123");
        assert_eq!(form.get("client_id").unwrap(), "test-client");
        assert_eq!(form.get("code_verifier").unwrap(), "test-verifier");
        assert_eq!(
            form.get("redirect_uri").unwrap(),
            "http://localhost:8080/callback"
        );
    }

    #[tokio::test]
    async fn exchange_code_error_status() {
        let app = axum::Router::new().route(
            "/token",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    "{\"error\": \"invalid_grant\"}",
                )
                    .into_response()
            }),
        );

        let (port, _handle) = start_mock_token_server(app).await;
        let params = test_exchange_params(port);

        let result = exchange_code(&params).await;
        let err = result.unwrap_err();
        assert!(
            matches!(err, TokenExchangeError::Status { status, ref body }
                if status == reqwest::StatusCode::BAD_REQUEST
                && body.contains("invalid_grant")),
            "expected Status error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn exchange_code_invalid_json() {
        let app = axum::Router::new().route("/token", post(|| async { "this is not json" }));

        let (port, _handle) = start_mock_token_server(app).await;
        let params = test_exchange_params(port);

        let result = exchange_code(&params).await;
        assert!(
            matches!(result, Err(TokenExchangeError::Parse(_))),
            "expected Parse error, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn exchange_code_connection_refused() {
        // Use a port that nothing is listening on.
        let params = TokenExchangeParams {
            token_url: Url::parse("http://127.0.0.1:1/token").unwrap(),
            code: "code".into(),
            redirect_uri: Url::parse("http://localhost/callback").unwrap(),
            client_id: "client".into(),
            code_verifier: None,
        };

        let result = exchange_code(&params).await;
        assert!(
            matches!(result, Err(TokenExchangeError::Request(_))),
            "expected Request error, got: {result:?}"
        );
    }
}

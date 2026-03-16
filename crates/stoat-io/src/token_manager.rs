//! Token lifecycle management.
//!
//! Provides a high-level interface that combines token file loading, expiry
//! checking, refresh, and persistence. This is the primary token interface
//! for the proxy server — it loads the token from disk, refreshes if needed,
//! writes the updated token back, and returns a valid access token.

use std::path::Path;

use stoat_core::config::TokenFormat;
use stoat_core::oauth::TokenRefreshParams;
use stoat_core::token::{DEFAULT_REFRESH_MARGIN_SECS, StoredToken};
use url::Url;

use crate::token_refresh::{self, TokenRefreshError};
use crate::token_store::{self, TokenStoreError};

/// Error from the token manager.
#[derive(Debug, thiserror::Error)]
pub enum TokenManagerError {
    /// Failed to load the token file.
    #[error("failed to load token")]
    Load(#[source] TokenStoreError),

    /// Failed to refresh the token.
    #[error("failed to refresh token")]
    Refresh(#[source] TokenRefreshError),

    /// Failed to save the refreshed token.
    #[error("failed to save refreshed token")]
    Save(#[source] TokenStoreError),
}

/// Load a valid access token, refreshing if necessary.
///
/// This is the main entry point for token management during proxying:
///
/// 1. Reads the stored token from `token_path`.
/// 2. Checks whether the token needs refreshing (expired or within the
///    default margin of [`DEFAULT_REFRESH_MARGIN_SECS`]).
/// 3. If refresh is needed, POSTs to `token_url` with the refresh token,
///    writes the updated token back to disk, and returns the new token.
/// 4. If no refresh is needed, returns the existing token as-is.
///
/// `now_unix` is the current Unix timestamp in seconds.
///
/// # Errors
///
/// Returns a [`TokenManagerError`] if the token file cannot be read, the
/// refresh request fails, or the updated token cannot be written.
pub async fn load_valid_token(
    token_path: &Path,
    token_url: &Url,
    client_id: &str,
    token_format: TokenFormat,
    now_unix: u64,
) -> Result<StoredToken, TokenManagerError> {
    let token = token_store::read_token(token_path).map_err(TokenManagerError::Load)?;

    if !token.needs_refresh(now_unix, DEFAULT_REFRESH_MARGIN_SECS) {
        return Ok(token);
    }

    let refresh_params = TokenRefreshParams {
        token_url: token_url.clone(),
        refresh_token: token.refresh_token.clone(),
        client_id: client_id.to_owned(),
        token_format,
    };

    let response = token_refresh::refresh_token(&refresh_params)
        .await
        .map_err(TokenManagerError::Refresh)?;

    let refreshed = response.into_refreshed_token(&token.refresh_token, now_unix);

    token_store::write_token(token_path, &refreshed).map_err(TokenManagerError::Save)?;

    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use axum::routing::post;
    use stoat_core::config::TokenFormat;
    use stoat_core::token::StoredToken;
    use url::Url;

    use super::*;
    use crate::token_store;

    /// Start a mock token endpoint that returns a valid refresh response.
    async fn start_mock_refresh_server(
        handler: axum::Router,
    ) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            axum::serve(listener, handler).await.unwrap();
        });
        (port, handle)
    }

    fn valid_token(expires_at: u64) -> StoredToken {
        StoredToken {
            access_token: "valid-access".into(),
            refresh_token: "valid-refresh".into(),
            expires_at,
        }
    }

    #[tokio::test]
    async fn load_valid_token_not_expired() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("tokens.json");

        // Token expires far in the future.
        let token = valid_token(u64::MAX);
        token_store::write_token(&token_path, &token).unwrap();

        let token_url = Url::parse("http://127.0.0.1:1/token").unwrap();
        let result = load_valid_token(
            &token_path,
            &token_url,
            "client-id",
            TokenFormat::Form,
            1_000,
        )
        .await;

        let loaded = result.unwrap();
        assert_eq!(loaded.access_token, "valid-access");
        assert_eq!(loaded.refresh_token, "valid-refresh");
    }

    #[tokio::test]
    async fn load_valid_token_expired_triggers_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("tokens.json");

        // Token is already expired (expires_at = 100, now = 1000).
        let token = valid_token(100);
        token_store::write_token(&token_path, &token).unwrap();

        // Mock refresh endpoint.
        let app = axum::Router::new().route(
            "/token",
            post(|| async {
                axum::Json(serde_json::json!({
                    "access_token": "refreshed-access",
                    "refresh_token": "refreshed-refresh",
                    "expires_in": 7200,
                    "token_type": "Bearer"
                }))
            }),
        );

        let (port, _handle) = start_mock_refresh_server(app).await;
        let token_url = Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap();

        let loaded = load_valid_token(
            &token_path,
            &token_url,
            "client-id",
            TokenFormat::Form,
            1_000,
        )
        .await
        .unwrap();

        assert_eq!(loaded.access_token, "refreshed-access");
        assert_eq!(loaded.refresh_token, "refreshed-refresh");
        assert_eq!(loaded.expires_at, 1_000 + 7200);

        // The refreshed token should also have been persisted to disk.
        let on_disk = token_store::read_token(&token_path).unwrap();
        assert_eq!(on_disk.access_token, "refreshed-access");
    }

    #[tokio::test]
    async fn load_valid_token_within_margin_triggers_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("tokens.json");

        // Token expires at 1050, now = 1000, margin = 60 → 1000+60 >= 1050 → needs refresh.
        let token = valid_token(1050);
        token_store::write_token(&token_path, &token).unwrap();

        let app = axum::Router::new().route(
            "/token",
            post(|| async {
                axum::Json(serde_json::json!({
                    "access_token": "margin-refreshed",
                    "refresh_token": "margin-refresh-tok",
                    "expires_in": 3600
                }))
            }),
        );

        let (port, _handle) = start_mock_refresh_server(app).await;
        let token_url = Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap();

        let loaded = load_valid_token(
            &token_path,
            &token_url,
            "client-id",
            TokenFormat::Form,
            1_000,
        )
        .await
        .unwrap();

        assert_eq!(loaded.access_token, "margin-refreshed");
    }

    #[tokio::test]
    async fn load_valid_token_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("nonexistent.json");
        let token_url = Url::parse("http://127.0.0.1:1/token").unwrap();

        let result = load_valid_token(
            &token_path,
            &token_url,
            "client-id",
            TokenFormat::Form,
            1_000,
        )
        .await;
        assert!(
            matches!(result, Err(TokenManagerError::Load(_))),
            "expected Load error, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn load_valid_token_refresh_fails() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("tokens.json");

        // Expired token → will attempt refresh.
        let token = valid_token(100);
        token_store::write_token(&token_path, &token).unwrap();

        // Point to a non-existent server so refresh fails.
        let token_url = Url::parse("http://127.0.0.1:1/token").unwrap();

        let result = load_valid_token(
            &token_path,
            &token_url,
            "client-id",
            TokenFormat::Form,
            1_000,
        )
        .await;
        assert!(
            matches!(result, Err(TokenManagerError::Refresh(_))),
            "expected Refresh error, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn load_valid_token_preserves_refresh_token_if_not_returned() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("tokens.json");

        let token = valid_token(100);
        token_store::write_token(&token_path, &token).unwrap();

        // Mock server returns a new access token but no refresh token.
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

        let (port, _handle) = start_mock_refresh_server(app).await;
        let token_url = Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap();

        let loaded = load_valid_token(
            &token_path,
            &token_url,
            "client-id",
            TokenFormat::Form,
            1_000,
        )
        .await
        .unwrap();

        assert_eq!(loaded.access_token, "new-access");
        assert_eq!(
            loaded.refresh_token, "valid-refresh",
            "should preserve original refresh token"
        );
    }
}

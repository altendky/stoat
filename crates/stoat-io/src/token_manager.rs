//! Token lifecycle management.
//!
//! Provides a high-level interface that combines token file loading, expiry
//! checking, refresh, and persistence. This is the primary token interface
//! for the proxy server — it loads the token from disk, refreshes if needed,
//! writes the updated token back, and returns a valid access token.

use std::path::Path;

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
    };

    let response = token_refresh::refresh_token(&refresh_params)
        .await
        .map_err(TokenManagerError::Refresh)?;

    let refreshed = response.into_refreshed_token(&token.refresh_token, now_unix);

    token_store::write_token(token_path, &refreshed).map_err(TokenManagerError::Save)?;

    Ok(refreshed)
}

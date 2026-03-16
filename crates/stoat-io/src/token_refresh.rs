//! OAuth token refresh via HTTP POST.
//!
//! Sends the refresh token to the token endpoint with
//! `grant_type=refresh_token` and returns the token response.

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

    let response = client
        .post(params.token_url.as_str())
        .form(&params.form_params())
        .send()
        .await
        .map_err(TokenRefreshError::Request)?;

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

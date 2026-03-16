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

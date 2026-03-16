//! Browser launch for the OAuth authorization flow.

use url::Url;

/// Open the given URL in the user's default web browser.
///
/// # Errors
///
/// Returns an error if the browser could not be launched.
pub fn open_browser(url: &Url) -> Result<(), OpenBrowserError> {
    open::that(url.as_str()).map_err(|source| OpenBrowserError {
        url: url.clone(),
        source,
    })
}

/// Error returned when the browser could not be launched.
#[derive(Debug, thiserror::Error)]
#[error("failed to open browser for {url}")]
pub struct OpenBrowserError {
    url: Url,
    source: std::io::Error,
}

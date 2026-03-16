//! Configuration types for stoat.
//!
//! These types represent the TOML config file that drives stoat's behavior.
//! All fields that are not required have sensible defaults.

use std::collections::HashMap;

use serde::Deserialize;

/// Default listen address: localhost with automatic port assignment.
const DEFAULT_LISTEN: &str = "127.0.0.1:0";

/// Default token file path (tilde-expanded at runtime by the I/O layer).
const DEFAULT_TOKEN_FILE: &str = "~/.config/stoat/tokens.json";

/// Top-level stoat configuration, deserialized from a TOML file.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Config {
    /// Address and port to listen on. Use port 0 for automatic assignment.
    listen: Option<String>,

    /// Path to the token storage file. Tilde (`~`) is expanded at runtime.
    token_file: Option<String>,

    /// Upstream API to proxy requests to.
    pub upstream: Upstream,

    /// OAuth PKCE configuration.
    pub oauth: OAuth,

    /// Request transformations applied to every proxied request.
    pub translation: Option<Translation>,
}

impl Config {
    /// Deserialize a [`Config`] from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns a [`toml::de::Error`] if the input is not valid TOML or does
    /// not match the expected schema.
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// The listen address, falling back to the default if not configured.
    #[must_use]
    pub fn listen_address(&self) -> &str {
        self.listen.as_deref().unwrap_or(DEFAULT_LISTEN)
    }

    /// The token file path, falling back to the default if not configured.
    #[must_use]
    pub fn token_file_path(&self) -> &str {
        self.token_file.as_deref().unwrap_or(DEFAULT_TOKEN_FILE)
    }
}

/// Upstream API target.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Upstream {
    /// Base URL of the upstream API.
    pub base_url: String,
}

/// OAuth PKCE configuration.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct OAuth {
    /// OAuth authorization endpoint.
    pub authorize_url: String,

    /// OAuth token exchange and refresh endpoint.
    pub token_url: String,

    /// OAuth client identifier.
    pub client_id: String,

    /// OAuth scopes to request.
    pub scopes: Vec<String>,

    /// Enable PKCE (S256). Defaults to `true` when not specified.
    pkce: Option<bool>,

    /// Redirect URI for the OAuth flow.
    pub redirect_uri: String,
}

impl OAuth {
    /// Whether PKCE is enabled, defaulting to `true`.
    #[must_use]
    pub fn pkce_enabled(&self) -> bool {
        self.pkce.unwrap_or(true)
    }
}

/// Request transformations applied to every proxied request.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Translation {
    /// Headers to remove from the incoming request before forwarding.
    pub strip_headers: Option<Vec<String>>,

    /// Headers to set on the outgoing request. Values support the
    /// `{access_token}` template variable.
    pub set_headers: Option<HashMap<String, String>>,

    /// Query parameters to append to every outgoing request URL.
    pub query_params: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full example config from `docs/src/project/configuration.md`.
    const FULL_CONFIG: &str = r#"
listen = "127.0.0.1:8080"
token_file = "~/.config/stoat/tokens.json"

[upstream]
base_url = "https://api.example.com"

[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "https://example.com/oauth/token"
client_id = "your-client-id"
scopes = ["scope1", "scope2"]
pkce = true
redirect_uri = "https://example.com/oauth/callback"

[translation]
strip_headers = ["x-api-key"]

[translation.query_params]
beta = "true"

[translation.set_headers]
Authorization = "Bearer {access_token}"
"#;

    /// Only the required fields — everything optional is omitted.
    const MINIMAL_CONFIG: &str = r#"
[upstream]
base_url = "https://api.example.com"

[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "https://example.com/oauth/token"
client_id = "your-client-id"
scopes = ["scope1"]
redirect_uri = "https://example.com/oauth/callback"
"#;

    #[test]
    fn deserialize_full_config() {
        let config = Config::from_toml(FULL_CONFIG).unwrap();

        assert_eq!(config.listen_address(), "127.0.0.1:8080");
        assert_eq!(config.token_file_path(), "~/.config/stoat/tokens.json");
        assert_eq!(config.upstream.base_url, "https://api.example.com");
        assert_eq!(
            config.oauth.authorize_url,
            "https://example.com/oauth/authorize"
        );
        assert_eq!(config.oauth.token_url, "https://example.com/oauth/token");
        assert_eq!(config.oauth.client_id, "your-client-id");
        assert_eq!(config.oauth.scopes, vec!["scope1", "scope2"]);
        assert!(config.oauth.pkce_enabled());
        assert_eq!(
            config.oauth.redirect_uri,
            "https://example.com/oauth/callback"
        );

        let translation = config.translation.unwrap();
        assert_eq!(
            translation.strip_headers.unwrap(),
            vec!["x-api-key".to_owned()]
        );

        let set_headers = translation.set_headers.unwrap();
        assert_eq!(
            set_headers.get("Authorization").unwrap(),
            "Bearer {access_token}"
        );

        let query_params = translation.query_params.unwrap();
        assert_eq!(query_params.get("beta").unwrap(), "true");
    }

    #[test]
    fn deserialize_minimal_config() {
        let config = Config::from_toml(MINIMAL_CONFIG).unwrap();

        assert_eq!(config.upstream.base_url, "https://api.example.com");
        assert_eq!(config.oauth.client_id, "your-client-id");
        assert_eq!(config.oauth.scopes, vec!["scope1"]);
        assert!(config.translation.is_none());
    }

    #[test]
    fn default_listen_address() {
        let config = Config::from_toml(MINIMAL_CONFIG).unwrap();
        assert_eq!(config.listen_address(), "127.0.0.1:0");
    }

    #[test]
    fn custom_listen_address() {
        let toml = format!("listen = \"0.0.0.0:9999\"\n{MINIMAL_CONFIG}");
        let config = Config::from_toml(&toml).unwrap();
        assert_eq!(config.listen_address(), "0.0.0.0:9999");
    }

    #[test]
    fn default_token_file() {
        let config = Config::from_toml(MINIMAL_CONFIG).unwrap();
        assert_eq!(config.token_file_path(), "~/.config/stoat/tokens.json");
    }

    #[test]
    fn custom_token_file() {
        let toml = format!("token_file = \"/tmp/tokens.json\"\n{MINIMAL_CONFIG}");
        let config = Config::from_toml(&toml).unwrap();
        assert_eq!(config.token_file_path(), "/tmp/tokens.json");
    }

    #[test]
    fn pkce_defaults_to_true() {
        let config = Config::from_toml(MINIMAL_CONFIG).unwrap();
        assert!(config.oauth.pkce_enabled());
    }

    #[test]
    fn pkce_explicit_false() {
        let toml = MINIMAL_CONFIG.replace(
            "redirect_uri = \"https://example.com/oauth/callback\"",
            "redirect_uri = \"https://example.com/oauth/callback\"\npkce = false",
        );
        let config = Config::from_toml(&toml).unwrap();
        assert!(!config.oauth.pkce_enabled());
    }

    #[test]
    fn missing_upstream_is_error() {
        let toml = r#"
[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "https://example.com/oauth/token"
client_id = "your-client-id"
scopes = ["scope1"]
redirect_uri = "https://example.com/oauth/callback"
"#;
        let err = Config::from_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("upstream"),
            "error should mention upstream: {msg}"
        );
    }

    #[test]
    fn missing_oauth_is_error() {
        let toml = r#"
[upstream]
base_url = "https://api.example.com"
"#;
        let err = Config::from_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("oauth"), "error should mention oauth: {msg}");
    }

    #[test]
    fn missing_oauth_client_id_is_error() {
        let toml = r#"
[upstream]
base_url = "https://api.example.com"

[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "https://example.com/oauth/token"
scopes = ["scope1"]
redirect_uri = "https://example.com/oauth/callback"
"#;
        let err = Config::from_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("client_id"),
            "error should mention client_id: {msg}"
        );
    }

    #[test]
    fn missing_oauth_scopes_is_error() {
        let toml = r#"
[upstream]
base_url = "https://api.example.com"

[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "https://example.com/oauth/token"
client_id = "your-client-id"
redirect_uri = "https://example.com/oauth/callback"
"#;
        let err = Config::from_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("scopes"), "error should mention scopes: {msg}");
    }

    #[test]
    fn empty_scopes_is_valid() {
        let toml = MINIMAL_CONFIG.replace("scopes = [\"scope1\"]", "scopes = []");
        let config = Config::from_toml(&toml).unwrap();
        assert!(config.oauth.scopes.is_empty());
    }

    #[test]
    fn translation_all_optional_fields() {
        let toml = format!("{MINIMAL_CONFIG}\n[translation]\n");
        let config = Config::from_toml(&toml).unwrap();
        let translation = config.translation.unwrap();
        assert!(translation.strip_headers.is_none());
        assert!(translation.set_headers.is_none());
        assert!(translation.query_params.is_none());
    }

    #[test]
    fn translation_strip_headers_only() {
        let toml = format!(
            "{MINIMAL_CONFIG}\n[translation]\nstrip_headers = [\"x-api-key\", \"x-custom\"]\n"
        );
        let config = Config::from_toml(&toml).unwrap();
        let translation = config.translation.unwrap();
        assert_eq!(
            translation.strip_headers.unwrap(),
            vec!["x-api-key".to_owned(), "x-custom".to_owned()]
        );
        assert!(translation.set_headers.is_none());
        assert!(translation.query_params.is_none());
    }

    #[test]
    fn translation_set_headers_only() {
        let toml = format!(
            "{MINIMAL_CONFIG}\n[translation.set_headers]\nAuthorization = \"Bearer {{access_token}}\"\n"
        );
        let config = Config::from_toml(&toml).unwrap();
        let translation = config.translation.unwrap();
        assert!(translation.strip_headers.is_none());
        let set_headers = translation.set_headers.unwrap();
        assert_eq!(
            set_headers.get("Authorization").unwrap(),
            "Bearer {access_token}"
        );
    }

    #[test]
    fn translation_query_params_only() {
        let toml = format!("{MINIMAL_CONFIG}\n[translation.query_params]\nbeta = \"true\"\n");
        let config = Config::from_toml(&toml).unwrap();
        let translation = config.translation.unwrap();
        assert!(translation.strip_headers.is_none());
        assert!(translation.set_headers.is_none());
        let query_params = translation.query_params.unwrap();
        assert_eq!(query_params.get("beta").unwrap(), "true");
    }
}

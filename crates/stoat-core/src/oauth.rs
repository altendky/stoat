//! OAuth authorization URL construction and helpers.
//!
//! Builds the authorization URL with the required query parameters for an
//! OAuth 2.0 PKCE authorization code flow. This module is pure — it only
//! manipulates URLs and strings.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use url::Url;

use crate::config::{OAuth, TokenFormat};
use crate::pkce::PkceChallenge;

/// Parameters for constructing an authorization URL.
///
/// Separating these from the [`OAuth`] config allows the caller to control
/// the state parameter and whether PKCE parameters are included.
pub struct AuthorizationRequest<'a> {
    /// The OAuth configuration from the config file.
    pub oauth: &'a OAuth,
    /// The PKCE challenge (included only when PKCE is enabled).
    pub pkce: Option<&'a PkceChallenge>,
    /// An opaque state value for CSRF protection.
    pub state: &'a str,
}

/// Build the authorization URL for an OAuth 2.0 authorization code flow.
///
/// The returned URL includes the following query parameters:
/// - `response_type=code`
/// - `client_id`
/// - `redirect_uri`
/// - `scope` (space-separated)
/// - `state`
/// - `code_challenge` and `code_challenge_method=S256` (when PKCE is provided)
#[must_use]
pub fn build_authorization_url(request: &AuthorizationRequest<'_>) -> Url {
    let mut url = request.oauth.authorize_url.clone();

    {
        let mut params = url.query_pairs_mut();
        params.append_pair("response_type", "code");
        params.append_pair("client_id", &request.oauth.client_id);
        params.append_pair("redirect_uri", request.oauth.redirect_uri.as_str());
        params.append_pair("scope", &request.oauth.scopes.join(" "));
        params.append_pair("state", request.state);

        if let Some(pkce) = request.pkce {
            params.append_pair("code_challenge", pkce.challenge());
            params.append_pair("code_challenge_method", "S256");
        }
    }

    url
}

/// Parameters for the token exchange request body.
///
/// This is a pure data structure — the actual HTTP POST is performed by the
/// I/O layer.
#[derive(Debug, Clone)]
pub struct TokenExchangeParams {
    /// The token endpoint URL.
    pub token_url: Url,
    /// The authorization code received from the authorization server.
    pub code: String,
    /// The redirect URI (must match the one used in the authorization request).
    pub redirect_uri: Url,
    /// The OAuth client identifier.
    pub client_id: String,
    /// The PKCE code verifier (if PKCE was used).
    pub code_verifier: Option<String>,
    /// The body format for the token endpoint request.
    pub token_format: TokenFormat,
}

/// Generate a random state parameter for CSRF protection.
///
/// Returns 16 random bytes encoded as base64url (no padding), producing
/// a 22-character string.
pub fn generate_state(rng: &mut impl rand::Rng) -> String {
    let mut bytes = [0u8; 16];
    rng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Check whether a redirect URI points to a localhost address.
///
/// Returns `true` if the URL's host is `localhost`, `127.0.0.1`, or `[::1]`.
/// This determines whether the callback can be received via a local HTTP
/// listener rather than paste mode.
#[must_use]
pub fn is_localhost_redirect(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// Extract the port from a redirect URI, if present.
///
/// Returns `None` if no explicit port is set (the URL uses the scheme's
/// default port).
#[must_use]
pub fn redirect_port(url: &Url) -> Option<u16> {
    url.port()
}

impl TokenExchangeParams {
    /// Build the form parameters for the token exchange POST body.
    #[must_use]
    pub fn form_params(&self) -> Vec<(&str, &str)> {
        let mut params = vec![
            ("grant_type", "authorization_code"),
            ("code", &self.code),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("client_id", &self.client_id),
        ];

        if let Some(verifier) = &self.code_verifier {
            params.push(("code_verifier", verifier));
        }

        params
    }

    /// Build a JSON-serializable map for the token exchange POST body.
    #[must_use]
    pub fn json_body(&self) -> HashMap<&str, &str> {
        let mut map = HashMap::new();
        map.insert("grant_type", "authorization_code");
        map.insert("code", &self.code);
        map.insert("redirect_uri", self.redirect_uri.as_str());
        map.insert("client_id", &self.client_id);

        if let Some(verifier) = &self.code_verifier {
            map.insert("code_verifier", verifier);
        }

        map
    }
}

/// Parameters for the token refresh request body.
///
/// This is a pure data structure — the actual HTTP POST is performed by the
/// I/O layer. Corresponds to an OAuth 2.0 `grant_type=refresh_token` request.
#[derive(Debug, Clone)]
pub struct TokenRefreshParams {
    /// The token endpoint URL.
    pub token_url: Url,
    /// The refresh token to exchange for a new access token.
    pub refresh_token: String,
    /// The OAuth client identifier.
    pub client_id: String,
    /// The body format for the token endpoint request.
    pub token_format: TokenFormat,
}

impl TokenRefreshParams {
    /// Build the form parameters for the token refresh POST body.
    #[must_use]
    pub fn form_params(&self) -> Vec<(&str, &str)> {
        vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", &self.refresh_token),
            ("client_id", &self.client_id),
        ]
    }

    /// Build a JSON-serializable map for the token refresh POST body.
    #[must_use]
    pub fn json_body(&self) -> HashMap<&str, &str> {
        let mut map = HashMap::new();
        map.insert("grant_type", "refresh_token");
        map.insert("refresh_token", &self.refresh_token);
        map.insert("client_id", &self.client_id);
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, TokenFormat};

    const MINIMAL_CONFIG: &str = r#"
[upstream]
base_url = "https://api.example.com"

[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "https://example.com/oauth/token"
client_id = "test-client-id"
scopes = ["scope1", "scope2"]
redirect_uri = "https://example.com/oauth/callback"
"#;

    fn test_config() -> Config {
        Config::from_toml(MINIMAL_CONFIG).unwrap()
    }

    #[test]
    fn authorization_url_without_pkce() {
        let config = test_config();
        let request = AuthorizationRequest {
            oauth: &config.oauth,
            pkce: None,
            state: "test-state",
        };

        let url = build_authorization_url(&request);

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("example.com"));
        assert_eq!(url.path(), "/oauth/authorize");

        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert!(pairs.contains(&("response_type".into(), "code".into())));
        assert!(pairs.contains(&("client_id".into(), "test-client-id".into())));
        assert!(pairs.contains(&(
            "redirect_uri".into(),
            "https://example.com/oauth/callback".into()
        )));
        assert!(pairs.contains(&("scope".into(), "scope1 scope2".into())));
        assert!(pairs.contains(&("state".into(), "test-state".into())));
        assert!(
            !pairs.iter().any(|(k, _)| k == "code_challenge"),
            "should not include code_challenge without PKCE"
        );
    }

    #[test]
    fn authorization_url_with_pkce() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let pkce = PkceChallenge::generate(&mut rng);

        let config = test_config();
        let request = AuthorizationRequest {
            oauth: &config.oauth,
            pkce: Some(&pkce),
            state: "test-state",
        };

        let url = build_authorization_url(&request);
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();

        assert!(pairs.contains(&("code_challenge".into(), pkce.challenge().to_owned())));
        assert!(pairs.contains(&("code_challenge_method".into(), "S256".into())));
    }

    #[test]
    fn authorization_url_empty_scopes() {
        let toml = MINIMAL_CONFIG.replace("scopes = [\"scope1\", \"scope2\"]", "scopes = []");
        let config = Config::from_toml(&toml).unwrap();
        let request = AuthorizationRequest {
            oauth: &config.oauth,
            pkce: None,
            state: "s",
        };

        let url = build_authorization_url(&request);
        assert!(
            url.query_pairs()
                .into_owned()
                .any(|p| p == ("scope".into(), String::new()))
        );
    }

    #[test]
    fn token_exchange_params_with_pkce() {
        let params = TokenExchangeParams {
            token_url: Url::parse("https://example.com/oauth/token").unwrap(),
            code: "auth-code-123".into(),
            redirect_uri: Url::parse("https://example.com/oauth/callback").unwrap(),
            client_id: "test-client".into(),
            code_verifier: Some("my-verifier".into()),
            token_format: TokenFormat::Form,
        };

        let form = params.form_params();
        assert!(form.contains(&("grant_type", "authorization_code")));
        assert!(form.contains(&("code", "auth-code-123")));
        assert!(form.contains(&("redirect_uri", "https://example.com/oauth/callback")));
        assert!(form.contains(&("client_id", "test-client")));
        assert!(form.contains(&("code_verifier", "my-verifier")));
    }

    #[test]
    fn token_exchange_params_without_pkce() {
        let params = TokenExchangeParams {
            token_url: Url::parse("https://example.com/oauth/token").unwrap(),
            code: "auth-code-123".into(),
            redirect_uri: Url::parse("https://example.com/oauth/callback").unwrap(),
            client_id: "test-client".into(),
            code_verifier: None,
            token_format: TokenFormat::Form,
        };

        let form = params.form_params();
        assert!(!form.iter().any(|(k, _)| *k == "code_verifier"));
    }

    #[test]
    fn token_exchange_json_body_with_pkce() {
        let params = TokenExchangeParams {
            token_url: Url::parse("https://example.com/oauth/token").unwrap(),
            code: "auth-code-123".into(),
            redirect_uri: Url::parse("https://example.com/oauth/callback").unwrap(),
            client_id: "test-client".into(),
            code_verifier: Some("my-verifier".into()),
            token_format: TokenFormat::Json,
        };

        let body = params.json_body();
        assert_eq!(body.get("grant_type"), Some(&"authorization_code"));
        assert_eq!(body.get("code"), Some(&"auth-code-123"));
        assert_eq!(
            body.get("redirect_uri"),
            Some(&"https://example.com/oauth/callback")
        );
        assert_eq!(body.get("client_id"), Some(&"test-client"));
        assert_eq!(body.get("code_verifier"), Some(&"my-verifier"));
    }

    #[test]
    fn token_exchange_json_body_without_pkce() {
        let params = TokenExchangeParams {
            token_url: Url::parse("https://example.com/oauth/token").unwrap(),
            code: "auth-code-123".into(),
            redirect_uri: Url::parse("https://example.com/oauth/callback").unwrap(),
            client_id: "test-client".into(),
            code_verifier: None,
            token_format: TokenFormat::Json,
        };

        let body = params.json_body();
        assert!(!body.contains_key("code_verifier"));
    }

    #[test]
    fn generate_state_length() {
        let mut rng = rand::rng();
        let state = generate_state(&mut rng);
        assert_eq!(state.len(), 22, "16 random bytes → 22 base64url chars");
    }

    #[test]
    fn generate_state_deterministic() {
        use rand::SeedableRng;
        let mut rng1 = rand::rngs::StdRng::seed_from_u64(99);
        let state1 = generate_state(&mut rng1);

        let mut rng2 = rand::rngs::StdRng::seed_from_u64(99);
        let state2 = generate_state(&mut rng2);

        assert_eq!(state1, state2);
    }

    #[test]
    fn is_localhost_redirect_127_0_0_1() {
        let url = Url::parse("http://127.0.0.1:8080/callback").unwrap();
        assert!(is_localhost_redirect(&url));
    }

    #[test]
    fn is_localhost_redirect_localhost() {
        let url = Url::parse("http://localhost:9000/callback").unwrap();
        assert!(is_localhost_redirect(&url));
    }

    #[test]
    fn is_localhost_redirect_ipv6() {
        let url = Url::parse("http://[::1]:8080/callback").unwrap();
        assert!(is_localhost_redirect(&url));
    }

    #[test]
    fn is_not_localhost_redirect() {
        let url = Url::parse("https://example.com/oauth/callback").unwrap();
        assert!(!is_localhost_redirect(&url));
    }

    #[test]
    fn redirect_port_explicit() {
        let url = Url::parse("http://localhost:8080/callback").unwrap();
        assert_eq!(redirect_port(&url), Some(8080));
    }

    #[test]
    fn redirect_port_default() {
        let url = Url::parse("http://localhost/callback").unwrap();
        assert_eq!(redirect_port(&url), None);
    }

    #[test]
    fn token_refresh_params_form() {
        let params = TokenRefreshParams {
            token_url: Url::parse("https://example.com/oauth/token").unwrap(),
            refresh_token: "my-refresh-token".into(),
            client_id: "test-client".into(),
            token_format: TokenFormat::Form,
        };

        let form = params.form_params();
        assert!(form.contains(&("grant_type", "refresh_token")));
        assert!(form.contains(&("refresh_token", "my-refresh-token")));
        assert!(form.contains(&("client_id", "test-client")));
        assert_eq!(form.len(), 3);
    }

    #[test]
    fn token_refresh_json_body() {
        let params = TokenRefreshParams {
            token_url: Url::parse("https://example.com/oauth/token").unwrap(),
            refresh_token: "my-refresh-token".into(),
            client_id: "test-client".into(),
            token_format: TokenFormat::Json,
        };

        let body = params.json_body();
        assert_eq!(body.get("grant_type"), Some(&"refresh_token"));
        assert_eq!(body.get("refresh_token"), Some(&"my-refresh-token"));
        assert_eq!(body.get("client_id"), Some(&"test-client"));
        assert_eq!(body.len(), 3);
    }
}

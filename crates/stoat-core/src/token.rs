//! Token types for stoat.
//!
//! Defines the stored token format and the OAuth token endpoint response
//! format. These are pure data types with serialization support — no I/O.

use serde::{Deserialize, Serialize};

/// Default refresh margin in seconds.
///
/// The token is considered to need refreshing when the current time is
/// within this many seconds of `expires_at`. This allows proactive refresh
/// before the token actually expires, avoiding 401 errors on forwarded
/// requests.
pub const DEFAULT_REFRESH_MARGIN_SECS: u64 = 60;

/// Stored token data, persisted to the token file.
///
/// This is the format written by `stoat login` and read/updated by
/// `stoat serve`. The `expires_at` field is a Unix timestamp (seconds).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredToken {
    /// The current OAuth access token.
    pub access_token: String,
    /// The refresh token for obtaining new access tokens.
    pub refresh_token: String,
    /// Unix timestamp (seconds) when the access token expires.
    pub expires_at: u64,
}

impl StoredToken {
    /// Serialize to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialization fails (should not
    /// happen for this type).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if the input is not valid JSON or
    /// does not match the expected schema.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Check whether the access token has expired.
    ///
    /// `now_unix` is the current Unix timestamp in seconds. Returns `true`
    /// if the current time is at or past `expires_at`.
    #[must_use]
    pub const fn is_expired(&self, now_unix: u64) -> bool {
        now_unix >= self.expires_at
    }

    /// Check whether the access token needs refreshing.
    ///
    /// Returns `true` if the token is expired or will expire within
    /// `margin_secs` seconds. This allows proactive refresh before the
    /// token actually expires, avoiding 401 errors on forwarded requests.
    ///
    /// Use [`DEFAULT_REFRESH_MARGIN_SECS`] for the standard margin.
    #[must_use]
    pub const fn needs_refresh(&self, now_unix: u64, margin_secs: u64) -> bool {
        now_unix + margin_secs >= self.expires_at
    }
}

/// Response from the OAuth token endpoint.
///
/// This is the standard OAuth 2.0 token response format. The `expires_in`
/// field is seconds from now, which must be converted to an absolute
/// `expires_at` timestamp for storage.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    /// The access token issued by the authorization server.
    pub access_token: String,
    /// The refresh token (may not always be present, but required for stoat).
    pub refresh_token: Option<String>,
    /// The lifetime of the access token in seconds.
    pub expires_in: Option<u64>,
    /// The token type (typically "Bearer").
    pub token_type: Option<String>,
}

impl TokenResponse {
    /// Convert to a [`StoredToken`] using the given current time.
    ///
    /// `now_unix` is the current Unix timestamp in seconds. If `expires_in`
    /// is not present, the token is assumed to expire in 3600 seconds (1 hour).
    ///
    /// # Errors
    ///
    /// Returns an error if `refresh_token` is `None`, since stoat requires
    /// a refresh token for automatic token renewal.
    pub fn into_stored_token(self, now_unix: u64) -> Result<StoredToken, MissingRefreshToken> {
        let refresh_token = self.refresh_token.ok_or(MissingRefreshToken)?;
        let expires_in = self.expires_in.unwrap_or(3600);
        Ok(StoredToken {
            access_token: self.access_token,
            refresh_token,
            expires_at: now_unix + expires_in,
        })
    }

    /// Convert a refresh response to a [`StoredToken`], using the existing
    /// refresh token as a fallback.
    ///
    /// During a token refresh, the authorization server may or may not issue
    /// a new refresh token. If the response does not include one, the
    /// existing `fallback_refresh_token` is preserved.
    ///
    /// `now_unix` is the current Unix timestamp in seconds. If `expires_in`
    /// is not present, the token is assumed to expire in 3600 seconds (1 hour).
    #[must_use]
    pub fn into_refreshed_token(self, fallback_refresh_token: &str, now_unix: u64) -> StoredToken {
        let refresh_token = self
            .refresh_token
            .unwrap_or_else(|| fallback_refresh_token.to_owned());
        let expires_in = self.expires_in.unwrap_or(3600);
        StoredToken {
            access_token: self.access_token,
            refresh_token,
            expires_at: now_unix + expires_in,
        }
    }
}

/// Error returned when the token response does not include a refresh token.
#[derive(Debug, Clone, thiserror::Error)]
#[error("token response did not include a refresh_token")]
pub struct MissingRefreshToken;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_token_roundtrip() {
        let token = StoredToken {
            access_token: "access-123".into(),
            refresh_token: "refresh-456".into(),
            expires_at: 1_710_000_000,
        };

        let json = token.to_json().unwrap();
        let parsed = StoredToken::from_json(&json).unwrap();
        assert_eq!(token, parsed);
    }

    #[test]
    fn stored_token_json_format() {
        let token = StoredToken {
            access_token: "eyJ...".into(),
            refresh_token: "eyJ...".into(),
            expires_at: 1_710_000_000,
        };

        let json = token.to_json().unwrap();
        // Verify the JSON contains the expected fields.
        assert!(json.contains("\"access_token\""));
        assert!(json.contains("\"refresh_token\""));
        assert!(json.contains("\"expires_at\""));
    }

    #[test]
    fn token_response_into_stored_token() {
        let response = TokenResponse {
            access_token: "access-abc".into(),
            refresh_token: Some("refresh-xyz".into()),
            expires_in: Some(7200),
            token_type: Some("Bearer".into()),
        };

        let now = 1_700_000_000;
        let stored = response.into_stored_token(now).unwrap();
        assert_eq!(stored.access_token, "access-abc");
        assert_eq!(stored.refresh_token, "refresh-xyz");
        assert_eq!(stored.expires_at, now + 7200);
    }

    #[test]
    fn token_response_default_expiry() {
        let response = TokenResponse {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            expires_in: None,
            token_type: None,
        };

        let now = 1_700_000_000;
        let stored = response.into_stored_token(now).unwrap();
        assert_eq!(stored.expires_at, now + 3600, "should default to 1 hour");
    }

    #[test]
    fn token_response_missing_refresh_token() {
        let response = TokenResponse {
            access_token: "access".into(),
            refresh_token: None,
            expires_in: Some(3600),
            token_type: Some("Bearer".into()),
        };

        let result = response.into_stored_token(1_700_000_000);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "token response did not include a refresh_token"
        );
    }

    #[test]
    fn deserialize_stored_token_from_doc_example() {
        let json = r#"{
  "access_token": "eyJ...",
  "refresh_token": "eyJ...",
  "expires_at": 1710000000
}"#;
        let token = StoredToken::from_json(json).unwrap();
        assert_eq!(token.access_token, "eyJ...");
        assert_eq!(token.refresh_token, "eyJ...");
        assert_eq!(token.expires_at, 1_710_000_000);
    }

    #[test]
    fn deserialize_token_response() {
        let json = r#"{
  "access_token": "abc",
  "refresh_token": "def",
  "expires_in": 3600,
  "token_type": "Bearer"
}"#;
        let response: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.access_token, "abc");
        assert_eq!(response.refresh_token.unwrap(), "def");
        assert_eq!(response.expires_in.unwrap(), 3600);
        assert_eq!(response.token_type.unwrap(), "Bearer");
    }

    #[test]
    fn is_expired_before_expiry() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        assert!(!token.is_expired(999));
    }

    #[test]
    fn is_expired_at_expiry() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        assert!(token.is_expired(1_000));
    }

    #[test]
    fn is_expired_after_expiry() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        assert!(token.is_expired(1_001));
    }

    #[test]
    fn needs_refresh_well_before_margin() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        // 800 + 60 = 860 < 1000, no refresh needed
        assert!(!token.needs_refresh(800, 60));
    }

    #[test]
    fn needs_refresh_within_margin() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        // 950 + 60 = 1010 >= 1000, needs refresh
        assert!(token.needs_refresh(950, 60));
    }

    #[test]
    fn needs_refresh_at_boundary() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        // 940 + 60 = 1000 >= 1000, needs refresh (at exact boundary)
        assert!(token.needs_refresh(940, 60));
    }

    #[test]
    fn needs_refresh_just_before_boundary() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        // 939 + 60 = 999 < 1000, no refresh needed
        assert!(!token.needs_refresh(939, 60));
    }

    #[test]
    fn needs_refresh_with_zero_margin_same_as_expired() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        assert_eq!(token.needs_refresh(999, 0), token.is_expired(999));
        assert_eq!(token.needs_refresh(1_000, 0), token.is_expired(1_000));
        assert_eq!(token.needs_refresh(1_001, 0), token.is_expired(1_001));
    }

    #[test]
    fn needs_refresh_already_expired() {
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 1_000,
        };
        assert!(token.needs_refresh(2_000, 60));
    }

    #[test]
    fn into_refreshed_token_with_new_refresh_token() {
        let response = TokenResponse {
            access_token: "new-access".into(),
            refresh_token: Some("new-refresh".into()),
            expires_in: Some(7200),
            token_type: Some("Bearer".into()),
        };

        let stored = response.into_refreshed_token("old-refresh", 1_700_000_000);
        assert_eq!(stored.access_token, "new-access");
        assert_eq!(stored.refresh_token, "new-refresh");
        assert_eq!(stored.expires_at, 1_700_000_000 + 7200);
    }

    #[test]
    fn into_refreshed_token_preserves_old_refresh_token() {
        let response = TokenResponse {
            access_token: "new-access".into(),
            refresh_token: None,
            expires_in: Some(3600),
            token_type: Some("Bearer".into()),
        };

        let stored = response.into_refreshed_token("old-refresh", 1_700_000_000);
        assert_eq!(stored.access_token, "new-access");
        assert_eq!(
            stored.refresh_token, "old-refresh",
            "should preserve the existing refresh token when none is returned"
        );
        assert_eq!(stored.expires_at, 1_700_000_000 + 3600);
    }

    #[test]
    fn into_refreshed_token_default_expiry() {
        let response = TokenResponse {
            access_token: "new-access".into(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
        };

        let stored = response.into_refreshed_token("old-refresh", 1_700_000_000);
        assert_eq!(
            stored.expires_at,
            1_700_000_000 + 3600,
            "should default to 1 hour"
        );
    }
}

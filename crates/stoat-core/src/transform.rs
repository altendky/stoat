//! Request transformation logic.
//!
//! Pure functions for transforming proxy requests according to the configured
//! translation rules:
//!
//! - Header stripping (case-insensitive)
//! - Header setting with `{access_token}` template resolution
//! - Query parameter appending
//! - Upstream URL construction

use std::collections::HashMap;

use url::Url;

/// Resolve the `{access_token}` template variable in a header value.
///
/// Currently the only supported template variable is `{access_token}`.
#[must_use]
#[allow(clippy::literal_string_with_formatting_args)]
pub fn resolve_template(template: &str, access_token: &str) -> String {
    template.replace("{access_token}", access_token)
}

/// Check whether a header name should be stripped (case-insensitive).
#[must_use]
pub fn should_strip_header(header_name: &str, strip_headers: &[String]) -> bool {
    strip_headers
        .iter()
        .any(|h| h.eq_ignore_ascii_case(header_name))
}

/// Resolve all configured set-headers, replacing `{access_token}` in values.
///
/// Returns a list of (header-name, resolved-value) pairs.
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn resolve_set_headers(
    set_headers: &HashMap<String, String>,
    access_token: &str,
) -> Vec<(String, String)> {
    set_headers
        .iter()
        .map(|(name, template)| (name.clone(), resolve_template(template, access_token)))
        .collect()
}

/// Build the upstream URL from the base URL and request path/query.
///
/// The request path is appended to the base URL path. The incoming query
/// string is preserved, and any configured extra query parameters are
/// appended with proper percent-encoding.
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn build_upstream_url(
    base_url: &Url,
    request_path: &str,
    request_query: Option<&str>,
    extra_query_params: Option<&HashMap<String, String>>,
) -> Url {
    let mut url = base_url.clone();

    // Combine the base URL path with the request path.
    let base_path = url.path().trim_end_matches('/');
    let req_path = request_path.trim_start_matches('/');
    let combined = if req_path.is_empty() {
        base_path.to_owned()
    } else {
        format!("{base_path}/{req_path}")
    };
    url.set_path(if combined.is_empty() { "/" } else { &combined });

    // Start with the incoming request's raw query string.
    url.set_query(request_query.filter(|q| !q.is_empty()));

    // Append extra query parameters from the translation config.
    if let Some(params) = extra_query_params.filter(|p| !p.is_empty()) {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in params {
            pairs.append_pair(key, value);
        }
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- resolve_template ---

    #[test]
    fn resolve_template_replaces_access_token() {
        assert_eq!(
            resolve_template("Bearer {access_token}", "tok123"),
            "Bearer tok123",
        );
    }

    #[test]
    fn resolve_template_no_variable() {
        assert_eq!(resolve_template("static-value", "tok"), "static-value");
    }

    #[test]
    fn resolve_template_multiple_occurrences() {
        assert_eq!(
            resolve_template("{access_token}:{access_token}", "abc"),
            "abc:abc",
        );
    }

    #[test]
    fn resolve_template_empty_token() {
        assert_eq!(resolve_template("Bearer {access_token}", ""), "Bearer ");
    }

    // --- should_strip_header ---

    #[test]
    fn strip_header_case_insensitive() {
        let strip = vec!["X-Api-Key".to_owned()];
        assert!(should_strip_header("x-api-key", &strip));
        assert!(should_strip_header("X-API-KEY", &strip));
        assert!(should_strip_header("X-Api-Key", &strip));
    }

    #[test]
    fn strip_header_no_match() {
        let strip = vec!["X-Api-Key".to_owned()];
        assert!(!should_strip_header("Authorization", &strip));
    }

    #[test]
    fn strip_header_empty_list() {
        assert!(!should_strip_header("x-api-key", &[]));
    }

    #[test]
    fn strip_header_multiple_entries() {
        let strip = vec!["X-Api-Key".to_owned(), "X-Custom".to_owned()];
        assert!(should_strip_header("x-api-key", &strip));
        assert!(should_strip_header("x-custom", &strip));
        assert!(!should_strip_header("authorization", &strip));
    }

    // --- resolve_set_headers ---

    #[test]
    fn resolve_set_headers_applies_template() {
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_owned(),
            "Bearer {access_token}".to_owned(),
        );
        headers.insert("X-Custom".to_owned(), "static".to_owned());

        let resolved = resolve_set_headers(&headers, "my-token");
        let resolved_map: HashMap<_, _> = resolved.into_iter().collect();

        assert_eq!(resolved_map["Authorization"], "Bearer my-token");
        assert_eq!(resolved_map["X-Custom"], "static");
    }

    #[test]
    fn resolve_set_headers_empty() {
        let headers = HashMap::new();
        let resolved = resolve_set_headers(&headers, "tok");
        assert!(resolved.is_empty());
    }

    // --- build_upstream_url ---

    #[test]
    fn upstream_url_simple_path() {
        let base = Url::parse("https://api.example.com").unwrap();
        let url = build_upstream_url(&base, "/v1/chat", None, None);
        assert_eq!(url.as_str(), "https://api.example.com/v1/chat");
    }

    #[test]
    fn upstream_url_base_with_path() {
        let base = Url::parse("https://api.example.com/api").unwrap();
        let url = build_upstream_url(&base, "/v1/chat", None, None);
        assert_eq!(url.as_str(), "https://api.example.com/api/v1/chat");
    }

    #[test]
    fn upstream_url_preserves_query() {
        let base = Url::parse("https://api.example.com").unwrap();
        let url = build_upstream_url(&base, "/search", Some("q=hello+world"), None);
        assert_eq!(url.as_str(), "https://api.example.com/search?q=hello+world");
    }

    #[test]
    fn upstream_url_appends_extra_params() {
        let base = Url::parse("https://api.example.com").unwrap();
        let mut extra = HashMap::new();
        extra.insert("beta".to_owned(), "true".to_owned());
        let url = build_upstream_url(&base, "/v1/chat", None, Some(&extra));
        assert_eq!(url.as_str(), "https://api.example.com/v1/chat?beta=true");
    }

    #[test]
    fn upstream_url_merges_query_and_extra() {
        let base = Url::parse("https://api.example.com").unwrap();
        let mut extra = HashMap::new();
        extra.insert("beta".to_owned(), "true".to_owned());
        let url = build_upstream_url(&base, "/v1/chat", Some("model=gpt4"), Some(&extra));
        let url_str = url.as_str();
        assert!(url_str.starts_with("https://api.example.com/v1/chat?"));
        assert!(url_str.contains("model=gpt4"));
        assert!(url_str.contains("beta=true"));
    }

    #[test]
    fn upstream_url_root_path() {
        let base = Url::parse("https://api.example.com").unwrap();
        let url = build_upstream_url(&base, "/", None, None);
        assert_eq!(url.as_str(), "https://api.example.com/");
    }

    #[test]
    fn upstream_url_empty_path() {
        let base = Url::parse("https://api.example.com").unwrap();
        let url = build_upstream_url(&base, "", None, None);
        assert_eq!(url.as_str(), "https://api.example.com/");
    }

    #[test]
    fn upstream_url_trailing_slash_base() {
        let base = Url::parse("https://api.example.com/api/").unwrap();
        let url = build_upstream_url(&base, "/v1/chat", None, None);
        assert_eq!(url.as_str(), "https://api.example.com/api/v1/chat");
    }

    #[test]
    fn upstream_url_empty_query_ignored() {
        let base = Url::parse("https://api.example.com").unwrap();
        let url = build_upstream_url(&base, "/v1/chat", Some(""), None);
        assert_eq!(url.as_str(), "https://api.example.com/v1/chat");
    }

    #[test]
    fn upstream_url_empty_extra_params_ignored() {
        let base = Url::parse("https://api.example.com").unwrap();
        let extra = HashMap::new();
        let url = build_upstream_url(&base, "/v1/chat", None, Some(&extra));
        assert_eq!(url.as_str(), "https://api.example.com/v1/chat");
    }

    #[test]
    fn upstream_url_deep_path() {
        let base = Url::parse("https://api.example.com/v1").unwrap();
        let url = build_upstream_url(&base, "/a/b/c/d", None, None);
        assert_eq!(url.as_str(), "https://api.example.com/v1/a/b/c/d");
    }

    #[test]
    fn upstream_url_preserves_encoded_path() {
        let base = Url::parse("https://api.example.com").unwrap();
        let url = build_upstream_url(&base, "/path%20with%20spaces", None, None);
        assert_eq!(url.as_str(), "https://api.example.com/path%20with%20spaces");
    }

    #[test]
    fn upstream_url_extra_params_encoded() {
        let base = Url::parse("https://api.example.com").unwrap();
        let mut extra = HashMap::new();
        extra.insert("name".to_owned(), "hello world".to_owned());
        let url = build_upstream_url(&base, "/v1/chat", None, Some(&extra));
        assert!(url.as_str().contains("name=hello+world"));
    }
}

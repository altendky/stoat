# Configuration

## Config File

stoat is configured via a TOML file passed with `--config`.
The config file contains all provider-specific details -- stoat itself is generic.

### Schema

```toml
# Address and port to listen on.
# Use port 0 for automatic assignment.
listen = "127.0.0.1:0"

# Path to the token storage file.
# Tilde (~) is expanded to the user's home directory.
token_file = "~/.config/stoat/tokens.json"

# Upstream API to proxy requests to.
[upstream]
base_url = "https://api.example.com"

# OAuth PKCE configuration for the upstream API.
[oauth]
authorize_url = "https://example.com/oauth/authorize"
token_url = "https://example.com/oauth/token"
client_id = "your-client-id"
scopes = ["scope1", "scope2"]
pkce = true
redirect_uri = "https://example.com/oauth/callback"

# Request transformations applied to every proxied request.
[translation]
# Headers to remove from the incoming request before forwarding.
strip_headers = ["x-api-key"]

# Query parameters to append to every outgoing request URL.
[translation.query_params]
beta = "true"

# Headers to set on the outgoing request.
# The template variable {access_token} is replaced with the current
# OAuth access token at request time.
[translation.set_headers]
Authorization = "Bearer {access_token}"
```

### Fields

| Field | Required | Description |
| ----- | -------- | ----------- |
| `listen` | No | Address to bind. Default: `127.0.0.1:0` |
| `token_file` | No | Path to token storage. Default: `~/.config/stoat/tokens.json` |
| `upstream.base_url` | Yes | Base URL of the upstream API |
| `oauth.authorize_url` | Yes | OAuth authorization endpoint |
| `oauth.token_url` | Yes | OAuth token exchange and refresh endpoint |
| `oauth.client_id` | Yes | OAuth client identifier |
| `oauth.scopes` | Yes | OAuth scopes to request |
| `oauth.pkce` | No | Enable PKCE (S256). Default: `true` |
| `oauth.redirect_uri` | Yes | Redirect URI for the OAuth flow |
| `translation.strip_headers` | No | List of header names to remove from incoming requests |
| `translation.set_headers` | No | Map of header name to value to set on outgoing requests |
| `translation.query_params` | No | Map of query parameter name to value to append |

### Template Variables

The `set_headers` values support one template variable:

| Variable | Replaced With |
| -------- | ------------- |
| `{access_token}` | The current OAuth access token (refreshed automatically if expired) |

## Token Storage

Tokens are stored in a JSON file at the path specified by `token_file`.
The file is created by `stoat login` and updated by `stoat serve` when tokens are refreshed.

### Format

```json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ...",
  "expires_at": 1710000000
}
```

| Field | Description |
| ----- | ----------- |
| `access_token` | Current OAuth access token |
| `refresh_token` | Refresh token for obtaining new access tokens |
| `expires_at` | Unix timestamp (seconds) when the access token expires |

### Security

- File permissions are set to `0600` (owner read/write only) on creation and update.
- The token file should not be committed to version control.

## CLI

### `stoat login`

Performs the OAuth PKCE authorization code flow.

```sh
stoat login --config config.toml
```

1. Generates a PKCE code verifier and S256 challenge.
2. Opens the browser to the configured `authorize_url` with the appropriate query parameters.
3. Receives the authorization code (via terminal paste or local HTTP listener).
4. Exchanges the code for access and refresh tokens at the configured `token_url`.
5. Writes tokens to `token_file`.

### `stoat serve`

Starts the proxy server.

```sh
stoat serve --config config.toml
```

1. Loads tokens from `token_file`.
2. Binds to the configured `listen` address (port 0 triggers automatic assignment).
3. Prints `port=<N>` to stdout once listening.
4. Logs to stderr via `tracing`.
5. For each incoming request: refreshes the token if needed, applies transformations, forwards to upstream, streams the response back.

// stoat: Streaming OAuth Transformer
//
// CLI entry point that wires stoat-core and stoat-io together.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use stoat_core::config::Config;
use stoat_core::oauth::{
    AuthorizationRequest, TokenExchangeParams, build_authorization_url, generate_state,
    is_localhost_redirect, redirect_port,
};
use stoat_core::pkce::PkceChallenge;
use tracing_subscriber::EnvFilter;

/// Streaming OAuth Transformer — a config-driven local reverse proxy for
/// OAuth token lifecycle management.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform the OAuth PKCE authorization code flow.
    Login {
        /// Path to the TOML config file.
        #[arg(long)]
        config: PathBuf,
    },

    /// Start the proxy server.
    Serve {
        /// Path to the TOML config file.
        #[arg(long)]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let config_path = match &cli.command {
        Commands::Login { config } | Commands::Serve { config } => config,
    };

    let toml_str = match stoat_io::read_file(config_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "error: failed to read config file: {}",
                format_error_chain(&e)
            );
            eprintln!("hint: check that the file exists and the path is correct");
            return ExitCode::FAILURE;
        }
    };

    let config = match Config::from_toml(&toml_str) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: invalid config: {}", format_error_chain(&e));
            return ExitCode::FAILURE;
        }
    };

    match &cli.command {
        Commands::Login { .. } => run_login(&config).await,
        Commands::Serve { .. } => run_serve(config).await,
    }
}

async fn run_login(config: &Config) -> ExitCode {
    // Resolve the token file path.
    let Some(home_dir) = stoat_io::home_dir() else {
        eprintln!("error: could not determine home directory");
        return ExitCode::FAILURE;
    };
    let token_path = stoat_core::paths::expand_tilde(config.token_file_path(), &home_dir);

    // Generate PKCE challenge and state before any await points, since
    // `ThreadRng` is not `Send`.
    let (pkce, state) = {
        let mut rng = rand::rng();
        let pkce = if config.oauth.pkce_enabled() {
            Some(PkceChallenge::generate(&mut rng))
        } else {
            None
        };
        let state = generate_state(&mut rng);
        (pkce, state)
    };

    // Build authorization URL.
    let auth_url = build_authorization_url(&AuthorizationRequest {
        oauth: &config.oauth,
        pkce: pkce.as_ref(),
        state: &state,
    });

    // Receive the authorization code via callback or paste.
    let code = if is_localhost_redirect(&config.oauth.redirect_uri) {
        match receive_code_via_callback(&auth_url, &config.oauth.redirect_uri, &state).await {
            Ok(code) => code,
            Err(code) => return code,
        }
    } else {
        match receive_code_via_paste(&auth_url) {
            Ok(code) => code,
            Err(code) => return code,
        }
    };

    if code.is_empty() {
        eprintln!("error: authorization code is empty");
        return ExitCode::FAILURE;
    }

    // Exchange the authorization code for tokens.
    let exchange_params = TokenExchangeParams {
        token_url: config.oauth.token_url.clone(),
        code,
        redirect_uri: config.oauth.redirect_uri.clone(),
        client_id: config.oauth.client_id.clone(),
        code_verifier: pkce.map(|p| p.verifier().to_owned()),
        token_format: config.oauth.token_format(),
    };

    eprintln!("Exchanging authorization code for tokens...");
    let token_response = match stoat_io::token_exchange::exchange_code(&exchange_params).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: token exchange failed: {}", format_error_chain(&e));
            eprintln!(
                "hint: check that your OAuth configuration \
                 (token_url, client_id, redirect_uri) is correct"
            );
            return ExitCode::FAILURE;
        }
    };

    // Convert to stored token format.
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let stored_token = match token_response.into_stored_token(now_unix) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {}", format_error_chain(&e));
            return ExitCode::FAILURE;
        }
    };

    // Write tokens to file.
    if let Err(e) = stoat_io::token_store::write_token(&token_path, &stored_token) {
        eprintln!("error: {}", format_error_chain(&e));
        eprintln!("hint: check file permissions for the token storage directory");
        return ExitCode::FAILURE;
    }

    eprintln!("Tokens saved to {}", token_path.display());
    ExitCode::SUCCESS
}

async fn run_serve(config: Config) -> ExitCode {
    // Set up tracing subscriber to log to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    // Resolve the token file path.
    let Some(home_dir) = stoat_io::home_dir() else {
        eprintln!("error: could not determine home directory");
        return ExitCode::FAILURE;
    };
    let token_path = stoat_core::paths::expand_tilde(config.token_file_path(), &home_dir);

    // Verify the token file exists before starting the server.
    if !token_path.exists() {
        eprintln!("error: token file not found: {}", token_path.display());
        eprintln!("hint: run `stoat login --config <config>` first to obtain OAuth tokens");
        return ExitCode::FAILURE;
    }

    // Start the proxy server.
    if let Err(e) = stoat_io::proxy::start(config, token_path).await {
        eprintln!("error: {}", format_error_chain(&e));
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// Receive the authorization code via a local HTTP callback listener.
///
/// Starts the listener, opens the browser, waits for the callback, and
/// verifies the state parameter.
async fn receive_code_via_callback(
    auth_url: &url::Url,
    redirect_uri: &url::Url,
    expected_state: &str,
) -> Result<String, ExitCode> {
    let port = redirect_port(redirect_uri).unwrap_or(0);

    // Start the callback listener before opening the browser.
    let listener = match stoat_io::callback::start_callback_listener(port).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: {}", format_error_chain(&e));
            return Err(ExitCode::FAILURE);
        }
    };

    eprintln!(
        "Listening for OAuth callback on http://127.0.0.1:{}",
        listener.port()
    );

    // Open the browser.
    eprintln!("Opening browser for authorization...");
    if let Err(e) = stoat_io::browser::open_browser(auth_url) {
        eprintln!("warning: {}", format_error_chain(&e));
        eprintln!("Please open this URL in your browser manually:");
        eprintln!("  {auth_url}");
    }

    // Wait for the callback.
    let callback = match listener.wait().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", format_error_chain(&e));
            return Err(ExitCode::FAILURE);
        }
    };

    // Verify the state parameter.
    if callback.state.as_deref() != Some(expected_state) {
        eprintln!(
            "error: state mismatch — possible CSRF attack \
             (expected {expected_state:?}, got {:?})",
            callback.state,
        );
        return Err(ExitCode::FAILURE);
    }

    Ok(callback.code)
}

/// Format an error and its entire source chain for user display.
///
/// Produces output like:
///
/// ```text
/// top-level message
///   caused by: intermediate error
///   caused by: root cause
/// ```
fn format_error_chain(error: &dyn std::error::Error) -> String {
    use std::fmt::Write as _;
    let mut msg = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let _ = write!(msg, "\n  caused by: {cause}");
        source = cause.source();
    }
    msg
}

/// Receive the authorization code via terminal paste.
fn receive_code_via_paste(auth_url: &url::Url) -> Result<String, ExitCode> {
    eprintln!("Opening browser for authorization...");
    if let Err(e) = stoat_io::browser::open_browser(auth_url) {
        eprintln!("warning: {}", format_error_chain(&e));
        eprintln!("Please open this URL in your browser manually:");
        eprintln!("  {auth_url}");
    }

    let mut stdin = std::io::stdin().lock();
    let mut stderr = std::io::stderr();
    match stoat_io::paste::read_authorization_code(&mut stdin, &mut stderr) {
        Ok(code) => Ok(code),
        Err(e) => {
            eprintln!(
                "error: failed to read authorization code: {}",
                format_error_chain(&e)
            );
            Err(ExitCode::FAILURE)
        }
    }
}

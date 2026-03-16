//! Token file storage with secure permissions.
//!
//! Reads and writes the token JSON file, ensuring `0600` permissions
//! (owner read/write only) on Unix systems.

use std::path::Path;

use stoat_core::token::StoredToken;

/// Error from token file operations.
#[derive(Debug, thiserror::Error)]
pub enum TokenStoreError {
    /// Failed to read the token file.
    #[error("failed to read token file at {}", path.display())]
    Read {
        /// The path that could not be read.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse the token file JSON.
    #[error("failed to parse token file at {}", path.display())]
    Parse {
        /// The path that could not be parsed.
        path: std::path::PathBuf,
        /// The underlying JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Failed to serialize the token to JSON.
    #[error("failed to serialize token data")]
    Serialize(#[source] serde_json::Error),

    /// Failed to create parent directories.
    #[error("failed to create parent directory for {}", path.display())]
    CreateDir {
        /// The path whose parent could not be created.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to write the token file.
    #[error("failed to write token file at {}", path.display())]
    Write {
        /// The path that could not be written.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to set file permissions.
    #[error("failed to set permissions on {}", path.display())]
    Permissions {
        /// The path whose permissions could not be set.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// Read a stored token from a JSON file.
///
/// # Errors
///
/// Returns a [`TokenStoreError`] if the file cannot be read or parsed.
pub fn read_token(path: &Path) -> Result<StoredToken, TokenStoreError> {
    let contents = std::fs::read_to_string(path).map_err(|source| TokenStoreError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    StoredToken::from_json(&contents).map_err(|source| TokenStoreError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Write a stored token to a JSON file with `0600` permissions.
///
/// Creates parent directories if they don't exist. On Unix, the file
/// permissions are set to `0600` (owner read/write only).
///
/// # Errors
///
/// Returns a [`TokenStoreError`] if the file cannot be written or
/// permissions cannot be set.
pub fn write_token(path: &Path, token: &StoredToken) -> Result<(), TokenStoreError> {
    // Create parent directories if needed.
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|source| TokenStoreError::CreateDir {
            path: path.to_path_buf(),
            source,
        })?;
    }

    let json = token.to_json().map_err(TokenStoreError::Serialize)?;

    std::fs::write(path, json.as_bytes()).map_err(|source| TokenStoreError::Write {
        path: path.to_path_buf(),
        source,
    })?;

    // Set file permissions to 0600 on Unix.
    set_owner_only_permissions(path)?;

    Ok(())
}

/// Set file permissions to owner read/write only (0600) on Unix.
///
/// On non-Unix platforms, this is a no-op.
#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> Result<(), TokenStoreError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, permissions).map_err(|source| TokenStoreError::Permissions {
        path: path.to_path_buf(),
        source,
    })
}

/// Set file permissions to owner read/write only (0600) on Unix.
///
/// On non-Unix platforms, this is a no-op.
#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> Result<(), TokenStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_token_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");

        let token = StoredToken {
            access_token: "access-abc".into(),
            refresh_token: "refresh-xyz".into(),
            expires_at: 1_710_000_000,
        };

        write_token(&path, &token).unwrap();
        let loaded = read_token(&path).unwrap();
        assert_eq!(token, loaded);
    }

    #[test]
    fn creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("tokens.json");

        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 0,
        };

        write_token(&path, &token).unwrap();
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn file_permissions_are_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");

        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 0,
        };

        write_token(&path, &token).unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file should have 0600 permissions");
    }

    #[test]
    fn read_nonexistent_file_returns_error() {
        let result = read_token(Path::new("/nonexistent/path/tokens.json"));
        assert!(result.is_err());
    }

    #[test]
    fn read_invalid_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        std::fs::write(&path, "not json").unwrap();

        let result = read_token(&path);
        assert!(result.is_err());
    }
}

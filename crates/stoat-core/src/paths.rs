//! Path utilities for stoat.
//!
//! Pure functions for path manipulation. The actual home directory must be
//! provided by the caller (the I/O layer), keeping this crate free of
//! filesystem or environment dependencies.

use std::path::{Path, PathBuf};

/// Expand a leading `~` in a path to the given home directory.
///
/// Only a bare `~` at the start of the path is expanded. `~username` syntax
/// is not supported, and `~` appearing elsewhere in the path is left as-is.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
/// use stoat_core::paths::expand_tilde;
///
/// let home = Path::new("/home/alice");
///
/// assert_eq!(
///     expand_tilde("~/tokens.json", home),
///     PathBuf::from("/home/alice/tokens.json"),
/// );
///
/// assert_eq!(
///     expand_tilde("/absolute/path", home),
///     PathBuf::from("/absolute/path"),
/// );
/// ```
#[must_use]
pub fn expand_tilde(path: &str, home_dir: &Path) -> PathBuf {
    if path == "~" {
        return home_dir.to_path_buf();
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir.join(rest);
    }

    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_with_subpath() {
        let home = Path::new("/home/alice");
        assert_eq!(
            expand_tilde("~/.config/stoat/tokens.json", home),
            PathBuf::from("/home/alice/.config/stoat/tokens.json"),
        );
    }

    #[test]
    fn bare_tilde() {
        let home = Path::new("/home/alice");
        assert_eq!(expand_tilde("~", home), PathBuf::from("/home/alice"));
    }

    #[test]
    fn absolute_path_unchanged() {
        let home = Path::new("/home/alice");
        assert_eq!(
            expand_tilde("/absolute/path", home),
            PathBuf::from("/absolute/path"),
        );
    }

    #[test]
    fn relative_path_unchanged() {
        let home = Path::new("/home/alice");
        assert_eq!(
            expand_tilde("relative/path", home),
            PathBuf::from("relative/path"),
        );
    }

    #[test]
    fn tilde_in_middle_unchanged() {
        let home = Path::new("/home/alice");
        assert_eq!(expand_tilde("foo/~/bar", home), PathBuf::from("foo/~/bar"),);
    }

    #[test]
    fn empty_path_unchanged() {
        let home = Path::new("/home/alice");
        assert_eq!(expand_tilde("", home), PathBuf::from(""));
    }

    #[test]
    fn tilde_username_not_expanded() {
        let home = Path::new("/home/alice");
        assert_eq!(
            expand_tilde("~bob/files", home),
            PathBuf::from("~bob/files"),
        );
    }
}

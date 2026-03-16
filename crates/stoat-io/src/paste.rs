//! Authorization code receipt via terminal paste.
//!
//! Prompts the user to paste the authorization code from their browser
//! into the terminal, then reads it from stdin.

use std::io::{BufRead, Write};

/// Read an authorization code from the terminal.
///
/// Prints a prompt to the given writer (typically stderr), then reads a
/// single line from the given reader (typically stdin). The line is trimmed
/// of leading/trailing whitespace.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if reading from the input fails.
pub fn read_authorization_code(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<String, std::io::Error> {
    write!(
        writer,
        "Paste the authorization code from your browser and press Enter: "
    )?;
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    Ok(line.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_and_trims_code() {
        let mut input = Cursor::new(b"  abc123  \n");
        let mut output = Vec::new();

        let code = read_authorization_code(&mut input, &mut output).unwrap();
        assert_eq!(code, "abc123");
    }

    #[test]
    fn prompt_is_written() {
        let mut input = Cursor::new(b"code\n");
        let mut output = Vec::new();

        let _code = read_authorization_code(&mut input, &mut output).unwrap();
        let prompt = String::from_utf8(output).unwrap();
        assert!(prompt.contains("Paste the authorization code"));
    }

    #[test]
    fn empty_input_returns_empty_string() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();

        let code = read_authorization_code(&mut input, &mut output).unwrap();
        assert_eq!(code, "");
    }
}

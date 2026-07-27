//! The shared secret every remote client must present.
//!
//! Inari Remote is a mixer hanging off a Wi-Fi network, so the token is the
//! only thing between the allowlist and whoever else is on that network. Three
//! properties are load-bearing and each is easy to lose by accident:
//!
//! * It comes from the OS CSPRNG. A timestamp, a PID or a `SystemTime`-seeded
//!   PRNG would be guessable by anyone who can see when the app started.
//! * It is compared in constant time, so a wrong guess cannot be refined byte
//!   by byte from how long the rejection took.
//! * It is never logged and never rendered by `Debug`. The log file is
//!   attached to bug reports; a token in it is a token published.
//!
//! It lives in its own 0600 file rather than in `prefs.json`, which the
//! desktop UI reads wholesale via `get_prefs` - the secret has no business
//! travelling to a webview that never needs it.

use std::path::PathBuf;

use subtle::ConstantTimeEq;

use crate::error::SinkError;
use crate::persistence::json::config_path;

/// File under `$XDG_CONFIG_HOME/inari` holding the raw token.
const FILE: &str = "remote-token";

/// Token length in bytes before hex encoding. 32 bytes = 256 bits: far past
/// anything brute-forceable over a network, and short enough that the pairing
/// QR stays readable at phone-camera distance.
const TOKEN_BYTES: usize = 32;

/// The remote's bearer token.
///
/// Deliberately has no `Debug`, no `Display` and no `Serialize`: the only ways
/// out are [`Token::expose`], which every call site has to name, and the
/// pairing payload the user explicitly asked for. It has no `PartialEq`
/// either, so [`Token::matches`] is the only comparison available and nobody
/// can reach for `==` and get a variable-time one.
pub struct Token(String);

impl Token {
    /// A fresh token from the OS CSPRNG.
    pub fn generate() -> Result<Self, SinkError> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)
            .map_err(|e| SinkError::Config(format!("no system randomness for the remote token: {e}")))?;
        Ok(Self(hex(&bytes)))
    }

    /// Does `presented` match? Constant time in the length of the token, so
    /// the comparison leaks nothing about where a guess first went wrong.
    ///
    /// A length mismatch short-circuits, which only reveals the token length -
    /// a compile-time constant that is public anyway.
    pub fn matches(&self, presented: &str) -> bool {
        let expected = self.0.as_bytes();
        let got = presented.as_bytes();
        if expected.len() != got.len() {
            return false;
        }
        expected.ct_eq(got).into()
    }

    /// The token as text. Named so that every place it escapes is greppable.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

fn path() -> Result<PathBuf, SinkError> {
    config_path(FILE)
}

/// Read the stored token, or `None` if the remote has never been enabled.
///
/// A file we cannot read is treated as absent: the caller mints a new token
/// rather than refusing to start, and the user re-pairs. That is strictly
/// better than a remote that cannot come up at all - and a token nobody holds
/// is not a token worth preserving.
pub fn load() -> Option<Token> {
    let raw = std::fs::read_to_string(path().ok()?).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(Token(trimmed.to_string()))
}

/// Write `token` to its own 0600 file inside the (0700) config directory.
pub fn store(token: &Token) -> Result<(), SinkError> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        crate::persistence::ensure_private_dir(parent)?;
    }
    crate::persistence::write_atomic(&path, token.expose())?;
    // `write_atomic` renames a temp file into place, so the mode has to be set
    // on the result rather than before the write.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// The stored token, minting and persisting one on first use.
pub fn load_or_create() -> Result<Token, SinkError> {
    if let Some(token) = load() {
        return Ok(token);
    }
    let token = Token::generate()?;
    store(&token)?;
    Ok(token)
}

/// Replace the stored token. Every paired device has to be paired again -
/// that is the point of the button.
pub fn regenerate() -> Result<Token, SinkError> {
    let token = Token::generate()?;
    store(&token)?;
    Ok(token)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        // Writing into a String cannot fail.
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_token_is_full_length_hex() {
        let token = Token::generate().expect("system randomness");
        assert_eq!(token.expose().len(), TOKEN_BYTES * 2);
        assert!(token.expose().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn two_tokens_are_never_the_same() {
        // The whole point of the CSPRNG: a clock- or counter-derived token
        // would collide here the moment two are minted in the same tick.
        let a = Token::generate().expect("system randomness");
        let b = Token::generate().expect("system randomness");
        assert_ne!(a.expose(), b.expose());
    }

    #[test]
    fn only_the_exact_token_matches() {
        let token = Token("a1b2c3".to_string());
        assert!(token.matches("a1b2c3"));
        assert!(!token.matches(""));
        assert!(!token.matches("a1b2c4"), "one byte off is still wrong");
        assert!(!token.matches("A1B2C3"), "hex compares as bytes, not as a number");
        assert!(!token.matches("a1b2c3 "), "no trimming on the wire");
        assert!(!token.matches("a1b2c31"), "a longer guess is not a prefix match");
        assert!(!token.matches("a1b2c"), "nor is a shorter one");
    }
}

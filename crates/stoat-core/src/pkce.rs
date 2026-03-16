//! PKCE (Proof Key for Code Exchange) support for OAuth 2.0.
//!
//! Implements [RFC 7636](https://tools.ietf.org/html/rfc7636) code verifier
//! generation and S256 challenge derivation. This module is pure — it takes
//! an `&mut impl rand::RngCore` so the caller controls randomness, making
//! tests deterministic when needed.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// Length of the random bytes used to generate the code verifier.
///
/// 64 random bytes → 86 base64url characters (within the 43–128 range
/// required by RFC 7636 § 4.1).
const VERIFIER_RANDOM_BYTES: usize = 64;

/// A PKCE code verifier and its derived S256 challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceChallenge {
    /// The code verifier string sent during token exchange.
    verifier: String,
    /// The S256 challenge string sent during authorization.
    challenge: String,
}

impl PkceChallenge {
    /// Generate a new PKCE code verifier from the given RNG and derive the
    /// S256 challenge.
    ///
    /// The verifier is 86 characters of base64url (no padding), produced
    /// from 64 random bytes. The challenge is `base64url(sha256(verifier))`.
    pub fn generate(rng: &mut impl rand::Rng) -> Self {
        let mut random_bytes = [0u8; VERIFIER_RANDOM_BYTES];
        rng.fill_bytes(&mut random_bytes);
        let verifier = URL_SAFE_NO_PAD.encode(random_bytes);
        let challenge = s256_challenge(&verifier);
        Self {
            verifier,
            challenge,
        }
    }

    /// The code verifier string.
    #[must_use]
    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    /// The S256 challenge string.
    #[must_use]
    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

/// Compute the S256 challenge for a given code verifier.
///
/// `challenge = base64url(sha256(ascii(verifier)))` per RFC 7636 § 4.2.
fn s256_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_length_is_86() {
        let mut rng = rand::rng();
        let pkce = PkceChallenge::generate(&mut rng);
        assert_eq!(
            pkce.verifier().len(),
            86,
            "64 random bytes should produce an 86-character base64url verifier"
        );
    }

    #[test]
    fn challenge_is_valid_base64url() {
        let mut rng = rand::rng();
        let pkce = PkceChallenge::generate(&mut rng);
        let decoded = URL_SAFE_NO_PAD.decode(pkce.challenge());
        assert!(decoded.is_ok(), "challenge should be valid base64url");
        assert_eq!(
            decoded.unwrap().len(),
            32,
            "SHA-256 digest should be 32 bytes"
        );
    }

    #[test]
    fn challenge_matches_verifier() {
        let mut rng = rand::rng();
        let pkce = PkceChallenge::generate(&mut rng);

        // Manually compute the expected challenge.
        let digest = Sha256::digest(pkce.verifier().as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(digest);
        assert_eq!(pkce.challenge(), expected);
    }

    #[test]
    fn deterministic_with_fixed_rng() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let pkce1 = PkceChallenge::generate(&mut rng);

        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let pkce2 = PkceChallenge::generate(&mut rng);

        assert_eq!(pkce1, pkce2, "same seed should produce same PKCE pair");
    }

    #[test]
    fn different_seeds_produce_different_verifiers() {
        use rand::SeedableRng;
        let mut rng1 = rand::rngs::StdRng::seed_from_u64(1);
        let pkce1 = PkceChallenge::generate(&mut rng1);

        let mut rng2 = rand::rngs::StdRng::seed_from_u64(2);
        let pkce2 = PkceChallenge::generate(&mut rng2);

        assert_ne!(pkce1.verifier(), pkce2.verifier());
    }

    /// RFC 7636 Appendix B test vector.
    ///
    /// The RFC gives a specific code verifier and its expected S256 challenge.
    #[test]
    fn rfc7636_appendix_b_test_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected_challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(s256_challenge(verifier), expected_challenge);
    }

    #[test]
    fn verifier_contains_only_valid_base64url_chars() {
        let mut rng = rand::rng();
        let pkce = PkceChallenge::generate(&mut rng);
        for ch in pkce.verifier().chars() {
            assert!(
                ch.is_ascii_alphanumeric() || ch == '-' || ch == '_',
                "verifier should only contain base64url characters, found: {ch}"
            );
        }
    }
}

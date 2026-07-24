//! OAuth 2.0 PKCE (RFC 7636) Generator & Verifier.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::Rng;
use sha2::{Digest as _, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceChallenge {
    pub code_verifier: String,
    pub code_challenge: String,
    pub code_challenge_method: &'static str,
    pub state: String,
}

impl PkceChallenge {
    pub fn new() -> Self {
        let mut verifier_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut verifier_bytes);
        let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

        let hash = Sha256::digest(code_verifier.as_bytes());
        let code_challenge = URL_SAFE_NO_PAD.encode(hash);

        let mut state_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut state_bytes);
        let state = URL_SAFE_NO_PAD.encode(state_bytes);

        Self {
            code_verifier,
            code_challenge,
            code_challenge_method: "S256",
            state,
        }
    }
}

impl Default for PkceChallenge {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
pub fn verify_pkce_challenge(verifier: &str, challenge: &str) -> bool {
    let hash = Sha256::digest(verifier.as_bytes());
    let computed = URL_SAFE_NO_PAD.encode(hash);
    computed == challenge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_pkce_generator_computes_valid_s256_challenge() {
        let pkce = PkceChallenge::new();
        assert_eq!(pkce.code_challenge_method, "S256");
        assert!(!pkce.code_verifier.is_empty());
        assert!(!pkce.code_challenge.is_empty());
        assert!(!pkce.state.is_empty());
        assert!(verify_pkce_challenge(
            &pkce.code_verifier,
            &pkce.code_challenge
        ));

        let verifier = &pkce.code_verifier;
        let challenge = &pkce.code_challenge;
        assert!(verify_pkce_challenge(verifier, challenge));
    }
}

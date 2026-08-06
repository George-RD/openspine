//! OpenAI Codex first-party client fingerprint for OAuth grants.
//!
//! A Codex OAuth grant is only accepted by the ChatGPT backend Responses
//! endpoint, and the requests it serves carry client-identifying headers the
//! owner never wrote. None of it is agent instruction: it identifies the
//! client to the provider.
//!
//! It still changes what the provider receives, so it cannot be invisible.
//! [`oauth_fingerprint_digest`] covers every constant transmitted below and
//! participates in [`crate::config::provider_config_digest`] for
//! `openai_codex` OAuth providers, which is what a model-swap approval binds.
//! Editing any constant here changes that digest and the swap needs approving
//! again: what the owner approved is what goes on the wire.
//!
//! A leaf module with no kernel imports, so `config`, `model_gateway`, and
//! `oauth` can all depend on it without a cycle.

use openspine_schemas::digest::{digest_of_bytes, Digest};

/// Client name presented in the authorize URL and on every inference
/// request. pi ships `originator=pi` against the same endpoints, so
/// non-enumerated originators are accepted; this is the honest value for
/// this runtime.
pub const ORIGINATOR: &str = "openspine";

/// Beta that admits a request on the Responses endpoint.
pub const OPENAI_BETA: &str = "responses=experimental";

/// User agent the client presents on inference. Fixed rather than derived
/// from the host so the digest is deterministic across machines.
pub const USER_AGENT: &str = "openspine-kernel";

/// Endpoint path appended to the configured base URL. Part of the client
/// surface: a path change is a different endpoint contract.
pub const RESPONSES_PATH: &str = "/codex/responses";

/// Accept header the SSE transport presents.
pub const ACCEPT: &str = "text/event-stream";

/// Bumped whenever any constant above changes, so a digest change is
/// deliberate and greppable rather than incidental.
pub const OAUTH_FINGERPRINT_VERSION: u32 = 1;

/// Every constant of the identifying client surface the transport consumes,
/// in digest order: headers and the endpoint path. The request BODY shape is
/// pinned by the transport wire tests instead — its fields are structural
/// (booleans and typed items the endpoint mandates), not free-standing
/// constants a digest string could keep honest.
fn fingerprint_parts() -> [&'static str; 5] {
    [ORIGINATOR, OPENAI_BETA, USER_AGENT, RESPONSES_PATH, ACCEPT]
}

/// Digest over the whole fingerprint, for the provider config digest.
pub fn oauth_fingerprint_digest() -> Digest {
    digest_of_bytes(
        fingerprint_material(OAUTH_FINGERPRINT_VERSION, &fingerprint_parts()).as_bytes(),
    )
}

fn fingerprint_material(version: u32, parts: &[&str]) -> String {
    format!("codex-v{version}\n{}", parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codex_fingerprint_digest_is_stable_across_calls() {
        assert_eq!(oauth_fingerprint_digest(), oauth_fingerprint_digest());
    }

    /// The point of the digest: any edit to what the provider receives has to
    /// surface as a different approval identity, not slip through unnoticed.
    #[test]
    fn every_transmitted_codex_element_participates_in_the_digest() {
        let parts = fingerprint_parts();
        let current = oauth_fingerprint_digest();

        assert_ne!(
            current,
            digest_of_bytes(fingerprint_material(99, &parts).as_bytes()),
            "the version must participate"
        );
        for index in 0..parts.len() {
            let mut mutated = parts;
            mutated[index] = "changed";
            assert_ne!(
                current,
                digest_of_bytes(
                    fingerprint_material(OAUTH_FINGERPRINT_VERSION, &mutated).as_bytes()
                ),
                "element {index} must participate"
            );
        }
    }

    /// The two OAuth client surfaces are different providers; their approval
    /// identities must never collide.
    #[test]
    fn the_codex_fingerprint_differs_from_the_anthropic_one() {
        assert_ne!(
            oauth_fingerprint_digest(),
            crate::anthropic_fingerprint::oauth_fingerprint_digest()
        );
    }
}

//! Anthropic first-party client fingerprint for OAuth grants.
//!
//! An Anthropic OAuth grant is only honoured for the client surface it was
//! issued against. Serving one therefore means sending that client's beta,
//! markers, user agents, and a leading system block. None of it is agent
//! instruction: it identifies the client to the provider.
//!
//! It still changes what the provider receives, so it cannot be invisible.
//! [`oauth_fingerprint_digest`] covers every element transmitted below and
//! participates in [`crate::config::provider_config_digest`] for OAuth
//! providers, which is what a model-swap approval binds. Editing any constant
//! here changes that digest and the swap needs approving again, which keeps one
//! property true: what the owner approved is what goes on the wire.
//!
//! A leaf module with no kernel imports, so both `config` and `model_gateway`
//! can depend on it without a cycle.

use openspine_schemas::digest::{digest_of_bytes, Digest};

/// Beta that admits an OAuth grant on the messages endpoint. Bearer alone is
/// rejected without it.
pub const OAUTH_BETA: &str = "oauth-2025-04-20";

/// Client marker sent alongside [`OAUTH_BETA`] on inference.
pub const OAUTH_DIRECT_BROWSER_ACCESS: &str = "true";

/// Client application marker sent alongside [`OAUTH_BETA`] on inference.
pub const OAUTH_APP: &str = "cli";

/// User agent the client presents on inference.
///
/// The `(external, cli)` variant belongs to the usage endpoint, not to
/// inference; only this one is sent with a messages request.
pub const OAUTH_USER_AGENT: &str = "claude-cli/2.1.165 (external, local-agent, agent-sdk/0.3.165)";

/// User agent the client presents on token refresh.
///
/// Deliberately different from [`OAUTH_USER_AGENT`]: the client sends the SDK
/// agent plus [`OAUTH_BETA`] when refreshing, and neither on the initial code
/// exchange.
pub const OAUTH_REFRESH_USER_AGENT: &str = "anthropic-sdk-typescript/0.94.0 userOAuthProvider";

/// Leading system block the client sends on every OAuth request.
///
/// Prepended at transmit time, ahead of the agent's own preamble, which stays
/// exactly as the prompt template composed it.
pub const OAUTH_CLIENT_INSTRUCTION: &str =
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.";

/// Bumped whenever any constant above changes, so a digest change is deliberate
/// and greppable rather than incidental.
pub const OAUTH_FINGERPRINT_VERSION: u32 = 1;

/// Every element of the client surface that reaches the provider, in digest
/// order. Anything transmitted but absent here would change the wire without
/// moving the approval identity.
fn fingerprint_parts() -> [&'static str; 6] {
    [
        OAUTH_BETA,
        OAUTH_DIRECT_BROWSER_ACCESS,
        OAUTH_APP,
        OAUTH_USER_AGENT,
        OAUTH_REFRESH_USER_AGENT,
        OAUTH_CLIENT_INSTRUCTION,
    ]
}

/// Digest over the whole fingerprint, for the provider config digest.
pub fn oauth_fingerprint_digest() -> Digest {
    digest_of_bytes(
        fingerprint_material(OAUTH_FINGERPRINT_VERSION, &fingerprint_parts()).as_bytes(),
    )
}

fn fingerprint_material(version: u32, parts: &[&str]) -> String {
    format!("v{version}\n{}", parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_digest_is_stable_across_calls() {
        assert_eq!(oauth_fingerprint_digest(), oauth_fingerprint_digest());
    }

    /// The point of the digest: any edit to what the provider receives has to
    /// surface as a different approval identity, not slip through unnoticed.
    #[test]
    fn every_transmitted_element_participates_in_the_digest() {
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
}

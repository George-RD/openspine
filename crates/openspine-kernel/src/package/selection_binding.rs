//! Trusted bindings of a selection decision (ANO-822).
//!
//! Neither value is supplied by a package, JSON document or raw CLI argument.
use openspine_schemas::digest::{digest_of, Digest};
use ulid::Ulid;

/// Domain-separated MAC over the persisted owner principal and identity IDs
/// under the protected artifact key. A copy of the complete owner, ledger and
/// key is the same logical instance; no cross-instance isolation is claimed.
pub(crate) fn owner_binding(key: &[u8; 32], principal: Ulid, identity: Ulid) -> String {
    let mut message = b"openspine:package-selection-owner:v1\0".to_vec();
    message.extend(principal.to_string().as_bytes());
    message.push(0);
    message.extend(identity.to_string().as_bytes());
    let mac = hmac_sha256::HMAC::mac(&message, key);
    let hex: String = mac.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("hmac-sha256:{hex}")
}

/// Identity of the evaluator, loader, schemas and catalog that produced a plan:
/// the bounded build-input fingerprint, not the user-facing package revision.
pub(crate) fn runtime_compatibility() -> Digest {
    digest_of(&serde_json::json!({
        "schema_version": 1,
        "build_input_fingerprint": env!("OPENSPINE_BUILD_INPUT_FINGERPRINT"),
        "inventory_format_version": super::INVENTORY_FORMAT_VERSION,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_binding_is_domain_separated_and_key_and_identity_sensitive() {
        let (principal, identity) = (Ulid::from(1u128), Ulid::from(2u128));
        let base = owner_binding(&[7; 32], principal, identity);
        assert!(base.starts_with("hmac-sha256:") && base.len() == 12 + 64);
        assert_eq!(base, owner_binding(&[7; 32], principal, identity));
        assert_ne!(base, owner_binding(&[8; 32], principal, identity));
        assert_ne!(base, owner_binding(&[7; 32], identity, principal));
        assert_ne!(base, owner_binding(&[7; 32], principal, Ulid::from(3u128)));
    }

    #[test]
    fn runtime_compatibility_binds_a_present_bounded_build_fingerprint() {
        let fingerprint = env!("OPENSPINE_BUILD_INPUT_FINGERPRINT");
        assert!(fingerprint.starts_with("sha256:") && fingerprint.len() == 7 + 64);
        assert_eq!(runtime_compatibility(), runtime_compatibility());
    }
}

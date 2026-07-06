//! Canonical JSON and content digests.
//!
//! Per decision D-028: YAML is the canonical *on-disk* artifact format, but
//! digesting/approval-binding needs a byte-stable pre-image. Canonical JSON
//! (recursive key-sort, no insignificant whitespace, UTF-8) is that
//! pre-image; it is used nowhere else. `Digest` is always `sha256:<64 lowercase hex>`.

use std::fmt;

use serde::{
    de::Error as _,
    ser::{SerializeMap, SerializeSeq},
    Deserialize, Deserializer, Serialize, Serializer,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

const PREFIX: &str = "sha256:";
const HEX_LEN: usize = 64;

/// A `sha256:<64 lowercase hex>` content digest.
///
/// Deserialization enforces the exact shape so a malformed digest can never
/// silently enter a task grant, approval record, or audit event.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Digest(String);

impl Digest {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(s: &str) -> bool {
        match s.strip_prefix(PREFIX) {
            Some(hex) => {
                hex.len() == HEX_LEN
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }
            None => false,
        }
    }

    /// Parse a digest string, rejecting anything that isn't exactly `sha256:<64 lowercase hex>`.
    pub fn parse(s: impl Into<String>) -> Result<Self, InvalidDigest> {
        let s = s.into();
        if Self::is_valid(&s) {
            Ok(Digest(s))
        } else {
            Err(InvalidDigest(s))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid digest {0:?}: expected \"sha256:<64 lowercase hex>\"")]
pub struct InvalidDigest(String);

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Digest::parse(s).map_err(D::Error::custom)
    }
}

struct CanonicalValue<'a>(&'a Value);

impl<'a> Serialize for CanonicalValue<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Number(n) => n.serialize(serializer),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(&CanonicalValue(item))?;
                }
                seq.end()
            }
            Value::Object(map) => {
                let len = map.len();
                if len == 0 {
                    let map_serializer = serializer.serialize_map(Some(0))?;
                    map_serializer.end()
                } else if len == 1 {
                    let mut map_serializer = serializer.serialize_map(Some(1))?;
                    let (k, v) = map.iter().next().expect("map has exactly 1 element");
                    map_serializer.serialize_entry(k, &CanonicalValue(v))?;
                    map_serializer.end()
                } else if len <= 16 {
                    let mut sorted = [None; 16];
                    for (i, entry) in map.iter().enumerate() {
                        sorted[i] = Some(entry);
                    }
                    let slice = &mut sorted[0..len];
                    slice.sort_unstable_by_key(|opt| opt.expect("slice element populated").0);

                    let mut map_serializer = serializer.serialize_map(Some(len))?;
                    for entry in slice {
                        let (k, v) = entry.expect("slice element populated");
                        map_serializer.serialize_entry(k, &CanonicalValue(v))?;
                    }
                    map_serializer.end()
                } else {
                    let mut sorted: Vec<(&String, &Value)> = map.iter().collect();
                    sorted.sort_unstable_by_key(|&(k, _)| k);
                    let mut map_serializer = serializer.serialize_map(Some(sorted.len()))?;
                    for (k, v) in sorted {
                        map_serializer.serialize_entry(k, &CanonicalValue(v))?;
                    }
                    map_serializer.end()
                }
            }
        }
    }
}

/// Recursively sort object keys and drop insignificant whitespace.
///
/// This is the canonical-JSON pre-image function: deterministic key order,
/// deterministic nesting, no whitespace. It is a pure transform of an
/// already-parsed [`Value`] — it never re-parses or reformats numbers.
pub fn canonical_json(v: &Value) -> String {
    serde_json::to_string(&CanonicalValue(v))
        .expect("canonical JSON of a Value never fails to serialize")
}

/// Convert a 32-byte SHA-256 hash output directly into a [`Digest`].
pub fn digest_from_hash(hash: [u8; 32]) -> Digest {
    let mut buf = Vec::with_capacity(71);
    buf.extend_from_slice(b"sha256:");
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    for &b in hash.iter() {
        buf.push(HEX_CHARS[(b >> 4) as usize]);
        buf.push(HEX_CHARS[(b & 0xf) as usize]);
    }
    let s = String::from_utf8(buf).expect("sha256 hex output is always ASCII");
    Digest(s)
}

/// Returns true if the given digest string corresponds to the given 32-byte SHA-256 hash.
///
/// This does not allocate memory and verifies the digest scheme matches the expected format.
pub fn digest_matches_hash(digest_str: &str, hash: &[u8; 32]) -> bool {
    if !digest_str.starts_with("sha256:") || digest_str.len() != 71 {
        return false;
    }
    let hash_bytes = &digest_str.as_bytes()[7..];
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    for (i, &b) in hash.iter().enumerate() {
        let h1 = HEX_CHARS[(b >> 4) as usize];
        let h2 = HEX_CHARS[(b & 0xf) as usize];
        if hash_bytes[i * 2] != h1 || hash_bytes[i * 2 + 1] != h2 {
            return false;
        }
    }
    true
}

struct HasherWriter<'a>(&'a mut Sha256);

impl<'a> std::io::Write for HasherWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Digest a `serde_json::Value` over its canonical-JSON pre-image.
///
/// Callers with typed structs must convert them via `serde_json::to_value` first.
pub fn digest_of(v: &Value) -> Digest {
    let mut hasher = Sha256::new();
    {
        let mut writer = HasherWriter(&mut hasher);
        serde_json::to_writer(&mut writer, &CanonicalValue(v))
            .expect("canonical JSON serialization to hasher never fails");
    }
    digest_from_hash(hasher.finalize().into())
}
/// Digest raw bytes directly (no canonical-JSON step). Used by the
/// artifact store (Step 4) to content-address encrypted blob plaintext
/// before encryption — the digest must be over the *plaintext* content, not
/// ciphertext (which varies per random nonce), so digest-bound approvals
/// (D-011) and content-addressed storage both see the same identity.
pub fn digest_of_bytes(bytes: &[u8]) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    digest_from_hash(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys_at_every_depth() {
        let a = json!({"b": 1, "a": {"z": 1, "y": 2}});
        let b = json!({"a": {"y": 2, "z": 1}, "b": 1});
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(canonical_json(&a), r#"{"a":{"y":2,"z":1},"b":1}"#);
    }

    #[test]
    fn canonical_json_sorts_within_arrays_of_objects() {
        let v = json!([{"b": 1, "a": 2}]);
        assert_eq!(canonical_json(&v), r#"[{"a":2,"b":1}]"#);
    }

    #[test]
    fn digest_of_is_a_pinned_golden_value() {
        // Golden value for {"a":1,"b":2} — pinned so an accidental change to
        // the canonicalization or hashing algorithm is caught immediately.
        let d = digest_of(&json!({"b": 2, "a": 1}));
        assert_eq!(
            d.as_str(),
            "sha256:43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
        );
    }

    #[test]
    fn digest_of_is_order_independent() {
        assert_eq!(
            digest_of(&json!({"a": 1, "b": 2})),
            digest_of(&json!({"b": 2, "a": 1}))
        );
    }

    #[test]
    fn digest_parse_rejects_wrong_shapes() {
        assert!(Digest::parse("sha256:abcd").is_err());
        assert!(Digest::parse(format!("sha1:{}", "a".repeat(64))).is_err());
        assert!(Digest::parse(format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(Digest::parse(format!("sha256:{}", "a".repeat(64))).is_ok());
    }

    #[test]
    fn digest_round_trips_through_serde() {
        let d = Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        let back: Digest = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn digest_of_bytes_hashes_raw_content_directly() {
        let d = digest_of_bytes(b"hello world");
        // Independently computed sha256("hello world").
        assert_eq!(
            d.as_str(),
            "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        // Different from digesting the same bytes as a JSON string (which
        // canonical-JSON-quotes them first) — these are deliberately
        // different pre-images.
        assert_ne!(d, digest_of(&json!("hello world")));
    }

    #[test]
    fn test_digest_matches_hash() {
        let raw = b"hello world";
        let digest = digest_of_bytes(raw);
        let mut hasher = Sha256::new();
        hasher.update(raw);
        let hash: [u8; 32] = hasher.finalize().into();
        assert!(digest_matches_hash(digest.as_str(), &hash));

        let mut bad_hash = hash;
        bad_hash[0] ^= 1;
        assert!(!digest_matches_hash(digest.as_str(), &bad_hash));
        assert!(!digest_matches_hash("sha256:invalid", &hash));
    }
}

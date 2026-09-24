//! Bounded terminal-safe package review output. This is never an approval.
use serde::Serialize;

pub(crate) fn escaped_json(value: &impl Serialize, pretty: bool) -> String {
    if pretty {
        serde_json::to_string_pretty(value).expect("review data is serializable")
    } else {
        serde_json::to_string(value).expect("review data is serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_labels_cannot_emit_terminal_or_bidi_controls() {
        let data =
            serde_json::json!({"id": "untrusted\u{1b}[2J\n\u{7f}\u{85}\u{202e}\u{2066}label"});
        for pretty in [false, true] {
            let output = escaped_json(&data, pretty);
            assert!(output.is_ascii());
            assert!(!output.contains('\u{1b}'));
            assert!(!output.contains('\u{7f}'));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&output).unwrap(),
                data
            );
        }
    }
}

//! Telegram message-length truncation helpers, split out of `actions.rs`
//! purely to keep that file under the 500-line gate — both functions exist
//! only to serve `dispatch_lyra_preview`'s WYSIWYS handling (D-045).

/// Telegram hard-caps a single message at 4096 UTF-16 code units — a
/// model-drafted body long enough to exceed that would otherwise turn a
/// successful draft into a failed `send_reply` call (`500`, from the
/// *kernel's* side, after everything upstream genuinely succeeded).
/// Truncates by actual UTF-16 unit count (`char::len_utf16`), not `char`
/// count — a `char` can be up to 2 UTF-16 units (e.g. many emoji), so
/// counting `char`s alone under-truncates for unit-count limits like
/// Telegram's.
pub(crate) const TELEGRAM_MAX_MESSAGE_UTF16_UNITS: usize = 4000;

pub(crate) fn truncate_for_telegram(text: &str) -> String {
    let mut units = 0usize;
    for (idx, ch) in text.char_indices() {
        let w = ch.len_utf16();
        if units + w > TELEGRAM_MAX_MESSAGE_UTF16_UNITS {
            let mut truncated = text[..idx].to_string();
            truncated.push_str("… [truncated]");
            return truncated;
        }
        units += w;
    }
    text.to_string()
}

/// D-045 (WYSIWYS): shown when `dispatch_lyra_preview` refuses to attach
/// an approval button because the preview itself didn't fit in one
/// Telegram message — binding approval to content the owner could not
/// have read in full would let a tap authorize an unseen tail. Splitting
/// the preview across multiple messages was rejected (drift risk between
/// what's shown across parts and what's approved as a whole); the owner
/// is instead told to ask for a shorter draft.
pub(super) const TRUNCATION_NOTICE: &str =
    "\n\n[Draft too long to approve via Telegram — ask for a shorter draft.]";

/// Truncates `full` so that `full`'s prefix plus [`TRUNCATION_NOTICE`]
/// together fit within [`TELEGRAM_MAX_MESSAGE_UTF16_UNITS`], then appends
/// the notice — same UTF-16-unit-counting approach as
/// [`truncate_for_telegram`], but budgeted to always leave room for the
/// notice rather than appending "… [truncated]".
pub(super) fn truncate_with_notice(full: &str) -> String {
    let budget =
        TELEGRAM_MAX_MESSAGE_UTF16_UNITS.saturating_sub(TRUNCATION_NOTICE.encode_utf16().count());
    let mut units = 0usize;
    for (idx, ch) in full.char_indices() {
        let w = ch.len_utf16();
        if units + w > budget {
            let mut truncated = full[..idx].to_string();
            truncated.push_str(TRUNCATION_NOTICE);
            return truncated;
        }
        units += w;
    }
    let mut truncated = full.to_string();
    truncated.push_str(TRUNCATION_NOTICE);
    truncated
}

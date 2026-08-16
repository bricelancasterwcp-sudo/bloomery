//! The envelope-v3 stop-sequence scan (protocol
//! `docs/superpowers/evidence/2026-08-15-g4-protocol.md` §11, Amendment 3).
//!
//! Pulled out of `llama::generate_from`'s generation loop into a pure,
//! feature-independent function so it can be unit-tested GPU-free: `llama.rs`
//! itself only compiles under the `llama` feature (it links llama.cpp), but
//! [`stop_hit`] touches no FFI and no GPU state at all — it is a plain scan
//! over already-accumulated bytes.

/// Where a stop sequence's first occurrence ends in `bytes`, if reachable
/// from the currently accumulated completion. Returns the byte offset ONE
/// PAST the match — `bytes[..cut]` is the truncated text to keep, tag
/// INCLUDED — or `None` when no match is reachable yet.
///
/// **`bytes` is not guaranteed to be valid UTF-8 as a whole** when this
/// runs, mid-generation: a multi-byte character may be mid-flight at the
/// tail (the common, transient case — a codepoint split across two
/// generated tokens), or a byte-fallback token may have appended bytes that
/// are not a valid UTF-8 continuation of anything at all (a genuinely
/// invalid sequence, not merely an incomplete trailing one — e.g. a lone
/// continuation byte with no leading byte ever preceding it). Requiring the
/// *whole* buffer to decode before scanning at all (an earlier version of
/// this code did exactly that, via `str::from_utf8(&bytes).ok()`) has two
/// costs this function avoids by instead scanning the LONGEST VALID UTF-8
/// PREFIX of `bytes` (`Utf8Error::valid_up_to`) rather than requiring the
/// whole buffer to decode:
///
/// - **Trailing-incomplete** (`Utf8Error::error_len() == None`): the valid
///   prefix covers everything except the still-in-flight tail, so a stop
///   tag that already appeared earlier in the buffer is found IMMEDIATELY,
///   in the same call the trailing bytes arrived — this is what removes a
///   detection lag a whole-buffer-only check has (it would otherwise wait a
///   full extra token for the trailing character to resolve before
///   checking again).
/// - **Genuinely invalid** (`error_len() == Some(_)`): the valid prefix
///   freezes at the first bad byte's position for the rest of the turn
///   (`bytes` only grows in the caller's loop, so that position never
///   moves), so a stop tag occurring AFTER the invalid byte is NOT
///   reachable this way and will never be found for the rest of this turn.
///   This is a documented, honest degradation — not a silent one: a tag
///   BEFORE the invalid byte is still found (see the "before"/"after"
///   tests below), and an invalid byte anywhere does not turn the whole
///   check off for the rest of generation the way `str::from_utf8(&bytes)`
///   over the *entire*, ever-growing buffer would (that was the bug this
///   module fixes — a byte-fallback token landing one invalid byte
///   anywhere in the stream silently disabled the stop check for every
///   subsequent token of the turn).
// Its only production caller (`llama::generate_from`) exists solely under
// the `llama` feature — that is the whole point of splitting this out (see
// the module doc comment), so a default (`cargo test --workspace`, no
// `llama` feature) build sees no non-test caller at all. Never silenced
// under `--features llama`, where the real caller keeps the lint honest.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
pub(crate) fn stop_hit(bytes: &[u8], stop: &str) -> Option<usize> {
    let valid = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&bytes[..e.valid_up_to()])
            .expect("valid_up_to() bounds a verified-valid UTF-8 prefix"),
    };
    valid.find(stop).map(|idx| idx + stop.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: &str = "</action>";

    fn action_block() -> Vec<u8> {
        b"<action verb=\"done\">\nok\n</action>".to_vec()
    }

    /// Case (a): a stop tag inside an entirely valid UTF-8 buffer, found
    /// with no truncation weirdness — the ordinary path.
    #[test]
    fn finds_the_stop_in_clean_valid_utf8() {
        let bytes = action_block();
        let expected_cut = bytes.len();
        assert_eq!(stop_hit(&bytes, TAG), Some(expected_cut));
    }

    /// A clean, fully-valid buffer (including a multi-byte character) with
    /// no stop tag present at all must return `None`, not panic or match
    /// spuriously.
    #[test]
    fn no_match_in_clean_valid_utf8_is_none() {
        let bytes = "hello world, no tag here \u{2705}".as_bytes();
        assert_eq!(stop_hit(bytes, TAG), None);
    }

    /// Case (c), the detection-lag removal: an INCOMPLETE trailing
    /// multi-byte sequence — the first two bytes of a 3-byte UTF-8
    /// character (`0xE2 0x9C`, the lead-in of U+2705's `E2 9C 85`
    /// encoding), missing its final continuation byte — sits AFTER an
    /// already-complete stop tag. This is exactly what a codepoint split
    /// across two generated tokens looks like mid-flight. The tag must
    /// still be found in THIS call, without waiting for the trailing
    /// sequence to complete on a later token.
    #[test]
    fn finds_the_stop_before_an_incomplete_trailing_multibyte_sequence() {
        let mut bytes = action_block();
        let expected_cut = bytes.len();
        bytes.extend_from_slice(&[0xE2, 0x9C]);
        assert_eq!(
            stop_hit(&bytes, TAG),
            Some(expected_cut),
            "an incomplete trailing sequence must not block finding an \
             already-present stop tag earlier in the buffer"
        );
    }

    /// Case (b): a genuinely invalid byte — `0x80`, a lone UTF-8
    /// continuation byte with no leading byte ever preceding it, so no
    /// amount of future bytes could ever make it valid — placed BEFORE the
    /// stop tag. `valid_up_to()` freezes the scannable prefix at that
    /// invalid byte's position (index 0 here), so the tag after it is NOT
    /// reachable. This is the honest limitation stated in `stop_hit`'s doc
    /// comment: must return `None` here, never panic, and never claim a
    /// match it structurally cannot see.
    #[test]
    fn a_genuinely_invalid_byte_before_the_stop_makes_it_unreachable() {
        let mut bytes = vec![0x80];
        bytes.extend_from_slice(&action_block());
        assert_eq!(
            stop_hit(&bytes, TAG),
            None,
            "a stop tag after a genuinely invalid byte is unreachable — \
             documented, not hidden"
        );
    }

    /// The counterpart to the previous test and the core regression this
    /// module fixes: a genuinely invalid byte placed AFTER the stop tag
    /// must NOT block finding it — the valid prefix up to (but excluding)
    /// the invalid byte still covers the whole tag. Before this fix, ANY
    /// invalid byte anywhere in the ever-growing buffer made
    /// `str::from_utf8(&bytes)` return `Err` for the rest of the turn,
    /// silently disabling the stop check entirely from that point on.
    #[test]
    fn a_genuinely_invalid_byte_after_the_stop_does_not_block_it() {
        let mut bytes = action_block();
        let expected_cut = bytes.len();
        bytes.push(0x80);
        bytes.extend_from_slice(b"more junk the model kept generating");
        assert_eq!(
            stop_hit(&bytes, TAG),
            Some(expected_cut),
            "an invalid byte AFTER the tag must not suppress the match — \
             this is the exact bug a whole-buffer-only from_utf8 check had"
        );
    }

    #[test]
    fn empty_bytes_is_none() {
        assert_eq!(stop_hit(b"", TAG), None);
    }
}

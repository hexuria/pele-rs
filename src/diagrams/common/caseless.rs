// Port of src/diagrams/common/caseless.ts.

/// Whether `word`, written in lower case, is at `at`. Only ASCII letters match in either case,
/// as with the `i` flag on a regular expression without the `u` flag.
pub fn word_at(src: &[u16], at: usize, word: &str) -> bool {
    for (k, w) in word.encode_utf16().enumerate() {
        let c = src.get(at + k).copied().map_or(u32::MAX, u32::from);
        let w = w as u32;
        if c != w && !((97..=122).contains(&w) && c == w - 32) {
            return false;
        }
    }
    true
}

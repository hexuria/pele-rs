// Port of src/util/chars.ts. Tests on UTF-16 code units, as the lexers read them; past the end
// of the text the lexers pass u32::MAX, which is none of these (upstream's NaN).

pub fn is_space(c: u32) -> bool {
    crate::js::is_space_u(c)
}

pub fn is_word(c: u32) -> bool {
    (48..=57).contains(&c) || (65..=90).contains(&c) || (97..=122).contains(&c) || c == 95
}

pub fn is_blank(c: u32) -> bool {
    c == 32 || c == 9
}

/// A line terminator: what `.` does not match.
pub fn is_line_end(c: u32) -> bool {
    c == 10 || c == 13 || c == 0x2028 || c == 0x2029
}

pub fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

pub fn is_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

pub fn is_hex(c: u32) -> bool {
    (48..=57).contains(&c) || (65..=70).contains(&c) || (97..=102).contains(&c)
}

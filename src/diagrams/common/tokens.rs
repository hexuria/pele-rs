// Port of src/diagrams/common/tokens.ts: the tokenizer for the diagram types Mermaid defines
// with Langium grammars. Token types are tried in order and the first match wins, as in
// Chevrotain. Upstream's sticky regular expressions are matchers here: each takes the text and
// a position and returns the end of its match.

use crate::errors::{syntax_error, PeleError};
use crate::js;
use std::rc::Rc;

/// Answers as a sticky regular expression does at `at`: the end of the match, or None.
pub type Matcher = Rc<dyn Fn(&[u16], usize) -> Option<usize>>;
/// Matches in place of the pattern; `previous` is the last visible token's type index, or -1.
pub type Scanner = Box<dyn FnMut(&[u16], usize, i32) -> Option<usize>>;

#[derive(Clone)]
pub enum Pattern {
    Literal(&'static str),
    Re(Matcher),
}

#[derive(Clone)]
pub struct TokenType {
    pub name: String,
    pub pattern: Pattern,
    pub hidden: bool,
    /// Start of a construct that needs a closer. When this matches but the whole pattern does
    /// not, no closer exists in the rest of the text, so the pattern is not tried again.
    pub opener: Option<Matcher>,
    /// Every ASCII character a match can start with, when that is known.
    pub first: Option<&'static str>,
    /// Chevrotain's LONGER_ALT, as indexes into the same table.
    pub longer: Vec<usize>,
    /// Makes a matcher for a pattern that could rescan the text; called once per text.
    pub scanner: Option<Rc<dyn Fn() -> Scanner>>,
    /// Matches in place of the pattern, for one that depends on the token before it.
    pub match_: Option<Rc<dyn Fn(&[u16], usize, i32) -> Option<usize>>>,
}

impl TokenType {
    pub fn new(name: &str, pattern: Pattern) -> Self {
        TokenType { name: name.to_string(), pattern, hidden: false, opener: None, first: None, longer: Vec::new(), scanner: None, match_: None }
    }
    pub fn re(name: &str, first: Option<&'static str>, m: impl Fn(&[u16], usize) -> Option<usize> + 'static) -> Self {
        TokenType { first, ..TokenType::new(name, Pattern::Re(Rc::new(m))) }
    }
    pub fn lit(name: &str, text: &'static str) -> Self {
        TokenType::new(name, Pattern::Literal(text))
    }
    pub fn named(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }
}

pub struct Tokens {
    pub kinds: Vec<i32>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
    pub src: Vec<u16>,
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn starts_with(src: &[u16], p: usize, lit: &str) -> bool {
    let mut k = p;
    for u in lit.encode_utf16() {
        if src.get(k) != Some(&u) {
            return false;
        }
        k += 1;
    }
    true
}

pub fn tokenize(src: &[u16], types: &[TokenType], diagram: &str) -> Result<Tokens, PeleError> {
    let mut kinds: Vec<i32> = Vec::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut dead = vec![false; types.len()];
    let first_unit = |t: &TokenType| -> Option<u16> {
        match &t.pattern {
            Pattern::Literal(s) => s.encode_utf16().next(),
            _ => None,
        }
    };
    let starting: Vec<Vec<usize>> = (0..128u16)
        .map(|c| {
            (0..types.len())
                .filter(|&k| match &types[k].pattern {
                    Pattern::Literal(_) => first_unit(&types[k]) == Some(c),
                    Pattern::Re(_) => types[k].first.is_none_or(|f| f.encode_utf16().any(|u| u == c)),
                })
                .collect()
        })
        .collect();
    let every: Vec<usize> = (0..types.len()).collect();
    let mut scanners: Vec<Option<Scanner>> = (0..types.len()).map(|_| None).collect();
    let n = src.len();

    let mut matcher = |k: usize, p: usize, kinds: &[i32], dead: &mut [bool]| -> Option<usize> {
        if dead[k] {
            return None;
        }
        let ty = &types[k];
        let previous = kinds.last().copied().unwrap_or(-1);
        if let Some(m) = &ty.match_ {
            return m(src, p, previous);
        }
        if let Some(make) = &ty.scanner {
            return scanners[k].get_or_insert_with(|| make())(src, p, previous);
        }
        match &ty.pattern {
            Pattern::Literal(s) => starts_with(src, p, s).then(|| p + s.encode_utf16().count()),
            Pattern::Re(re) => {
                let end = re(src, p);
                if end.is_none() {
                    if let Some(opener) = &ty.opener {
                        if opener(src, p).is_some() {
                            dead[k] = true;
                        }
                    }
                }
                end
            }
        }
    };

    let mut p = 0;
    'scan: while p < n {
        let c = src[p];
        let list = if c < 128 { &starting[c as usize] } else { &every };
        for &k in list {
            let Some(mut end) = matcher(k, p, &kinds, &mut dead) else { continue };
            if end <= p {
                continue;
            }
            let mut kind = k;
            for &alt in &types[k].longer {
                if let Some(alt_end) = matcher(alt, p, &kinds, &mut dead) {
                    if alt_end > end {
                        kind = alt;
                        end = alt_end;
                        break;
                    }
                }
            }
            if !types[kind].hidden {
                kinds.push(kind as i32);
                starts.push(p);
                ends.push(end);
            }
            p = end;
            continue 'scan;
        }
        return Err(syntax_error(diagram, src, p, "", true));
    }
    kinds.push(-1);
    starts.push(n);
    ends.push(n);
    Ok(Tokens { kinds, starts, ends, src: src.to_vec() })
}

// Pieces the shared patterns are built from.

fn is_lt(c: u32) -> bool {
    c == 10 || c == 13 || c == 0x2028 || c == 0x2029
}

/// `[\t ]*` from `p`.
pub fn blanks(src: &[u16], mut p: usize) -> usize {
    while matches!(at(src, p), 9 | 32) {
        p += 1;
    }
    p
}

/// `[^\n\r]*?(?=%%)|[^\n\r]*` from `p`: up to the first `%%` on the line, else to its end.
pub fn rest_before_comment(src: &[u16], mut p: usize) -> usize {
    while p < src.len() && src[p] != 10 && src[p] != 13 {
        if src[p] == 37 && at(src, p + 1) == 37 {
            return p;
        }
        p += 1;
    }
    p
}

/// `[^\n\r]*` from `p`.
pub fn rest_of_line(src: &[u16], mut p: usize) -> usize {
    while p < src.len() && src[p] != 10 && src[p] != 13 {
        p += 1;
    }
    p
}

/// `(?!\S)` at `p`: the end, or whitespace.
pub fn not_ink(src: &[u16], p: usize) -> bool {
    p >= src.len() || js::is_space_u(src[p] as u32)
}

/// `\r?\n` at `p`.
pub fn newline(src: &[u16], p: usize) -> Option<usize> {
    match at(src, p) {
        13 if at(src, p + 1) == 10 => Some(p + 2),
        10 => Some(p + 1),
        _ => None,
    }
}

/// `(?:\r?\n|(?!\S))` at `p`.
pub fn line_or_space(src: &[u16], p: usize) -> Option<usize> {
    newline(src, p).or_else(|| not_ink(src, p).then_some(p))
}

fn acc_descr_match(src: &[u16], p: usize) -> Option<usize> {
    let q = blanks(src, p);
    if !starts_with(src, q, "accDescr") {
        return None;
    }
    let q = q + 8;
    let r = blanks(src, q);
    if at(src, r) == 58 {
        return Some(rest_before_comment(src, r + 1));
    }
    let mut r = q;
    while r < src.len() && js::is_space_u(src[r] as u32) {
        r += 1;
    }
    if at(src, r) != 123 {
        return None;
    }
    src[r + 1..].iter().position(|&c| c == 125).map(|k| r + 1 + k + 1)
}

pub fn acc_descr() -> TokenType {
    TokenType::re("ACC_DESCR", Some("\t a"), acc_descr_match)
}

pub fn acc_title() -> TokenType {
    TokenType::re("ACC_TITLE", Some("\t a"), |src, p| {
        let q = blanks(src, p);
        if !starts_with(src, q, "accTitle") {
            return None;
        }
        let r = blanks(src, q + 8);
        (at(src, r) == 58).then(|| rest_before_comment(src, r + 1))
    })
}

fn title_match(src: &[u16], p: usize, rest: Option<&dyn Fn(&[u16], usize) -> bool>) -> Option<usize> {
    let q = blanks(src, p);
    if !starts_with(src, q, "title") {
        return None;
    }
    let q = q + 5;
    if matches!(at(src, q), 9 | 32) {
        return Some(rest_before_comment(src, q + 1));
    }
    match rest {
        Some(rest) if rest(src, q) => None,
        _ => Some(q),
    }
}

pub fn title() -> TokenType {
    TokenType::re("TITLE", Some("\t t"), |src, p| title_match(src, p, None))
}

/// `rest` matches a name suffix so `title` does not split longer names (upstream passes it as
/// regular expression source for a negative lookahead).
pub fn title_before(rest: impl Fn(&[u16], usize) -> bool + 'static) -> TokenType {
    TokenType::re("TITLE", Some("\t t"), move |src, p| title_match(src, p, Some(&rest)))
}

pub fn string() -> TokenType {
    TokenType::re("STRING", Some("\"'"), |src, p| {
        let q = at(src, p);
        if q != 34 && q != 39 {
            return None;
        }
        let mut i = p + 1;
        while i < src.len() {
            let c = src[i] as u32;
            if c == q {
                return Some(i + 1);
            }
            if c == 92 {
                if i + 1 >= src.len() || is_lt(src[i + 1] as u32) {
                    return None;
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        None
    })
}

pub fn newline_token() -> TokenType {
    TokenType::re("NEWLINE", Some("\r\n"), newline)
}

pub fn whitespace() -> TokenType {
    TokenType::re("WHITESPACE", Some("\t "), |src, p| {
        let q = blanks(src, p);
        (q > p).then_some(q)
    })
    .hidden()
}

// `---[\t ]*\r?\n`
fn yaml_open(src: &[u16], p: usize) -> Option<usize> {
    if !starts_with(src, p, "---") {
        return None;
    }
    newline(src, blanks(src, p + 3))
}

pub fn yaml() -> TokenType {
    let mut t = TokenType::re("YAML", Some("-"), |src, p| {
        let q = yaml_open(src, p)?;
        let close = |k: usize| -> Option<usize> {
            if !starts_with(src, k, "---") {
                return None;
            }
            line_or_space(src, k + 3)
        };
        // (?:[\S\s]*?\r?\n)? is tried first, shortest body first.
        for k in q..src.len() {
            if let Some(after) = newline(src, k) {
                if let Some(end) = close(after) {
                    return Some(end);
                }
            }
        }
        close(q)
    })
    .hidden();
    t.opener = Some(Rc::new(yaml_open));
    t
}

// `[\t ]*%%{`
fn directive_open(src: &[u16], p: usize) -> Option<usize> {
    let q = blanks(src, p);
    starts_with(src, q, "%%{").then_some(q + 3)
}

pub fn directive() -> TokenType {
    let mut t = TokenType::re("DIRECTIVE", Some("\t %"), |src, p| {
        let q = directive_open(src, p)?;
        for k in q..src.len() {
            if starts_with(src, k, "}%%") {
                if let Some(end) = line_or_space(src, k + 3) {
                    return Some(end);
                }
            }
        }
        None
    })
    .hidden();
    t.opener = Some(Rc::new(directive_open));
    t
}

pub fn single_line_comment() -> TokenType {
    TokenType::re("SINGLE_LINE_COMMENT", Some("\t %"), |src, p| {
        let q = blanks(src, p);
        starts_with(src, q, "%%").then(|| rest_of_line(src, q + 2))
    })
    .hidden()
}

/// Mermaid ends a keyword only at whitespace, a comment, or the end of input.
pub fn keyword(word: &'static str) -> TokenType {
    let first: &'static str = &word[..word.chars().next().map_or(0, char::len_utf8)];
    TokenType::re(word, Some(first), move |src, p| {
        if !starts_with(src, p, word) {
            return None;
        }
        let e = p + word.encode_utf16().count();
        (starts_with(src, e, "%%") || not_ink(src, e)).then_some(e)
    })
}

fn single(text: &str) -> String {
    collapse_blanks(js::trim(text), ' ')
}

// `.replace(/[\t ]{2,}/gm, with)`
fn collapse_blanks(text: &str, with: char) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in text.chars() {
        if c == ' ' || c == '\t' {
            run.push(c);
            continue;
        }
        if run.len() >= 2 {
            out.push(with);
        } else {
            out.push_str(&run);
        }
        run.clear();
        out.push(c);
    }
    if run.len() >= 2 {
        out.push(with);
    } else {
        out.push_str(&run);
    }
    out
}

fn find(text: &str, word: &str) -> usize {
    text.find(word).expect("the token matched")
}

/// The value of a TITLE token: `title([\t ][^\n\r]*|)`.
pub fn title_value(text: &str) -> String {
    let rest = &text[find(text, "title") + 5..];
    let value = if rest.starts_with([' ', '\t']) { &rest[..rest.find(['\n', '\r']).unwrap_or(rest.len())] } else { "" };
    single(value)
}

/// The value of an ACC_TITLE token: `accTitle[\t ]*:([^\n\r]*)`.
pub fn acc_title_value(text: &str) -> String {
    let rest = text[find(text, "accTitle") + 8..].trim_start_matches([' ', '\t']);
    let rest = &rest[1..];
    single(&rest[..rest.find(['\n', '\r']).unwrap_or(rest.len())])
}

fn is_lt_char(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// The value of an ACC_DESCR token: `accDescr(?:[\t ]*:([^\n\r]*)|\s*{([^}]*)})`.
pub fn acc_descr_value(text: &str) -> String {
    let rest = &text[find(text, "accDescr") + 8..];
    let after = rest.trim_start_matches([' ', '\t']);
    if let Some(v) = after.strip_prefix(':') {
        return single(&v[..v.find(['\n', '\r']).unwrap_or(v.len())]);
    }
    let body = js::trim_start(rest).strip_prefix('{').unwrap_or("");
    let body = &body[..body.find('}').unwrap_or(body.len())];
    let c: Vec<char> = body.chars().collect();
    // .replace(/^\s*/gm, '')
    let mut a: Vec<char> = Vec::new();
    let mut i = 0;
    while i <= c.len() {
        if i == 0 || is_lt_char(c[i - 1]) {
            let mut j = i;
            while j < c.len() && js::is_space(c[j]) {
                j += 1;
            }
            if j > i {
                i = j;
                continue;
            }
        }
        if i < c.len() {
            a.push(c[i]);
        }
        i += 1;
    }
    // .replace(/\s+$/gm, ''): a whitespace run up to its last line terminator, or to the end.
    let mut b: Vec<char> = Vec::new();
    let mut i = 0;
    while i < a.len() {
        if !js::is_space(a[i]) {
            b.push(a[i]);
            i += 1;
            continue;
        }
        let mut e = i;
        while e < a.len() && js::is_space(a[e]) {
            e += 1;
        }
        let k = if e == a.len() { Some(e) } else { (i + 1..e).rev().find(|&k| is_lt_char(a[k])) };
        match k {
            Some(k) => i = k,
            None => {
                b.extend_from_slice(&a[i..e]);
                i = e;
            }
        }
    }
    let s: String = b.into_iter().collect();
    let s = collapse_blanks(&s, ' ');
    // .replace(/[\n\r]{2,}/gm, '\n')
    let mut out = String::new();
    let mut run = String::new();
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            run.push(ch);
            continue;
        }
        out.push_str(if run.len() >= 2 { "\n" } else { &run });
        run.clear();
        out.push(ch);
    }
    out.push_str(if run.len() >= 2 { "\n" } else { &run });
    out
}

/// A STRING token's value: the quotes off, and backslash escapes read.
pub fn string_value(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    if !text.contains('\\') {
        return c[1..c.len() - 1].iter().collect();
    }
    let mut out = String::new();
    let mut i = 1;
    while i + 1 < c.len() {
        let ch = c[i];
        if ch == '\\' {
            i += 1;
            let next = c[i];
            out.push(match next {
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\u{b}',
                '0' => '\0',
                other => other,
            });
        } else {
            out.push(ch);
        }
        i += 1;
    }
    out
}

/// Reads tokens for a hand-written parser, with Chevrotain-style messages on failure.
pub struct Reader<'a> {
    pub i: usize,
    pub tokens: &'a Tokens,
    pub types: &'a [TokenType],
    pub diagram: &'a str,
}

impl<'a> Reader<'a> {
    pub fn new(tokens: &'a Tokens, types: &'a [TokenType], diagram: &'a str) -> Self {
        Reader { i: 0, tokens, types, diagram }
    }

    pub fn kind(&self) -> i32 {
        self.tokens.kinds[self.i]
    }

    pub fn text_at(&self, index: usize) -> String {
        js::from16(&self.tokens.src[self.tokens.starts[index]..self.tokens.ends[index]])
    }

    pub fn text(&self) -> String {
        self.text_at(self.i)
    }

    pub fn take(&mut self) -> String {
        let t = self.text_at(self.i);
        self.i += 1;
        t
    }

    pub fn accept(&mut self, kind: i32) -> bool {
        if self.kind() != kind {
            return false;
        }
        self.i += 1;
        true
    }

    pub fn expect(&mut self, kind: i32) -> Result<String, PeleError> {
        if self.kind() != kind {
            return Err(self.fail(&format!("token of type '{}'", self.types[kind as usize].name)));
        }
        Ok(self.take())
    }

    pub fn fail(&self, expected: &str) -> PeleError {
        let found = if self.kind() == -1 { "end of input".to_string() } else { format!("`{}`", self.text()) };
        let t = self.tokens;
        let mut at = t.starts[self.i];
        if self.kind() == -1 {
            // Locate EOF errors after the last token, before trailing line breaks and hidden comments.
            let mut k = self.i;
            while k > 0 && js::trim(&js::from16(&t.src[t.starts[k - 1]..t.ends[k - 1]])).is_empty() {
                k -= 1;
            }
            if k > 0 {
                at = t.ends[k - 1];
            }
        }
        syntax_error(self.diagram, &t.src, at, &format!("Expecting {} but found {}.", expected, found), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_take_the_first_match_and_skip_hidden_ones() {
        let types = vec![keyword("pie"), title(), string(), newline_token(), whitespace(), single_line_comment()];
        let src = js::u16s("pie title Pets %% c\n\"a\\\"b\"\n");
        let t = tokenize(&src, &types, "pie").unwrap();
        assert_eq!(t.kinds, vec![0, 1, 3, 2, 3, -1]);
        let r = Reader::new(&t, &types, "pie");
        assert_eq!(title_value(&r.text_at(1)), "Pets");
        assert_eq!(string_value(&r.text_at(3)), "a\"b");
        assert!(tokenize(&js::u16s("pie ?"), &types, "pie").is_err());
    }

    #[test]
    fn descriptions_read_as_mermaid_reads_them() {
        assert_eq!(acc_title_value("accTitle :  A   title "), "A title");
        assert_eq!(acc_descr_value("accDescr: one  line"), "one line");
        assert_eq!(acc_descr_value("accDescr {\n   first  \n\n\n   second\n}"), "first\nsecond");
        assert_eq!(acc_descr_value("accDescr{a\nb}"), "a\nb");
    }

    #[test]
    fn yaml_and_directives_end_at_their_closers() {
        let types = vec![yaml(), directive(), keyword("x"), newline_token(), whitespace()];
        let t = tokenize(&js::u16s("---\na: 1\n---\n%%{init: {}}%%\nx\n"), &types, "x").unwrap();
        assert_eq!(t.kinds, vec![2, 3, -1]);
    }
}

// Port of src/diagrams/common/scan.ts: lexer rules that Mermaid's journey and timeline grammars
// have in common, ported from the compiled Jison rules: comments, whitespace, and the accTitle
// and accDescr statements.

use crate::js;
use crate::util::chars::is_space;

#[derive(Default, Debug)]
pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

pub fn line_end(src: &[u16], from: usize) -> usize {
    src[from.min(src.len())..].iter().position(|&c| c == 10).map_or(src.len(), |k| from + k)
}

/// `word` is lowercase ASCII letters; the grammars are case-insensitive.
pub fn keyword_at(src: &[u16], p: usize, word: &str) -> bool {
    word.bytes().enumerate().all(|(i, w)| at(src, p + i) != u32::MAX && (at(src, p + i) | 32) == w as u32)
}

/// Skips comments and whitespace. Stops at a line break that starts a token, which is a
/// NEWLINE. Jison compiles `\%%` to `%`, so a single `%` starts a comment.
pub fn skip_trivia(src: &[u16], from: usize) -> usize {
    let n = src.len();
    let mut p = from;
    while p < n {
        let c = src[p] as u32;
        if c == 37 && at(src, p + 1) != 123 {
            p = line_end(src, p);
        } else if c == 10 {
            return p;
        } else if is_space(c) {
            p += 1;
            while p < n && is_space(src[p] as u32) {
                p += 1;
            }
        } else if c == 35 {
            p = line_end(src, p);
        } else {
            return p;
        }
    }
    p
}

fn skip_space(src: &[u16], from: usize) -> usize {
    let mut p = from;
    while is_space(at(src, p)) {
        p += 1;
    }
    p
}

/// Lexes an accTitle or accDescr statement at `p` and returns where it ends, or `p` if there is
/// none. The token types acc_title, acc_title_value, acc_descr, acc_descr_value and
/// acc_descr_multiline_value must be numbered consecutively from `first`.
pub fn accessibility(src: &[u16], p: usize, out: &mut Tokens, first: u32) -> usize {
    let descr = keyword_at(src, p, "accdescr");
    if !descr && !keyword_at(src, p, "acctitle") {
        return p;
    }
    let n = src.len();
    let mut q = skip_space(src, p + 8);
    let c = at(src, q);
    let mut emit = |ty: u32, s: usize, e: usize| {
        out.types.push(ty);
        out.texts.push(js::from16(&src[s..e]));
        out.starts.push(s);
    };
    if c == 58 {
        q = skip_space(src, q + 1);
        let end = line_end(src, q);
        emit(if descr { first + 2 } else { first }, p, q);
        emit(if descr { first + 3 } else { first + 1 }, q, end);
        return end;
    }
    if !descr || c != 123 {
        return p;
    }
    q = skip_space(src, q + 1);
    while at(src, q) != 125 {
        let end = src[q.min(n)..].iter().position(|&c| c == 125).map_or(n, |k| q + k);
        emit(first + 4, q, end);
        if q == n {
            return n;
        }
        q = end;
    }
    q + 1
}

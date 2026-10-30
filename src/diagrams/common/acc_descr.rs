// Port of src/diagrams/common/accDescr.ts.

use super::tokens::{acc_descr as acc_descr_token, blanks, rest_before_comment, Scanner, TokenType};
use crate::js;
use std::rc::Rc;

// RE_HEAD /[\t ]*accDescr(?:[\t ]*:(?:[^\n\r]*?(?=%%)|[^\n\r]*)|\s*({))/y: the end of the
// match, and whether it was the braced form.
fn head(src: &[u16], p: usize) -> Option<(usize, bool)> {
    let q = blanks(src, p);
    if !js::from16(&src[q.min(src.len())..(q + 8).min(src.len())]).eq("accDescr") {
        return None;
    }
    let q = q + 8;
    let r = blanks(src, q);
    if src.get(r) == Some(&58) {
        return Some((rest_before_comment(src, r + 1), false));
    }
    let mut r = q;
    while r < src.len() && js::is_space_u(src[r] as u32) {
        r += 1;
    }
    (src.get(r) == Some(&123)).then_some((r + 1, true))
}

// The braced form scans for its closing brace. Once none is found there is none further on,
// so later braced descriptions fail without scanning the rest of the text again.
fn scanner() -> Scanner {
    let mut unclosed = false;
    Box::new(move |src, at, _| {
        let (end, braced) = head(src, at)?;
        if !braced {
            return Some(end);
        }
        if unclosed {
            return None;
        }
        let close = src[end..].iter().position(|&c| c == 125).map(|k| end + k);
        unclosed = close.is_none();
        close.map(|c| c + 1)
    })
}

/// The common accDescr terminal for grammars where lexing goes on after an `accDescr {` that
/// never closes, because `accDescr` is also a name there. Each one would otherwise read to the
/// end of the text.
pub fn acc_descr(name: &str) -> TokenType {
    let mut t = acc_descr_token().named(name);
    t.scanner = Some(Rc::new(scanner));
    t
}

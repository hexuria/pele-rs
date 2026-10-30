// Port of src/diagrams/timeline/parser.ts.

use super::db::TimelineDb;
use super::lexer::*;
use crate::errors::{syntax_error, PeleError};
use crate::js;

const STATEMENT: &str = "'EOF', 'SPACE', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'section', 'period', 'event'";

// `text.substring(k)`, for a text whose first k code units are BMP characters.
fn after(text: &str, k: usize) -> String {
    text.chars().skip(k).collect()
}

pub fn parse_timeline(src: &[u16], db: &mut TimelineDb) -> Result<(), PeleError> {
    let t = tokenize(src);
    let fail = |i: usize, expected: &str| -> PeleError {
        let got = if t.types[i] == END { "1" } else { TOKEN_NAMES[t.types[i] as usize] };
        syntax_error("timeline", src, t.starts[i], &format!("Expecting {}, got '{}'", expected, got), false)
    };
    let head = t.types[0];
    if head != TIMELINE && head != TIMELINE_LR && head != TIMELINE_TD {
        return Err(fail(0, "'timeline', 'timeline_lr', 'timeline_td'"));
    }
    if head != TIMELINE {
        db.direction = if head == TIMELINE_LR { "LR" } else { "TD" };
    }
    let mut i = 1;
    loop {
        let text = &t.texts[i];
        match t.types[i] {
            NEWLINE => {}
            TITLE => db.set_diagram_title(&after(text, 6)),
            ACC_TITLE => {
                i += 1;
                db.set_acc_title(js::trim(&t.texts[i]));
            }
            ACC_DESCR => {
                i += 1;
                db.set_acc_description(js::trim(&t.texts[i]));
            }
            ACC_DESCR_MULTILINE_VALUE => db.set_acc_description(js::trim(text)),
            SECTION => db.add_section(&after(text, 8)),
            PERIOD => db.add_task(text, ""),
            EVENT => db.add_event(&after(text, 2))?,
            EOF => return Ok(()),
            _ => return Err(fail(i, STATEMENT)),
        }
        i += 1;
    }
}

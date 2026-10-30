// Port of src/diagrams/journey/parser.ts.

use super::db::JourneyDb;
use super::lexer::*;
use crate::errors::{syntax_error, PeleError};
use crate::js;

const STATEMENT: &str = "'EOF', 'SPACE', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'section', 'taskName'";

// `text.substring(k)`, for a text whose first k code units are BMP characters.
fn after(text: &str, k: usize) -> String {
    text.chars().skip(k).collect()
}

pub fn parse_journey(src: &[u16], db: &mut JourneyDb) -> Result<(), PeleError> {
    let t = tokenize(src);
    let fail = |i: usize, expected: &str| -> PeleError {
        let got = if t.types[i] == END { "1" } else { TOKEN_NAMES[t.types[i] as usize] };
        syntax_error("journey", src, t.starts[i], &format!("Expecting {}, got '{}'", expected, got), false)
    };
    if t.types[0] != JOURNEY {
        return Err(fail(0, "'journey'"));
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
            TASK_NAME => {
                i += 1;
                if t.types[i] != TASK_DATA {
                    return Err(fail(i, "'taskData'"));
                }
                db.add_task(text, &t.texts[i]);
            }
            EOF => return Ok(()),
            _ => return Err(fail(i, STATEMENT)),
        }
        i += 1;
    }
}

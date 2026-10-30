// Port of src/diagrams/timeline/db.ts.

use crate::diagrams::info::newline_dedent;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;

pub struct TimelinePeriod {
    pub text: String,
    pub events: Vec<String>,
    // Name of the section in force when the period was declared, '' before the first one.
    pub section: String,
    // Index into `sections` of that section, -1 before the first one.
    pub section_index: i64,
}

pub struct TimelineDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    // 'LR' or 'TD'.
    pub direction: &'static str,
    pub sections: Vec<String>,
    pub periods: Vec<TimelinePeriod>,
    current_section: String,
}

impl Default for TimelineDb {
    fn default() -> Self {
        TimelineDb { title: None, acc_title: None, acc_descr: None, direction: "LR", sections: Vec::new(), periods: Vec::new(), current_section: String::new() }
    }
}

impl TimelineDb {
    pub fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_string());
    }
    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }
    pub fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(newline_dedent(text));
    }
    pub fn add_section(&mut self, text: &str) {
        self.current_section = text.to_string();
        self.sections.push(text.to_string());
    }
    pub fn add_task(&mut self, period: &str, event: &str) {
        self.periods.push(TimelinePeriod {
            text: period.to_string(),
            events: if event.is_empty() { Vec::new() } else { vec![event.to_string()] },
            section: self.current_section.clone(),
            section_index: self.sections.len() as i64 - 1,
        });
    }
    pub fn add_event(&mut self, event: &str) -> Result<(), PeleError> {
        // Mermaid fails here too, with a TypeError.
        let Some(period) = self.periods.last_mut() else {
            return Err(PeleError::typed("An event needs a time period before it.", PeleErrorCode::Semantic, "timeline"));
        };
        period.events.push(event.to_string());
        Ok(())
    }
}

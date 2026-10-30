// Port of src/diagrams/gantt/db.ts and types.ts.

use super::clock::Date;
use super::dates::{add, add_days, format, iso_date, parse_strict, WEEKDAYS};
use super::parse_date::parse_date;
use crate::diagrams::info::newline_dedent;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use crate::util::url::safe_url;
use std::collections::{HashMap, HashSet};

pub struct GanttTask {
    pub id: String,
    // The task's label, under the name Mermaid gives it.
    pub task: String,
    pub section: String,
    // Unset until scheduled, and still unset when what it refers to never resolves.
    pub start_time: Option<Date>,
    pub end_time: Option<Date>,
    // Where the bar ends when excluded days pushed the task's end out.
    pub render_end_time: Option<Date>,
    pub manual_end_time: bool,
    pub processed: bool,
    pub active: bool,
    pub done: bool,
    pub crit: bool,
    pub milestone: bool,
    pub vert: bool,
    // Row index in source order; -1 for vertical markers, which take no row.
    pub order: i64,
    pub classes: Vec<String>,
    pub prev_task_id: Option<String>,
    raw_start: Option<String>,
    raw_end: String,
}

// Scheduling walks tasks day by day and parses dates with a format the diagram supplies. This
// bounds the total, in rough character operations, so a hostile diagram fails instead of hanging.
const WORK_LIMIT: f64 = 4e8;
const DAY_STEP: f64 = 400.0;
const DATED_STEP: f64 = 200.0;
const FORMAT_STEP: f64 = 400.0;

/// The one place the current time is read.
pub fn clock() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

// /^(\d+(?:\.\d+)?)([Mdhmswy]|ms)$/ on the trimmed text.
pub fn parse_duration(s: &str) -> (f64, String) {
    let s = js::trim(s);
    let digits = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return (f64::NAN, "ms".into());
    }
    let mut e = digits;
    let b = s.as_bytes();
    if e < b.len() && b[e] == b'.' {
        let f = b[e + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if f > 0 {
            e += 1 + f;
        }
    }
    let unit = &s[e..];
    if matches!(unit, "M" | "d" | "h" | "m" | "s" | "w" | "y" | "ms") {
        (js::parse_float(&s[..e]), unit.to_string())
    } else {
        (f64::NAN, "ms".into())
    }
}

fn merge_tokens(existing: &[String], txt: &str) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    for t in existing {
        if !merged.contains(t) {
            merged.push(t.clone());
        }
    }
    for token in txt.to_lowercase().split(|c: char| js::is_space(c) || c == ',') {
        if !token.is_empty() && !merged.iter().any(|m| m == token) {
            merged.push(token.to_string());
        }
    }
    merged
}

// /^\s*(active|done|crit|milestone|vert)\s*$/
fn tag(s: &str) -> Option<&str> {
    let t = js::trim(s);
    matches!(t, "active" | "done" | "crit" | "milestone" | "vert").then_some(t)
}

// /^[YMDHhmsSZXx\-/.:, T]*$/
fn numeric_format(s: &str) -> bool {
    s.chars().all(|c| "YMDHhmsSZXx-/.:, T".contains(c))
}

// /^(after|until)\s+([\d\w- ]+)/: the list.
fn reference_list<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(word)?;
    let after = js::trim_start(rest);
    if after.len() == rest.len() {
        return None;
    }
    let n = after.bytes().take_while(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b' ').count();
    (n > 0).then_some(&after[..n])
}

#[derive(Default)]
pub struct GanttDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub date_format: String,
    pub axis_format: String,
    pub tick_interval: Option<String>,
    pub today_marker: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub inclusive_end_dates: bool,
    pub top_axis: bool,
    pub display_mode: String,
    pub weekday: Option<String>,
    pub weekend: Option<String>,
    pub sections: Vec<String>,
    pub links: HashMap<String, String>,
    pub tasks: Vec<GanttTask>,
    by_id: HashMap<String, usize>,
    current_section: String,
    task_count: usize,
    last_order: i64,
    last_task_id: Option<String>,
    now: f64,
    work: f64,
    include_set: HashSet<String>,
    exclude_set: HashSet<String>,
    excluded_days: [bool; 7],
    dated_tokens: bool,
}

fn semantic(message: String) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "gantt")
}

impl GanttDb {
    pub fn set_include_tokens(&mut self, txt: &str) {
        self.includes = merge_tokens(&self.includes, txt);
    }
    pub fn set_exclude_tokens(&mut self, txt: &str) {
        self.excludes = merge_tokens(&self.excludes, txt);
    }
    pub fn set_acc_title(&mut self, txt: &str) {
        self.acc_title = Some(js::trim_start(txt).to_string());
    }
    pub fn set_acc_description(&mut self, txt: &str) {
        self.acc_descr = Some(newline_dedent(txt));
    }
    pub fn add_section(&mut self, txt: &str) {
        self.current_section = txt.to_string();
        self.sections.push(txt.to_string());
    }

    pub fn add_task(&mut self, descr: &str, data_str: &str) -> Result<(), PeleError> {
        let body = data_str.strip_prefix(':').unwrap_or(data_str);
        let mut data: Vec<String> = body.split(',').map(str::to_string).collect();
        let mut task = GanttTask {
            id: String::new(),
            task: descr.to_string(),
            section: self.current_section.clone(),
            start_time: None,
            end_time: None,
            render_end_time: None,
            manual_end_time: false,
            processed: false,
            active: false,
            done: false,
            crit: false,
            milestone: false,
            vert: false,
            order: -1,
            classes: Vec::new(),
            prev_task_id: self.last_task_id.clone(),
            raw_start: None,
            raw_end: String::new(),
        };
        while let Some(t) = data.first().and_then(|d| tag(d)) {
            match t {
                "active" => task.active = true,
                "done" => task.done = true,
                "crit" => task.crit = true,
                "milestone" => task.milestone = true,
                _ => task.vert = true,
            }
            data.remove(0);
        }
        // Mermaid fails with a TypeError on both of these.
        if data.is_empty() {
            return Err(semantic(format!("Task \"{}\" needs a duration or an end date.", js::trim(descr))));
        }
        if data.len() > 3 {
            return Err(semantic(format!("Task \"{}\" has too many fields. A task takes an id, a start, and an end or duration.", js::trim(descr))));
        }
        for d in data.iter_mut() {
            *d = js::trim(d).to_string();
        }
        if data.len() == 3 {
            task.id = data[0].clone();
        } else {
            self.task_count += 1;
            task.id = format!("task{}", self.task_count);
        }
        if data.len() > 1 {
            task.raw_start = Some(data[data.len() - 2].clone());
        }
        task.raw_end = data[data.len() - 1].clone();
        if !task.vert {
            task.order = self.last_order;
            self.last_order += 1;
        }
        self.last_task_id = Some(task.id.clone());
        self.by_id.insert(task.id.clone(), self.tasks.len());
        self.tasks.push(task);
        Ok(())
    }

    pub fn set_link(&mut self, ids: &str, link: &str) {
        let href = safe_url(link);
        for id in ids.split(',') {
            if self.by_id.contains_key(id) {
                self.links.insert(id.to_string(), href.clone());
            }
        }
        self.set_class(ids, "clickable");
    }

    pub fn set_class(&mut self, ids: &str, class_name: &str) {
        for id in ids.split(',') {
            if let Some(&k) = self.by_id.get(id) {
                self.tasks[k].classes.push(class_name.to_string());
            }
        }
    }

    /// Resolves every task's start and end, from scratch. A task may refer to tasks defined after
    /// it, so, like Mermaid, this makes up to eleven passes.
    pub fn get_tasks(&mut self, now: f64) -> Result<(), PeleError> {
        self.now = now;
        self.work = 0.0;
        self.include_set = self.includes.iter().cloned().collect();
        self.exclude_set = self.excludes.iter().cloned().collect();
        let weekend = self.weekend.as_deref().unwrap_or("saturday");
        let weekend_start = match weekend {
            "friday" => 5.0,
            "saturday" => 6.0,
            _ => f64::NAN,
        };
        let weekends = self.exclude_set.contains("weekends");
        for day in 0..7 {
            let iso = if day == 0 { 7.0 } else { day as f64 };
            self.excluded_days[day] = (weekends && (iso == weekend_start || iso == weekend_start + 1.0)) || self.exclude_set.contains(&WEEKDAYS[day].to_lowercase());
        }
        self.dated_tokens = !self.includes.is_empty() || !numeric_format(js::trim(&self.date_format));
        for token in &self.excludes {
            if token != "weekends" && !WEEKDAYS.iter().any(|d| d.to_lowercase() == *token) {
                self.dated_tokens = true;
            }
        }
        for task in self.tasks.iter_mut() {
            task.start_time = None;
            task.end_time = None;
            task.render_end_time = None;
            task.processed = false;
            task.manual_end_time = false;
        }
        let mut all = self.compile_tasks()?;
        let mut i = 0;
        while !all && i < 10 {
            all = self.compile_tasks()?;
            i += 1;
        }
        Ok(())
    }

    /// Whether a day is excluded: by its weekday name, by its date as YYYY-MM-DD, or by its date
    /// written in the chart's own format.
    pub fn is_invalid_date(&mut self, date: Date) -> Result<bool, PeleError> {
        let day = date.day();
        let weekday = !day.is_nan() && self.excluded_days[day as usize];
        if !self.dated_tokens {
            return Ok(weekday);
        }
        let template = js::trim(&self.date_format).to_string();
        let date_only = iso_date(date);
        let mut formatted = Some(date_only.clone());
        self.spend(DATED_STEP)?;
        if template != "YYYY-MM-DD" {
            self.spend(FORMAT_STEP + js::len16(&template) as f64 * 8.0)?;
            formatted = format(date, &template);
        }
        let listed = |set: &HashSet<String>| set.contains(&date_only) || formatted.as_ref().is_some_and(|f| set.contains(f));
        if listed(&self.include_set) {
            return Ok(false);
        }
        Ok(weekday || listed(&self.exclude_set))
    }

    fn spend(&mut self, units: f64) -> Result<(), PeleError> {
        self.work += units;
        if self.work > WORK_LIMIT {
            return Err(PeleError::typed("The gantt chart takes too much work to schedule.", PeleErrorCode::Limit, "gantt"));
        }
        Ok(())
    }

    fn parse(&mut self, s: &str, template: &str) -> Result<Option<Date>, PeleError> {
        self.spend((js::len16(template) as f64 + 1.0) * (js::len16(s) as f64 + 1.0))?;
        Ok(parse_strict(s, template, Date(self.now)))
    }

    fn today(&self) -> Date {
        let mut today = Date(self.now);
        today.set_hours(0.0, 0.0, 0.0, 0.0);
        today
    }

    fn compile_tasks(&mut self) -> Result<bool, PeleError> {
        let mut all = true;
        for k in 0..self.tasks.len() {
            self.compile_task(k)?;
            all = all && self.tasks[k].processed;
        }
        Ok(all)
    }

    fn compile_task(&mut self, k: usize) -> Result<(), PeleError> {
        match self.tasks[k].raw_start.clone() {
            None => {
                // Mermaid fails with a TypeError when the first task has no start. It starts today here.
                let previous = self.tasks[k].prev_task_id.as_ref().and_then(|id| self.by_id.get(id)).copied();
                self.tasks[k].start_time = match previous {
                    Some(p) => self.tasks[p].end_time,
                    None => Some(self.today()),
                };
            }
            Some(start) => {
                if let Some(s) = self.get_start_date(&start)? {
                    self.tasks[k].start_time = Some(s);
                }
            }
        }
        let Some(start) = self.tasks[k].start_time else { return Ok(()) };
        let raw_end = self.tasks[k].raw_end.clone();
        let end = self.get_end_date(start, &raw_end)?;
        self.tasks[k].end_time = end;
        if end.is_none() {
            return Ok(());
        }
        self.tasks[k].processed = true;
        self.tasks[k].manual_end_time = self.parse(&raw_end, "YYYY-MM-DD")?.is_some();
        self.check_task_dates(k)
    }

    fn references(&self, list: &str) -> Vec<usize> {
        list.split(' ').filter(|id| !id.is_empty()).filter_map(|id| self.by_id.get(id).copied()).collect()
    }

    fn get_start_date(&mut self, raw: &str) -> Result<Option<Date>, PeleError> {
        let s = js::trim(raw).to_string();
        let template = js::trim(&self.date_format).to_string();
        if (template == "x" || template == "X") && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            let date = Date(js::to_number(&s));
            let mut d = date;
            d.set_time(date.0);
            if !d.valid() {
                return Err(semantic(format!("Invalid date:{}", s)));
            }
            return Ok(Some(d));
        }
        if let Some(list) = reference_list(&s, "after") {
            let mut latest: Option<usize> = None;
            for t in self.references(list) {
                if latest.is_none_or(|l| self.tasks[t].end_time.map_or(f64::NAN, |d| d.0) > self.tasks[l].end_time.map_or(f64::NAN, |d| d.0)) {
                    latest = Some(t);
                }
            }
            return Ok(match latest {
                Some(l) => self.tasks[l].end_time,
                None => Some(self.today()),
            });
        }
        if let Some(parsed) = self.parse(&s, &template)? {
            return Ok(Some(parsed));
        }
        // What does not fit the format gets a second chance with the JavaScript date parser, as in Mermaid.
        let date = Date(parse_date(&s));
        if !date.valid() || date.full_year() < -10000.0 || date.full_year() > 10000.0 {
            return Err(semantic(format!("Invalid date:{}", s)));
        }
        Ok(Some(date))
    }

    fn get_end_date(&mut self, start_time: Date, raw: &str) -> Result<Option<Date>, PeleError> {
        let s = js::trim(raw).to_string();
        if let Some(list) = reference_list(&s, "until") {
            let mut earliest: Option<usize> = None;
            for t in self.references(list) {
                if earliest.is_none_or(|e| self.tasks[t].start_time.map_or(f64::NAN, |d| d.0) < self.tasks[e].start_time.map_or(f64::NAN, |d| d.0)) {
                    earliest = Some(t);
                }
            }
            return Ok(match earliest {
                Some(e) => self.tasks[e].start_time,
                None => Some(self.today()),
            });
        }
        let template = js::trim(&self.date_format).to_string();
        if let Some(parsed) = self.parse(&s, &template)? {
            return Ok(Some(if self.inclusive_end_dates { add_days(parsed, 1.0) } else { parsed }));
        }
        let (value, unit) = parse_duration(&s);
        if value.is_nan() {
            return Ok(Some(start_time));
        }
        let end = add(start_time, value, &unit);
        Ok(Some(if end.valid() { end } else { start_time }))
    }

    // Pushes the end of a task out by one day for every excluded day it covers.
    fn check_task_dates(&mut self, k: usize) -> Result<(), PeleError> {
        if self.excludes.is_empty() || self.tasks[k].manual_end_time {
            return Ok(());
        }
        let mut cursor = add_days(self.tasks[k].start_time.unwrap(), 1.0);
        let mut end_time = self.tasks[k].end_time.unwrap();
        let max_end_time = add_days(end_time, 10000.0).0;
        let mut invalid = false;
        let mut render_end_time: Option<f64> = None;
        while cursor.0 <= end_time.0 {
            self.spend(DAY_STEP)?;
            if !invalid {
                render_end_time = Some(end_time.0);
            }
            invalid = self.is_invalid_date(cursor)?;
            if invalid {
                end_time.set_date(end_time.date() + 1.0);
                if end_time.0 > max_end_time {
                    return Err(semantic("Failed to find a valid date that was not excluded by `excludes` after 10,000 iterations.".into()));
                }
            }
            cursor.set_date(cursor.date() + 1.0);
        }
        self.tasks[k].end_time = Some(end_time);
        self.tasks[k].render_end_time = render_end_time.map(Date);
        Ok(())
    }
}

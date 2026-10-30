// Port of src/diagrams/gantt/dates.ts: the parts of dayjs 1.11 that Mermaid's gantt code relies
// on, with the plugins it loads (custom parse format, advanced format, ISO week) and the English
// locale: strict parsing against a format, formatting, and calendar arithmetic, all in local
// time. Quirks are kept, because a date is accepted only when formatting the parsed result gives
// back the input.

use super::clock::Date;
use crate::js::{self, num_str};

pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

const DAY: f64 = 86400000.0;
const WEEK: f64 = 7.0 * DAY;

fn pad(value: f64, length: usize) -> String {
    let s = num_str(value);
    if s.len() >= length {
        s
    } else {
        "0".repeat(length - s.len()) + &s
    }
}

fn ordinal(n: f64) -> String {
    let s = ["th", "st", "nd", "rd"];
    let v = n % 100.0;
    let k = (v - 20.0) % 10.0;
    let pick = |i: f64| if i >= 0.0 && i < 4.0 && i.fract() == 0.0 { Some(s[i as usize]) } else { None };
    num_str(n) + pick(k).or(pick(v)).unwrap_or(s[0])
}

fn utc_offset(date: Date) -> f64 {
    let offset = -js::round(date.timezone_offset() / 15.0) * 15.0;
    if offset == 0.0 {
        0.0
    } else {
        offset
    }
}

fn zone(date: Date) -> String {
    let offset = utc_offset(date);
    let minutes = offset.abs();
    format!("{}{}:{}", if offset >= 0.0 { '+' } else { '-' }, pad((minutes / 60.0).floor(), 2), pad(minutes % 60.0, 2))
}

pub fn iso_weekday(date: Date) -> f64 {
    let d = date.day();
    if d == 0.0 || d.is_nan() {
        7.0
    } else {
        d
    }
}

pub fn add_days(date: Date, days: f64) -> Date {
    let mut next = date;
    next.set_date(date.date() + days);
    next
}

fn days_in_month(date: Date) -> f64 {
    let mut last = Date::local(date.full_year(), date.month() + 1.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    last.set_hours(23.0, 59.0, 59.0, 999.0);
    last.date()
}

// Sets the month or the year, keeping the day of the month unless the new month is shorter.
fn set_calendar(date: Date, apply: impl FnOnce(&mut Date)) -> Date {
    let mut next = date;
    next.set_date(1.0);
    apply(&mut next);
    let d = date.date().min(days_in_month(next));
    next.set_date(if date.date().is_nan() || days_in_month(next).is_nan() { f64::NAN } else { d });
    next
}

pub fn add(date: Date, amount: f64, unit: &str) -> Date {
    match unit {
        "M" => set_calendar(date, |d| d.set_month(date.month() + amount)),
        "y" => set_calendar(date, |d| d.set_full_year(date.full_year() + amount)),
        "d" => add_days(date, js::round(amount)),
        "w" => add_days(date, js::round(7.0 * amount)),
        "h" => Date(date.time() + amount * 3600000.0),
        "m" => Date(date.time() + amount * 60000.0),
        "s" => Date(date.time() + amount * 1000.0),
        _ => Date(date.time() + amount),
    }
    .clip()
}

impl Date {
    fn clip(self) -> Date {
        let mut d = self;
        d.set_time(self.0);
        d
    }
}

fn week_thursday(date: Date) -> Date {
    add_days(date, 4.0 - iso_weekday(date))
}

fn iso_week(date: Date) -> f64 {
    let thursday = week_thursday(date);
    let jan1 = Date::local(thursday.full_year(), 0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
    let weekday = iso_weekday(jan1);
    let first = add_days(jan1, 4.0 - weekday + if weekday > 4.0 { 7.0 } else { 0.0 });
    let zone_delta = (utc_offset(first) - utc_offset(thursday)) * 60000.0;
    let weeks = (thursday.time() - first.time() - zone_delta) / WEEK;
    (if weeks < 0.0 { js::or(weeks.ceil(), || 0.0) + 0.0 } else { weeks.floor() }) + 1.0
}

// Splits `str` on a regex given as a matcher at each position: (kind, matched text) pieces, the
// text between matches as kind None.
fn scan<'a>(s: &'a [char], matcher: impl Fn(&[char], usize) -> Option<usize>) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut plain = String::new();
    while i < s.len() {
        if let Some(e) = matcher(s, i) {
            if !plain.is_empty() {
                out.push((false, std::mem::take(&mut plain)));
            }
            out.push((true, s[i..e].iter().collect()));
            i = e;
        } else {
            plain.push(s[i]);
            i += 1;
        }
    }
    if !plain.is_empty() {
        out.push((false, plain));
    }
    out
}

fn bracket(s: &[char], i: usize) -> Option<usize> {
    if s[i] != '[' {
        return None;
    }
    let close = s[i + 1..].iter().position(|&c| c == ']')?;
    (close > 0).then_some(i + 1 + close + 1)
}

fn starts(s: &[char], i: usize, w: &str) -> bool {
    let w: Vec<char> = w.chars().collect();
    i + w.len() <= s.len() && s[i..i + w.len()] == w[..]
}

fn first_of(s: &[char], i: usize, words: &[&str]) -> Option<usize> {
    words.iter().find(|w| starts(s, i, w)).map(|w| i + w.chars().count())
}

fn run_of(s: &[char], i: usize, c: char, min: usize, max: usize) -> Option<usize> {
    let mut e = i;
    while e < s.len() && s[e] == c && e - i < max {
        e += 1;
    }
    (e - i >= min).then_some(e)
}

// /\[([^\]]+)]|Q|wo|ww|w|WW|W|zzz|z|gggg|GGGG|Do|X|x|k{1,2}|S/g
fn advanced_token(s: &[char], i: usize) -> Option<usize> {
    bracket(s, i).or_else(|| first_of(s, i, &["Q", "wo", "ww", "w", "WW", "W", "zzz", "z", "gggg", "GGGG", "Do", "X", "x"])).or_else(|| run_of(s, i, 'k', 1, 2)).or_else(|| first_of(s, i, &["S"]))
}

// /\[([^\]]+)]|YYYY|YY|M{1,4}|D{1,2}|d{1,4}|H{1,2}|h{1,2}|a|A|m{1,2}|s{1,2}|Z{1,2}|SSS/g
fn format_token(s: &[char], i: usize) -> Option<usize> {
    bracket(s, i)
        .or_else(|| first_of(s, i, &["YYYY", "YY"]))
        .or_else(|| run_of(s, i, 'M', 1, 4))
        .or_else(|| run_of(s, i, 'D', 1, 2))
        .or_else(|| run_of(s, i, 'd', 1, 4))
        .or_else(|| run_of(s, i, 'H', 1, 2))
        .or_else(|| run_of(s, i, 'h', 1, 2))
        .or_else(|| first_of(s, i, &["a", "A"]))
        .or_else(|| run_of(s, i, 'm', 1, 2))
        .or_else(|| run_of(s, i, 's', 1, 2))
        .or_else(|| run_of(s, i, 'Z', 1, 2))
        .or_else(|| first_of(s, i, &["SSS"]))
}

/// None for the week-of-year and time zone name tokens, which need dayjs plugins Mermaid does
/// not load and so fail there.
pub fn format(date: Date, template: &str) -> Option<String> {
    if !date.valid() {
        return Some("Invalid Date".into());
    }
    let s = if template.is_empty() { "YYYY-MM-DDTHH:mm:ssZ" } else { template };
    if s == "YYYY-MM-DD" {
        return Some(iso_date(date));
    }
    let chars: Vec<char> = s.chars().collect();
    let mut unsupported = false;
    let mut advanced = String::new();
    for (token, text) in scan(&chars, advanced_token) {
        if !token {
            advanced.push_str(&text);
            continue;
        }
        let f = date.fields();
        advanced.push_str(&match text.as_str() {
            "Q" => num_str(((f.month + 1.0) / 3.0).ceil()),
            "Do" => format!("[{}]", ordinal(f.date)),
            "GGGG" => num_str(week_thursday(date).full_year()),
            "W" => num_str(iso_week(date)),
            "WW" => pad(iso_week(date), 2),
            "k" => num_str(js::or(f.hours, || 24.0)),
            "kk" => pad(js::or(f.hours, || 24.0), 2),
            "X" => num_str((date.time() / 1000.0).floor()),
            "x" => num_str(date.time()),
            "gggg" | "wo" | "w" | "ww" | "z" | "zzz" => {
                unsupported = true;
                text.clone()
            }
            _ => text.clone(),
        });
    }
    if unsupported {
        return None;
    }
    let f = date.fields();
    let hours = f.hours;
    let chars: Vec<char> = advanced.chars().collect();
    let mut out = String::new();
    for (token, text) in scan(&chars, format_token) {
        if !token {
            out.push_str(&text);
            continue;
        }
        if text.starts_with('[') {
            out.push_str(&text[1..text.len() - 1]);
            continue;
        }
        out.push_str(&match text.as_str() {
            "YY" => {
                let y = num_str(f.year);
                let n = y.chars().count();
                y.chars().skip(n.saturating_sub(2)).collect()
            }
            "YYYY" => pad(f.year, 4),
            "M" => num_str(f.month + 1.0),
            "MM" => pad(f.month + 1.0, 2),
            "MMM" => MONTHS[f.month as usize][..3].to_string(),
            "MMMM" => MONTHS[f.month as usize].to_string(),
            "D" => num_str(f.date),
            "DD" => pad(f.date, 2),
            "d" => num_str(f.day),
            "dd" => WEEKDAYS[f.day as usize][..2].to_string(),
            "ddd" => WEEKDAYS[f.day as usize][..3].to_string(),
            "dddd" => WEEKDAYS[f.day as usize].to_string(),
            "H" => num_str(hours),
            "HH" => pad(hours, 2),
            "h" => num_str(js::or(hours % 12.0, || 12.0)),
            "hh" => pad(js::or(hours % 12.0, || 12.0), 2),
            "a" => (if hours < 12.0 { "am" } else { "pm" }).to_string(),
            "A" => (if hours < 12.0 { "AM" } else { "PM" }).to_string(),
            "m" => num_str(f.minutes),
            "mm" => pad(f.minutes, 2),
            "s" => num_str(f.seconds),
            "ss" => pad(f.seconds, 2),
            "SSS" => pad(f.ms, 3),
            "Z" => zone(date),
            _ => zone(date).replacen(':', "", 1),
        });
    }
    Some(out)
}

pub fn iso_date(date: Date) -> String {
    let f = date.fields();
    let (year, month, day) = (f.year, f.month + 1.0, f.date);
    format!(
        "{}{}{}{}{}",
        if (1000.0..=9999.0).contains(&year) { num_str(year) } else { pad(year, 4) },
        if month < 10.0 { "-0" } else { "-" },
        num_str(month),
        if day < 10.0 { "-0" } else { "-" },
        num_str(day)
    )
}

#[derive(Default)]
struct ParsedFields {
    year: Option<f64>,
    month: Option<f64>,
    day: Option<f64>,
    day_given: bool,
    hours: Option<f64>,
    minutes: Option<f64>,
    seconds: Option<f64>,
    milliseconds: Option<f64>,
    week: Option<f64>,
    offset: Option<f64>,
    afternoon: Option<bool>,
}

#[derive(Clone)]
enum Step {
    Text(String),
    Read(String),
}

fn is_sep(c: char) -> bool {
    matches!(c, '-' | '_' | ':' | '/' | '.' | ',' | '(' | ')') || js::is_space(c)
}

// RE_TOKENS: /(\[[^[]*\])|([-_:/.,()\s]+)|(A|a|Q|YYYY|YY?|ww?|MM?M?M?|Do|DD?|hh?|HH?|mm?|ss?|S{1,3}|z|ZZ?)/g
fn template_tokens(t: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let end = (|| {
            if t[i] == '[' {
                // \[[^[]*\]: greedy to the last ']' before the next '['.
                let stop = t[i + 1..].iter().position(|&c| c == '[').map_or(t.len(), |k| i + 1 + k);
                if let Some(k) = t[i + 1..stop].iter().rposition(|&c| c == ']') {
                    return Some(i + 1 + k + 1);
                }
            }
            if is_sep(t[i]) {
                let mut e = i;
                while e < t.len() && is_sep(t[e]) {
                    e += 1;
                }
                return Some(e);
            }
            first_of(t, i, &["A", "a", "Q", "YYYY"])
                .or_else(|| run_of(t, i, 'Y', 1, 2))
                .or_else(|| run_of(t, i, 'w', 1, 2))
                .or_else(|| run_of(t, i, 'M', 1, 4))
                .or_else(|| first_of(t, i, &["Do"]))
                .or_else(|| run_of(t, i, 'D', 1, 2))
                .or_else(|| run_of(t, i, 'h', 1, 2))
                .or_else(|| run_of(t, i, 'H', 1, 2))
                .or_else(|| run_of(t, i, 'm', 1, 2))
                .or_else(|| run_of(t, i, 's', 1, 2))
                .or_else(|| run_of(t, i, 'S', 1, 3))
                .or_else(|| first_of(t, i, &["z"]))
                .or_else(|| run_of(t, i, 'Z', 1, 2))
        })();
        match end {
            Some(e) => {
                out.push(t[i..e].iter().collect());
                i = e;
            }
            None => i += 1,
        }
    }
    out
}

const READ_TOKENS: [&str; 29] = ["A", "a", "Q", "S", "SS", "SSS", "s", "ss", "m", "mm", "H", "h", "HH", "hh", "D", "DD", "Do", "w", "ww", "M", "MM", "MMM", "MMMM", "Y", "YY", "YYYY", "Z", "ZZ", "z"];

fn compile(template: &str) -> Option<Vec<Step>> {
    // dayjs expands localized tokens through a locale table the English locale does not have.
    let chars: Vec<char> = template.chars().collect();
    let mut stripped = String::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(e) = bracket(&chars, i) {
            i = e;
            continue;
        }
        stripped.push(chars[i]);
        i += 1;
    }
    if stripped.contains(['L', 'l']) {
        return None;
    }
    let tokens = template_tokens(&chars);
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .into_iter()
            .map(|t| {
                if READ_TOKENS.contains(&t.as_str()) && t != "z" {
                    Step::Read(t)
                } else {
                    let mut s = t.as_str();
                    s = s.strip_prefix('[').unwrap_or(s);
                    s = s.strip_suffix(']').unwrap_or(s);
                    Step::Text(s.to_string())
                }
            })
            .collect(),
    )
}

fn digits_re(s: &[char], min: usize, max: usize) -> Option<(usize, usize)> {
    // The first place in s with at least `min` digits, taking up to `max`.
    for i in 0..s.len() {
        let mut e = i;
        while e < s.len() && s[e].is_ascii_digit() && e - i < max {
            e += 1;
        }
        if e - i >= min {
            return Some((i, e));
        }
    }
    None
}

// The first match of a reader's pattern in `s`: (start, end).
fn find(token: &str, s: &[char]) -> Option<(usize, usize)> {
    match token {
        "Q" | "S" => digits_re(s, 1, 1),
        "SS" | "DD" | "ww" | "MM" | "YY" => digits_re(s, 2, 2),
        "SSS" => digits_re(s, 3, 3),
        "YYYY" => digits_re(s, 4, 4),
        "s" | "ss" | "m" | "mm" | "H" | "h" | "HH" | "hh" | "D" | "w" | "M" => digits_re(s, 1, 2),
        "Y" => {
            // /[+-]?\d+/
            for i in 0..s.len() {
                let d = if s[i] == '+' || s[i] == '-' { i + 1 } else { i };
                let mut e = d;
                while e < s.len() && s[e].is_ascii_digit() {
                    e += 1;
                }
                if e > d {
                    return Some((i, e));
                }
            }
            None
        }
        "Z" | "ZZ" => {
            // /[+-]\d\d:?(\d\d)?|Z/
            for i in 0..s.len() {
                if (s[i] == '+' || s[i] == '-') && i + 2 < s.len() + 0 && s.get(i + 1).is_some_and(|c| c.is_ascii_digit()) && s.get(i + 2).is_some_and(|c| c.is_ascii_digit()) {
                    let mut e = i + 3;
                    let colon = s.get(e) == Some(&':');
                    let after = if colon { e + 1 } else { e };
                    if s.get(after).is_some_and(|c| c.is_ascii_digit()) && s.get(after + 1).is_some_and(|c| c.is_ascii_digit()) {
                        e = after + 2;
                    } else if colon {
                        e += 1;
                    }
                    return Some((i, e));
                }
                if s[i] == 'Z' {
                    return Some((i, i + 1));
                }
            }
            None
        }
        _ => {
            // MATCH_WORD /\d*[^-_:/,()\s\d]+/
            let word = |c: char| !(matches!(c, '-' | '_' | ':' | '/' | ',' | '(' | ')') || js::is_space(c) || c.is_ascii_digit());
            for i in 0..s.len() {
                let mut d = i;
                while d < s.len() && s[d].is_ascii_digit() {
                    d += 1;
                }
                // \d* backtracks: the word part must start right after some digits taken.
                let mut k = d;
                loop {
                    let mut e = k;
                    while e < s.len() && word(s[e]) {
                        e += 1;
                    }
                    if e > k {
                        return Some((i, e));
                    }
                    if k == i {
                        break;
                    }
                    k -= 1;
                }
            }
            None
        }
    }
}

fn read(token: &str, f: &mut ParsedFields, input: &str) -> bool {
    let n = js::to_number(input);
    match token {
        "A" => f.afternoon = Some(input == "PM"),
        "a" => f.afternoon = Some(input == "pm"),
        "Q" => f.month = Some((n - 1.0) * 3.0 + 1.0),
        "S" => f.milliseconds = Some(n * 100.0),
        "SS" => f.milliseconds = Some(n * 10.0),
        "SSS" => f.milliseconds = Some(n),
        "s" | "ss" => f.seconds = Some(n),
        "m" | "mm" => f.minutes = Some(n),
        "H" | "h" | "HH" | "hh" => f.hours = Some(n),
        "D" | "DD" => f.day = Some(n),
        "Do" => {
            let digits: String = input.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return false;
            }
            // dayjs keeps the digits as a string here, so even "0" counts as a day having been given.
            f.day = Some(js::to_number(&digits));
            f.day_given = true;
            for i in 1..=31 {
                if ordinal(i as f64) == input {
                    f.day = Some(i as f64);
                }
            }
        }
        "w" | "ww" => f.week = Some(n),
        "M" | "MM" => f.month = Some(n),
        "MMM" | "MMMM" => {
            let index = MONTHS.iter().position(|m| if token == "MMM" { &m[..3] == input } else { *m == input }).map_or(0, |k| k + 1);
            if index < 1 {
                return false;
            }
            f.month = Some(js::or((index % 12) as f64, || index as f64));
        }
        "Y" | "YYYY" => f.year = Some(n),
        "YY" => f.year = Some(n + if n > 68.0 { 1900.0 } else { 2000.0 }),
        "Z" | "ZZ" => {
            if input == "Z" {
                f.offset = Some(0.0);
                return true;
            }
            // input.match(/([+-]|\d\d)/g)
            let c: Vec<char> = input.chars().collect();
            let mut parts: Vec<String> = Vec::new();
            let mut i = 0;
            while i < c.len() {
                if c[i] == '+' || c[i] == '-' {
                    parts.push(c[i].to_string());
                    i += 1;
                } else if i + 1 < c.len() && c[i].is_ascii_digit() && c[i + 1].is_ascii_digit() {
                    parts.push(c[i..i + 2].iter().collect());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            let part = |k: usize| parts.get(k).map_or(f64::NAN, |p| js::to_number(p));
            let minutes = part(1) * 60.0 + js::or(part(2), || 0.0);
            f.offset = Some(if minutes == 0.0 {
                0.0
            } else if parts[0] == "+" {
                -minutes
            } else {
                minutes
            });
        }
        _ => {}
    }
    true
}

fn truthy(v: Option<f64>) -> bool {
    v.is_some_and(|x| x != 0.0 && !x.is_nan())
}

fn loose(input: &str, template: &str, now: Date) -> Option<Date> {
    if template == "x" || template == "X" {
        return Some(Date(js::to_number(input) * if template == "X" { 1000.0 } else { 1.0 }).clip());
    }
    let steps = compile(template)?;
    let mut f = ParsedFields::default();
    let mut rest: Vec<char> = input.chars().collect();
    let mut start = 0usize;
    for step in &steps {
        match step {
            Step::Text(s) => {
                start += js::len16(s);
            }
            Step::Read(token) => {
                let from = start.min(rest.len());
                let (s, e) = find(token, &rest[from..])?;
                let matched: String = rest[from + s..from + e].iter().collect();
                if !read(token, &mut f, &matched) {
                    return None;
                }
                // dayjs drops the first occurrence of the matched text, wherever it is, and does not move `start`.
                let m: Vec<char> = matched.chars().collect();
                if let Some(at) = (0..=rest.len().saturating_sub(m.len())).find(|&k| rest[k..k + m.len()] == m[..]) {
                    rest.drain(at..at + m.len());
                }
            }
        }
    }
    if let (Some(afternoon), Some(h)) = (f.afternoon, f.hours) {
        if afternoon {
            if h < 12.0 {
                f.hours = Some(h + 12.0);
            }
        } else if h == 12.0 {
            f.hours = Some(0.0);
        }
    }
    let nf = now.fields();
    let day = if f.day_given || truthy(f.day) {
        f.day.unwrap()
    } else if !truthy(f.year) && !truthy(f.month) {
        nf.date
    } else {
        1.0
    };
    let year = if truthy(f.year) { f.year.unwrap() } else { nf.year };
    let mut month = 0.0;
    if !(truthy(f.year) && !truthy(f.month)) {
        month = if f.month.is_some_and(|m| m > 0.0) { f.month.unwrap() - 1.0 } else { nf.month };
    }
    let or0 = |v: Option<f64>| if truthy(v) { v.unwrap() } else { 0.0 };
    let (hours, minutes, seconds, ms) = (or0(f.hours), or0(f.minutes), or0(f.seconds), or0(f.milliseconds));
    if let Some(offset) = f.offset {
        return Some(Date(Date::utc(year, month, day, hours, minutes, seconds, ms + offset * 60000.0)));
    }
    // Applying a week number needs a dayjs plugin Mermaid does not load.
    if truthy(f.week) {
        return None;
    }
    Some(Date::local(year, month, day, hours, minutes, seconds, ms))
}

/// Parses the way `dayjs(input, format, true)` does. `now` supplies the fields the format leaves out.
pub fn parse_strict(input: &str, template: &str, now: Date) -> Option<Date> {
    let date = loose(input, template, now)?;
    (date.valid() && format(date, template).as_deref() == Some(input)).then_some(date)
}

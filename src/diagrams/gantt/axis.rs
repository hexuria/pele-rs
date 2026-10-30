// Port of src/diagrams/gantt/axis.ts: time axis support, following d3-time and d3-time-format
// (English locale, local time): the `axisFormat` specifiers, tick intervals with d3's `every`
// rule, and its choice of automatic ticks.

use super::clock::Date;
use super::dates::{MONTHS, WEEKDAYS};
use crate::js::{self, num_str};

const SECOND: f64 = 1000.0;
const MINUTE: f64 = 60.0 * SECOND;
const HOUR: f64 = 60.0 * MINUTE;
const DAY: f64 = 24.0 * HOUR;
const WEEK: f64 = 7.0 * DAY;
const MONTH: f64 = 30.0 * DAY;
const YEAR: f64 = 365.0 * DAY;

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

fn start_of_day(d: &mut Date) {
    d.set_hours(0.0, 0.0, 0.0, 0.0);
}

#[derive(Clone, Copy)]
struct Calendar {
    unit: Unit,
    step: f64,
    week_start: f64,
    // For `every` on a week: the floored epoch.
    epoch: f64,
}

impl Calendar {
    fn cycle(&self) -> f64 {
        match self.unit {
            Unit::Second | Unit::Minute => 60.0,
            Unit::Hour => 24.0,
            Unit::Day => 31.0,
            Unit::Month => 12.0,
            _ => f64::INFINITY,
        }
    }
    fn ms(&self) -> f64 {
        match self.unit {
            Unit::Millisecond => 1.0,
            Unit::Second => SECOND,
            Unit::Minute => MINUTE,
            Unit::Hour => HOUR,
            Unit::Day => DAY,
            Unit::Week => WEEK,
            Unit::Month => MONTH,
            Unit::Year => YEAR,
        }
    }
    fn field(&self, d: Date) -> f64 {
        match self.unit {
            Unit::Second => d.utc_seconds(),
            Unit::Minute => d.minutes(),
            Unit::Hour => d.hours(),
            Unit::Day => d.date() - 1.0,
            Unit::Month => d.month(),
            _ => f64::NAN,
        }
    }
    fn base_floor(&self, d: &mut Date) {
        match self.unit {
            Unit::Millisecond => {}
            Unit::Second => d.set_time(d.0 - d.milliseconds()),
            Unit::Minute => d.set_time(d.0 - d.milliseconds() - d.seconds() * SECOND),
            Unit::Hour => d.set_time(d.0 - d.milliseconds() - d.seconds() * SECOND - d.minutes() * MINUTE),
            Unit::Day => start_of_day(d),
            Unit::Week => {
                d.set_date(d.date() - (d.day() + 7.0 - self.week_start) % 7.0);
                start_of_day(d);
            }
            Unit::Month => {
                d.set_date(1.0);
                start_of_day(d);
            }
            Unit::Year => {
                d.set_month_date(0.0, 1.0);
                start_of_day(d);
            }
        }
    }
    fn base_offset(&self, d: &mut Date, step: f64) {
        match self.unit {
            Unit::Millisecond => d.set_time(d.0 + step),
            Unit::Second => d.set_time(d.0 + step * SECOND),
            Unit::Minute => d.set_time(d.0 + step * MINUTE),
            Unit::Hour => d.set_time(d.0 + step * HOUR),
            Unit::Day => d.set_date(d.date() + step),
            Unit::Week => d.set_date(d.date() + step * 7.0),
            Unit::Month => d.set_month(d.month() + step),
            Unit::Year => d.set_full_year(d.full_year() + step),
        }
    }
    fn test(&self, d: Date) -> bool {
        self.field(d) % self.step == 0.0
    }
    fn floor(&self, d: &mut Date) {
        if self.step <= 1.0 {
            return self.base_floor(d);
        }
        match self.unit {
            Unit::Millisecond => d.set_time((d.0 / self.step).floor() * self.step),
            Unit::Year => {
                d.set_full_year((d.full_year() / self.step).floor() * self.step);
                self.base_floor(d);
            }
            Unit::Week => {
                self.base_floor(d);
                let back = -mod_(span(Date(self.epoch), *d, WEEK), self.step);
                self.base_offset(d, back);
            }
            _ => {
                // Date overflow makes field(d) NaN, which can never match the interval.
                self.base_floor(d);
                while d.0.is_finite() && !self.test(*d) {
                    d.set_time(d.0 - 1.0);
                    self.base_floor(d);
                }
            }
        }
    }
    fn offset(&self, d: &mut Date, n: f64) {
        if self.step <= 1.0 {
            return self.base_offset(d, n);
        }
        match self.unit {
            Unit::Millisecond => d.set_time(d.0 + n * self.step),
            Unit::Year => d.set_full_year(d.full_year() + n * self.step),
            Unit::Week => self.base_offset(d, n * self.step),
            _ => {
                let mut i = 0.0;
                while i < n && d.0.is_finite() {
                    loop {
                        self.base_offset(d, 1.0);
                        if !(d.0.is_finite() && !self.test(*d)) {
                            break;
                        }
                    }
                    i += 1.0;
                }
            }
        }
    }
}

// Whole units between two floored dates, discounting a change of UTC offset between them.
fn span(start: Date, end: Date, unit: f64) -> f64 {
    ((end.0 - start.0 - (end.timezone_offset() - start.timezone_offset()) * MINUTE) / unit).floor()
}

fn mod_(a: f64, b: f64) -> f64 {
    ((a % b) + b) % b
}

// The interval d3's `unit.every(step)` gives.
fn every(unit: Unit, step: f64, week_start: f64) -> Calendar {
    let mut c = Calendar { unit, step, week_start, epoch: 0.0 };
    if unit == Unit::Week && step > 1.0 {
        let mut epoch = Date(0.0);
        c.base_floor(&mut epoch);
        c.epoch = epoch.0;
    }
    c
}

// Ticks from start to stop inclusive, or None when there would be more than `limit`.
fn range(calendar: &Calendar, start: f64, stop: f64, limit: f64) -> Option<Vec<f64>> {
    let mut d = Date(start - 1.0);
    calendar.floor(&mut d);
    calendar.offset(&mut d, 1.0);
    calendar.floor(&mut d);
    let mut out = Vec::new();
    let mut previous = f64::NEG_INFINITY;
    while d.0 <= stop && d.0 > previous {
        if out.len() as f64 >= limit {
            return None;
        }
        previous = d.0;
        out.push(previous);
        calendar.offset(&mut d, 1.0);
        calendar.floor(&mut d);
    }
    Some(out)
}

fn tick_step(start: f64, stop: f64, count: f64) -> f64 {
    let step = (stop - start) / count.max(0.0);
    let power = step.log10().floor();
    let error = step / 10f64.powf(power);
    let factor = if error >= 50f64.sqrt() {
        10.0
    } else if error >= 10f64.sqrt() {
        5.0
    } else if error >= std::f64::consts::SQRT_2 {
        2.0
    } else {
        1.0
    };
    10f64.powf(power) * factor
}

const AUTO: [(Unit, f64, f64); 18] = [
    (Unit::Second, 1.0, SECOND),
    (Unit::Second, 5.0, 5.0 * SECOND),
    (Unit::Second, 15.0, 15.0 * SECOND),
    (Unit::Second, 30.0, 30.0 * SECOND),
    (Unit::Minute, 1.0, MINUTE),
    (Unit::Minute, 5.0, 5.0 * MINUTE),
    (Unit::Minute, 15.0, 15.0 * MINUTE),
    (Unit::Minute, 30.0, 30.0 * MINUTE),
    (Unit::Hour, 1.0, HOUR),
    (Unit::Hour, 3.0, 3.0 * HOUR),
    (Unit::Hour, 6.0, 6.0 * HOUR),
    (Unit::Hour, 12.0, 12.0 * HOUR),
    (Unit::Day, 1.0, DAY),
    (Unit::Day, 2.0, 2.0 * DAY),
    (Unit::Week, 1.0, WEEK),
    (Unit::Month, 1.0, MONTH),
    (Unit::Month, 3.0, 3.0 * MONTH),
    (Unit::Year, 1.0, YEAR),
];

/// About `count` ticks at round times, chosen the way d3's time scale chooses them.
pub fn auto_ticks(start: f64, stop: f64, count: f64) -> Vec<f64> {
    let target = (stop - start) / count;
    let mut i = 0;
    while i < AUTO.len() && AUTO[i].2 <= target {
        i += 1;
    }
    let calendar = if i == AUTO.len() {
        every(Unit::Year, 1f64.max(tick_step(start / YEAR, stop / YEAR, count)), 0.0)
    } else if i == 0 {
        every(Unit::Millisecond, tick_step(start, stop, count).max(1.0), 0.0)
    } else {
        let (unit, step, _) = AUTO[if target / AUTO[i - 1].2 < AUTO[i].2 / target { i - 1 } else { i }];
        every(unit, step, 0.0)
    };
    range(&calendar, start, stop, 4.0 * count + 8.0).unwrap_or_default()
}

const WEEK_STARTS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

/// Ticks for a `tickInterval` such as `1week`, or None when it is not one or asks for more than `limit`.
pub fn interval_ticks(interval: &str, weekday: &str, start: f64, stop: f64, limit: f64) -> Option<Vec<f64>> {
    // /^([1-9]\d*)(millisecond|second|minute|hour|day|week|month)$/
    let digits = interval.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || interval.as_bytes()[0] == b'0' {
        return None;
    }
    let unit = match &interval[digits..] {
        "millisecond" => Unit::Millisecond,
        "second" => Unit::Second,
        "minute" => Unit::Minute,
        "hour" => Unit::Hour,
        "day" => Unit::Day,
        "week" => Unit::Week,
        "month" => Unit::Month,
        _ => return None,
    };
    let step = js::parse_int(&interval[..digits], 10);
    let week_start = WEEK_STARTS.iter().position(|w| *w == weekday).map_or(0.0, |k| k as f64);
    let calendar = every(unit, step, week_start);
    if (stop - start) / (step.min(calendar.cycle()) * calendar.ms()) > limit {
        return None;
    }
    range(&calendar, start, stop, limit)
}

fn pad(value: f64, fill: &str, width: usize) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let digits = num_str(if value < 0.0 { -value } else { value });
    let n = digits.chars().count();
    format!("{}{}", sign, if n < width { fill.repeat(width - n) + &digits } else { digits })
}

fn floored(date: Date, calendar: &Calendar) -> Date {
    let mut d = date;
    calendar.floor(&mut d);
    d
}

fn week_calendar(week_start: f64) -> Calendar {
    every(Unit::Week, 1.0, week_start)
}

fn year_calendar() -> Calendar {
    every(Unit::Year, 1.0, 0.0)
}

// Weeks since the last `weekStart` day before the year began, which is d3's %U and %W.
fn week_of_year(date: Date, week_start: f64) -> f64 {
    let week = week_calendar(week_start);
    let year_start = floored(date, &year_calendar());
    span(floored(Date(year_start.0 - 1.0), &week), floored(date, &week), WEEK)
}

// The Thursday that decides which ISO week and ISO year a date belongs to.
fn iso_thursday(date: Date) -> Date {
    let thursday = week_calendar(4.0);
    let day = date.day();
    let late = day >= 4.0 || day == 0.0;
    let mut d = floored(if late { date } else { Date(date.0 - 1.0) }, &thursday);
    if !late {
        thursday.offset(&mut d, 1.0);
        thursday.floor(&mut d);
    }
    d
}

/// Formats a date with a d3 time format specifier such as `%Y-%m-%d`.
pub fn time_format(specifier: &str, date: Date) -> String {
    let c: Vec<char> = specifier.chars().collect();
    let mut out = String::new();
    let mut from = 0;
    let mut i = 0;
    while i < c.len() {
        if c[i] != '%' {
            i += 1;
            continue;
        }
        out.extend(&c[from..i]);
        i += 1;
        let mut ch = c.get(i).copied();
        let fill = match ch {
            Some('-') => Some(""),
            Some('_') => Some(" "),
            Some('0') => Some("0"),
            _ => None,
        };
        let fill = match fill {
            Some(f) => {
                i += 1;
                ch = c.get(i).copied();
                f
            }
            None => {
                if ch == Some('e') {
                    " "
                } else {
                    "0"
                }
            }
        };
        out.push_str(&directive(ch, date, fill));
        from = (i + 1).min(c.len());
        i += 1;
    }
    out.extend(&c[from.min(c.len())..]);
    out
}

fn directive(c: Option<char>, d: Date, p: &str) -> String {
    let Some(c) = c else { return String::new() };
    let f = d.fields();
    let idx = |v: f64| v as usize;
    match c {
        'a' => WEEKDAYS[idx(f.day)][..3].to_string(),
        'A' => WEEKDAYS[idx(f.day)].to_string(),
        'b' => MONTHS[idx(f.month)][..3].to_string(),
        'B' => MONTHS[idx(f.month)].to_string(),
        'c' => time_format("%x, %X", d),
        'd' | 'e' => pad(f.date, p, 2),
        'f' => pad(f.ms, p, 3) + "000",
        'g' => pad(iso_thursday(d).full_year() % 100.0, p, 2),
        'G' => pad(iso_thursday(d).full_year() % 10000.0, p, 4),
        'H' => pad(f.hours, p, 2),
        'I' => pad(js::or(f.hours % 12.0, || 12.0), p, 2),
        'j' => pad(1.0 + span(floored(d, &year_calendar()), floored(d, &every(Unit::Day, 1.0, 0.0)), DAY), p, 3),
        'L' => pad(f.ms, p, 3),
        'm' => pad(f.month + 1.0, p, 2),
        'M' => pad(f.minutes, p, 2),
        'p' => (if f.hours >= 12.0 { "PM" } else { "AM" }).to_string(),
        'q' => num_str(1.0 + (f.month / 3.0).floor()),
        'Q' => num_str(d.0),
        's' => num_str((d.0 / 1000.0).floor()),
        'S' => pad(f.seconds, p, 2),
        'u' => num_str(js::or(f.day, || 7.0)),
        'U' => pad(week_of_year(d, 0.0), p, 2),
        'V' => {
            let thursday = iso_thursday(d);
            let year_start = floored(thursday, &year_calendar());
            let first = floored(year_start, &week_calendar(4.0));
            pad(span(first, thursday, WEEK) + if year_start.day() == 4.0 { 1.0 } else { 0.0 }, p, 2)
        }
        'w' => num_str(f.day),
        'W' => pad(week_of_year(d, 1.0), p, 2),
        'x' => time_format("%-m/%-d/%Y", d),
        'X' => time_format("%-I:%M:%S %p", d),
        'y' => pad(f.year % 100.0, p, 2),
        'Y' => pad(f.year % 10000.0, p, 4),
        'Z' => {
            let z = d.timezone_offset();
            let abs = z.abs();
            format!("{}{}{}", if z > 0.0 { '-' } else { '+' }, pad((abs / 60.0).floor(), "0", 2), pad(abs % 60.0, "0", 2))
        }
        '%' => "%".into(),
        other => other.to_string(),
    }
}

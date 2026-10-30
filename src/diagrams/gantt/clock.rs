// JavaScript's Date in local time, as the gantt code uses it: a time value in milliseconds, its
// local fields, and the constructor and setters that go from local fields back to a time value.
// Not an upstream file. The UTC offset of an instant comes from the C library's `localtime_r`,
// which reads the same time zone (`TZ`, else the system's) as Node does. A local time that falls
// in a gap or an overlap resolves to the offset in force before the transition, as V8 does with
// ICU's FORMER rule.

use std::os::raw::{c_char, c_int, c_long};
use std::sync::Once;

const DAY: f64 = 86400000.0;

#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

extern "C" {
    fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
    fn tzset();
}

/// The local time zone's offset from UTC at the instant `t` (ms since the epoch), in ms.
pub fn offset_at(t: f64) -> f64 {
    static INIT: Once = Once::new();
    // SAFETY: tzset only reads the environment and the zone files.
    INIT.call_once(|| unsafe { tzset() });
    let secs = (t / 1000.0).floor();
    if !secs.is_finite() || secs.abs() > 1e15 {
        return 0.0;
    }
    let secs = secs as i64;
    let mut tm = Tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0, tm_gmtoff: 0, tm_zone: std::ptr::null() };
    // SAFETY: both pointers are to live locals of the right types; localtime_r is reentrant.
    let ok = unsafe { !localtime_r(&secs, &mut tm).is_null() };
    if ok {
        tm.tm_gmtoff as f64 * 1000.0
    } else {
        0.0
    }
}

/// The time value of a local time, by the FORMER rule for gaps and overlaps.
pub fn utc_of_local(local: f64) -> f64 {
    if !local.is_finite() {
        return f64::NAN;
    }
    let before = offset_at(local - DAY);
    let after = offset_at(local + DAY);
    if before == after {
        let u = local - before;
        let o = offset_at(u);
        return if o == before { u } else { local - o };
    }
    let u1 = local - before;
    if offset_at(u1) == before {
        return u1;
    }
    let u2 = local - after;
    if offset_at(u2) == after {
        return u2;
    }
    u1
}

fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 {
        f64::NAN
    } else {
        t.trunc() + 0.0
    }
}

fn integer(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.trunc()
    }
}

// Days from the epoch to the first of a month, for any integer year and month.
fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if !year.is_finite() || !month.is_finite() || !date.is_finite() {
        return f64::NAN;
    }
    let y = integer(year) + (integer(month) / 12.0).floor();
    let m = integer(month).rem_euclid(12.0);
    if y.abs() > 400000.0 {
        return f64::NAN;
    }
    days_from_civil(y as i64, m as i64 + 1, 1) as f64 + integer(date) - 1.0
}

fn make_time(h: f64, m: f64, s: f64, ms: f64) -> f64 {
    if !h.is_finite() || !m.is_finite() || !s.is_finite() || !ms.is_finite() {
        return f64::NAN;
    }
    integer(h) * 3600000.0 + integer(m) * 60000.0 + integer(s) * 1000.0 + integer(ms)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A JS Date: its time value, NaN when invalid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Date(pub f64);

pub struct Fields {
    pub year: f64,
    pub month: f64,
    pub date: f64,
    pub hours: f64,
    pub minutes: f64,
    pub seconds: f64,
    pub ms: f64,
    pub day: f64,
}

impl Date {
    pub fn local(year: f64, month: f64, date: f64, h: f64, m: f64, s: f64, ms: f64) -> Date {
        Date(time_clip(utc_of_local(make_day(year, month, date) * DAY + make_time(h, m, s, ms))))
    }
    pub fn utc(year: f64, month: f64, date: f64, h: f64, m: f64, s: f64, ms: f64) -> f64 {
        time_clip(make_day(year, month, date) * DAY + make_time(h, m, s, ms))
    }
    pub fn time(self) -> f64 {
        self.0
    }
    pub fn valid(self) -> bool {
        !self.0.is_nan()
    }
    pub fn fields(self) -> Fields {
        if self.0.is_nan() {
            return Fields { year: f64::NAN, month: f64::NAN, date: f64::NAN, hours: f64::NAN, minutes: f64::NAN, seconds: f64::NAN, ms: f64::NAN, day: f64::NAN };
        }
        let lt = self.0 + offset_at(self.0);
        let days = (lt / DAY).floor();
        let in_day = lt - days * DAY;
        let (y, m, d) = civil_from_days(days as i64);
        Fields {
            year: y as f64,
            month: (m - 1) as f64,
            date: d as f64,
            hours: (in_day / 3600000.0).floor(),
            minutes: (in_day / 60000.0).floor().rem_euclid(60.0),
            seconds: (in_day / 1000.0).floor().rem_euclid(60.0),
            ms: in_day.rem_euclid(1000.0),
            day: (days + 4.0).rem_euclid(7.0),
        }
    }
    pub fn full_year(self) -> f64 {
        self.fields().year
    }
    pub fn month(self) -> f64 {
        self.fields().month
    }
    pub fn date(self) -> f64 {
        self.fields().date
    }
    pub fn day(self) -> f64 {
        self.fields().day
    }
    pub fn hours(self) -> f64 {
        self.fields().hours
    }
    pub fn minutes(self) -> f64 {
        self.fields().minutes
    }
    pub fn seconds(self) -> f64 {
        self.fields().seconds
    }
    pub fn milliseconds(self) -> f64 {
        self.fields().ms
    }
    pub fn utc_seconds(self) -> f64 {
        if self.0.is_nan() {
            return f64::NAN;
        }
        (self.0 / 1000.0).floor().rem_euclid(60.0)
    }
    /// `getTimezoneOffset()`, in minutes.
    pub fn timezone_offset(self) -> f64 {
        if self.0.is_nan() {
            return f64::NAN;
        }
        -offset_at(self.0) / 60000.0
    }
    fn with(self, f: impl FnOnce(&mut Fields)) -> Date {
        if self.0.is_nan() {
            return self;
        }
        let mut x = self.fields();
        f(&mut x);
        Date::local(x.year, x.month, x.date, x.hours, x.minutes, x.seconds, x.ms)
    }
    pub fn set_date(&mut self, date: f64) {
        *self = self.with(|x| x.date = date);
    }
    pub fn set_month(&mut self, month: f64) {
        *self = self.with(|x| x.month = month);
    }
    pub fn set_month_date(&mut self, month: f64, date: f64) {
        *self = self.with(|x| {
            x.month = month;
            x.date = date;
        });
    }
    /// `setFullYear`: an invalid date counts as +0 here.
    pub fn set_full_year(&mut self, year: f64) {
        if self.0.is_nan() {
            *self = Date(0.0);
        }
        *self = self.with(|x| x.year = year);
    }
    pub fn set_hours(&mut self, h: f64, m: f64, s: f64, ms: f64) {
        *self = self.with(|x| {
            x.hours = h;
            x.minutes = m;
            x.seconds = s;
            x.ms = ms;
        });
    }
    pub fn set_time(&mut self, t: f64) {
        *self = Date(time_clip(t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_days_round_trip() {
        for z in [-800000i64, -719468, -1, 0, 1, 10957, 20000, 2932896] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(Date::utc(2024.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0), 1704067200000.0);
        assert_eq!(Date::utc(2024.0, 13.0, 1.0, 0.0, 0.0, 0.0, 0.0), Date::utc(2025.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0));
    }
}

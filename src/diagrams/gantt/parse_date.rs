// `new Date(string)` for gantt's fallback parse. Not an upstream file. The ECMAScript date
// time string format is read as V8 reads it: a date alone is UTC, a date with a time and no
// offset is local time. V8's legacy fallback formats (`Jan 6 2014`, `2014/01/06`) are not
// covered and read as an invalid date.

use super::clock::Date;

fn digits(c: &[char], i: &mut usize, n: usize) -> Option<f64> {
    if *i + n > c.len() || !c[*i..*i + n].iter().all(|d| d.is_ascii_digit()) {
        return None;
    }
    let v: String = c[*i..*i + n].iter().collect();
    *i += n;
    v.parse().ok()
}

fn expect(c: &[char], i: &mut usize, ch: char) -> Option<()> {
    if c.get(*i) == Some(&ch) {
        *i += 1;
        Some(())
    } else {
        None
    }
}

fn iso(s: &str) -> Option<f64> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let year = match c.first() {
        Some('+') | Some('-') => {
            let neg = c[0] == '-';
            i = 1;
            let y = digits(&c, &mut i, 6)?;
            if neg && y == 0.0 {
                return None;
            }
            if neg {
                -y
            } else {
                y
            }
        }
        _ => digits(&c, &mut i, 4)?,
    };
    let mut month = 1.0;
    let mut day = 1.0;
    if expect(&c, &mut i, '-').is_some() {
        month = digits(&c, &mut i, 2)?;
        if expect(&c, &mut i, '-').is_some() {
            day = digits(&c, &mut i, 2)?;
        }
    }
    if !(1.0..=12.0).contains(&month) || !(1.0..=31.0).contains(&day) {
        return None;
    }
    if i == c.len() {
        return Some(Date::utc(year, month - 1.0, day, 0.0, 0.0, 0.0, 0.0));
    }
    expect(&c, &mut i, 'T').or_else(|| expect(&c, &mut i, 't'))?;
    let h = digits(&c, &mut i, 2)?;
    expect(&c, &mut i, ':')?;
    let m = digits(&c, &mut i, 2)?;
    let (mut s, mut ms) = (0.0, 0.0);
    if expect(&c, &mut i, ':').is_some() {
        s = digits(&c, &mut i, 2)?;
        if expect(&c, &mut i, '.').is_some() {
            let from = i;
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
            if i == from {
                return None;
            }
            let frac: String = c[from..i].iter().copied().chain("000".chars()).take(3).collect();
            ms = frac.parse().ok()?;
        }
    }
    if h > 24.0 || m > 59.0 || s > 59.0 || (h == 24.0 && (m > 0.0 || s > 0.0 || ms > 0.0)) {
        return None;
    }
    if i == c.len() {
        return Some(Date::local(year, month - 1.0, day, h, m, s, ms).0);
    }
    let offset = match c[i] {
        'Z' | 'z' => {
            i += 1;
            0.0
        }
        '+' | '-' => {
            let sign = if c[i] == '-' { -1.0 } else { 1.0 };
            i += 1;
            let oh = digits(&c, &mut i, 2)?;
            expect(&c, &mut i, ':')?;
            let om = digits(&c, &mut i, 2)?;
            sign * (oh * 60.0 + om)
        }
        _ => return None,
    };
    if i != c.len() {
        return None;
    }
    Some(Date::utc(year, month - 1.0, day, h, m, s, ms) - offset * 60000.0)
}

/// The time value `new Date(text)` gives, NaN when it cannot read the text.
pub fn parse_date(text: &str) -> f64 {
    iso(text).unwrap_or(f64::NAN)
}

//! How an item repeats: the Microsoft To Do set (daily, weekdays, weekly, monthly, yearly and
//! custom "every N days/weeks/months/years", weeks on chosen days) plus every N minutes or hours,
//! optionally only on some days and between two times.
//!
//! Stored in items.json as a short readable string, because the desk dashboard reads the field
//! as text: `daily`, `weekdays`, `weekly`, `monthly`, `yearly`, or
//! `every 30 minutes[ on mon,tue,wed][ 09:00-17:00]`.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

impl Unit {
    pub const ALL: [Unit; 6] = [Unit::Minutes, Unit::Hours, Unit::Days, Unit::Weeks, Unit::Months, Unit::Years];
    pub fn plural(self) -> &'static str {
        match self {
            Unit::Minutes => "minutes",
            Unit::Hours => "hours",
            Unit::Days => "days",
            Unit::Weeks => "weeks",
            Unit::Months => "months",
            Unit::Years => "years",
        }
    }
    pub fn singular(self) -> &'static str {
        self.plural().trim_end_matches('s')
    }
    fn from_word(w: &str) -> Option<Unit> {
        Some(match w {
            "m" | "min" | "mins" | "minute" | "minutes" => Unit::Minutes,
            "h" | "hr" | "hrs" | "hour" | "hours" => Unit::Hours,
            "d" | "day" | "days" => Unit::Days,
            "w" | "wk" | "wks" | "week" | "weeks" => Unit::Weeks,
            "month" | "months" | "mo" | "mth" | "mths" => Unit::Months,
            "y" | "yr" | "yrs" | "year" | "years" => Unit::Years,
            _ => return None,
        })
    }
    pub fn parse(w: &str) -> Option<Unit> {
        Unit::from_word(w)
    }
    /// Minutes and hours: these repeat through the day.
    pub fn intraday(self) -> bool {
        matches!(self, Unit::Minutes | Unit::Hours)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Daily,
    Weekdays,
    Weekly,
    Monthly,
    Yearly,
    /// Every `n` units. `days` is a weekday mask (bit 0 Monday … bit 6 Sunday): for weeks, the
    /// days it falls on; for minutes and hours, the days it runs (0 = every day). `window` is
    /// minutes after midnight, for minutes and hours only.
    Every { n: u32, unit: Unit, days: u8, window: Option<(u16, u16)> },
}

pub const WEEKDAYS: u8 = 0b0011111;
pub const WEEKEND: u8 = 0b1100000;
const DAY_CODES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

pub fn day_bit(d: Weekday) -> u8 {
    1 << d.num_days_from_monday()
}

fn has(mask: u8, d: Weekday) -> bool {
    mask & day_bit(d) != 0
}

fn hhmm(m: u16) -> NaiveTime {
    NaiveTime::from_hms_opt((m / 60).min(23) as u32, (m % 60) as u32, 0).unwrap_or(NaiveTime::MIN)
}

pub fn time_label(m: u16) -> String {
    let t = hhmm(m);
    if t.minute() == 0 { t.format("%-I%P").to_string() } else { t.format("%-I:%M%P").to_string() }
}

fn days_label(mask: u8) -> String {
    match mask {
        WEEKDAYS => "weekdays".into(),
        WEEKEND => "weekends".into(),
        _ => (0..7).filter(|i| mask & (1 << i) != 0).map(|i| DAY_NAMES[i]).collect::<Vec<_>>().join(", "),
    }
}

impl Repeat {
    /// The presets shown first in the editor, To Do style.
    pub const PRESETS: [Repeat; 5] = [Repeat::Daily, Repeat::Weekdays, Repeat::Weekly, Repeat::Monthly, Repeat::Yearly];

    pub fn every(n: u32, unit: Unit) -> Repeat {
        Repeat::Every { n: n.max(1), unit, days: 0, window: None }
    }

    /// The same schedule as "every N unit", for the custom editor.
    pub fn as_custom(self) -> Repeat {
        match self {
            Repeat::Daily => Repeat::every(1, Unit::Days),
            Repeat::Weekdays => Repeat::Every { n: 1, unit: Unit::Weeks, days: WEEKDAYS, window: None },
            Repeat::Weekly => Repeat::every(1, Unit::Weeks),
            Repeat::Monthly => Repeat::every(1, Unit::Months),
            Repeat::Yearly => Repeat::every(1, Unit::Years),
            e => e,
        }
    }

    pub fn is_custom(self) -> bool {
        matches!(self, Repeat::Every { .. })
    }

    /// Repeats through the day (every N minutes or hours).
    pub fn intraday(self) -> bool {
        matches!(self, Repeat::Every { unit, .. } if unit.intraday())
    }

    pub fn label(self) -> String {
        match self {
            Repeat::Daily => "every day".into(),
            Repeat::Weekdays => "weekdays".into(),
            Repeat::Weekly => "every week".into(),
            Repeat::Monthly => "every month".into(),
            Repeat::Yearly => "every year".into(),
            Repeat::Every { n, unit, days, window } => {
                match (n, unit, days) {
                    (1, Unit::Weeks, WEEKDAYS) => return "weekdays".into(),
                    (1, Unit::Weeks, WEEKEND) => return "every weekend".into(),
                    _ => {}
                }
                let mut s = match (n, unit) {
                    (1, Unit::Weeks) if days != 0 => String::from("every"),
                    (2, Unit::Days) => "every other day".into(),
                    (2, Unit::Weeks) if days == 0 => "fortnightly".into(),
                    (1, u) => format!("every {}", u.singular()),
                    (n, Unit::Minutes) => format!("every {n} min"),
                    (n, u) => format!("every {n} {}", u.plural()),
                };
                if days != 0 {
                    if unit == Unit::Weeks {
                        if n > 1 {
                            s.push_str(" on");
                        }
                        s.push(' ');
                        s.push_str(&days_label(days));
                    } else if unit.intraday() {
                        s.push_str(&format!(" · {}", days_label(days)));
                    }
                }
                if let (Some((a, b)), true) = (window, unit.intraday()) {
                    s.push_str(&format!(" · {}–{}", time_label(a), time_label(b)));
                }
                s
            }
        }
    }

    fn code(self) -> String {
        match self {
            Repeat::Daily => "daily".into(),
            Repeat::Weekdays => "weekdays".into(),
            Repeat::Weekly => "weekly".into(),
            Repeat::Monthly => "monthly".into(),
            Repeat::Yearly => "yearly".into(),
            Repeat::Every { n, unit, days, window } => {
                let mut s = format!("every {n} {}", unit.plural());
                if days != 0 {
                    let d: Vec<&str> = (0..7).filter(|i| days & (1 << i) != 0).map(|i| DAY_CODES[i]).collect();
                    s.push_str(&format!(" on {}", d.join(",")));
                }
                if let Some((a, b)) = window {
                    s.push_str(&format!(" {:02}:{:02}-{:02}:{:02}", a / 60, a % 60, b / 60, b % 60));
                }
                s
            }
        }
    }

    fn from_code(s: &str) -> Option<Repeat> {
        Some(match s.trim() {
            "daily" => Repeat::Daily,
            "weekdays" => Repeat::Weekdays,
            "weekly" => Repeat::Weekly,
            "monthly" => Repeat::Monthly,
            "yearly" => Repeat::Yearly,
            other => {
                let mut w = other.split_whitespace();
                if w.next()? != "every" {
                    return None;
                }
                let n: u32 = w.next()?.parse().ok()?;
                let unit = Unit::from_word(w.next()?)?;
                let (mut days, mut window) = (0u8, None);
                while let Some(x) = w.next() {
                    if x == "on" {
                        for d in w.next()?.split(',') {
                            days |= 1 << DAY_CODES.iter().position(|c| *c == d)?;
                        }
                    } else if let Some((a, b)) = x.split_once('-') {
                        let m = |t: &str| -> Option<u16> {
                            let (h, mi) = t.split_once(':')?;
                            Some(h.parse::<u16>().ok()? * 60 + mi.parse::<u16>().ok()?)
                        };
                        window = Some((m(a)?, m(b)?));
                    }
                }
                Repeat::Every { n: n.max(1), unit, days, window }
            }
        })
    }

    /// The next occurrence strictly after `from`.
    pub fn next(self, from: DateTime<Local>) -> DateTime<Local> {
        let n = self.next_naive(from.naive_local());
        Local.from_local_datetime(&n).earliest().unwrap_or(from + Duration::days(1))
    }

    pub fn next_naive(self, from: NaiveDateTime) -> NaiveDateTime {
        let add_months = |d: NaiveDateTime, m: u32| d.checked_add_months(chrono::Months::new(m)).unwrap_or(d + Duration::days(30 * m as i64));
        match self {
            Repeat::Daily => from + Duration::days(1),
            Repeat::Weekly => from + Duration::days(7),
            Repeat::Weekdays => {
                let mut d = from + Duration::days(1);
                while !has(WEEKDAYS, d.weekday()) {
                    d += Duration::days(1);
                }
                d
            }
            Repeat::Monthly => add_months(from, 1),
            Repeat::Yearly => add_months(from, 12),
            Repeat::Every { n, unit, days, window } => {
                let n = n.max(1);
                match unit {
                    Unit::Minutes | Unit::Hours => {
                        let step = if unit == Unit::Minutes { Duration::minutes(n as i64) } else { Duration::hours(n as i64) };
                        let (ws, we) = window.filter(|(a, b)| a < b).unwrap_or((0, 24 * 60));
                        let mut c = from + step;
                        for _ in 0..400 {
                            let day_ok = days == 0 || has(days, c.weekday());
                            let m = (c.hour() * 60 + c.minute()) as u16;
                            if day_ok && m >= ws && m <= we {
                                return c;
                            }
                            // jump to the start of the next allowed window
                            c = if day_ok && m < ws { c.date().and_time(hhmm(ws)) } else { (c.date() + Duration::days(1)).and_time(hhmm(ws)) };
                            if days == 0 || has(days, c.weekday()) {
                                return c;
                            }
                        }
                        c
                    }
                    Unit::Days => from + Duration::days(n as i64),
                    Unit::Weeks if days == 0 => from + Duration::weeks(n as i64),
                    Unit::Weeks => {
                        let monday = |d: NaiveDate| d - Duration::days(d.weekday().num_days_from_monday() as i64);
                        let w0 = monday(from.date());
                        for k in 1..=(7 * n as i64 + 7) {
                            let c = from + Duration::days(k);
                            let week = (monday(c.date()) - w0).num_days() / 7;
                            if has(days, c.weekday()) && week % n as i64 == 0 {
                                return c;
                            }
                        }
                        from + Duration::weeks(n as i64)
                    }
                    Unit::Months => add_months(from, n),
                    Unit::Years => add_months(from, 12 * n),
                }
            }
        }
    }

    /// First occurrence at or after `start` that is later than `now`: `start` itself if it is
    /// in the future and on an allowed day, otherwise the next one.
    pub fn first_from(self, start: NaiveDateTime, now: NaiveDateTime) -> NaiveDateTime {
        let allowed = match self {
            Repeat::Weekdays => has(WEEKDAYS, start.weekday()),
            Repeat::Every { unit: Unit::Weeks, days, .. } if days != 0 => has(days, start.weekday()),
            _ => true,
        };
        let mut d = if allowed { start } else { self.next_naive(start) };
        let mut guard = 0;
        while d <= now && guard < 100_000 {
            d = self.next_naive(d);
            guard += 1;
        }
        d
    }
}

impl Serialize for Repeat {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.code())
    }
}

impl<'de> Deserialize<'de> for Repeat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Repeat, D::Error> {
        let s = String::deserialize(d)?;
        Repeat::from_code(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown repeat {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(d: u32, h: u32, m: u32) -> NaiveDateTime {
        // November 2026: the 25th is a Wednesday
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn codes_round_trip() {
        for r in [
            Repeat::Daily,
            Repeat::Yearly,
            Repeat::every(30, Unit::Minutes),
            Repeat::Every { n: 2, unit: Unit::Weeks, days: day_bit(Weekday::Mon) | day_bit(Weekday::Thu), window: None },
            Repeat::Every { n: 1, unit: Unit::Hours, days: WEEKDAYS, window: Some((9 * 60, 17 * 60 + 30)) },
        ] {
            assert_eq!(Repeat::from_code(&r.code()), Some(r), "{}", r.code());
        }
        assert_eq!(serde_json::to_string(&Repeat::Weekly).unwrap(), "\"weekly\"");
    }

    #[test]
    fn minutes_inside_a_window() {
        let r = Repeat::Every { n: 30, unit: Unit::Minutes, days: WEEKDAYS, window: Some((9 * 60, 17 * 60)) };
        assert_eq!(r.next_naive(at(25, 10, 0)), at(25, 10, 30));
        assert_eq!(r.next_naive(at(25, 16, 45)), at(26, 9, 0)); // 17:15 is past the window
        assert_eq!(r.next_naive(at(27, 17, 0)), at(30, 9, 0)); // Friday → Monday
        assert_eq!(r.next_naive(at(25, 6, 0)), at(25, 9, 0));
        assert_eq!(Repeat::every(30, Unit::Minutes).next_naive(at(25, 23, 45)), at(26, 0, 15));
    }

    #[test]
    fn weeks_on_days() {
        let mon_thu = day_bit(Weekday::Mon) | day_bit(Weekday::Thu);
        let r = Repeat::Every { n: 1, unit: Unit::Weeks, days: mon_thu, window: None };
        assert_eq!(r.next_naive(at(25, 9, 0)), at(26, 9, 0)); // Wed → Thu
        assert_eq!(r.next_naive(at(26, 9, 0)), at(30, 9, 0)); // Thu → Mon
        let r2 = Repeat::Every { n: 2, unit: Unit::Weeks, days: mon_thu, window: None };
        assert_eq!(r2.next_naive(at(26, 9, 0)), NaiveDate::from_ymd_opt(2026, 12, 7).unwrap().and_hms_opt(9, 0, 0).unwrap());
        assert_eq!(r.first_from(at(25, 9, 0), at(25, 8, 0)), at(26, 9, 0));
    }

    #[test]
    fn labels() {
        assert_eq!(Repeat::every(30, Unit::Minutes).label(), "every 30 min");
        assert_eq!(Repeat::every(1, Unit::Hours).label(), "every hour");
        assert_eq!(Repeat::every(2, Unit::Days).label(), "every other day");
        let r = Repeat::Every { n: 1, unit: Unit::Weeks, days: day_bit(Weekday::Mon) | day_bit(Weekday::Wed), window: None };
        assert_eq!(r.label(), "every Mon, Wed");
        let r = Repeat::Every { n: 1, unit: Unit::Hours, days: WEEKDAYS, window: Some((540, 1020)) };
        assert_eq!(r.label(), "every hour · weekdays · 9am–5pm");
    }
}

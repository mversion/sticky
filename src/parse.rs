//! Turns what you type into an item: "remind me to clean the coffee machine at 4pm" becomes a
//! reminder "Clean the coffee machine" due today at 16:00 with a pop-up.
//!
//! Understands (case-insensitive, anywhere in the text):
//! - kind:      `note:` / `n:` make a note (no checkbox); `remind me (to)`, `reminder:`,
//!              `don't forget (to)` turn on the pop-up
//! - times:     `at 4pm`, `4:30pm`, `at 16:30`, `at 4` (1-6 means pm), `noon`, `midnight`,
//!              `tonight`, `this morning/afternoon/evening`, `tomorrow morning`
//! - relative:  `in 10 min`, `in 2 hours`, `in an hour`, `in half an hour`, `in 3 days`, `in 10m`
//! - dates:     `today`, `tomorrow`/`tmrw`, `day after tomorrow`, `(on|next|this) friday`,
//!              `next week`, `25/12`, `25/12/2027`, `25 dec`, `dec 25th`
//! - repeat:    `every day`/`daily`, `every weekday`, `every monday`, `every week`/`weekly`,
//!              `every month`/`monthly`
//! - priority:  `!` low, `!!` medium, `!!!` high, or `!low` `!med` `!high`, `p1` `p2` `p3`, `urgent`, `asap`
//! - category:  `#upnext`, `#later`, `#today` … (matched against your categories, prefix ok);
//!              any other `#word` becomes a tag

use crate::store::{Kind, Priority, Repeat};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub text: String,
    pub kind: Kind,
    pub due: Option<NaiveDateTime>,
    pub all_day: bool,
    pub remind: bool,
    pub priority: Priority,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub repeat: Option<Repeat>,
}

pub struct Opts<'a> {
    pub categories: &'a [String],
    /// A time on its own ("call Bob at 3pm") also sets a pop-up.
    pub remind_when_timed: bool,
    /// Time used for a reminder that only has a date ("remind me tomorrow to …").
    pub default_time: NaiveTime,
}

struct Tok {
    orig: String,
    w: String,
    used: bool,
}

pub fn parse(input: &str, now: NaiveDateTime, o: &Opts) -> Parsed {
    let mut kind = Kind::Task;
    let mut remind = false;
    let mut s = input.trim().to_string();

    // ---- leading phrases
    let lower = s.to_lowercase();
    let prefixes: [(&str, u8); 12] = [
        ("remind me to ", 1),
        ("remind me that ", 1),
        ("remind me about ", 1),
        ("remind me ", 1),
        ("reminder: ", 1),
        ("reminder ", 1),
        ("don't forget to ", 1),
        ("dont forget to ", 1),
        ("note: ", 2),
        ("n: ", 2),
        ("todo: ", 0),
        ("task: ", 0),
    ];
    for (p, k) in prefixes {
        if lower.starts_with(p) {
            s = s[p.len()..].trim_start().to_string();
            match k {
                1 => remind = true,
                2 => kind = Kind::Note,
                _ => {}
            }
            break;
        }
    }
    // "remind me at 4pm to clean …": the "to" comes after the time.
    let mut toks: Vec<Tok> = s
        .split_whitespace()
        .map(|t| Tok {
            orig: t.to_string(),
            w: t.to_lowercase().trim_end_matches([',', '.', ';', '?']).to_string(),
            used: false,
        })
        .collect();

    let today = now.date();
    let mut date: Option<NaiveDate> = None;
    let mut time: Option<NaiveTime> = None;
    let mut rel: Option<Duration> = None;
    let mut repeat: Option<Repeat> = None;
    let mut priority = Priority::None;
    let mut category: Option<String> = None;
    let mut tags = vec![];
    let mut repeat_weekday: Option<Weekday> = None;

    let n = toks.len();
    let mut i = 0;
    while i < n {
        let w = toks[i].w.clone();
        let next = |k: usize| toks.get(i + k).map(|t| t.w.as_str()).unwrap_or("");
        let mut take = 0usize; // tokens consumed starting at i

        // ---- priority
        if let Some(p) = priority_word(&w) {
            priority = p;
            take = 1;
        }
        // ---- category / tag
        else if w.len() > 1 && w.starts_with('#') {
            let tag = toks[i].orig.trim_start_matches('#').trim_end_matches([',', '.', ';']).to_string();
            match match_category(&tag, o.categories) {
                Some(c) => category = Some(c),
                None => tags.push(tag),
            }
            take = 1;
        }
        // ---- repeat
        else if w == "daily" {
            repeat = Some(Repeat::Daily);
            take = 1;
        } else if w == "weekly" {
            repeat = Some(Repeat::Weekly);
            take = 1;
        } else if w == "monthly" {
            repeat = Some(Repeat::Monthly);
            take = 1;
        } else if w == "every" || w == "each" {
            let a = next(1);
            if a == "day" || a == "morning" {
                repeat = Some(Repeat::Daily);
                if a == "morning" && time.is_none() {
                    time = NaiveTime::from_hms_opt(9, 0, 0);
                }
                take = 2;
            } else if a == "weekday" || a == "weekdays" {
                repeat = Some(Repeat::Weekdays);
                take = 2;
            } else if a == "week" {
                repeat = Some(Repeat::Weekly);
                take = 2;
            } else if a == "month" {
                repeat = Some(Repeat::Monthly);
                take = 2;
            } else if let Some(wd) = weekday(a) {
                repeat = Some(Repeat::Weekly);
                repeat_weekday = Some(wd);
                take = 2;
            }
        }
        // ---- relative: in 10 min / in an hour / in half an hour / in 10m
        else if w == "in" || w == "within" {
            if let Some((d, k)) = relative(&[next(1), next(2), next(3), next(4)]) {
                rel = Some(d);
                take = 1 + k;
            }
        }
        // ---- named days
        else if w == "today" {
            date = Some(today);
            take = 1;
        } else if w == "tonight" {
            date = Some(today);
            if time.is_none() {
                time = NaiveTime::from_hms_opt(20, 0, 0);
            }
            take = 1;
        } else if matches!(w.as_str(), "tomorrow" | "tmrw" | "tmr" | "tomoz" | "tomorow") {
            date = Some(today + Duration::days(1));
            take = 1;
        } else if w == "day" && next(1) == "after" && matches!(next(2), "tomorrow" | "tmrw") {
            date = Some(today + Duration::days(2));
            take = 3;
        } else if w == "next" && next(1) == "week" {
            let mut d = today + Duration::days(1);
            while d.weekday() != Weekday::Mon {
                d += Duration::days(1);
            }
            date = Some(d);
            take = 2;
        } else if let Some(wd) = weekday(&w) {
            date = Some(next_weekday(today, wd));
            take = 1;
        } else if matches!(w.as_str(), "on" | "next" | "this" | "by" | "due") && weekday(next(1)).is_some() {
            date = Some(next_weekday(today, weekday(next(1)).unwrap()));
            take = 2;
        } else if (w == "this" || w == "in") && day_part(next(1)).is_some() {
            if time.is_none() {
                time = day_part(next(1));
            }
            if date.is_none() {
                date = Some(today);
            }
            take = 2;
        } else if day_part(&w).is_some() && i > 0 && toks[i - 1].used && (date.is_some() || repeat.is_some()) {
            // "tomorrow morning", "every day morning"
            if time.is_none() {
                time = day_part(&w);
            }
            take = 1;
        }
        // ---- calendar dates: 25/12, on 25/12, 25 dec, dec 25th
        else if let Some((d, k)) = calendar_date(&[&w, next(1), next(2)], today) {
            date = Some(d);
            take = k;
        } else if matches!(w.as_str(), "on" | "by" | "due")
            && let Some((d, k)) = calendar_date(&[next(1), next(2), next(3)], today)
        {
            date = Some(d);
            take = 1 + k;
        }
        // ---- clock times
        else if w == "noon" || w == "midday" {
            time = NaiveTime::from_hms_opt(12, 0, 0);
            take = 1;
        } else if w == "midnight" {
            time = NaiveTime::from_hms_opt(23, 59, 0);
            take = 1;
        } else if matches!(w.as_str(), "at" | "@" | "by" | "around" | "from") {
            if matches!(next(1), "noon" | "midday") {
                time = NaiveTime::from_hms_opt(12, 0, 0);
                take = 2;
            } else if next(1) == "midnight" {
                time = NaiveTime::from_hms_opt(23, 59, 0);
                take = 2;
            } else if let Some((t, k)) = clock(next(1), next(2), true) {
                time = Some(t);
                take = 1 + k;
            }
        } else if let Some((t, k)) = clock(&w, next(1), false) {
            time = Some(t);
            take = k;
        }

        if take > 0 {
            for t in toks.iter_mut().skip(i).take(take) {
                t.used = true;
            }
            i += take;
        } else {
            i += 1;
        }
    }

    // ---- what is left is the text
    let mut words: Vec<&str> = toks.iter().filter(|t| !t.used).map(|t| t.orig.as_str()).collect();
    // dangling connectors left at the ends: "call bob at" / "to clean"
    while let Some(f) = words.first() {
        if matches!(f.to_lowercase().as_str(), "to" | "that" | "about" | "-" | "–" | ":") {
            words.remove(0);
        } else {
            break;
        }
    }
    while let Some(l) = words.last() {
        if matches!(l.to_lowercase().as_str(), "at" | "on" | "by" | "in" | "to" | "-" | "–" | "for" | "due") {
            words.pop();
        } else {
            break;
        }
    }
    let mut text = words.join(" ");
    text = text.trim().trim_end_matches([',', ';']).trim().to_string();
    if let Some(c) = text.chars().next() {
        text = c.to_uppercase().collect::<String>() + &text[c.len_utf8()..];
    }

    // ---- work out the due time
    let mut all_day = false;
    let due = if let Some(r) = rel {
        Some(now + r)
    } else if date.is_some() || time.is_some() || repeat.is_some() {
        if let Some(wd) = repeat_weekday
            && date.is_none()
        {
            date = Some(if today.weekday() == wd { today } else { next_weekday(today, wd) });
        }
        let t = match time {
            Some(t) => t,
            None => {
                all_day = true;
                o.default_time
            }
        };
        let mut dt = date.unwrap_or(today).and_time(t);
        if dt <= now && date.is_none() {
            // A bare time that has passed today means tomorrow (or the next repeat).
            dt = match repeat {
                Some(Repeat::Weekdays) => {
                    let mut d = dt + Duration::days(1);
                    while matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
                        d += Duration::days(1);
                    }
                    d
                }
                Some(Repeat::Weekly) => dt + Duration::days(7),
                Some(Repeat::Monthly) => dt.checked_add_months(chrono::Months::new(1)).unwrap_or(dt),
                _ => dt + Duration::days(1),
            };
        } else if dt <= now && repeat_weekday.is_some() {
            dt += Duration::days(7);
        }
        Some(dt)
    } else {
        None
    };

    let timed = due.is_some() && !all_day;
    let remind = kind == Kind::Task && due.is_some() && (remind || (o.remind_when_timed && timed));

    Parsed { text, kind, due, all_day: all_day && !remind, remind, priority, category, tags, repeat }
}

fn priority_word(w: &str) -> Option<Priority> {
    Some(match w {
        "!" | "!low" | "p3" | "!l" => Priority::Low,
        "!!" | "!med" | "!medium" | "p2" | "!m" => Priority::Medium,
        "!!!" | "!high" | "!h" | "p1" | "urgent" | "asap" | "!urgent" => Priority::High,
        _ => return None,
    })
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// `upnext`, `up-next`, `next` and `up` all find "Up next".
pub fn match_category(tag: &str, cats: &[String]) -> Option<String> {
    let t = norm(tag);
    if t.is_empty() {
        return None;
    }
    cats.iter()
        .find(|c| norm(c) == t)
        .or_else(|| cats.iter().find(|c| norm(c).starts_with(&t)))
        .or_else(|| cats.iter().find(|c| c.split_whitespace().any(|w| norm(w) == t)))
        .cloned()
}

fn weekday(w: &str) -> Option<Weekday> {
    Some(match w {
        "monday" | "mon" => Weekday::Mon,
        "tuesday" | "tue" | "tues" => Weekday::Tue,
        "wednesday" | "wed" => Weekday::Wed,
        "thursday" | "thu" | "thur" | "thurs" => Weekday::Thu,
        "friday" | "fri" => Weekday::Fri,
        "saturday" | "sat" => Weekday::Sat,
        "sunday" | "sun" => Weekday::Sun,
        _ => return None,
    })
}

/// The next `wd` after today (a week away if today is `wd`).
fn next_weekday(today: NaiveDate, wd: Weekday) -> NaiveDate {
    let mut d = today + Duration::days(1);
    while d.weekday() != wd {
        d += Duration::days(1);
    }
    d
}

fn day_part(w: &str) -> Option<NaiveTime> {
    match w {
        "morning" => NaiveTime::from_hms_opt(9, 0, 0),
        "lunch" | "lunchtime" => NaiveTime::from_hms_opt(12, 30, 0),
        "afternoon" => NaiveTime::from_hms_opt(14, 0, 0),
        "arvo" => NaiveTime::from_hms_opt(14, 0, 0),
        "evening" => NaiveTime::from_hms_opt(18, 0, 0),
        "night" => NaiveTime::from_hms_opt(20, 0, 0),
        _ => None,
    }
}

fn number_word(w: &str) -> Option<f64> {
    Some(match w {
        "a" | "an" | "one" => 1.0,
        "two" | "couple" => 2.0,
        "three" => 3.0,
        "four" => 4.0,
        "five" => 5.0,
        "ten" => 10.0,
        "fifteen" => 15.0,
        "twenty" => 20.0,
        "thirty" => 30.0,
        "half" => 0.5,
        _ => return w.parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 10000.0),
    })
}

fn unit(w: &str) -> Option<f64> {
    // minutes per unit
    Some(match w {
        "s" | "sec" | "secs" | "second" | "seconds" => 1.0 / 60.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 1.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
        "d" | "day" | "days" => 1440.0,
        "w" | "wk" | "wks" | "week" | "weeks" => 10080.0,
        _ => return None,
    })
}

/// After "in": `10 min`, `an hour`, `half an hour`, `10m`, `1.5 hours`, `a couple of hours`.
fn relative(w: &[&str]) -> Option<(Duration, usize)> {
    let mins = |v: f64| Duration::seconds((v * 60.0).round() as i64);
    // half an hour
    if w[0] == "half" && matches!(w[1], "an" | "a") && unit(w[2]).is_some() {
        return Some((mins(0.5 * unit(w[2])?), 3));
    }
    // a couple of hours
    if w[0] == "a" && w[1] == "couple" && w[2] == "of" && unit(w[3]).is_some() {
        return Some((mins(2.0 * unit(w[3])?), 4));
    }
    // 10m / 2h / 90min
    let split = w[0].find(|c: char| c.is_alphabetic());
    if let Some(p) = split
        && p > 0
        && let (Ok(v), Some(u)) = (w[0][..p].parse::<f64>(), unit(&w[0][p..]))
    {
        return Some((mins(v * u), 1));
    }
    let v = number_word(w[0])?;
    let u = unit(w[1])?;
    Some((mins(v * u), 2))
}

/// `4pm`, `4:30pm`, `4.30 pm`, `16:30`, and after "at" also a bare `4` (1-6 means afternoon).
/// Returns the time and how many tokens it used.
fn clock(a: &str, b: &str, after_at: bool) -> Option<(NaiveTime, usize)> {
    let (body, mut suffix) = if let Some(x) = a.strip_suffix("pm").or_else(|| a.strip_suffix("p.m")) {
        (x, Some(true))
    } else if let Some(x) = a.strip_suffix("am").or_else(|| a.strip_suffix("a.m")) {
        (x, Some(false))
    } else {
        (a, None)
    };
    let mut used = 1;
    if suffix.is_none() {
        match b {
            "pm" | "p.m." | "p.m" => {
                suffix = Some(true);
                used = 2;
            }
            "am" | "a.m." | "a.m" => {
                suffix = Some(false);
                used = 2;
            }
            _ => {}
        }
    }
    if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit() || c == ':' || c == '.') {
        return None;
    }
    let (h, m) = match body.split_once([':', '.']) {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (body.parse::<u32>().ok()?, 0),
    };
    let has_colon = body.contains([':', '.']);
    // A bare number needs "at" or am/pm, so "buy 4 eggs" stays text.
    if suffix.is_none() && !has_colon && !after_at {
        return None;
    }
    if suffix.is_none() && has_colon && !after_at && body.contains('.') {
        return None; // "2.5" on its own is not a time
    }
    if m > 59 {
        return None;
    }
    let h = match suffix {
        Some(pm) => {
            if !(1..=12).contains(&h) {
                return None;
            }
            match (pm, h) {
                (true, 12) => 12,
                (true, h) => h + 12,
                (false, 12) => 0,
                (false, h) => h,
            }
        }
        None => {
            if h > 23 {
                return None;
            }
            if (1..=6).contains(&h) { h + 12 } else { h }
        }
    };
    Some((NaiveTime::from_hms_opt(h, m, 0)?, used))
}

fn month(w: &str) -> Option<u32> {
    let w = w.trim_end_matches('.');
    let names = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    if w.len() < 3 {
        return None;
    }
    names.iter().position(|m| w.starts_with(m)).filter(|_| {
        // "march", "mar", "sept" – but not "maybe" or "decide"
        let full = [
            "january", "february", "march", "april", "may", "june", "july", "august", "september", "october",
            "november", "december",
        ];
        full.iter().any(|f| f.starts_with(w)) || w == "sept"
    }).map(|p| p as u32 + 1)
}

fn day_num(w: &str) -> Option<u32> {
    let w = w.trim_end_matches("st").trim_end_matches("nd").trim_end_matches("rd").trim_end_matches("th");
    w.parse::<u32>().ok().filter(|d| (1..=31).contains(d))
}

/// `25/12`, `25/12/27`, `25-12-2027`, `25 dec`, `25th of december`, `dec 25`.
fn calendar_date(w: &[&str], today: NaiveDate) -> Option<(NaiveDate, usize)> {
    let pick = |d: u32, m: u32, y: Option<i32>| -> Option<NaiveDate> {
        match y {
            Some(y) => NaiveDate::from_ymd_opt(y, m, d),
            None => {
                let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
                if this < today { NaiveDate::from_ymd_opt(today.year() + 1, m, d) } else { Some(this) }
            }
        }
    };
    let a = w[0];
    if a.contains(['/', '-']) && a.chars().all(|c| c.is_ascii_digit() || c == '/' || c == '-') {
        let parts: Vec<&str> = a.split(['/', '-']).collect();
        if parts.len() == 2 || parts.len() == 3 {
            let d = parts[0].parse().ok()?;
            let m = parts[1].parse().ok()?;
            let y = parts.get(2).and_then(|y| y.parse::<i32>().ok()).map(|y| if y < 100 { 2000 + y } else { y });
            return pick(d, m, y).map(|x| (x, 1));
        }
        return None;
    }
    if let (Some(d), Some(m)) = (day_num(a), month(w[1])) {
        return pick(d, m, None).map(|x| (x, 2));
    }
    if let Some(d) = day_num(a)
        && w[1] == "of"
        && let Some(m) = month(w[2])
    {
        return pick(d, m, None).map(|x| (x, 3));
    }
    if let (Some(m), Some(d)) = (month(a), day_num(w[1])) {
        return pick(d, m, None).map(|x| (x, 2));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats() -> Vec<String> {
        ["Up next", "Today", "Later", "Someday"].iter().map(|s| s.to_string()).collect()
    }
    // Wednesday 25 Nov 2026, 10:00
    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 25).unwrap().and_hms_opt(10, 0, 0).unwrap()
    }
    fn p(s: &str) -> Parsed {
        let c = cats();
        parse(s, now(), &Opts { categories: &c, remind_when_timed: true, default_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap() })
    }
    fn at(d: u32, h: u32, m: u32) -> Option<NaiveDateTime> {
        Some(NaiveDate::from_ymd_opt(2026, if d < 25 { 12 } else { 11 }, d).unwrap().and_hms_opt(h, m, 0).unwrap())
    }

    #[test]
    fn coffee_machine() {
        let r = p("remind me to clean the coffee machine at 4pm");
        assert_eq!(r.text, "Clean the coffee machine");
        assert_eq!(r.due, at(25, 16, 0));
        assert!(r.remind);
    }
    #[test]
    fn time_first() {
        let r = p("remind me at 4:30 pm to call Bob");
        assert_eq!(r.text, "Call Bob");
        assert_eq!(r.due, at(25, 16, 30));
    }
    #[test]
    fn relative_minutes() {
        let r = p("check the oven in 10 min");
        assert_eq!(r.text, "Check the oven");
        assert_eq!(r.due, Some(now() + Duration::minutes(10)));
        assert!(r.remind);
        assert_eq!(p("stretch in half an hour").due, Some(now() + Duration::minutes(30)));
        assert_eq!(p("stretch in 2h").due, Some(now() + Duration::hours(2)));
        assert_eq!(p("stretch in an hour").text, "Stretch");
    }
    #[test]
    fn tomorrow_morning() {
        let r = p("email supplier tomorrow morning");
        assert_eq!(r.text, "Email supplier");
        assert_eq!(r.due, at(26, 9, 0));
    }
    #[test]
    fn bare_numbers_stay_text() {
        let r = p("buy 4 eggs and 2.5kg flour");
        assert_eq!(r.text, "Buy 4 eggs and 2.5kg flour");
        assert_eq!(r.due, None);
        assert!(!r.remind);
    }
    #[test]
    fn at_bare_hour_is_afternoon() {
        assert_eq!(p("meeting at 3").due, at(25, 15, 0));
        assert_eq!(p("standup at 9").due, at(26, 9, 0)); // 9am passed → tomorrow
    }
    #[test]
    fn date_only_no_popup() {
        let r = p("pay rego friday");
        assert_eq!(r.text, "Pay rego");
        assert_eq!(r.due, at(27, 9, 0));
        assert!(r.all_day);
        assert!(!r.remind);
        let r = p("remind me friday to pay rego");
        assert!(r.remind);
        assert!(!r.all_day);
    }
    #[test]
    fn calendar() {
        assert_eq!(p("xmas lunch 25/12 at 1pm").due, Some(NaiveDate::from_ymd_opt(2026, 12, 25).unwrap().and_hms_opt(13, 0, 0).unwrap()));
        assert_eq!(p("renew domain on 3 jan").due.unwrap().date(), NaiveDate::from_ymd_opt(2027, 1, 3).unwrap());
        assert_eq!(p("renew domain dec 1st").due.unwrap().date(), NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
        assert_eq!(p("maybe decide later").due, None);
    }
    #[test]
    fn priority_category_tags() {
        let r = p("fix the printer !!! #upnext #office");
        assert_eq!(r.text, "Fix the printer");
        assert_eq!(r.priority, Priority::High);
        assert_eq!(r.category.as_deref(), Some("Up next"));
        assert_eq!(r.tags, vec!["office".to_string()]);
        assert_eq!(p("x #later").category.as_deref(), Some("Later"));
        assert_eq!(p("x #next").category.as_deref(), Some("Up next"));
        assert_eq!(p("x !").priority, Priority::Low);
        assert_eq!(p("call bob asap").priority, Priority::High);
    }
    #[test]
    fn notes() {
        let r = p("note: wifi password is on the router at 4pm");
        assert_eq!(r.kind, Kind::Note);
        assert!(!r.remind);
    }
    #[test]
    fn repeats() {
        let r = p("take tablets every day at 8pm");
        assert_eq!(r.repeat, Some(Repeat::Daily));
        assert_eq!(r.due, at(25, 20, 0));
        assert_eq!(r.text, "Take tablets");
        let r = p("team standup every monday at 9am");
        assert_eq!(r.repeat, Some(Repeat::Weekly));
        assert_eq!(r.due, Some(NaiveDate::from_ymd_opt(2026, 11, 30).unwrap().and_hms_opt(9, 0, 0).unwrap()));
        assert_eq!(p("backup every weekday at 5pm").repeat, Some(Repeat::Weekdays));
    }
    #[test]
    fn tonight_and_noon() {
        assert_eq!(p("put bins out tonight").due, at(25, 20, 0));
        assert_eq!(p("lunch with sam at noon").due, at(25, 12, 0));
        assert_eq!(p("lunch with sam at noon").text, "Lunch with sam");
    }
    #[test]
    fn next_week() {
        assert_eq!(p("plan budget next week").due.unwrap().date(), NaiveDate::from_ymd_opt(2026, 11, 30).unwrap());
    }
}

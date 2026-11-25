//! Items, the activity log and where everything lives on disk.
//!
//! Data sits in `%USERPROFILE%\.sticky` (not AppData: Claude and other MSIX apps get a private
//! copy of AppData, so a copy started from them would see different data).
//! `items.json` is the single source of truth and is shared by every Sticky process and by the
//! desk dashboard tile. Writers take `store.lock`, re-read, change, and replace the file atomically.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::SystemTime;

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("STICKY_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".sticky")))
        .unwrap_or_else(|| PathBuf::from(".sticky"));
    let _ = fs::create_dir_all(&base);
    base
}
pub fn items_path() -> PathBuf {
    data_dir().join("items.json")
}
pub fn log_path() -> PathBuf {
    data_dir().join("log.jsonl")
}
pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}
pub fn request_path() -> PathBuf {
    data_dir().join("ui.request")
}
pub fn mtime(p: &PathBuf) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Task,
    Note,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    #[default]
    None,
    Low,
    Medium,
    High,
}
impl Priority {
    pub const ALL: [Priority; 4] = [Priority::None, Priority::Low, Priority::Medium, Priority::High];
    pub fn label(self) -> &'static str {
        match self {
            Priority::None => "No priority",
            Priority::Low => "Low",
            Priority::Medium => "Medium",
            Priority::High => "High",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    Daily,
    Weekdays,
    Weekly,
    Monthly,
}
impl Repeat {
    pub fn label(self) -> &'static str {
        match self {
            Repeat::Daily => "every day",
            Repeat::Weekdays => "weekdays",
            Repeat::Weekly => "every week",
            Repeat::Monthly => "every month",
        }
    }
    /// The next occurrence strictly after `from`.
    pub fn next(self, from: DateTime<Local>) -> DateTime<Local> {
        let mut d = from;
        match self {
            Repeat::Daily => d + Duration::days(1),
            Repeat::Weekly => d + Duration::days(7),
            Repeat::Weekdays => {
                loop {
                    d += Duration::days(1);
                    if !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
                        return d;
                    }
                }
            }
            Repeat::Monthly => from
                .checked_add_months(chrono::Months::new(1))
                .unwrap_or(from + Duration::days(30)),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub done: bool,
    pub created: DateTime<Local>,
    #[serde(default)]
    pub done_at: Option<DateTime<Local>>,
    /// When it is due. With `remind` set, a pop-up appears at this time.
    #[serde(default)]
    pub due: Option<DateTime<Local>>,
    /// Due on a day with no particular time.
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub remind: bool,
    /// The pop-up for the current `due` has been shown.
    #[serde(default)]
    pub fired: bool,
    #[serde(default)]
    pub snoozed_until: Option<DateTime<Local>>,
    #[serde(default)]
    pub repeat: Option<Repeat>,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    /// Manual order inside a category (lower first).
    #[serde(default)]
    pub order: i64,
}

impl Item {
    /// When the next pop-up is due, if any.
    pub fn alarm_at(&self) -> Option<DateTime<Local>> {
        if self.done || !self.remind {
            return None;
        }
        if let Some(s) = self.snoozed_until {
            return Some(s);
        }
        if self.fired {
            return None;
        }
        self.due
    }
    pub fn is_overdue(&self, now: DateTime<Local>) -> bool {
        if self.done {
            return false;
        }
        match self.due {
            Some(d) if self.all_day => d.date_naive() < now.date_naive(),
            Some(d) => d < now,
            None => false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Data {
    #[serde(default = "one")]
    pub next_id: u64,
    #[serde(default)]
    pub items: Vec<Item>,
    /// Written on every save so readers (the desk dashboard) can show "as of".
    #[serde(default)]
    pub saved: Option<DateTime<Local>>,
}
fn one() -> u64 {
    1
}

impl Data {
    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }
    pub fn add(&mut self, mut item: Item) -> u64 {
        item.id = self.next_id.max(self.items.iter().map(|i| i.id + 1).max().unwrap_or(1));
        self.next_id = item.id + 1;
        item.order = self.items.iter().map(|i| i.order).min().unwrap_or(0) - 1;
        let id = item.id;
        self.items.push(item);
        id
    }
}

pub fn load() -> Data {
    let p = items_path();
    match fs::read(&p) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            // Keep the unreadable file aside rather than overwrite it later.
            let bad = p.with_extension(format!("bad-{}.json", Local::now().format("%Y%m%d%H%M%S")));
            let _ = fs::copy(&p, &bad);
            eprintln!("items.json unreadable ({e}); copied to {}", bad.display());
            Data { next_id: 1, ..Default::default() }
        }),
        Err(_) => Data { next_id: 1, ..Default::default() },
    }
}

fn save(data: &mut Data) -> std::io::Result<()> {
    data.saved = Some(Local::now());
    let p = items_path();
    let tmp = p.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(data).map_err(std::io::Error::other)?;
    fs::write(&tmp, json)?;
    // Retry: a reader (dashboard, other process) may hold the file open for a moment.
    let mut last = Ok(());
    for _ in 0..20 {
        last = fs::rename(&tmp, &p);
        if last.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    last
}

/// Lock, re-read, change, save. Returns what the closure returns.
pub fn update<R>(f: impl FnOnce(&mut Data) -> R) -> R {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir().join("store.lock"));
    if let Ok(l) = &lock {
        let _ = l.lock();
    }
    let mut data = load();
    let r = f(&mut data);
    if let Err(e) = save(&mut data) {
        eprintln!("save failed: {e}");
    }
    if let Ok(l) = &lock {
        let _ = l.unlock();
    }
    r
}

// ---------------------------------------------------------------- activity log

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LogEntry {
    pub at: DateTime<Local>,
    pub action: String,
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub text: String,
}

pub fn log(action: &str, id: u64, text: &str) {
    let e = LogEntry { at: Local::now(), action: action.into(), id, text: text.into() };
    if let Ok(line) = serde_json::to_string(&e) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path()) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The newest `max` entries, newest first.
pub fn read_log(max: usize) -> Vec<LogEntry> {
    let Ok(f) = File::open(log_path()) else { return vec![] };
    let mut all: Vec<LogEntry> = BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    all.reverse();
    all.truncate(max);
    all
}

// ---------------------------------------------------------------- shared actions

/// Tick or untick. A repeating item moves to its next occurrence instead of closing.
pub fn set_done(id: u64, done: bool) {
    let msg = update(|d| {
        let item = d.get_mut(id)?;
        if done && item.repeat.is_some() && item.due.is_some() {
            let r = item.repeat.unwrap();
            let mut next = r.next(item.due.unwrap());
            let now = Local::now();
            while next <= now {
                next = r.next(next);
            }
            item.due = Some(next);
            item.fired = false;
            item.snoozed_until = None;
            return Some(("done (repeats)", item.text.clone()));
        }
        item.done = done;
        item.done_at = done.then(Local::now);
        if done {
            item.snoozed_until = None;
        }
        Some((if done { "done" } else { "reopened" }, item.text.clone()))
    });
    if let Some((a, t)) = msg {
        log(a, id, &t);
    }
}

pub fn snooze(id: u64, minutes: i64) {
    let until = Local::now() + Duration::minutes(minutes);
    let t = update(|d| {
        d.get_mut(id).map(|i| {
            i.snoozed_until = Some(until);
            i.text.clone()
        })
    });
    if let Some(t) = t {
        log(&format!("snoozed {minutes} min"), id, &t);
    }
}

pub fn dismiss(id: u64) {
    update(|d| {
        if let Some(i) = d.get_mut(id) {
            i.snoozed_until = None;
            i.fired = true;
        }
    });
}

pub fn delete(id: u64) {
    let t = update(|d| {
        let pos = d.items.iter().position(|i| i.id == id)?;
        Some(d.items.remove(pos).text)
    });
    if let Some(t) = t {
        log("deleted", id, &t);
    }
}

/// Human description of a due time relative to now: "4:00 pm", "Tomorrow 9:00 am", "Fri 26 Dec".
pub fn describe_due(due: DateTime<Local>, all_day: bool, h24: bool) -> String {
    let now = Local::now();
    let today = now.date_naive();
    let d = due.date_naive();
    let time = if h24 { due.format("%H:%M").to_string() } else { fmt_12(due) };
    let day = day_word(d, today);
    match (all_day, day.as_str()) {
        (true, _) => day,
        (false, "Today") => time,
        (false, _) => format!("{day} {time}"),
    }
}

pub fn fmt_12(t: DateTime<Local>) -> String {
    t.format("%-I:%M %P").to_string()
}

pub fn day_word(d: NaiveDate, today: NaiveDate) -> String {
    let diff = (d - today).num_days();
    match diff {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        2..=6 => d.format("%A").to_string(),
        _ if d.year() == today.year() => d.format("%a %-d %b").to_string(),
        _ => d.format("%-d %b %Y").to_string(),
    }
}

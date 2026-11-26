//! Home Assistant alerts. The helper keeps a websocket open to Home Assistant and turns two
//! things into reminder pop-ups:
//!
//! - a 3D printer (Moonraker integration) finishing, being cancelled or failing, with a photo
//!   from its camera. A print that ended while Sticky wasn't running (up to 12 hours ago) still
//!   pops up when it connects.
//! - any Home Assistant notification whose ID starts with `sticky` (made by an automation, for
//!   example one with a webhook trigger). A Markdown picture in it, `![](url)`, comes along.
//!   It is cleared in Home Assistant once Sticky has it.
//! - a doorbell being pressed (a Reolink's `binary_sensor.*_visitor` turning on): a ding-dong
//!   and a pop-up that shows the doorbell's camera live for a few minutes.
//!
//! The connection state goes to `.sticky\ha-status.json` for the settings page.

use crate::net::{self, Url, Ws};
use crate::settings::Settings;
use crate::store::{self, Item, Kind, Priority};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DONE_STATES: [&str; 3] = ["complete", "cancelled", "error"];
const BUSY_STATES: [&str; 2] = ["printing", "paused"];

// ---------------------------------------------------------------- status file

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Status {
    /// "off", "connecting", "connected" or "error"
    pub state: String,
    pub detail: String,
    pub at: Option<DateTime<Local>>,
    /// Printer being watched, e.g. "U1"
    pub printer: String,
    pub camera: String,
    /// Epoch seconds of the last print end we popped up (so it isn't repeated).
    pub last_print_end: f64,
    /// Doorbell being watched, e.g. "Driveway", and its camera
    pub doorbell: String,
    pub doorbell_camera: String,
}

pub fn status_path() -> std::path::PathBuf {
    store::data_dir().join("ha-status.json")
}
pub fn load_status() -> Status {
    std::fs::read(status_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
fn save_status(s: &Status) {
    if let Ok(j) = serde_json::to_vec_pretty(s) {
        let p = status_path();
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, j).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}
fn set_state(state: &str, detail: &str) {
    let mut s = load_status();
    if s.state != state || s.detail != detail {
        s.state = state.into();
        s.detail = detail.into();
        s.at = Some(Local::now());
        save_status(&s);
    }
}

// ---------------------------------------------------------------- settings helpers

pub struct Conn {
    pub url: Url,
    pub token: String,
}

impl Conn {
    pub fn from_settings(s: &Settings) -> Option<Conn> {
        let url = Url::parse(&s.ha_url)?;
        let token = crate::secret::open(&s.ha_token)?;
        Some(Conn { url, token })
    }
}

/// What the settings page offers to pick from.
#[derive(Default, Clone, Debug)]
pub struct Found {
    /// (print state sensor, printer name)
    pub printers: Vec<(String, String)>,
    /// (camera entity, friendly name)
    pub cameras: Vec<(String, String)>,
    /// (visitor sensor, doorbell name)
    pub doorbells: Vec<(String, String)>,
    pub location: String,
}

pub fn discover(c: &Conn) -> Result<Found, String> {
    let body = net::get(&c.url, "/api/states", Some(&c.token))?;
    let states: Vec<Value> = serde_json::from_slice(&body).map_err(|_| "that address didn't answer like Home Assistant".to_string())?;
    let mut f = Found::default();
    for s in &states {
        let id = s["entity_id"].as_str().unwrap_or_default();
        let name = s["attributes"]["friendly_name"].as_str().unwrap_or(id).to_string();
        if id.starts_with("sensor.") && id.ends_with("_current_print_state") {
            f.printers.push((id.to_string(), printer_name(&name)));
        } else if id.starts_with("camera.") {
            f.cameras.push((id.to_string(), name));
        } else if id.starts_with("binary_sensor.") && id.ends_with("_visitor") {
            f.doorbells.push((id.to_string(), doorbell_name(&name)));
        } else if id == "zone.home" {
            f.location = name;
        }
    }
    f.printers.sort();
    f.cameras.sort();
    f.doorbells.sort();
    Ok(f)
}

fn printer_name(friendly: &str) -> String {
    let n = friendly.trim();
    let lower = n.to_lowercase();
    let n = if lower.ends_with("current print state") { n[..n.len() - "current print state".len()].trim() } else { n };
    if n.is_empty() { "3D".into() } else { n.to_string() }
}

fn doorbell_name(friendly: &str) -> String {
    let n = friendly.trim();
    let n = if n.to_lowercase().ends_with(" visitor") { n[..n.len() - " visitor".len()].trim() } else { n };
    if n.is_empty() { "Front".into() } else { n.to_string() }
}

/// The doorbell's camera: the setting, else its own (a Reolink's sub stream "fluent" is quick,
/// then the main "clear" stream, then any of its cameras).
fn pick_doorbell_camera(setting: &str, sensor: &str, cameras: &[String]) -> Option<String> {
    if setting == "none" {
        return None;
    }
    if !setting.is_empty() {
        return Some(setting.to_string());
    }
    let base = sensor.trim_start_matches("binary_sensor.").trim_end_matches("_visitor");
    for suffix in ["fluent", "clear"] {
        let c = format!("camera.{base}_{suffix}");
        if cameras.contains(&c) {
            return Some(c);
        }
    }
    let prefix = format!("camera.{base}_");
    let mut own: Vec<&String> = cameras.iter().filter(|c| c.starts_with(&prefix)).collect();
    own.sort();
    own.first().map(|c| c.to_string())
}

/// The camera to photograph a printer with: the setting, else one of the printer's own
/// cameras (not the slicer thumbnail).
fn pick_camera(setting: &str, state_sensor: &str, cameras: &[String]) -> Option<String> {
    if setting == "none" {
        return None;
    }
    if !setting.is_empty() {
        return Some(setting.to_string());
    }
    let prefix = format!("camera.{}", state_sensor.trim_start_matches("sensor.").trim_end_matches("_current_print_state"));
    let mut own: Vec<&String> = cameras.iter().filter(|c| c.starts_with(&prefix) && !c.contains("thumbnail")).collect();
    own.sort();
    own.first().map(|c| c.to_string())
}

// ---------------------------------------------------------------- alerts

/// Add an item that pops up now. Returns its id.
pub fn add_alert(text: &str, detail: &str, source: &str, picture: Option<String>) -> u64 {
    add_alert_with(text, detail, source, picture, "", "")
}

/// `add_alert` with its own pop-up sound and a camera to show live.
pub fn add_alert_with(text: &str, detail: &str, source: &str, picture: Option<String>, sound: &str, live: &str) -> u64 {
    let s = Settings::load();
    let now = Local::now();
    let category = if s.smart_category && s.categories.iter().any(|c| c == "Today") { "Today".to_string() } else { s.default_category.clone() };
    let item = Item {
        id: 0,
        text: text.trim().to_string(),
        kind: Kind::Task,
        done: false,
        created: now,
        done_at: None,
        due: Some(now),
        all_day: false,
        remind: true,
        fired: false,
        snoozed_until: None,
        repeat: None,
        priority: Priority::None,
        category,
        tags: vec![],
        pinned: false,
        order: 0,
        steps: vec![],
        notes: String::new(),
        detail: detail.trim().to_string(),
        source: source.to_string(),
        picture,
        sound: sound.to_string(),
        live: live.to_string(),
    };
    let id = store::update(|d| d.add(item));
    store::log(&format!("alert from {}", source.to_lowercase()), id, text);
    id
}

fn tidy_file_name(f: &str) -> String {
    let base = f.rsplit(['/', '\\']).next().unwrap_or(f);
    let base = base.strip_suffix(".gcode").or_else(|| base.strip_suffix(".3mf")).unwrap_or(base);
    base.replace('_', " ").trim().to_string()
}

fn duration_text(value: f64, unit: &str) -> String {
    let secs = match unit {
        "h" => value * 3600.0,
        "min" => value * 60.0,
        "ms" => value / 1000.0,
        _ => value,
    } as i64;
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 { format!("{h}h {m}m") } else { format!("{m}m") }
}

/// Pull `![alt](url)` out of Markdown text: (text without it, url).
pub fn take_picture(md: &str) -> (String, Option<String>) {
    if let Some(i) = md.find("![")
        && let Some(j) = md[i..].find("](").map(|j| i + j + 2)
        && let Some(k) = md[j..].find(')').map(|k| j + k)
    {
        let url = md[j..k].trim().to_string();
        let rest = format!("{}{}", &md[..i], &md[k + 1..]);
        return (rest.trim().to_string(), (!url.is_empty()).then_some(url));
    }
    (md.trim().to_string(), None)
}

// ---------------------------------------------------------------- the connection

pub fn start() {
    std::thread::spawn(|| {
        let mut wait = 5u64;
        loop {
            let s = Settings::load();
            let key = settings_key(&s);
            let wanted = s.ha_print_alerts || s.ha_notifications || s.ha_doorbell;
            let conn = if wanted { Conn::from_settings(&s) } else { None };
            let Some(conn) = conn else {
                set_state("off", if !wanted { "Alerts are turned off" } else if s.ha_url.trim().is_empty() { "Not set up" } else { "No token saved" });
                wait_for_change(&key, Duration::from_secs(3600));
                continue;
            };
            set_state("connecting", &conn.url.origin());
            match session(&conn, &s, &key) {
                Ok(()) => wait = 5, // settings changed: reconnect straight away
                Err(e) => {
                    set_state("error", &e);
                    store::log("home assistant", 0, &e);
                    wait_for_change(&key, Duration::from_secs(wait));
                    wait = (wait * 2).min(300);
                }
            }
        }
    });
}

fn settings_key(s: &Settings) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        s.ha_url, s.ha_token, s.ha_print_alerts, s.ha_printer, s.ha_camera, s.ha_notifications, s.ha_doorbell, s.ha_doorbell_sensor, s.ha_doorbell_camera
    )
}

/// Sleep up to `d`, returning early when the Home Assistant settings change.
fn wait_for_change(key: &str, d: Duration) {
    let end = Instant::now() + d;
    let mut m = store::mtime(&store::settings_path());
    while Instant::now() < end {
        std::thread::sleep(Duration::from_secs(1));
        let now = store::mtime(&store::settings_path());
        if now != m {
            m = now;
            if settings_key(&Settings::load()) != key {
                return;
            }
        }
    }
}

struct Printer {
    state_id: String,
    name: String,
    camera: Option<String>,
    file_id: String,
    duration_id: String,
}

struct Doorbell {
    sensor: String,
    name: String,
    camera: Option<String>,
}

struct Session<'a> {
    conn: &'a Conn,
    ws: Arc<Ws>,
    next: u64,
    printer: Option<Printer>,
    doorbell: Option<Doorbell>,
    last_ring: Option<Instant>,
    states: HashMap<String, Value>,
    handled: HashSet<String>,
    ids: Ids,
}
#[derive(Default)]
struct Ids {
    get_states: u64,
    entities: u64,
    notes: u64,
}

fn session(conn: &Conn, s: &Settings, key: &str) -> Result<(), String> {
    let ws = Arc::new(Ws::connect(&conn.url, "/api/websocket")?);
    let hello: Value = serde_json::from_str(&ws.recv()?).map_err(|_| "not a Home Assistant websocket".to_string())?;
    if hello["type"] != "auth_required" {
        return Err("not a Home Assistant websocket".into());
    }
    ws.send(&json!({"type": "auth", "access_token": conn.token}).to_string())?;
    let auth: Value = serde_json::from_str(&ws.recv()?).unwrap_or_default();
    if auth["type"] != "auth_ok" {
        return Err("Home Assistant refused the token".into());
    }
    let version = auth["ha_version"].as_str().unwrap_or("").to_string();

    let (tx, rx) = mpsc::channel::<Result<String, String>>();
    let reader = ws.clone();
    std::thread::spawn(move || {
        loop {
            let m = reader.recv();
            let stop = m.is_err();
            if tx.send(m).is_err() || stop {
                break;
            }
        }
    });

    let mut se = Session { conn, ws: ws.clone(), next: 1, printer: None, doorbell: None, last_ring: None, states: HashMap::new(), handled: HashSet::new(), ids: Ids::default() };
    if s.ha_print_alerts || s.ha_doorbell {
        se.ids.get_states = se.send(json!({"type": "get_states"}))?;
    }
    if s.ha_notifications {
        se.ids.notes = se.send(json!({"type": "persistent_notification/subscribe"}))?;
    }
    set_state("connected", &format!("{} (Home Assistant {version})", conn.url.origin()));

    let mut last_heard = Instant::now();
    let mut last_ping = Instant::now();
    let mut last_check = Instant::now();
    let mut m = store::mtime(&store::settings_path());
    let result = loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(text)) => {
                last_heard = Instant::now();
                if let Ok(v) = serde_json::from_str::<Value>(&text)
                    && let Err(e) = se.handle(v, s)
                {
                    break Err(e);
                }
            }
            Ok(Err(e)) => break Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break Err("connection lost".into()),
        }
        if last_ping.elapsed() > Duration::from_secs(30) {
            last_ping = Instant::now();
            se.send(json!({"type": "ping"}))?;
        }
        if last_heard.elapsed() > Duration::from_secs(95) {
            break Err("Home Assistant stopped answering".into());
        }
        if last_check.elapsed() > Duration::from_secs(2) {
            last_check = Instant::now();
            let now = store::mtime(&store::settings_path());
            if now != m {
                m = now;
                if settings_key(&Settings::load()) != key {
                    break Ok(());
                }
            }
        }
    };
    ws.close();
    result
}

impl Session<'_> {
    fn send(&mut self, mut v: Value) -> Result<u64, String> {
        let id = self.next;
        self.next += 1;
        v["id"] = json!(id);
        self.ws.send(&v.to_string())?;
        Ok(id)
    }

    fn handle(&mut self, v: Value, s: &Settings) -> Result<(), String> {
        let id = v["id"].as_u64().unwrap_or(0);
        match v["type"].as_str().unwrap_or_default() {
            "result" if id == self.ids.get_states && id != 0 => {
                if v["success"] == false {
                    return Err(format!("Home Assistant: {}", v["error"]["message"].as_str().unwrap_or("request failed")));
                }
                self.found_states(v["result"].as_array().cloned().unwrap_or_default(), s)?;
            }
            "result" if v["success"] == false && id == self.ids.notes => {
                // older Home Assistant or not allowed: keep the printer part going
                store::log("home assistant", 0, "notifications aren't available to this token");
            }
            "event" if id == self.ids.entities && id != 0 => self.entity_event(&v["event"]),
            "event" if id == self.ids.notes && id != 0 => self.notification_event(&v["event"]),
            _ => {}
        }
        Ok(())
    }

    /// The first full list of states: pick the printer and the doorbell, catch up on a missed
    /// print finish, then follow their sensors.
    fn found_states(&mut self, list: Vec<Value>, s: &Settings) -> Result<(), String> {
        let cameras: Vec<String> = list.iter().filter_map(|x| x["entity_id"].as_str()).filter(|e| e.starts_with("camera.")).map(String::from).collect();
        let mut ids = vec![];
        if s.ha_print_alerts {
            ids.extend(self.setup_printer(&list, &cameras, s));
        }
        let door = if s.ha_doorbell { self.setup_doorbell(&list, &cameras, s) } else { vec![] };
        if !s.ha_doorbell {
            let mut st = load_status();
            st.doorbell.clear();
            st.doorbell_camera.clear();
            save_status(&st);
        }
        ids.extend(door);
        if !ids.is_empty() {
            self.ids.entities = self.send(json!({"type": "subscribe_entities", "entity_ids": ids}))?;
        }
        Ok(())
    }

    fn setup_printer(&mut self, list: &[Value], cameras: &[String], s: &Settings) -> Vec<String> {
        let state_id = if s.ha_printer.is_empty() {
            let mut p: Vec<&str> = list.iter().filter_map(|x| x["entity_id"].as_str()).filter(|e| e.starts_with("sensor.") && e.ends_with("_current_print_state")).collect();
            p.sort();
            p.first().map(|x| x.to_string())
        } else {
            Some(s.ha_printer.clone())
        };
        let Some(state_id) = state_id else {
            let mut st = load_status();
            st.printer = String::new();
            st.detail = format!("{} · no 3D printer found", st.detail);
            save_status(&st);
            return vec![];
        };
        let entry = list.iter().find(|x| x["entity_id"] == state_id.as_str()).cloned().unwrap_or_default();
        let name = printer_name(entry["attributes"]["friendly_name"].as_str().unwrap_or(""));
        let base = state_id.trim_start_matches("sensor.").trim_end_matches("_current_print_state").to_string();
        let camera = pick_camera(&s.ha_camera, &state_id, cameras);
        let p = Printer { state_id: state_id.clone(), name: name.clone(), camera: camera.clone(), file_id: format!("sensor.{base}_filename"), duration_id: format!("sensor.{base}_print_duration") };
        for x in list {
            if let Some(e) = x["entity_id"].as_str()
                && (e == p.state_id || e == p.file_id || e == p.duration_id)
            {
                self.states.insert(e.to_string(), json!({"s": x["state"], "a": x["attributes"]}));
            }
        }
        let mut st = load_status();
        st.printer = name;
        st.camera = camera.unwrap_or_default();
        // a print that ended while Sticky was away
        let state = entry["state"].as_str().unwrap_or_default().to_string();
        let ended = entry["last_changed"].as_str().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp() as f64).unwrap_or(0.0);
        let first_run = st.last_print_end == 0.0;
        if first_run {
            st.last_print_end = Local::now().timestamp() as f64;
        }
        save_status(&st);
        let ids = vec![p.state_id.clone(), p.file_id.clone(), p.duration_id.clone()];
        self.printer = Some(p);
        if !first_run && DONE_STATES.contains(&state.as_str()) && ended > st.last_print_end + 1.0 && Local::now().timestamp() as f64 - ended < 12.0 * 3600.0 {
            self.print_ended(&state, ended, true);
        }
        if std::env::var_os("STICKY_FAKE_PRINT_END").is_some() {
            // try the alert without waiting for a real print to end
            self.print_ended("complete", Local::now().timestamp() as f64, false);
        }
        ids
    }

    fn setup_doorbell(&mut self, list: &[Value], cameras: &[String], s: &Settings) -> Vec<String> {
        let sensor = if s.ha_doorbell_sensor.is_empty() {
            let mut d: Vec<&str> = list.iter().filter_map(|x| x["entity_id"].as_str()).filter(|e| e.starts_with("binary_sensor.") && e.ends_with("_visitor")).collect();
            d.sort();
            d.first().map(|x| x.to_string())
        } else {
            Some(s.ha_doorbell_sensor.clone())
        };
        let mut st = load_status();
        let Some(sensor) = sensor else {
            st.doorbell.clear();
            st.doorbell_camera.clear();
            save_status(&st);
            return vec![];
        };
        let entry = list.iter().find(|x| x["entity_id"] == sensor.as_str()).cloned().unwrap_or_default();
        let name = doorbell_name(entry["attributes"]["friendly_name"].as_str().unwrap_or(""));
        let camera = pick_doorbell_camera(&s.ha_doorbell_camera, &sensor, cameras);
        self.states.insert(sensor.clone(), json!({"s": entry["state"], "a": entry["attributes"]}));
        st.doorbell = name.clone();
        st.doorbell_camera = camera.clone().unwrap_or_default();
        save_status(&st);
        self.doorbell = Some(Doorbell { sensor: sensor.clone(), name, camera });
        if std::env::var_os("STICKY_FAKE_DOORBELL").is_some() {
            // try the doorbell without anyone pressing it
            self.doorbell_pressed();
        }
        vec![sensor]
    }

    /// `subscribe_entities` sends {"a": {id: {s, a}}} first, then {"c": {id: {"+": {s, a}}}}.
    fn entity_event(&mut self, ev: &Value) {
        let state_of = |m: &HashMap<String, Value>, id: &str| m.get(id).and_then(|x| x["s"].as_str()).unwrap_or("").to_string();
        let print_id = self.printer.as_ref().map(|p| p.state_id.clone());
        let door_id = self.doorbell.as_ref().map(|d| d.sensor.clone());
        let print_before = print_id.as_deref().map(|i| state_of(&self.states, i));
        let door_before = door_id.as_deref().map(|i| state_of(&self.states, i));
        if let Some(a) = ev["a"].as_object() {
            for (k, v) in a {
                self.states.insert(k.clone(), json!({"s": v["s"], "a": v["a"]}));
            }
        }
        if let Some(c) = ev["c"].as_object() {
            for (k, v) in c {
                let plus = &v["+"];
                let e = self.states.entry(k.clone()).or_insert_with(|| json!({"s": "", "a": {}}));
                if !plus["s"].is_null() {
                    e["s"] = plus["s"].clone();
                }
                if let (Some(add), Some(cur)) = (plus["a"].as_object(), e["a"].as_object_mut()) {
                    for (ak, av) in add {
                        cur.insert(ak.clone(), av.clone());
                    }
                }
            }
        }
        if let (Some(id), Some(before)) = (&print_id, print_before) {
            let after = state_of(&self.states, id);
            if before != after && BUSY_STATES.contains(&before.as_str()) && DONE_STATES.contains(&after.as_str()) {
                self.print_ended(&after, Local::now().timestamp() as f64, false);
            }
        }
        if let (Some(id), Some(before)) = (&door_id, door_before) {
            let after = state_of(&self.states, id);
            if before == "off" && after == "on" {
                self.doorbell_pressed();
            }
        }
    }

    /// Someone pressed the doorbell: a ding-dong pop-up with the camera's picture, shown live.
    fn doorbell_pressed(&mut self) {
        if self.last_ring.is_some_and(|t| t.elapsed() < Duration::from_secs(15)) {
            return; // pressed again straight away: the pop-up is already up
        }
        self.last_ring = Some(Instant::now());
        let Some(d) = &self.doorbell else { return };
        let picture = d.camera.as_ref().and_then(|c| match net::get(&self.conn.url, &format!("/api/camera_proxy/{c}"), Some(&self.conn.token)) {
            Ok(b) => crate::picture::save(&b),
            Err(e) => {
                store::log("home assistant", 0, &format!("doorbell picture from {c}: {e}"));
                None
            }
        });
        let s = Settings::load();
        let at = Local::now().format(if s.use_24h { "%H:%M" } else { "%-I:%M %P" });
        let detail = format!("{} doorbell · {at}", d.name);
        add_alert_with("Someone's at the door", &detail, "Doorbell", picture, "doorbell", d.camera.as_deref().unwrap_or(""));
    }

    fn print_ended(&mut self, state: &str, at: f64, late: bool) {
        let Some(p) = &self.printer else { return };
        let what = match state {
            "complete" => "finished",
            "cancelled" => "cancelled",
            _ => "failed",
        };
        let file = self.states.get(&p.file_id).and_then(|x| x["s"].as_str()).map(tidy_file_name).filter(|f| !f.is_empty() && f != "unknown" && f != "unavailable");
        let text = match &file {
            Some(f) => format!("{} print {what}: {f}", p.name),
            None => format!("{} print {what}", p.name),
        };
        let mut detail = vec![];
        if let Some(d) = self.states.get(&p.duration_id)
            && let Some(v) = d["s"].as_str().and_then(|x| x.parse::<f64>().ok())
            && v > 0.0
        {
            detail.push(format!("Took {}", duration_text(v, d["a"]["unit_of_measurement"].as_str().unwrap_or("s"))));
        }
        if late {
            let t = DateTime::from_timestamp(at as i64, 0).map(|t| t.with_timezone(&Local)).unwrap_or_else(Local::now);
            detail.push(format!("ended {}", store::describe_due(t, false, false)));
        }
        let picture = p.camera.as_ref().and_then(|c| match net::get(&self.conn.url, &format!("/api/camera_proxy/{c}"), Some(&self.conn.token)) {
            Ok(b) => crate::picture::save(&b),
            Err(e) => {
                store::log("home assistant", 0, &format!("photo from {c}: {e}"));
                None
            }
        });
        add_alert(&text, &detail.join(" · "), "3D printer", picture);
        let mut st = load_status();
        st.last_print_end = st.last_print_end.max(at);
        save_status(&st);
    }

    /// persistent_notification/subscribe: {"type": "current"|"added"|"updated"|"removed", "notifications": {id: {...}}}
    fn notification_event(&mut self, ev: &Value) {
        if !matches!(ev["type"].as_str(), Some("current" | "added" | "updated")) {
            return;
        }
        let Some(all) = ev["notifications"].as_object() else { return };
        for (nid, n) in all {
            if !nid.to_lowercase().starts_with("sticky") || !self.handled.insert(nid.clone()) {
                continue;
            }
            let title = n["title"].as_str().unwrap_or("").trim().to_string();
            let (message, url) = take_picture(n["message"].as_str().unwrap_or(""));
            let (text, detail) = if title.is_empty() {
                let mut lines = message.lines();
                (lines.next().unwrap_or("Alert").to_string(), lines.collect::<Vec<_>>().join("\n"))
            } else {
                (title, message)
            };
            let picture = url.and_then(|u| match net::get_any(&u, &self.conn.url, &self.conn.token) {
                Ok(b) => crate::picture::save(&b),
                Err(e) => {
                    store::log("home assistant", 0, &format!("picture {u}: {e}"));
                    None
                }
            });
            add_alert(&text, &detail, "Home Assistant", picture);
            let _ = self.send(json!({"type": "call_service", "domain": "persistent_notification", "service": "dismiss", "service_data": {"notification_id": nid}}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doorbell_bits() {
        assert_eq!(doorbell_name("Driveway Visitor"), "Driveway");
        assert_eq!(doorbell_name(""), "Front");
        let cams: Vec<String> = ["camera.driveway_high_resolution_channel", "camera.driveway_fluent", "camera.driveway_clear"].iter().map(|x| x.to_string()).collect();
        assert_eq!(pick_doorbell_camera("", "binary_sensor.driveway_visitor", &cams).as_deref(), Some("camera.driveway_fluent"));
        assert_eq!(pick_doorbell_camera("none", "binary_sensor.driveway_visitor", &cams), None);
        assert_eq!(pick_doorbell_camera("camera.x", "binary_sensor.driveway_visitor", &cams).as_deref(), Some("camera.x"));
        let wav = crate::sound::doorbell_wav();
        assert_eq!(&wav[0..4], b"RIFF");
        assert!(crate::sound::louder(&wav, 2.0).is_some());
    }

    #[test]
    fn pictures_and_names() {
        let (t, u) = take_picture("Done!\n![photo](/api/camera_proxy/camera.x) after");
        assert_eq!(u.as_deref(), Some("/api/camera_proxy/camera.x"));
        assert_eq!(t, "Done!\n after");
        assert_eq!(take_picture("plain").1, None);
        assert_eq!(printer_name("U1 Current Print State"), "U1");
        assert_eq!(tidy_file_name("705S2_stand_adaptor_v2d_nuts_PLA_15h1m.gcode"), "705S2 stand adaptor v2d nuts PLA 15h1m");
        assert_eq!(duration_text(716.73, "min"), "11h 56m");
        assert_eq!(duration_text(3000.0, "s"), "50m");
        let cams = vec!["camera.laundry_u1_thumbnail".to_string(), "camera.laundry_u1_main".into(), "camera.laundry_u1_case".into(), "camera.door".into()];
        assert_eq!(pick_camera("", "sensor.laundry_u1_current_print_state", &cams).as_deref(), Some("camera.laundry_u1_case"));
        assert_eq!(pick_camera("none", "sensor.laundry_u1_current_print_state", &cams), None);
    }
}

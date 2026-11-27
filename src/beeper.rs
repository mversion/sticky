//! Chat alerts through Beeper Desktop. Beeper bridges Slack, WhatsApp, Telegram and the rest
//! into one app on this PC, and it serves a local API on 127.0.0.1:23373. The helper asks it
//! every few seconds for the newest messages in the chats picked in Settings > Chat alerts,
//! and each batch of new messages from other people pops up as an alert with "Open in Beeper".
//!
//! Sticky connects with Beeper's own OAuth sign-in (PKCE, read-only): Settings > Chat alerts >
//! Connect to Beeper opens Beeper's approval page in the browser and catches the answer on a
//! one-off local port. The token is kept DPAPI-encrypted in settings.json.
//!
//! The newest message number seen in each chat goes to `.sticky\beeper-status.json`, so a chat
//! added later doesn't replay its history and messages that came while the PC was off still pop up.

use crate::net::{self, Url};
use crate::settings::{Settings, WatchedChat};
use crate::store;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const API: &str = "http://127.0.0.1:23373";
const POLL: Duration = Duration::from_secs(5);
/// How many messages one alert lists before "…and n more"
const SHOWN: usize = 6;

fn api() -> Url {
    Url::parse(API).expect("fixed address")
}

// ---------------------------------------------------------------- status file

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Status {
    /// "off", "connected" or "error"
    pub state: String,
    pub detail: String,
    pub at: Option<DateTime<Local>>,
    /// The newest message number (Beeper's sortKey) seen in each chat
    pub seen: HashMap<String, u64>,
}

pub fn status_path() -> std::path::PathBuf {
    store::data_dir().join("beeper-status.json")
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
fn set_state(st: &mut Status, state: &str, detail: &str) {
    if st.state != state || st.detail != detail {
        if state == "error" {
            store::log("beeper", 0, detail);
        }
        st.state = state.into();
        st.detail = detail.into();
        st.at = Some(Local::now());
    }
}

// ---------------------------------------------------------------- the watcher

pub fn start() {
    std::thread::spawn(|| {
        loop {
            let s = Settings::load();
            let before = load_status();
            let mut st = before.clone();
            let wait = check(&s, &mut st);
            if st != before {
                save_status(&st);
            }
            wait_for_change(&key(&s), wait);
        }
    });
}

fn key(s: &Settings) -> String {
    format!("{}|{}|{:?}", s.beeper_token, s.beeper_alerts, s.beeper_chats)
}

/// Sleep up to `d`, returning early when the chat alert settings change.
fn wait_for_change(key: &str, d: Duration) {
    let end = Instant::now() + d;
    let mut m = store::mtime(&store::settings_path());
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(500));
        let now = store::mtime(&store::settings_path());
        if now != m {
            m = now;
            if self::key(&Settings::load()) != key {
                return;
            }
        }
    }
}

/// One round: every watched chat once. Returns how long to wait before the next.
fn check(s: &Settings, st: &mut Status) -> Duration {
    if s.beeper_chats.is_empty() || !s.beeper_alerts {
        set_state(st, "off", if !s.beeper_alerts { "Alerts are turned off" } else { "No chats picked" });
        return Duration::from_secs(3600);
    }
    let Some(token) = crate::secret::open(&s.beeper_token) else {
        set_state(st, "off", "Not connected");
        return Duration::from_secs(3600);
    };
    for c in &s.beeper_chats {
        let items = match messages(&token, &c.id) {
            Ok(v) => v,
            Err(e) => {
                set_state(st, "error", &e);
                return Duration::from_secs(30);
            }
        };
        let newest = items.iter().filter_map(sort_key).max();
        if let Some(&last) = st.seen.get(&c.id) {
            let mut new: Vec<&Value> = items.iter().filter(|m| sort_key(m).is_some_and(|k| k > last) && worth_telling(m)).collect();
            new.sort_by_key(|m| sort_key(m));
            if !new.is_empty() {
                alert(c, &new);
            }
        }
        // a chat seen for the first time starts from now: its history doesn't pop up
        if let Some(n) = newest {
            let e = st.seen.entry(c.id.clone()).or_insert(n);
            *e = (*e).max(n);
        }
    }
    let names: Vec<&str> = s.beeper_chats.iter().map(|c| c.name.as_str()).collect();
    set_state(st, "connected", &format!("watching {}", names.join(", ")));
    POLL
}

/// The newest page of a chat's messages (about 20).
fn messages(token: &str, chat: &str) -> Result<Vec<Value>, String> {
    let path = format!("/v1/chats/{}/messages", enc(chat));
    let body = net::get(&api(), &path, Some(token)).map_err(|e| why(&e))?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(v["items"].as_array().cloned().unwrap_or_default())
}

/// Plain words for what went wrong.
fn why(e: &str) -> String {
    if e.contains("refused") {
        "Beeper refused the token. Connect again.".into()
    } else if e.contains("couldn't connect") {
        "Beeper Desktop isn't running".into()
    } else {
        format!("Beeper: {e}")
    }
}

fn sort_key(m: &Value) -> Option<u64> {
    m["sortKey"].as_str().and_then(|k| k.parse().ok()).or_else(|| m["sortKey"].as_u64())
}

/// Someone else's message with something in it (not a reaction, an edit or your own reply).
fn worth_telling(m: &Value) -> bool {
    let flag = |k: &str| m[k].as_bool().unwrap_or(false);
    !flag("isSender") && !flag("isHidden") && !flag("isDeleted") && m["type"].as_str() != Some("REACTION") && !body(m, 10).is_empty()
}

fn sender(m: &Value) -> String {
    m["senderName"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or("Someone").trim().to_string()
}

/// The message as plain text, cut to `max` characters. Quoted replies are left out.
fn body(m: &Value, max: usize) -> String {
    let mut t = plain_text(m["text"].as_str().unwrap_or(""));
    if t.is_empty() {
        let atts = m["attachments"].as_array().map(|a| a.len()).unwrap_or(0);
        t = match (m["type"].as_str().unwrap_or(""), atts) {
            ("IMAGE", _) => "sent a picture".into(),
            ("VIDEO", _) => "sent a video".into(),
            ("VOICE" | "AUDIO", _) => "sent a voice message".into(),
            (_, n) if n > 0 => "sent a file".into(),
            _ => String::new(),
        };
    }
    if t.chars().count() > max {
        t = t.chars().take(max).collect::<String>().trim_end().to_string() + "…";
    }
    t
}

/// Beeper's message HTML as text: line breaks kept, tags and quoted replies dropped.
pub fn plain_text(html: &str) -> String {
    let mut s = html.to_string();
    // quoted replies repeat an earlier message
    while let Some(i) = s.find("<blockquote") {
        let end = s[i..].find("</blockquote>").map(|j| i + j + "</blockquote>".len()).unwrap_or(s.len());
        s.replace_range(i..end, "");
    }
    for br in ["<br>", "<br/>", "<br />", "</p>", "</li>", "</div>"] {
        s = s.replace(br, "\n");
    }
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&");
    // at most one blank line in a row
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut tidy: Vec<&str> = vec![];
    for l in lines {
        if l.trim().is_empty() && tidy.last().is_none_or(|p| p.trim().is_empty()) {
            continue;
        }
        tidy.push(l);
    }
    tidy.join("\n").trim().to_string()
}

fn alert(c: &WatchedChat, new: &[&Value]) {
    let (text, detail) = alert_text(&c.name, new);
    let source = if c.network.is_empty() { "Beeper" } else { c.network.as_str() };
    let mut item = crate::ha::new_alert(&text, &detail, source);
    item.open = format!("beeper:{}", c.id);
    let id = store::update(|d| d.add(item));
    store::log(&format!("alert from {}", source.to_lowercase()), id, &text);
}

/// (title, detail) for a batch of new messages, oldest first.
fn alert_text(chat: &str, new: &[&Value]) -> (String, String) {
    if let [m] = new {
        return (format!("{} in {chat}", sender(m)), body(m, 600));
    }
    let skip = new.len().saturating_sub(SHOWN);
    let mut lines: Vec<String> = new[skip..].iter().map(|m| format!("{}: {}", sender(m), body(m, 160).replace('\n', " "))).collect();
    if skip > 0 {
        lines.insert(0, format!("…{skip} earlier"));
    }
    (format!("{} new messages in {chat}", new.len()), lines.join("\n"))
}

/// Bring Beeper up on a chat ("beeper:<chat id>" from an alert). Runs in the background.
pub fn open(link: &str) {
    let Some(chat) = link.strip_prefix("beeper:").map(str::to_string) else { return };
    std::thread::spawn(move || {
        let s = Settings::load();
        let opened = crate::secret::open(&s.beeper_token).is_some_and(|t| net::post(&api(), "/v1/focus", Some(&t), "application/json", json!({ "chatID": chat }).to_string().as_bytes()).is_ok());
        if !opened {
            store::log("beeper", 0, "couldn't open the chat in Beeper");
        }
    });
}

// ---------------------------------------------------------------- settings page helpers

/// A chat that can be watched: (chat, last activity), most recent first.
pub fn chats(token: &str) -> Result<Vec<WatchedChat>, String> {
    let body = net::get(&api(), "/v1/chats?limit=200", Some(token)).map_err(|e| why(&e))?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(v["items"]
        .as_array()
        .map(|a| a.iter().filter(|c| c["mergedIntoChatID"].is_null()).filter_map(watched_chat).collect())
        .unwrap_or_default())
}

fn watched_chat(c: &Value) -> Option<WatchedChat> {
    let id = c["id"].as_str()?.to_string();
    let network = c["network"].as_str().unwrap_or("").to_string();
    let title = c["title"].as_str().unwrap_or("").trim();
    let title = if title.is_empty() { "Untitled chat" } else { title };
    // Slack channels read as #channel
    let name = if network == "Slack" && c["type"].as_str() == Some("group") && !title.starts_with('#') { format!("#{title}") } else { title.to_string() };
    Some(WatchedChat { id, name, network })
}

/// Connect to Beeper: registers Sticky as a read-only client, opens Beeper's approval page and
/// waits (up to 5 minutes) for the answer on a local port. Returns the access token.
pub fn connect() -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let reg = json!({
        "client_name": "Sticky",
        "client_uri": "https://github.com/mversion/sticky",
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
        "scope": "read",
        "token_endpoint_auth_method": "none",
    });
    let body = net::post(&api(), "/oauth/register", None, "application/json", reg.to_string().as_bytes()).map_err(|e| why(&e))?;
    let client: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let client_id = client["client_id"].as_str().ok_or("Beeper didn't register Sticky")?.to_string();

    let verifier = random_hex(64);
    let challenge = {
        use sha2::{Digest, Sha256};
        b64url(&Sha256::digest(verifier.as_bytes()))
    };
    let state = random_hex(16);
    let url = format!(
        "{API}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope=read&state={state}&code_challenge={challenge}&code_challenge_method=S256",
        enc(&client_id),
        enc(&redirect)
    );
    std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", &url]).spawn().map_err(|e| e.to_string())?;

    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let end = Instant::now() + Duration::from_secs(300);
    let code = loop {
        if Instant::now() > end {
            return Err("No answer from Beeper within 5 minutes".into());
        }
        let mut conn = match listener.accept() {
            Ok((c, _)) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = conn.set_nonblocking(false);
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = vec![0u8; 8192];
        let n = conn.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let target = req.split_whitespace().nth(1).unwrap_or("").to_string();
        let Some(query) = target.strip_prefix("/callback?") else {
            let _ = conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        };
        let q = query_params(query);
        let (ok, page) = match (q.get("error"), q.get("state"), q.get("code")) {
            (Some(e), _, _) => (Err(format!("Beeper said no ({e})")), "Sticky wasn't connected. You can close this tab."),
            (_, Some(s), Some(c)) if *s == state => (Ok(c.clone()), "Sticky is connected to Beeper. You can close this tab."),
            _ => (Err("Beeper's answer didn't match".to_string()), "Sticky wasn't connected. You can close this tab."),
        };
        let html = format!("<!doctype html><meta charset=utf-8><title>Sticky</title><body style=\"font:16px Segoe UI,sans-serif;padding:40px\">{page}</body>");
        let _ = conn.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len()).as_bytes());
        break ok?;
    };

    let form = format!("grant_type=authorization_code&code={}&code_verifier={verifier}&client_id={}&redirect_uri={}", enc(&code), enc(&client_id), enc(&redirect));
    let body = net::post(&api(), "/oauth/token", None, "application/x-www-form-urlencoded", form.as_bytes()).map_err(|e| why(&e))?;
    let tok: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    tok["access_token"].as_str().map(str::to_string).ok_or_else(|| "Beeper sent no token".into())
}

fn query_params(q: &str) -> HashMap<String, String> {
    q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (dec(k), dec(v))).collect()
}

/// Percent-encode everything but the unreserved characters.
fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out += &format!("%{b:02X}");
        }
    }
    out
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn random_hex(len: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    while out.len() < len {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.write_usize(out.len());
        out += &format!("{:016x}", h.finish());
    }
    out.truncate(len);
    out
}

fn b64url(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for ch in data.chunks(3) {
        let n = ch.iter().enumerate().fold(0u32, |a, (i, &b)| a | (b as u32) << (16 - 8 * i));
        let k = ch.len() + 1;
        for i in 0..k {
            out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text() {
        let t = "Hi <a href=\"https://matrix.to/#/@bilgin:beeper.com\">Bilgin Ozkan</a> - just following up &amp; checking 🙂<br><blockquote><b>Teresa</b><br>old message</blockquote>";
        assert_eq!(plain_text(t), "Hi Bilgin Ozkan - just following up & checking 🙂");
        assert_eq!(plain_text("one<br><br><br>two"), "one\n\ntwo");
    }

    #[test]
    fn picks_new_messages() {
        let m = |k: &str, who: &str, me: bool, ty: &str, text: &str| json!({"sortKey": k, "senderName": who, "isSender": me, "type": ty, "text": text});
        let items = [m("10", "Teresa", false, "TEXT", "old"), m("11", "bilgin", true, "TEXT", "mine"), m("12", "Maevelyn", false, "REACTION", ""), m("13", "Petra", false, "TEXT", "Thanks!")];
        let new: Vec<&Value> = items.iter().filter(|x| sort_key(x).is_some_and(|k| k > 10) && worth_telling(x)).collect();
        assert_eq!(new.len(), 1);
        assert_eq!(alert_text("#radio-parts", &new), ("Petra in #radio-parts".to_string(), "Thanks!".to_string()));
        let two = [&items[0], &items[3]];
        assert_eq!(alert_text("#radio-parts", &two).0, "2 new messages in #radio-parts");
        assert_eq!(alert_text("#radio-parts", &two).1, "Teresa: old\nPetra: Thanks!");
    }

    #[test]
    fn pkce_bits() {
        // RFC 7636 appendix B
        use sha2::{Digest, Sha256};
        assert_eq!(b64url(&Sha256::digest(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(enc("!wye:beeper.local"), "%21wye%3Abeeper.local");
        assert_eq!(dec("a%20b+c"), "a b c");
        assert_eq!(random_hex(64).len(), 64);
    }
}

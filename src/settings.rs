//! Settings in `%USERPROFILE%\.sticky\settings.json`. Every running Sticky process picks up
//! changes within a second, so hand edits work too.

use crate::store;
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    // General
    pub hotkey: String,
    pub start_with_windows: bool,
    pub kite_keyword: String,
    pub window_screen: String,
    pub close_capture_after_add: bool,
    pub capture_shows_list: bool,
    pub capture_list_size: usize,
    // Appearance
    pub theme: String,
    pub accent: String,
    pub font_scale: f32,
    pub use_24h: bool,
    pub compact: bool,
    // Tasks
    pub categories: Vec<String>,
    pub default_category: String,
    pub smart_category: bool,
    pub show_done_days: u32,
    pub confirm_delete: bool,
    // Reminders
    pub remind_when_timed: bool,
    pub default_time: String,
    pub snooze_minutes: u32,
    pub sound: bool,
    pub sound_name: String,
    pub sound_repeat: bool,
    pub sound_boost: u32,
    pub sound_duck: bool,
    pub popup_corner: String,
    pub popup_monitor: String,
    // Sticky note window
    pub note_window: bool,
    pub note_on_top: bool,
    pub note_show_done: bool,
    pub note_color: String,
    pub note_pos: Option<[i32; 2]>,
    pub note_size: [f32; 2],
    /// Colour and icon per folder, by folder name (set from the sidebar's right-click menu)
    pub folder_styles: std::collections::BTreeMap<String, FolderStyle>,
    // Main window
    pub main_size: [f32; 2],
    /// Width of the details pane on the right
    pub detail_width: f32,
    // Updates
    pub check_updates: bool,
    pub auto_update: bool,
    // Home Assistant
    pub ha_url: String,
    /// DPAPI-encrypted (see secret.rs)
    pub ha_token: String,
    pub ha_print_alerts: bool,
    /// Print state sensor; empty picks the first printer found
    pub ha_printer: String,
    /// Camera for the photo; empty picks the printer's own, "none" for no photo
    pub ha_camera: String,
    pub ha_notifications: bool,
    /// Doorbell pressed: ding-dong and a pop-up with live footage
    pub ha_doorbell: bool,
    /// The doorbell's visitor sensor; empty picks the first `binary_sensor.*_visitor`
    pub ha_doorbell_sensor: String,
    /// Camera for the footage; empty picks the doorbell's own, "none" for no picture
    pub ha_doorbell_camera: String,
    // Chat alerts (Beeper Desktop)
    /// DPAPI-encrypted Beeper Desktop API token (see secret.rs)
    pub beeper_token: String,
    pub beeper_alerts: bool,
    /// Chats whose new messages pop up
    pub beeper_chats: Vec<WatchedChat>,
}

/// A Beeper chat being watched for new messages.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct WatchedChat {
    /// Beeper's chat ID, e.g. "!wye92hH5xB7AcZtCX8qX:beeper.local"
    pub id: String,
    /// "#radio-parts", "Petra Manos"
    pub name: String,
    /// "Slack", "WhatsApp": the pop-up's heading
    pub network: String,
}

/// A folder's look. Empty strings mean the default (grey folder icon).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct FolderStyle {
    /// "#RRGGBB"
    pub color: String,
    /// A name from `ui::FOLDER_ICONS`
    pub icon: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: "Ctrl+Alt+N".into(),
            start_with_windows: true,
            kite_keyword: "p".into(),
            window_screen: "primary".into(),
            close_capture_after_add: true,
            capture_shows_list: true,
            capture_list_size: 6,
            theme: "midnight".into(),
            accent: "#FFC83D".into(),
            font_scale: 1.0,
            use_24h: false,
            compact: false,
            categories: vec!["Up next".into(), "Today".into(), "Later".into(), "Someday".into()],
            default_category: "Up next".into(),
            smart_category: true,
            show_done_days: 7,
            confirm_delete: false,
            remind_when_timed: true,
            default_time: "09:00".into(),
            snooze_minutes: 10,
            sound: true,
            sound_name: "alarm".into(),
            sound_repeat: false,
            sound_boost: 4,
            sound_duck: true,
            popup_corner: "bottom-right".into(),
            popup_monitor: "primary".into(),
            note_window: false,
            note_on_top: true,
            note_show_done: true,
            note_color: "yellow".into(),
            note_pos: None,
            note_size: [300.0, 420.0],
            folder_styles: Default::default(),
            main_size: [920.0, 640.0],
            detail_width: 380.0,
            check_updates: true,
            auto_update: true,
            ha_url: String::new(),
            ha_token: String::new(),
            ha_print_alerts: true,
            ha_printer: String::new(),
            ha_camera: String::new(),
            ha_notifications: true,
            ha_doorbell: true,
            ha_doorbell_sensor: String::new(),
            ha_doorbell_camera: String::new(),
            beeper_token: String::new(),
            beeper_alerts: true,
            beeper_chats: vec![],
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read(store::settings_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(j) = serde_json::to_vec_pretty(self) {
            let p = store::settings_path();
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, j).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }
    /// Save once on first run so the file exists to hand-edit.
    pub fn ensure_file() {
        if !store::settings_path().exists() {
            Settings::default().save();
        }
    }
    pub fn default_time(&self) -> NaiveTime {
        NaiveTime::parse_from_str(&self.default_time, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
    }
}

/// One row of the settings screen. `keywords` are extra words the search box matches.
pub struct Meta {
    pub key: &'static str,
    pub section: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub keywords: &'static str,
}

pub const SECTIONS: [&str; 9] = ["General", "Appearance", "Tasks", "Reminders", "Home Assistant", "Chat alerts", "Sticky note", "Updates", "Data"];

pub const META: &[Meta] = &[
    Meta { key: "hotkey", section: "General", label: "Quick-add hotkey", help: "Opens the quick-add box from anywhere. Example: Ctrl+Alt+N, Win+Shift+Space.", keywords: "shortcut keyboard key global launch open" },
    Meta { key: "kite_keyword", section: "General", label: "Kite keyword", help: "Type this in Kite, press Tab, type the task and Enter. Press Enter on an empty box to open quick add.", keywords: "launcher launchy runner tab alt space integration" },
    Meta { key: "start_with_windows", section: "General", label: "Start with Windows", help: "Runs the small background helper (tray icon, hotkey and reminder clock) when you sign in.", keywords: "startup boot login autostart run" },
    Meta { key: "window_screen", section: "General", label: "Open Sticky on", help: "The screen quick add, the main window and the \"Added\" flash appear on.", keywords: "monitor display screen launch position where main primary second mouse" },
    Meta { key: "close_capture_after_add", section: "General", label: "Close quick add after adding", help: "Off keeps the box open so you can add several in a row.", keywords: "capture enter multiple" },
    Meta { key: "capture_shows_list", section: "General", label: "Show what's next under quick add", help: "Lists your top open items under the box so you can tick them off.", keywords: "capture list upcoming preview" },
    Meta { key: "theme", section: "Appearance", label: "Theme", help: "Colours for every Sticky window.", keywords: "dark light colour color mode midnight graphite paper sticky system" },
    Meta { key: "accent", section: "Appearance", label: "Accent colour", help: "Highlights, the checkbox tick and today's times.", keywords: "colour color highlight brand" },
    Meta { key: "font_scale", section: "Appearance", label: "Text size", help: "Scales all text.", keywords: "font zoom bigger smaller size" },
    Meta { key: "compact", section: "Appearance", label: "Compact rows", help: "Tighter list rows to fit more on screen.", keywords: "density spacing dense" },
    Meta { key: "use_24h", section: "Appearance", label: "24-hour clock", help: "Show 16:00 instead of 4:00 pm.", keywords: "time format am pm" },
    Meta { key: "categories", section: "Tasks", label: "Folders", help: "Your folders, in order. You can also add one with + next to Folders in the sidebar, and right-click a folder to rename it, move it, delete it or give it a colour and icon. Type #name when adding (#upnext, #later). Prefixes work: #up, #some.", keywords: "folders categories lists groups up next later someday today buckets" },
    Meta { key: "default_category", section: "Tasks", label: "Default folder", help: "Where new items go when you don't say.", keywords: "folder category list group new" },
    Meta { key: "smart_category", section: "Tasks", label: "Due today goes to Today", help: "Items due today land in the Today folder if you have one.", keywords: "auto smart sort" },
    Meta { key: "show_done_days", section: "Tasks", label: "Keep done items visible (days)", help: "Done items older than this are hidden from lists and the dashboard, but stay in Done and the log.", keywords: "completed archive history finished" },
    Meta { key: "confirm_delete", section: "Tasks", label: "Ask before deleting", help: "Confirm before an item is deleted.", keywords: "remove trash safety" },
    Meta { key: "remind_when_timed", section: "Reminders", label: "Pop up whenever a time is given", help: "\"Call Bob at 3pm\" pops up at 3pm even without \"remind me\".", keywords: "alarm notification alert automatic" },
    Meta { key: "default_time", section: "Reminders", label: "Time for day-only reminders", help: "\"Remind me Friday to pay rego\" pops up at this time (HH:MM).", keywords: "morning default alarm" },
    Meta { key: "snooze_minutes", section: "Reminders", label: "Snooze length (minutes)", help: "What the Snooze button on a pop-up uses.", keywords: "delay later postpone" },
    Meta { key: "sound", section: "Reminders", label: "Play a sound", help: "Plays when a reminder pops up. Test plays it now.", keywords: "audio chime beep noise mute alarm ring volume" },
    Meta { key: "sound_boost", section: "Reminders", label: "Alarm loudness", help: "Makes the sound louder than the file itself (x1 is as recorded). Windows' volume still applies.", keywords: "volume loud louder boost quiet hear audio gain" },
    Meta { key: "sound_duck", section: "Reminders", label: "Turn other sounds down while it plays", help: "Music and videos drop to a fifth of their volume while the alarm sounds, then come back.", keywords: "music spotify duck lower quiet volume mute audio" },
    Meta { key: "sound_repeat", section: "Reminders", label: "Repeat the sound until I respond", help: "Plays again every minute (up to 10 times) while the pop-up is open.", keywords: "audio alarm ring again nag loud" },
    Meta { key: "popup_corner", section: "Reminders", label: "Pop-up position", help: "Where reminder pop-ups appear.", keywords: "corner location place screen" },
    Meta { key: "popup_monitor", section: "Reminders", label: "Pop-up screen", help: "The main screen, a particular screen, or whichever screen the mouse is on.", keywords: "monitor display" },
    Meta { key: "ha", section: "Home Assistant", label: "Connect to Home Assistant", help: "Alerts from home pop up here. Use your Home Assistant address and a long-lived access token (your profile in Home Assistant, Security tab). The token is kept encrypted for this Windows account.", keywords: "home assistant hass webhook relay alert notification external integration token mversion smart home" },
    Meta { key: "ha_print_alerts", section: "Home Assistant", label: "3D print alerts", help: "Pops up when a print finishes, is cancelled or fails, with a photo from the printer's camera. A print that ended while the PC was off pops up later (up to 12 hours).", keywords: "printer 3d print snapmaker u1 moonraker klipper finished done camera photo" },
    Meta { key: "ha_printer", section: "Home Assistant", label: "Printer", help: "Printers come from Home Assistant's Moonraker integration.", keywords: "3d printer u1 snapmaker moonraker" },
    Meta { key: "ha_camera", section: "Home Assistant", label: "Photo from", help: "The camera that takes the picture on a print alert.", keywords: "camera photo picture snapshot image" },
    Meta { key: "ha_doorbell", section: "Home Assistant", label: "Doorbell", help: "When someone presses the doorbell, Sticky plays a ding-dong and pops up with live footage from the doorbell's camera for 3 minutes (click the picture to watch again). Works with a Reolink doorbell, or any doorbell with a visitor sensor.", keywords: "doorbell door bell reolink visitor ring chime front door camera live footage video" },
    Meta { key: "ha_doorbell_sensor", section: "Home Assistant", label: "Doorbell button", help: "The doorbell's visitor sensor in Home Assistant.", keywords: "doorbell visitor button press reolink" },
    Meta { key: "ha_doorbell_camera", section: "Home Assistant", label: "Footage from", help: "The camera the doorbell pop-up shows live.", keywords: "doorbell camera live footage video picture" },
    Meta { key: "ha_notifications", section: "Home Assistant", label: "Alerts from automations and webhooks", help: "A Home Assistant notification whose ID starts with \"sticky\" pops up here and is then cleared. A picture in it, ![](address), comes too. Copy webhook setup copies a ready-made automation.", keywords: "webhook automation notification relay external services ifttt api url persistent" },
    Meta { key: "beeper", section: "Chat alerts", label: "Connect to Beeper", help: "New messages in chats you pick pop up here, from any network Beeper Desktop has: Slack, WhatsApp, Telegram, Messenger and more. Beeper must be running on this PC. Sticky only reads; the token is kept encrypted for this Windows account.", keywords: "beeper slack whatsapp telegram messenger instagram linkedin chat message channel dm notification" },
    Meta { key: "beeper_alerts", section: "Chat alerts", label: "Pop up new messages", help: "Off pauses chat alerts without forgetting the chats.", keywords: "beeper slack chat message alert pause mute" },
    Meta { key: "beeper_chats", section: "Chat alerts", label: "Chats to watch", help: "Each new message from someone else pops up, with Open in Beeper. Several at once come as one pop-up.", keywords: "beeper slack channel chat group dm watch pick" },
    Meta { key: "note_window", section: "Sticky note", label: "Show the sticky note on the desktop", help: "A small always-there note with your open and done items. The desk dashboard has its own Sticky tile too.", keywords: "pin desktop widget permanent window dashboard" },
    Meta { key: "note_on_top", section: "Sticky note", label: "Keep the sticky note on top", help: "Stays above other windows.", keywords: "always top float pin" },
    Meta { key: "note_show_done", section: "Sticky note", label: "Show done items on the note", help: "Lists items finished today, crossed out.", keywords: "completed finished" },
    Meta { key: "note_color", section: "Sticky note", label: "Note colour", help: "Paper colour for the sticky note window.", keywords: "yellow pink blue green paper colour color" },
    Meta { key: "check_updates", section: "Updates", label: "Check for new versions", help: "Looks at the GitHub releases twice a day. Check now does it straight away.", keywords: "upgrade update version release github download new" },
    Meta { key: "auto_update", section: "Updates", label: "Install updates automatically", help: "Installs a new version when no Sticky window is open. Off: use Update in the tray menu.", keywords: "upgrade update automatic silent install" },
    Meta { key: "data", section: "Data", label: "Data folder", help: "items.json (what the dashboard reads), log.jsonl and settings.json live here.", keywords: "files folder backup export json location path" },
];

# Sticky

Post-it notes, to-dos and reminders for Windows, written in Rust. Type what you mean:
"remind me to clean the coffee machine at 4pm" becomes a reminder that pops up at 4pm.

## Install or upgrade

Download `sticky-setup.exe` from the [latest release](https://github.com/mversion/sticky/releases/latest)
and run it. It installs for your account only (no admin) into `%LOCALAPPDATA%\Programs\Sticky`,
adds Sticky to the Start menu and to Settings > Apps (uninstall it there), can start it with
Windows and add a Kite launcher command, and closes any other copy that is running.
The exe isn't signed, so Windows SmartScreen may warn the first time: More info > Run anyway.

Or without the window, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/mversion/sticky/main/install.ps1 | iex
```

After that Sticky updates itself: the tray helper checks the GitHub releases twice a day and
installs a new version when no Sticky window is open (Settings > Updates, or tray > Check for
updates to do it now). Your notes and settings live in `%USERPROFILE%\.sticky` and survive
upgrades and uninstalling.

## Using it

| How | What happens |
|---|---|
| **Ctrl+Alt+N** (Settings > Quick-add hotkey) | Quick-add box on the main display (Settings > Open Sticky on: main display, a particular screen, or where the mouse is) |
| Start menu: Sticky | The main window |
| Tray icon | Left-click opens Sticky; right-click: quick add, sticky note, settings, log, updates, quit |
| Kite launcher: `p` Tab *text* Enter | Adds it straight away (`sticky kite` sets this up) |
| `sticky add <text>` | Adds from a script or another launcher |

Quick add: Enter adds, Esc closes, Ctrl+O opens the full window, Ctrl+, opens settings. It closes
when you click away.

The main window has All open (grouped by folder), each folder (+ next to Folders adds one; right-click a folder to rename, move or delete it, or give it a colour and an icon), Reminders, Notes, Done, an
Activity log and a searchable Settings page. Click an item to open its details in a pane on the right (drag its edge to resize); hover for pin and delete.
An optional always-on-top sticky note shows open items and what got done today.

## What you can type

- `remind me to …`, `reminder: …`, `don't forget to …` turn on the pop-up; any time on its own does too (setting)
- times: `at 4pm`, `4:30pm`, `16:30`, `at 4` (1-6 means afternoon), `noon`, `tonight`, `tomorrow morning`, `this arvo`
- relative: `in 10 min`, `in 2h`, `in an hour`, `in half an hour`, `in 3 days`
- dates: `today`, `tomorrow`, `friday`, `next week`, `25/12` (day first), `3 jan`, `dec 1st`
- repeats: `every day`, `daily`, `every weekday`, `every weekend`, `every monday`, `every mon, wed and fri`, `weekly`, `fortnightly`, `monthly`, `yearly`, `every other day`, `every 3 days`, `every 2 weeks`, `every 30 minutes`, `every 2h`, `hourly`, and for those `between 9am and 5pm` / `from 9 to 5`. The item editor has the same as Microsoft To Do (daily, weekdays, weekly, monthly, yearly, custom) plus every N minutes or hours
- priority: `!` `!!` `!!!`, `!high`, `p1`, `urgent`, `asap`
- folder: `#upnext`, `#later`, `#today`, `#someday` (prefixes work: `#up`); other `#words` become tags
- `note: …` makes a note (no tick box); anything added on the Notes page is a note
- web addresses (`https://…`, `www.…`) stay as typed and show as clickable link chips

Click an item to edit it: notes, steps (ticking the last step ticks the item off; a repeating one moves to its next time with its steps cleared), when, pop-up, repeat, folder, priority, type. Steps can be ticked on the reminder pop-up too.

Reminder pop-ups stay on top with Done, Snooze and more snooze choices, and play a sound you pick in Settings > Reminders (Alarm by default, with a Test button, a loudness boost up to x8, music turned down while it plays, and an option to repeat it every minute until you respond). Reminders missed while the
PC was off pop up when it's back.

## Alerts from Home Assistant

Settings > Home Assistant connects Sticky to your Home Assistant (its address plus a long-lived
access token from your Home Assistant profile, Security tab; the token is stored encrypted with
Windows DPAPI). The background helper keeps a websocket open and turns these into pop-ups:

- **3D prints**: a printer from the Moonraker integration finishing, being cancelled or failing,
  with a photo from its camera (the printer's own camera by default, or any camera you pick).
  A print that ended while the PC was off pops up when Sticky reconnects (up to 12 hours later).
- **Doorbell**: when someone presses the doorbell (a Reolink, or any doorbell with a
  `binary_sensor.*_visitor`), Sticky plays a ding-dong and pops up with the doorbell's camera
  shown live (about three pictures a second) for 3 minutes; click the picture to watch again.
- **Anything else**: a Home Assistant notification whose ID starts with `sticky` pops up and is
  then cleared. A Markdown picture in the message, `![](/api/camera_proxy/camera.x)` or
  `![](https://…)`, shows on the pop-up. **Copy webhook setup** copies a ready-made automation
  with a webhook trigger, so any service can send an alert:

  ```
  POST https://<your home assistant>/api/webhook/sticky-<random>
  {"title": "Print done", "message": "Took 2h", "camera": "camera.printer"}   ("image": "https://…" works too)
  ```

  Keep the webhook address private: anyone who has it can send you alerts.

Locally, `sticky alert <text> [--detail <text>] [--picture <file>]` pops one up from a script.

## How it's built

One small exe (about 6 MB) that runs in roles, so nothing heavy stays in memory:

| Role | Memory | Runs |
|---|---|---|
| `daemon`: tray, hotkey, reminder clock, update check, Home Assistant connection | about 2 MB private | always (HKCU Run key `Sticky`) |
| `ui`: quick add and the main window | about 65 MB, mostly the GL driver | while open |
| `popup`: a reminder | about 65 MB | until you act on it |
| `note`: the desktop sticky note | about 65 MB | only if turned on |

Data lives in `%USERPROFILE%\.sticky`: `items.json` (all items; other tools can read it),
`log.jsonl` (activity) and `settings.json` (hand edits apply within a second). It is kept out of
AppData on purpose, because packaged (MSIX) apps that start Sticky get their own copy of AppData.

### Commands

```
sticky                      quick add               sticky add <text>       add without a window
sticky open|settings|log    main window on a page   sticky done|undo <id>   tick or untick
sticky note                 the desktop note        sticky export           all items as JSON
sticky kite [keyword]       add the Kite runner     sticky update           install the latest release
sticky setup                installer window        sticky install          install without a window
sticky uninstall [--quiet]  remove it               sticky version
sticky alert <text> [--detail <text>] [--picture <file>]   pop up an alert now
```

### Developing

```powershell
cargo test
cargo build --release
```

`STICKY_HOME=<folder>` runs against a test data folder, beside the real copy (it gets its own
window locks). `tools\grab.ps1 -Title "Sticky" -Out x.png [-ProcessId n]` screenshots a Sticky
window. `sticky ha-import` copies the desk dashboard's Home Assistant sign-in into the settings,
`sticky ha-watch` runs only the Home Assistant connection, and `STICKY_FAKE_PRINT_END=1` makes it
send a print alert as soon as it connects. `STICKY_SETTINGS_SEARCH=<words>` pre-fills the settings
search and `STICKY_EDIT=<id>` opens that item's details pane, and `STICKY_NO_FOCUS=1` opens windows
without taking keyboard focus (for screenshots), `STICKY_MUTE=1` plays no sounds and
`STICKY_FAKE_DOORBELL=1` makes the helper act as if the doorbell was pressed once it connects.

### Releasing

```powershell
.\scripts\release.ps1 0.2.0
```

That sets the version in `Cargo.toml`, runs the tests, commits, tags `v0.2.0` and pushes. The
GitHub Actions workflow builds `sticky.exe`, publishes it with its SHA-256 as a release, and
running copies update themselves.

## Licence

MIT

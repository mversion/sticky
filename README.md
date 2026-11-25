# Sticky

Post-it notes, to-dos and reminders for Windows, written in Rust. Type what you mean:
"remind me to clean the coffee machine at 4pm" becomes a reminder that pops up at 4pm.

## Install or upgrade

In PowerShell:

```powershell
irm https://raw.githubusercontent.com/mversion/sticky/main/install.ps1 | iex
```

This puts `sticky.exe` in `%LOCALAPPDATA%\Programs\Sticky`, adds a Start-menu shortcut and starts
it. After that Sticky updates itself: the tray helper checks the GitHub releases twice a day and
installs a new version when no Sticky window is open (Settings > Updates, or tray > Check for
updates to do it now). Running the line again also upgrades.

## Using it

| How | What happens |
|---|---|
| **Ctrl+Alt+N** (Settings > Quick-add hotkey) | Quick-add box on the screen the mouse is on |
| Start menu: Sticky | The main window |
| Tray icon | Left-click opens Sticky; right-click: quick add, sticky note, settings, log, updates, quit |
| Kite launcher: `p` Tab *text* Enter | Adds it straight away (`sticky kite` sets this up) |
| `sticky add <text>` | Adds from a script or another launcher |

Quick add: Enter adds, Esc closes, Ctrl+O opens the full window, Ctrl+, opens settings. It closes
when you click away.

The main window has All open (grouped by category), each category, Reminders, Notes, Done, an
Activity log and a searchable Settings page. Click an item to edit it; hover for pin and delete.
An optional always-on-top sticky note shows open items and what got done today.

## What you can type

- `remind me to …`, `reminder: …`, `don't forget to …` turn on the pop-up; any time on its own does too (setting)
- times: `at 4pm`, `4:30pm`, `16:30`, `at 4` (1-6 means afternoon), `noon`, `tonight`, `tomorrow morning`, `this arvo`
- relative: `in 10 min`, `in 2h`, `in an hour`, `in half an hour`, `in 3 days`
- dates: `today`, `tomorrow`, `friday`, `next week`, `25/12` (day first), `3 jan`, `dec 1st`
- repeats: `every day`, `daily`, `every weekday`, `every monday`, `weekly`, `monthly`
- priority: `!` `!!` `!!!`, `!high`, `p1`, `urgent`, `asap`
- category: `#upnext`, `#later`, `#today`, `#someday` (prefixes work: `#up`); other `#words` become tags
- `note: …` makes a note (no tick box)

Reminder pop-ups stay on top with Done, Snooze and more snooze choices. Reminders missed while the
PC was off pop up when it's back.

## How it's built

One small exe (about 6 MB) that runs in roles, so nothing heavy stays in memory:

| Role | Memory | Runs |
|---|---|---|
| `daemon`: tray, hotkey, reminder clock, update check | about 2 MB private | always (HKCU Run key `Sticky`) |
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
sticky version
```

### Developing

```powershell
cargo test
cargo build --release
```

`STICKY_HOME=<folder>` runs against a test data folder. `tools\grab.ps1 -Title "Sticky" -Out x.png`
screenshots a Sticky window.

### Releasing

```powershell
.\scripts\release.ps1 0.2.0
```

That sets the version in `Cargo.toml`, runs the tests, commits, tags `v0.2.0` and pushes. The
GitHub Actions workflow builds `sticky.exe`, publishes it with its SHA-256 as a release, and
running copies update themselves.

## Licence

MIT

//! Adds Sticky to Kite (the Launchy-style launcher) as a runner command:
//! type the keyword, Tab, the task, Enter. Kite runs `sticky.exe add "<text>"`.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Kite").join("settings.json"))
}

/// Kite is installed (its settings file exists).
pub fn found() -> bool {
    settings_path().is_some_and(|p| p.exists())
}

fn is_sticky(c: &Value) -> bool {
    c.get("file").and_then(Value::as_str).is_some_and(|f| f.to_lowercase().ends_with("sticky.exe"))
}

fn load() -> Result<(PathBuf, Value), String> {
    let p = settings_path().ok_or("APPDATA not set")?;
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok((p, v))
}

fn commands(v: &mut Value) -> Result<&mut Vec<Value>, String> {
    let plugins = v.as_object_mut().ok_or("unexpected settings")?.entry("plugins").or_insert_with(|| json!({}));
    let runner = plugins.as_object_mut().ok_or("unexpected plugins")?.entry("runner").or_insert_with(|| json!({}));
    let cmds = runner.as_object_mut().ok_or("unexpected runner")?.entry("commands").or_insert_with(|| json!([]));
    cmds.as_array_mut().ok_or_else(|| "unexpected commands".to_string())
}

fn save(p: &Path, v: &Value) -> Result<PathBuf, String> {
    let backup = p.with_file_name(format!("settings.before-sticky-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::copy(p, &backup).map_err(|e| format!("backup: {e}"))?;
    let out = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(p, out).map_err(|e| e.to_string())?;
    Ok(backup)
}

/// The keyword of the Kite command that already runs Sticky, if there is one.
pub fn existing_keyword() -> Option<String> {
    let (_, mut v) = load().ok()?;
    commands(&mut v).ok()?.iter().find(|c| is_sticky(c)).and_then(|c| c.get("name")?.as_str().map(String::from))
}

pub fn install(keyword: &str) -> Result<String, String> {
    install_at(&std::env::current_exe().map_err(|e| e.to_string())?, keyword)
}

/// Make Kite's Sticky command run `exe`. An existing Sticky command keeps its keyword (only the
/// path changes); otherwise one is added as `keyword`.
pub fn install_at(exe: &Path, keyword: &str) -> Result<String, String> {
    let keyword = keyword.trim();
    let (p, mut v) = load()?;
    let arr = commands(&mut v)?;
    let file = exe.display().to_string();
    let (i, what) = match arr.iter().position(is_sticky) {
        Some(i) => (i, "updated"),
        None => {
            if keyword.is_empty() {
                return Err("the keyword is empty".into());
            }
            match arr.iter().position(|c| c.get("name").and_then(Value::as_str) == Some(keyword)) {
                Some(i) => (i, "replaced"),
                None => {
                    arr.push(json!({}));
                    (arr.len() - 1, "added")
                }
            }
        }
    };
    let name = arr[i].get("name").and_then(Value::as_str).filter(|_| what == "updated").unwrap_or(keyword).to_string();
    let entry = json!({ "name": name, "file": file, "args": "add \"$$\"" });
    if arr[i] == entry {
        return Ok(format!("Kite already has \"{name}\""));
    }
    arr[i] = entry;
    let backup = save(&p, &v)?;
    Ok(format!("Kite command \"{name}\" {what} (backup {})", backup.display()))
}

/// Remove the Kite commands that run `exe`. True if any were removed.
pub fn remove(exe: &Path) -> Result<bool, String> {
    let (p, mut v) = load()?;
    let target = exe.display().to_string().to_lowercase();
    let arr = commands(&mut v)?;
    let before = arr.len();
    arr.retain(|c| c.get("file").and_then(Value::as_str).map(str::to_lowercase).as_deref() != Some(target.as_str()));
    if arr.len() == before {
        return Ok(false);
    }
    save(&p, &v)?;
    Ok(true)
}

/// Run `change` with Kite closed, then start it again, so Kite neither misses the change nor
/// writes its old settings over it on the way out.
pub fn with_kite_closed<T>(change: impl FnOnce() -> T) -> T {
    let running = crate::win::processes(&["kite.exe"]);
    for p in &running {
        crate::win::kill(p.pid);
    }
    if !running.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let out = change();
    if let Some(path) = running.iter().find_map(|p| p.path.clone()) {
        let _ = std::process::Command::new(&path).current_dir(path.parent().unwrap_or(Path::new("."))).spawn();
    }
    out
}

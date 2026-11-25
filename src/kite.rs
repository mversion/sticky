//! Adds Sticky to Kite (the Launchy-style launcher) as a runner command:
//! type the keyword, Tab, the task, Enter. Kite runs `sticky.exe add "<text>"`.

use serde_json::{Value, json};
use std::path::PathBuf;

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Kite").join("settings.json"))
}

pub fn install(keyword: &str) -> Result<String, String> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Err("the keyword is empty".into());
    }
    let p = settings_path().ok_or("APPDATA not set")?;
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut v: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.display().to_string();

    let plugins = v.as_object_mut().ok_or("unexpected settings")?.entry("plugins").or_insert_with(|| json!({}));
    let runner = plugins.as_object_mut().ok_or("unexpected plugins")?.entry("runner").or_insert_with(|| json!({}));
    let cmds = runner.as_object_mut().ok_or("unexpected runner")?.entry("commands").or_insert_with(|| json!([]));
    let arr = cmds.as_array_mut().ok_or("unexpected commands")?;
    let entry = json!({ "name": keyword, "file": exe, "args": "add \"$$\"" });
    let existing = arr.iter().position(|c| {
        c.get("name").and_then(Value::as_str) == Some(keyword)
            || c.get("file").and_then(Value::as_str).is_some_and(|f| f.to_lowercase().ends_with("sticky.exe"))
    });
    let what = match existing {
        Some(i) if arr[i] == entry => return Ok(format!("Kite already has \"{keyword}\"")),
        Some(i) => {
            arr[i] = entry;
            "updated"
        }
        None => {
            arr.push(entry);
            "added"
        }
    };
    let backup = p.with_file_name(format!("settings.before-sticky-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::copy(&p, &backup).map_err(|e| format!("backup: {e}"))?;
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(&p, out).map_err(|e| e.to_string())?;
    Ok(format!("Kite runner \"{keyword}\" {what} (backup {})", backup.display()))
}

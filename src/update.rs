//! Upgrades from GitHub releases.
//!
//! Each release carries `sticky.exe` and `sticky.exe.sha256`. `sticky update` checks the latest
//! release, downloads the exe beside the running one, checks the hash, swaps the files (a running
//! exe can be renamed on Windows, just not overwritten), stops the other Sticky processes and
//! starts the helper again from the new exe. The helper runs `sticky update --auto` on a timer
//! when nothing is open. Downloads use Windows' own curl.exe, so there's no HTTP stack in the app.

use crate::store;
use sha2::{Digest, Sha256};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const REPO: &str = "mversion/sticky";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const NO_WINDOW: u32 = 0x0800_0000;

pub struct Release {
    pub version: String,
    exe_url: String,
    sha_url: Option<String>,
}

fn ver(s: &str) -> (u32, u32, u32) {
    let mut p = s.trim().trim_start_matches(['v', 'V']).split(['.', '-']).map(|x| x.parse::<u32>().unwrap_or(0));
    (p.next().unwrap_or(0), p.next().unwrap_or(0), p.next().unwrap_or(0))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    ver(remote) > ver(local)
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("curl.exe")
        .args(["-sSfL", "--max-time", "120", "-H", "User-Agent: sticky-updater"])
        .args(args)
        .creation_flags(NO_WINDOW)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

/// The latest published release, whatever its version.
pub fn latest() -> Result<Release, String> {
    let body = curl(&["-H", "Accept: application/vnd.github+json", &format!("https://api.github.com/repos/{REPO}/releases/latest")])?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("no tag on the latest release")?.to_string();
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let url = |name: &str| {
        assets.iter().find(|a| a["name"].as_str() == Some(name)).and_then(|a| a["browser_download_url"].as_str()).map(String::from)
    };
    Ok(Release { version: tag.trim_start_matches('v').to_string(), exe_url: url("sticky.exe").ok_or("release has no sticky.exe")?, sha_url: url("sticky.exe.sha256") })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Download, verify and swap in `r`. Returns the path of the new exe.
fn install(r: &Release) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no folder")?.to_path_buf();
    let bytes = curl(&[&r.exe_url])?;
    if bytes.len() < 500_000 || &bytes[..2] != b"MZ" {
        return Err("download doesn't look like an exe".into());
    }
    if let Some(u) = &r.sha_url {
        let want = String::from_utf8_lossy(&curl(&[u])?).split_whitespace().next().unwrap_or("").to_lowercase();
        let got = hex(&Sha256::digest(&bytes));
        if want != got {
            return Err(format!("checksum mismatch (expected {want}, got {got})"));
        }
    }
    let new = dir.join("sticky.new.exe");
    let old = dir.join("sticky.old.exe");
    std::fs::write(&new, &bytes).map_err(|e| format!("write: {e}"))?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("move old exe aside: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // put it back
        return Err(format!("swap: {e}"));
    }
    Ok(exe)
}

/// `sticky update [--auto]`. Interactive runs say what happened in a toast.
pub fn run(auto: bool) {
    let say = |title: &str, detail: &str| {
        if !auto || title.starts_with("Updated") {
            crate::ui::toast::run(title.to_string(), detail.to_string());
        }
    };
    let r = match latest() {
        Ok(r) => r,
        Err(e) => {
            if !auto {
                store::log("update check failed", 0, &e);
            }
            say("Couldn't check for updates", &e);
            return;
        }
    };
    if !is_newer(&r.version, VERSION) {
        say(&format!("Sticky {VERSION} is the latest"), "");
        return;
    }
    store::log("updating", 0, &format!("{VERSION} → {}", r.version));
    match install(&r) {
        Ok(exe) => {
            stop_others();
            let _ = Command::new(&exe).arg("daemon").creation_flags(NO_WINDOW | 0x8).spawn();
            store::log("updated", 0, &format!("to {}", r.version));
            say(&format!("Updated to Sticky {}", r.version), &format!("from {VERSION}"));
        }
        Err(e) => {
            store::log("update failed", 0, &e);
            say("Update failed", &e);
        }
    }
}

/// Remove the previous exe left by an update (it can't be deleted while it was running).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let _ = std::fs::remove_file(dir.join("sticky.old.exe"));
    }
}

/// End every other sticky.exe running from this folder (helper, note, windows).
fn stop_others() {
    if let Ok(exe) = std::env::current_exe() {
        crate::win::stop_sticky(Some(exe.parent().unwrap_or(Path::new("."))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }
}

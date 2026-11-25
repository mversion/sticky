//! Tokens are kept in settings.json encrypted with Windows DPAPI (only this Windows account on
//! this PC can read them), as "dpapi:<hex>".

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::*;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn protect(data: &[u8], decrypt: bool) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 || out.pbData.is_null() {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData as _) };
    Some(v)
}

/// Encrypt a token for settings.json. Empty stays empty.
pub fn seal(token: &str) -> String {
    let t = token.trim();
    if t.is_empty() {
        return String::new();
    }
    protect(t.as_bytes(), false).map(|b| format!("dpapi:{}", hex(&b))).unwrap_or_default()
}

/// Decrypt what `seal` made. None when empty or unreadable (another account or PC).
pub fn open(sealed: &str) -> Option<String> {
    let b = unhex(sealed.strip_prefix("dpapi:")?)?;
    String::from_utf8(protect(&b, true)?).ok().filter(|s| !s.is_empty())
}

/// A PowerShell `Export-Clixml` credential (address as the user name, token as the password),
/// as the desk dashboard keeps one in `%LOCALAPPDATA%\RadioParts\home-assistant.xml`.
pub fn read_clixml(path: &std::path::Path) -> Option<(String, String)> {
    let x = std::fs::read_to_string(path).ok()?;
    let between = |a: &str, b: &str| -> Option<String> {
        let i = x.find(a)? + a.len();
        let j = x[i..].find(b)? + i;
        Some(x[i..j].to_string())
    };
    let user = between("<S N=\"UserName\">", "</S>")?;
    let enc = unhex(&between("<SS N=\"Password\">", "</SS>")?)?;
    let plain = protect(&enc, true)?;
    // SecureString text is UTF-16LE
    let u: Vec<u16> = plain.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let token = String::from_utf16(&u).ok()?;
    Some((user.trim().to_string(), token.trim().to_string()))
}

/// Where the desk dashboard keeps its Home Assistant sign-in, if this PC has one.
pub fn desk_dashboard_credential() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("RadioParts").join("home-assistant.xml");
    p.exists().then_some(p)
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let s = super::seal("abc.def");
        assert!(s.starts_with("dpapi:"));
        assert_eq!(super::open(&s).as_deref(), Some("abc.def"));
        assert_eq!(super::seal(""), "");
        assert!(super::open("").is_none());
    }
}

//! Reminder sounds. `sticky ring <name>` runs in its own short-lived process so a pop-up that
//! closes straight away can't cut the sound off or leave other apps turned down.
//!
//! Loudness: the Windows alarm files peak around -9 dBFS, so there is room to make them louder
//! before Windows' own volume. Samples are pushed through a soft limiter (tanh) so the boost
//! doesn't crackle. While the sound plays, other apps' audio (music) can be turned down and
//! brought back afterwards.

use crate::settings::Settings;
use crate::win::{self, wide};
use std::path::PathBuf;
use std::time::Duration;

/// (setting key, label, file in C:\Windows\Media). "windows" is whatever the Windows sound
/// scheme has for Calendar Reminder.
pub const SOUNDS: [(&str, &str, &str); 7] = [
    ("alarm", "Alarm", "Alarm01.wav"),
    ("alarm2", "Alarm 2", "Alarm02.wav"),
    ("ring", "Ring", "Ring01.wav"),
    ("chimes", "Chimes", "chimes.wav"),
    ("notify", "Notify", "Windows Notify System Generic.wav"),
    ("windows", "Windows reminder", ""),
    ("doorbell", "Doorbell", ""),
];

const RING_MUTEX: &str = "Local\\Sticky.Ring";

/// Start the sound in its own process and return straight away.
pub fn ring(name: &str) {
    if std::env::var_os("STICKY_MUTE").is_some() {
        return; // test runs
    }
    win::spawn_self(&["ring", name]);
}

/// The `ring` role: turn other apps down, play the sound to the end, turn them back up.
pub fn run(name: &str) {
    let s = Settings::load();
    // One at a time, so two reminders together don't stack the turn-down and restore wrongly.
    let _turn = win::wait_mutex(RING_MUTEX, Duration::from_secs(20));
    let wav = if name == "doorbell" { Some(doorbell_wav()) } else { file(name).and_then(|p| std::fs::read(p).ok()) };
    let wav = wav.map(|w| louder(&w, s.sound_boost.clamp(1, 8) as f32).unwrap_or(w));
    let ducked = if s.sound_duck { duck::down(0.2) } else { vec![] };
    match &wav {
        Some(w) => play_sync(w),
        None => win::play_alias_sync("SystemAsterisk"),
    }
    duck::restore(ducked);
}

fn file(name: &str) -> Option<PathBuf> {
    let media = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("Media");
    let p = if name == "windows" {
        let v = win::reg_get_str(r"AppEvents\Schemes\Apps\.Default\Notification.Reminder\.Current", "")?;
        PathBuf::from(expand_env(&v))
    } else {
        media.join(SOUNDS.iter().find(|s| s.0 == name).map(|s| s.2).unwrap_or("Alarm01.wav"))
    };
    p.is_file().then_some(p)
}

/// A two-note ding-dong (E5 then C5, bell-like with a few overtones), as a 16-bit mono WAV.
/// Peaks around -10 dBFS like the Windows files, so the loudness setting treats it the same.
pub fn doorbell_wav() -> Vec<u8> {
    let rate = 22050u32;
    let len = (rate as f32 * 2.4) as usize;
    let mut buf = vec![0f32; len];
    let tau = std::f32::consts::TAU;
    for (f, start) in [(659.25f32, 0.0f32), (523.25, 0.6)] {
        let s0 = (start * rate as f32) as usize;
        for (i, out) in buf.iter_mut().enumerate().skip(s0) {
            let t = (i - s0) as f32 / rate as f32;
            let attack = (t * 300.0).min(1.0);
            let v = (tau * f * t).sin() * (-t * 2.0).exp() + 0.4 * (tau * 2.0 * f * t).sin() * (-t * 4.5).exp() + 0.2 * (tau * 3.01 * f * t).sin() * (-t * 7.0).exp();
            *out += 0.2 * attack * v;
        }
    }
    let data: Vec<u8> = buf.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let mut w = Vec::with_capacity(44 + data.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

fn expand_env(s: &str) -> String {
    let mut out = s.to_string();
    for (k, v) in std::env::vars() {
        out = out.replace(&format!("%{k}%"), &v).replace(&format!("%{}%", k.to_uppercase()), &v);
    }
    out
}

fn play_sync(wav: &[u8]) {
    use windows_sys::Win32::Media::Audio::*;
    unsafe {
        if PlaySoundW(wav.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_SYNC | SND_NODEFAULT) == 0 {
            PlaySoundW(wide("SystemAsterisk").as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_SYNC);
        }
    }
}

/// A louder copy of a 16-bit PCM WAV: gain `g` through a soft limiter. Other formats: None.
pub fn louder(wav: &[u8], g: f32) -> Option<Vec<u8>> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let (mut fmt, mut data) = (None, None);
    let mut i = 12;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let size = u32::from_le_bytes(wav[i + 4..i + 8].try_into().ok()?) as usize;
        let body = i + 8..(i + 8 + size).min(wav.len());
        match id {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + size + (size & 1);
    }
    let (fmt, data) = (fmt?, data?);
    let f = &wav[fmt.clone()];
    let tag = u16::from_le_bytes([f[0], f[1]]);
    let bits = u16::from_le_bytes([f[14], f[15]]);
    if tag != 1 || bits != 16 {
        return None;
    }
    let norm = g.tanh(); // so full scale stays full scale
    let mut samples: Vec<u8> = Vec::with_capacity(data.len());
    for c in wav[data].chunks_exact(2) {
        let x = i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0;
        let y = if g <= 1.0 { x } else { (g * x).tanh() / norm };
        samples.extend_from_slice(&((y.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    let mut out = Vec::with_capacity(44 + samples.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((4 + 8 + f.len() + 8 + samples.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&(f.len() as u32).to_le_bytes());
    out.extend_from_slice(f);
    if f.len() & 1 == 1 {
        out.push(0);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    out.extend_from_slice(&samples);
    Some(out)
}

/// Turning other apps' audio down while the alarm plays, through their Windows volume-mixer
/// sliders, and putting each one back where it was.
mod duck {
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    use windows::core::Interface;

    pub struct Ducked(ISimpleAudioVolume, f32);

    pub fn down(to: f32) -> Vec<Ducked> {
        unsafe { try_down(to).unwrap_or_default() }
    }

    unsafe fn try_down(to: f32) -> windows::core::Result<Vec<Ducked>> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None)?;
            let list = mgr.GetSessionEnumerator()?;
            let me = std::process::id();
            let mut out = vec![];
            for i in 0..list.GetCount()? {
                let Ok(c) = list.GetSession(i) else { continue };
                if c.GetState().ok() != Some(AudioSessionStateActive) {
                    continue; // only what is actually playing
                }
                let Ok(c2) = c.cast::<IAudioSessionControl2>() else { continue };
                if c2.GetProcessId().ok() == Some(me) || c2.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK {
                    continue;
                }
                let Ok(v) = c.cast::<ISimpleAudioVolume>() else { continue };
                let Ok(was) = v.GetMasterVolume() else { continue };
                if v.SetMasterVolume(was * to, std::ptr::null()).is_ok() {
                    out.push(Ducked(v, was));
                }
            }
            Ok(out)
        }
    }

    /// Bring them back up over about half a second.
    pub fn restore(list: Vec<Ducked>) {
        if list.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        let cur: Vec<f32> = list.iter().map(|d| unsafe { d.0.GetMasterVolume().unwrap_or(d.1) }).collect();
        for step in 1..=6 {
            let t = step as f32 / 6.0;
            for (d, c) in list.iter().zip(&cur) {
                unsafe {
                    let _ = d.0.SetMasterVolume(c + (d.1 - c) * t, std::ptr::null());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn louder_keeps_full_scale_and_boosts_quiet() {
        let mut w = Vec::new();
        let samples: [i16; 4] = [3000, -3000, 32767, -32768];
        let fmt: [u8; 16] = [1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0];
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(4 + 8 + 16 + 8 + 8u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&fmt);
        w.extend_from_slice(b"data");
        w.extend_from_slice(&8u32.to_le_bytes());
        for s in samples {
            w.extend_from_slice(&s.to_le_bytes());
        }
        let out = super::louder(&w, 4.0).unwrap();
        let d = &out[out.len() - 8..];
        let s: Vec<i16> = d.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert!(s[0] > 3000 * 3, "quiet sample boosted: {}", s[0]);
        assert_eq!(s[0], -s[1]);
        assert!(s[2] >= 32700 && s[3] <= -32700);
    }
}

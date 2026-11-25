//! Pictures on alerts: decoded with Windows Imaging Component (JPEG, PNG, GIF, BMP, WebP where
//! installed), so Sticky carries no image library. Files live in `.sticky\pictures`.

use std::path::PathBuf;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

pub fn dir() -> PathBuf {
    let d = crate::store::data_dir().join("pictures");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn path(name: &str) -> PathBuf {
    dir().join(name)
}

/// Save picture bytes, returning the file name to keep on the item.
pub fn save(bytes: &[u8]) -> Option<String> {
    let ext = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(b"GIF") {
        "gif"
    } else if bytes.starts_with(b"RIFF") {
        "webp"
    } else {
        "jpg"
    };
    let name = format!("{}.{ext}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"));
    std::fs::write(path(&name), bytes).ok()?;
    Some(name)
}

/// Remove pictures older than `days` that no open item uses.
pub fn prune(days: u64, keep: &[String]) {
    let Ok(rd) = std::fs::read_dir(dir()) else { return };
    let limit = std::time::Duration::from_secs(days * 86_400);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a > limit);
        if old && !keep.contains(&name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Decode to RGBA, scaled down to at most `max_w` pixels wide. Returns (width, height, pixels).
pub fn decode(bytes: &[u8], max_w: u32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = f.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let dec = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let conv = f.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut buf).ok()?;
        // WIC's scalers wrap dark pixels round to white on some JPEGs, so shrink here instead
        if w > max_w {
            let nh = ((h as u64 * max_w as u64) / w as u64).max(1) as u32;
            buf = shrink(&buf, w, h, max_w, nh);
            w = max_w;
            h = nh;
        }
        // Lift the darkest tones a touch: the pop-up's drop shadow (an extended DWM frame) makes
        // near-black pixels see-through, which shows as specks.
        for px in buf.chunks_exact_mut(4) {
            for c in &mut px[..3] {
                *c = 20 + (*c as u32 * 235 / 255) as u8;
            }
        }
        Some((w, h, buf))
    }
}

/// Box-filter downscale of RGBA pixels: each output pixel is the average of the source pixels it covers.
fn shrink(src: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    let (w, h, nw, nh) = (w as usize, h as usize, nw as usize, nh as usize);
    let mut out = vec![0u8; nw * nh * 4];
    for oy in 0..nh {
        let y0 = oy * h / nh;
        let y1 = ((oy + 1) * h / nh).max(y0 + 1).min(h);
        for ox in 0..nw {
            let x0 = ox * w / nw;
            let x1 = ((ox + 1) * w / nw).max(x0 + 1).min(w);
            let mut acc = [0u32; 4];
            for y in y0..y1 {
                let row = &src[(y * w + x0) * 4..(y * w + x1) * 4];
                for px in row.chunks_exact(4) {
                    for c in 0..4 {
                        acc[c] += px[c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (oy * nw + ox) * 4;
            for c in 0..4 {
                out[o + c] = (acc[c] / n) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn shrink_averages() {
        // 2x2 -> 1x1: black, white, black, white -> mid grey
        let px = [0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        assert_eq!(super::shrink(&px, 2, 2, 1, 1), vec![127, 127, 127, 255]);
    }
}


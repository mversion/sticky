//! Gives sticky.exe its icon and version details, so Explorer, Kite and Task Manager show the note.
//! The icon is drawn by the same code the app uses (src/icon_art.rs) and written as a .ico here.

#[allow(dead_code)]
mod art {
    include!("src/icon_art.rs");
}

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon_art.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("sticky.ico");
    std::fs::write(&ico, ico_bytes(&[16, 20, 24, 32, 40, 48, 64, 256])).expect("write icon");

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico.to_str().unwrap());
    res.set("FileDescription", "Sticky");
    res.set("ProductName", "Sticky");
    res.set("CompanyName", "mversion");
    res.set("LegalCopyright", "MIT licence");
    res.set("OriginalFilename", "sticky.exe");
    res.compile().expect("compile Windows resources");
}

/// A .ico with one image per size: 32-bit bitmaps up to 64 px, PNG for the big one.
fn ico_bytes(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| if s >= 256 { png(s) } else { dib(s) }).collect();
    let mut f = Vec::new();
    f.extend_from_slice(&0u16.to_le_bytes()); // reserved
    f.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    f.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, img) in sizes.iter().zip(&images) {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        f.extend_from_slice(&[b, b, 0, 0]);
        f.extend_from_slice(&1u16.to_le_bytes()); // planes
        f.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        f.extend_from_slice(&(img.len() as u32).to_le_bytes());
        f.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        f.extend_from_slice(&img);
    }
    f
}

/// BITMAPINFOHEADER, bottom-up BGRA pixels, then an empty AND mask (alpha does the work).
fn dib(s: u32) -> Vec<u8> {
    let px = art::rgba(s, false);
    let mask_row = s.div_ceil(32) * 4;
    let mut b = Vec::new();
    for v in [40u32, s, s * 2] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    for v in [0u32, s * s * 4 + mask_row * s, 0, 0, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for y in (0..s).rev() {
        for x in 0..s {
            let i = ((y * s + x) * 4) as usize;
            b.extend_from_slice(&[px[i + 2], px[i + 1], px[i], px[i + 3]]);
        }
    }
    b.resize(b.len() + (mask_row * s) as usize, 0);
    b
}

fn png(s: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, s, s);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("png header");
        w.write_image_data(&art::rgba(s, false)).expect("png data");
    }
    out
}

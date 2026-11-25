// The app icon, drawn in code: a yellow note with a folded corner and two lines.
// Shared by the app (tray, window icons) and build.rs (the exe's icon), so it has no dependencies.

pub fn rgba(size: u32, badge: bool) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let r = s * 0.16; // corner radius
    let fold = s * 0.30;
    let m = s * 0.06; // margin
    let (x0, y0, x1, y1) = (m, m, s - m, s - m);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let i = ((y * size + x) * 4) as usize;
            // rounded rect coverage (cheap AA)
            let cov = rrect_cov(fx, fy, x0, y0, x1, y1, r);
            if cov <= 0.0 {
                continue;
            }
            // folded corner bottom-right: cut along the diagonal
            let d = (fx - (x1 - fold)) + (fy - (y1 - fold)) - fold;
            let (mut col, mut a) = ([255u8, 200, 61], cov);
            if d > 0.0 {
                continue;
            }
            if d > -fold * 0.9 && fx > x1 - fold && fy > y1 - fold {
                col = [214, 160, 30]; // the fold
            }
            // two text lines
            let line = |ly: f32| (fy - ly).abs() < s * 0.045 && fx > x0 + s * 0.16 && fx < x1 - s * 0.2;
            if line(s * 0.36) || (line(s * 0.54) && fx < x1 - s * 0.34) {
                col = [92, 70, 10];
            }
            if badge {
                let (bx, by, br) = (s * 0.78, s * 0.22, s * 0.2);
                let dd = ((fx - bx).powi(2) + (fy - by).powi(2)).sqrt();
                if dd < br {
                    col = [230, 60, 60];
                    a = 1.0;
                }
            }
            px[i] = col[0];
            px[i + 1] = col[1];
            px[i + 2] = col[2];
            px[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    px
}

fn rrect_cov(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

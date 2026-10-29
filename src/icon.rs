//! The app icon, drawn in code: a coffee cup seen from above.

pub const SIZE: u32 = 32;

/// RGBA pixels for a `SIZE`×`SIZE` icon: espresso rim, porcelain ring, coffee centre.
pub fn rgba() -> Vec<u8> {
    let layers: [(f32, [u8; 3]); 3] = [
        (15.5, [0x2E, 0x22, 0x1B]),
        (13.8, [0xFA, 0xF9, 0xF7]),
        (9.0, [0xB0, 0x70, 0x3A]),
    ];
    let c = SIZE as f32 / 2.0;
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
            let mut px = [0f32; 4];
            for (r, rgb) in layers {
                // One pixel of anti-aliasing at each edge.
                let cover = (r - d + 0.5).clamp(0.0, 1.0);
                for i in 0..3 {
                    px[i] = px[i] * (1.0 - cover) + rgb[i] as f32 * cover;
                }
                px[3] = px[3].max(cover * 255.0);
            }
            out.extend(px.map(|v| v.round() as u8));
        }
    }
    out
}

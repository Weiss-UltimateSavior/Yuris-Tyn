//! 整串 100px 放大判定。
fn main() {
    let bytes = std::fs::read(r"C:\Windows\Fonts\simhei.ttf").unwrap();
    let font = fontdue::Font::from_bytes(
        bytes,
        fontdue::FontSettings {
            collection_index: 0,
            scale: 40.0,
            load_substitutions: true,
        },
    )
    .unwrap();
    let text = "測試男性7@大牌製作人:「唔唔」";
    let px = 100.0f32;
    let mut glyphs = Vec::new();
    let mut w_total = 20usize;
    for ch in text.chars() {
        let (m, bmp) = font.rasterize(ch, px);
        w_total += m.advance_width as usize;
        glyphs.push((m, bmp));
    }
    let h = 160usize;
    let w = w_total;
    let mut canvas = vec![255u8; w * h * 4];
    let mut pen = 10usize;
    for (m, bmp) in glyphs {
        for i in 0..bmp.len() {
            let a = bmp[i];
            if a < 8 {
                continue;
            }
            let bx = m.xmin + (i % m.width) as i32;
            let by = m.ymin + (i / m.width) as i32;
            let cx = pen as i32 + bx;
            let cy = 120i32 + by;
            if cx < 0 || cy < 0 {
                continue;
            }
            let (cx, cy) = (cx as usize, cy as usize);
            if cx >= w || cy >= h {
                continue;
            }
            let o = (cy * w + cx) * 4;
            let v = 255u8.saturating_sub(a);
            canvas[o] = v;
            canvas[o + 1] = v;
            canvas[o + 2] = v;
            canvas[o + 3] = 255;
        }
        pen += m.advance_width as usize;
    }
    let img = image::RgbaImage::from_fn(w as u32, h as u32, |x, y| {
        let o = (y as usize * w + x as usize) * 4;
        image::Rgba([canvas[o], canvas[o + 1], canvas[o + 2], 255])
    });
    img.save(r"C:\Users\weiss\AppData\Local\Temp\font_full.png").unwrap();
    println!("saved {}x{}", w, h);
}

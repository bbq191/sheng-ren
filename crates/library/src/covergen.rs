//! 找不到原作封面时生成一张：书名在上、作者头像居中、作者名在下（用户 2026-09-27）。
//!
//! 样式按书的 id 挑（配色、边框、分隔线），每本书不一样，但同一本书每次生成都一样（产物指纹稳定）。
//! 配色都是深浅对比强的组合，黑白屏上也清楚。没有头像时，中间画作者名第一个字的圆形字标。
//! 字体用系统里的思源宋体繁体（`fc-match` 找），环境变量 `BOOKLIB_COVER_FONT=字体文件[:序号]` 可以换。

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use image::{Rgb, RgbImage};

const W: u32 = 1200;
const H: u32 = 1800;

/// 配色：(背景, 文字, 点缀)。
const PALETTES: &[([u8; 3], [u8; 3], [u8; 3])] = &[
    ([24, 36, 58], [242, 234, 216], [196, 164, 104]),   // 深蓝 + 米白 + 金
    ([242, 236, 222], [30, 30, 30], [140, 36, 36]),      // 米白 + 墨黑 + 朱红
    ([20, 20, 20], [240, 240, 240], [170, 170, 170]),    // 黑 + 白 + 灰
    ([34, 60, 48], [236, 230, 210], [200, 180, 120]),    // 墨绿 + 米白 + 金
    ([96, 28, 36], [244, 234, 220], [226, 196, 150]),    // 酒红 + 米白 + 浅金
    ([250, 250, 246], [20, 32, 56], [20, 32, 56]),       // 白 + 藏青
    ([62, 64, 70], [246, 244, 238], [210, 200, 180]),    // 石板灰 + 白
];

/// 找封面用的字体：`BOOKLIB_COVER_FONT`，否则 `fc-match` 找思源宋体繁体粗体，再退到任意繁体中文衬线体。
pub(crate) fn load_font() -> Result<FontVec, String> {
    let (path, index) = match std::env::var("BOOKLIB_COVER_FONT").ok().filter(|v| !v.is_empty()) {
        Some(v) => match v.rsplit_once(':').and_then(|(p, i)| i.parse::<u32>().ok().map(|i| (p.to_string(), i))) {
            Some(x) => x,
            None => (v, 0),
        },
        None => ["Noto Serif CJK TC:style=Bold", "Source Han Serif TC:style=Bold", "serif:lang=zh-tw:style=Bold", "sans-serif:lang=zh-tw"]
            .iter()
            .find_map(|pat| {
                let out = std::process::Command::new("fc-match").args(["-f", "%{file}\t%{index}\t%{lang}", pat]).output().ok()?;
                let s = String::from_utf8_lossy(&out.stdout).into_owned();
                let mut it = s.split('\t');
                let (file, idx, langs) = (it.next()?.to_string(), it.next()?.parse().ok()?, it.next().unwrap_or(""));
                langs.split('|').any(|l| l.starts_with("zh")).then_some((file, idx))
            })
            .ok_or("找不到中文字体（装 Noto Serif CJK，或用 BOOKLIB_COVER_FONT=字体文件 指定）")?,
    };
    let data = std::fs::read(&path).map_err(|e| format!("读字体 {path}: {e}"))?;
    FontVec::try_from_vec_and_index(data, index).map_err(|e| format!("字体 {path}: {e}"))
}

fn blend(img: &mut RgbImage, x: i64, y: i64, c: [u8; 3], a: f32) {
    if x < 0 || y < 0 || x >= W as i64 || y >= H as i64 || a <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    for (dst, src) in p.0.iter_mut().zip(c) {
        *dst = (*dst as f32 * (1.0 - a) + src as f32 * a).round() as u8;
    }
}

fn fill_rect(img: &mut RgbImage, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 3]) {
    for y in y0.max(0)..y1.min(H as i64) {
        for x in x0.max(0)..x1.min(W as i64) {
            img.put_pixel(x as u32, y as u32, Rgb(c));
        }
    }
}

/// 矩形边框（线宽 `t`）。
fn frame(img: &mut RgbImage, inset: i64, t: i64, c: [u8; 3]) {
    let (w, h) = (W as i64, H as i64);
    fill_rect(img, inset, inset, w - inset, inset + t, c);
    fill_rect(img, inset, h - inset - t, w - inset, h - inset, c);
    fill_rect(img, inset, inset, inset + t, h - inset, c);
    fill_rect(img, w - inset - t, inset, w - inset, h - inset, c);
}

/// 一行字的宽度。
fn line_width(font: &FontVec, scale: PxScale, s: &str) -> f32 {
    let sf = font.as_scaled(scale);
    let mut w = 0.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

/// 以 (cx, baseline) 水平居中画一行字。
fn draw_line(img: &mut RgbImage, font: &FontVec, scale: PxScale, s: &str, cx: f32, baseline: f32, c: [u8; 3]) {
    let sf = font.as_scaled(scale);
    let mut x = cx - line_width(font, scale, s) / 2.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(scale, ab_glyph::point(x, baseline));
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, cov| blend(img, b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64, c, cov));
        }
        x += sf.h_advance(id);
        prev = Some(id);
    }
}

/// 折行：汉字逐字可断，连续的拉丁字母、数字当一个词不拆开。
fn wrap(font: &FontVec, scale: PxScale, text: &str, max_w: f32) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for ch in text.chars() {
        let latin = ch.is_ascii_alphanumeric() || "'-.".contains(ch);
        match tokens.last_mut() {
            Some(t) if latin && t.chars().last().is_some_and(|c| c.is_ascii_alphanumeric() || "'-.".contains(c)) => t.push(ch),
            _ => tokens.push(ch.to_string()),
        }
    }
    let mut lines: Vec<String> = vec![String::new()];
    for t in tokens {
        let cur = lines.last_mut().unwrap();
        let candidate = format!("{cur}{t}");
        if cur.is_empty() || line_width(font, scale, &candidate) <= max_w {
            *cur = candidate;
        } else {
            lines.push(t.trim_start().to_string());
        }
    }
    lines.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// 书名：从大字号往下试，放得进 `max_lines` 行为止；再把各行排匀（不让最后一行只剩一两个字）。
fn fit_title(font: &FontVec, title: &str, max_w: f32, max_lines: usize) -> (PxScale, Vec<String>) {
    let mut size = 150.0;
    loop {
        let scale = PxScale::from(size);
        let lines = wrap(font, scale, title, max_w);
        if lines.len() <= max_lines || size <= 56.0 {
            return (scale, balance(font, scale, title, max_w, lines));
        }
        size -= 8.0;
    }
}

/// 行数不变的前提下，把每行的最大宽度压到最小（二分行宽），各行长短接近。
fn balance(font: &FontVec, scale: PxScale, text: &str, max_w: f32, lines: Vec<String>) -> Vec<String> {
    let n = lines.len();
    if n <= 1 {
        return lines;
    }
    let (mut lo, mut hi) = (line_width(font, scale, text) / n as f32, max_w);
    let mut best = lines;
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        let try_lines = wrap(font, scale, text, mid);
        if try_lines.len() <= n {
            best = try_lines;
            hi = mid;
        } else {
            lo = mid;
        }
    }
    best
}

/// 显示用：全角字母数字换成半角（"１３級階梯" 的全角数字字距太开）。
fn halfwidth(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() && ('\u{FF01}'..='\u{FF5E}').contains(&c) { char::from_u32(c as u32 - 0xFEE0).unwrap_or(c) } else { c }).collect()
}

/// 头像：裁成正方形（偏上，脸通常在上半部），缩放，贴成带描边的圆。
fn draw_portrait(img: &mut RgbImage, photo: &image::DynamicImage, cx: i64, cy: i64, r: i64, ring: [u8; 3]) {
    let (pw, ph) = (photo.width(), photo.height());
    let side = pw.min(ph);
    let x0 = (pw - side) / 2;
    let y0 = if ph > pw { ((ph - side) as f32 * 0.2) as u32 } else { 0 };
    let sq = photo.crop_imm(x0, y0, side, side).resize_exact((2 * r) as u32, (2 * r) as u32, image::imageops::FilterType::Lanczos3).to_rgb8();
    for dy in -r - 8..=r + 8 {
        for dx in -r - 8..=r + 8 {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            if d <= r as f32 {
                let p = sq.get_pixel((dx + r).clamp(0, 2 * r - 1) as u32, (dy + r).clamp(0, 2 * r - 1) as u32).0;
                blend(img, cx + dx, cy + dy, p, (r as f32 - d + 0.5).clamp(0.0, 1.0));
            }
            if (d - (r as f32 + 4.0)).abs() <= 4.0 {
                blend(img, cx + dx, cy + dy, ring, (4.5 - (d - (r as f32 + 4.0)).abs()).clamp(0.0, 1.0));
            }
        }
    }
}

/// 没有头像：圆环里放作者名（或书名）的第一个字。
fn draw_monogram(img: &mut RgbImage, font: &FontVec, ch: &str, cx: i64, cy: i64, r: i64, c: [u8; 3]) {
    for dy in -r - 6..=r + 6 {
        for dx in -r - 6..=r + 6 {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let a = (4.0 - (d - r as f32).abs()).clamp(0.0, 1.0);
            blend(img, cx + dx, cy + dy, c, a);
        }
    }
    let scale = PxScale::from(r as f32 * 1.1);
    let sf = font.as_scaled(scale);
    let baseline = cy as f32 + (sf.ascent() + sf.descent()) / 2.0;
    draw_line(img, font, scale, ch, cx as f32, baseline, c);
}

/// 生成封面 JPEG。`seed` 决定样式（传书的 id 的哈希，同一本书每次一样）。
pub(crate) fn render(font: &FontVec, title: &str, author: &str, portrait: Option<&[u8]>, seed: u64) -> Result<Vec<u8>, String> {
    let (bg, fg, accent) = PALETTES[(seed % PALETTES.len() as u64) as usize];
    let style = (seed / 7) % 3;
    let mut img = RgbImage::from_pixel(W, H, Rgb(bg));
    // 边框：双线 / 单线 / 上下两道粗线
    match style {
        0 => {
            frame(&mut img, 48, 6, accent);
            frame(&mut img, 68, 2, accent);
        }
        1 => frame(&mut img, 60, 3, accent),
        _ => {
            fill_rect(&mut img, 0, 0, W as i64, 36, accent);
            fill_rect(&mut img, 0, H as i64 - 36, W as i64, H as i64, accent);
        }
    }
    let cx = W as f32 / 2.0;
    // 书名（上）
    let (title, author) = (halfwidth(title.trim()), halfwidth(author.trim()));
    let (scale, lines) = fit_title(font, &title, 940.0, 3);
    let sf = font.as_scaled(scale);
    let line_h = sf.height() * 1.15;
    let top = 200.0;
    for (i, l) in lines.iter().enumerate() {
        draw_line(&mut img, font, scale, l, cx, top + sf.ascent() + i as f32 * line_h, fg);
    }
    let title_bottom = top + lines.len() as f32 * line_h;
    // 分隔线
    let rule_y = (title_bottom + 50.0) as i64;
    fill_rect(&mut img, 480, rule_y, 720, rule_y + 4, accent);
    // 头像（居中）
    let (pcx, pcy, r) = (W as i64 / 2, ((rule_y as f32 + 1480.0) / 2.0) as i64, 260);
    let photo = portrait.and_then(|b| image::load_from_memory(b).ok());
    match &photo {
        Some(p) => draw_portrait(&mut img, p, pcx, pcy, r, accent),
        None => {
            let first: String = author.chars().chain(title.chars()).find(|c| c.is_alphanumeric()).map(String::from).unwrap_or_default();
            draw_monogram(&mut img, font, &first, pcx, pcy, r, accent);
        }
    }
    // 作者名（下）
    if !author.is_empty() {
        let (ascale, alines) = fit_title(font, &author, 940.0, 1);
        let ascale = PxScale::from(ascale.x.min(76.0));
        draw_line(&mut img, font, ascale, alines.first().map_or(author.as_str(), String::as_str), cx, 1620.0, fg);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&img).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_title_and_monogram_deterministically() {
        let Ok(font) = load_font() else { return }; // 没有中文字体的环境跳过
        let a = render(&font, "傑克倫敦短篇小說選一", "傑克‧倫敦", None, 42).unwrap();
        assert_eq!(a, render(&font, "傑克倫敦短篇小說選一", "傑克‧倫敦", None, 42).unwrap(), "同一本书每次一样");
        let img = image::load_from_memory(&a).unwrap();
        assert_eq!((img.width(), img.height()), (W, H));
        let (scale, lines) = fit_title(&font, "傑克倫敦短篇小說選一", 940.0, 3);
        assert!(lines.len() == 2 && lines.iter().all(|l| l.chars().count() == 5), "两行排匀: {lines:?} {scale:?}");
        assert_eq!(halfwidth("１３級階梯"), "13級階梯");
    }
}

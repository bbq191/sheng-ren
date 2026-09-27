//! 真实可阅读范围测量：生成一本"测量书"，在设备上对两张测试图各截一张屏，从截图里量出阅读器实际给图片的显示范围。
//!
//! 测量书里两张纯黑的大图，都比任何目标屏幕大，阅读器会等比缩小到能放下为止：
//! - **竖长图**（宽:高 = 1:4）先碰到高度上限 → 截图里黑块的高 = 可用高度；
//! - **横宽图**（4:1）先碰到宽度上限 → 黑块的宽 = 可用宽度。
//!
//! 截图是设备原始分辨率，黑块取截图里**最大的一块连通暗色区域**（状态栏文字、进度条都是零碎的小块）。
//! 结果就是 profile 里 `[readable.<格式>]` 要填的数（像素，竖屏）。阅读器的页边距等设置会影响结果，测之前先调成平时用的样子。

use image::{GrayImage, Luma};

/// 竖长图、横宽图的尺寸（比所有目标屏幕都大，保证被缩小而不是被放大）。
pub const TALL: (u32, u32) = (800, 3200);
pub const WIDE: (u32, u32) = (3200, 800);

/// 暗色阈值（灰度 < 这个值算黑块）。
const DARK: u8 = 80;

fn black_png(w: u32, h: u32) -> Result<Vec<u8>, String> {
    let img = GrayImage::from_pixel(w, h, Luma([0]));
    let mut out = Vec::new();
    image::DynamicImage::ImageLuma8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out)
}

const GUIDE: &str = "<h1>测量书</h1>\
<p>用来测量这台阅读器实际能用来显示图片的范围，结果用于按设备优化漫画和插图。</p>\
<ol>\
<li>先把页边距、字号、行距等设置调成你平时看书用的样子（页边距对结果影响最大），记下是什么设置。</li>\
<li>翻到下一页（竖长黑块），截一张屏；再翻到下一页（横宽黑块），再截一张屏。</li>\
<li>截屏方法：Kindle 同时按住屏幕左上角和右下角（或右上角和左下角），屏幕闪一下即成功，PNG 存在设备根目录；其他设备用自带的截屏功能。</li>\
<li>把两张截图拷到电脑上，运行 <code>readable-measure 竖长图截图.png 横宽图截图.png</code>，或者交给开发者。</li>\
</ol>";

/// 生成测量书（EPUB）：说明页 + 竖长黑图页 + 横宽黑图页，每页一个文件（各自从新的一页开始）。
pub fn probe_epub() -> Result<Vec<u8>, String> {
    use crate::epub::{Book, BookMeta, Chapter, Resource};
    let page = |title: &str, body: &str| Chapter { title: title.into(), html_body: body.into(), level: 1 };
    let mut book = Book {
        meta: BookMeta {
            book_id: "readable-probe".into(),
            title: "测量书（可阅读范围）".into(),
            author: "".into(),
            language: "zh".into(),
            publisher: "".into(),
            cover: None,
            cover_ext: "png".into(),
            cover_media_type: "image/png".into(),
        },
        chapters: vec![
            page("说明", GUIDE),
            page("竖长黑块", r#"<div><img src="images/tall.png" alt="竖长黑块"/></div>"#),
            page("横宽黑块", r#"<div><img src="images/wide.png" alt="横宽黑块"/></div>"#),
        ],
        resources: vec![
            Resource { path: "images/tall.png".into(), media_type: "image/png".into(), bytes: black_png(TALL.0, TALL.1)? },
            Resource { path: "images/wide.png".into(), media_type: "image/png".into(), bytes: black_png(WIDE.0, WIDE.1)? },
        ],
        nav: Vec::new(),
    };
    crate::epub::assemble(&mut book)
}

/// 截图里最大一块连通暗色区域的外框 `(x0, y0, x1, y1)`（右下不含）。没有暗色像素时 `None`。
pub fn largest_dark_box(img: &GrayImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = img.dimensions();
    let (wu, hu) = (w as usize, h as usize);
    let dark: Vec<bool> = img.pixels().map(|p| p.0[0] < DARK).collect();
    let mut seen = vec![false; wu * hu];
    let mut best: Option<(usize, (usize, usize, usize, usize))> = None;
    let mut stack = Vec::new();
    for start in 0..wu * hu {
        if !dark[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut n, mut x0, mut y0, mut x1, mut y1) = (0usize, usize::MAX, usize::MAX, 0usize, 0usize);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % wu, i / wu);
            n += 1;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            let mut push = |j: usize| {
                if dark[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < wu {
                push(i + 1);
            }
            if y > 0 {
                push(i - wu);
            }
            if y + 1 < hu {
                push(i + wu);
            }
        }
        if best.is_none_or(|(bn, _)| n > bn) {
            best = Some((n, (x0, y0, x1 + 1, y1 + 1)));
        }
    }
    best.map(|(_, (x0, y0, x1, y1))| (x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

/// 一次测量的结果（像素）。
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// 截图（屏幕）尺寸。
    pub screen: (u32, u32),
    /// 可用宽、高。
    pub readable: (u32, u32),
    /// 竖长图、横宽图在截图里的外框 `(x0, y0, x1, y1)`。
    pub tall_box: (u32, u32, u32, u32),
    pub wide_box: (u32, u32, u32, u32),
    /// 发现的可疑之处（黑块比例不对等），为空才可直接采用。
    pub warnings: Vec<String>,
}

/// 从竖长图、横宽图两张截图量出可阅读范围。
pub fn measure(tall: &GrayImage, wide: &GrayImage) -> Result<Measurement, String> {
    if tall.dimensions() != wide.dimensions() {
        return Err(format!("两张截图尺寸不同：{:?} 和 {:?}，应该来自同一台设备、同一方向", tall.dimensions(), wide.dimensions()));
    }
    let tb = largest_dark_box(tall).ok_or("竖长图截图里没找到黑块")?;
    let wb = largest_dark_box(wide).ok_or("横宽图截图里没找到黑块")?;
    let (tw, th) = (tb.2 - tb.0, tb.3 - tb.1);
    let (ww, wh) = (wb.2 - wb.0, wb.3 - wb.1);
    let mut warnings = Vec::new();
    // 等比缩放时黑块比例应与原图一致（1:4 / 4:1），差太多说明被裁切或拉伸，或者截错了页。
    let ratio_ok = |a: u32, b: u32| (b as f64 / a as f64 - 4.0).abs() <= 4.0 * 0.03;
    if !ratio_ok(tw, th) {
        warnings.push(format!("竖长黑块是 {tw}×{th}，不是 1:4，可能被裁切/拉伸，或截错了页"));
    }
    if !ratio_ok(wh, ww) {
        warnings.push(format!("横宽黑块是 {ww}×{wh}，不是 4:1，可能被裁切/拉伸，或截错了页"));
    }
    let (sw, sh) = tall.dimensions();
    if sw > sh {
        warnings.push(format!("截图是横屏（{sw}×{sh}），profile 里按竖屏填"));
    }
    Ok(Measurement { screen: (sw, sh), readable: (ww, th), tall_box: tb, wide_box: wb, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模拟一张截图：白底，指定位置一块黑，再加几行"状态栏文字"（零碎小黑块）。
    fn shot(sw: u32, sh: u32, bx: (u32, u32, u32, u32)) -> GrayImage {
        let mut img = GrayImage::from_pixel(sw, sh, Luma([255]));
        for y in bx.1..bx.3 {
            for x in bx.0..bx.2 {
                img.put_pixel(x, y, Luma([10]));
            }
        }
        for i in 0..20 {
            for y in 5..15 {
                for x in (30 + i * 12)..(36 + i * 12) {
                    img.put_pixel(x, y, Luma([0]));
                }
            }
        }
        img
    }

    #[test]
    fn measures_width_from_wide_and_height_from_tall() {
        // 1264×1680 的屏，阅读范围 1180×1560（左右各 42、上 60 下 60）
        let tall = shot(1264, 1680, (437, 60, 827, 1620)); // 390×1560
        let wide = shot(1264, 1680, (42, 693, 1222, 988)); // 1180×295
        let m = measure(&tall, &wide).unwrap();
        assert_eq!(m.readable, (1180, 1560));
        assert_eq!(m.screen, (1264, 1680));
        assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    }

    #[test]
    fn warns_when_block_ratio_is_off() {
        let tall = shot(1264, 1680, (100, 60, 1100, 1620)); // 被拉宽了
        let wide = shot(1264, 1680, (42, 693, 1222, 988));
        assert_eq!(measure(&tall, &wide).unwrap().warnings.len(), 1);
        assert!(measure(&tall, &shot(1000, 1680, (0, 0, 10, 10))).is_err(), "尺寸不同的截图拒绝");
    }

    #[test]
    fn probe_book_has_both_images_in_separate_pages() {
        let entries = crate::epubzip::read_entries(&probe_epub().unwrap()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"OEBPS/images/tall.png") && names.contains(&"OEBPS/images/wide.png"), "{names:?}");
        let pages = entries.iter().filter(|e| e.name.ends_with(".xhtml") && String::from_utf8_lossy(&e.data).contains("<img")).count();
        assert_eq!(pages, 2);
    }
}

//! 横线图与包框表格（拆分自 `box_fix.rs`）：横线图的画法与认法、包框用的单格表格开头。

/// 横线图的宽（像素）：显示时宽 100%，高按比例。
const RULE_W: u32 = 1000;

/// 包进单格表格的块：(元素, 外层 div 的类规则, 单元格的行内 `style`, 窄的定宽块的表格宽百分比和靠哪边)。
pub(super) type Frame = (usize, String, String, Option<(String, &'static str)>);

/// 横线怎么画。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum LineStyle {
    Solid,
    Dotted,
    Dashed,
}

/// 一张横线图：颜色（ARGB）、样式、线粗、连着的内边距、显示宽（都是屏幕像素）、线在上还是在下。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Line {
    pub(super) color: u32,
    pub(super) style: LineStyle,
    pub(super) border_px: f64,
    pub(super) pad_px: f64,
    pub(super) width_px: f64,
    pub(super) top: bool,
}

impl Line {
    /// 图的高（像素，宽 [`RULE_W`]）和线占的行数。
    fn rows(&self) -> (u32, u32) {
        let scale = f64::from(RULE_W) / self.width_px;
        let line = (self.border_px * scale).round().max(1.0) as u32;
        let pad = (self.pad_px * scale).round().max(0.0) as u32;
        (line + pad, line)
    }

    /// 同样的图合用一个文件：按画出来的像素认。
    pub(super) fn key(&self) -> LineKey {
        let (h, l) = self.rows();
        (self.color, self.style as u8, h, l, self.top)
    }

    pub(super) fn png(&self) -> Vec<u8> {
        use image::ImageEncoder;
        let (h, line) = self.rows();
        let (a, r, g, b) = (
            (self.color >> 24) as u8,
            (self.color >> 16) as u8,
            (self.color >> 8) as u8,
            self.color as u8,
        );
        let mut px = vec![0u8; (RULE_W * h * 4) as usize];
        let rows = if self.top { 0..line } else { h - line..h };
        // 点线、虚线：按线粗算一段多长（点：粗 1 段、空 1 段；虚线：3 段实、2 段空）
        let unit = line.max(2);
        for y in rows {
            for x in 0..RULE_W {
                let on = match self.style {
                    LineStyle::Solid => true,
                    LineStyle::Dotted => (x / unit) % 2 == 0,
                    LineStyle::Dashed => (x / unit) % 5 < 3,
                };
                if on {
                    let i = ((y * RULE_W + x) * 4) as usize;
                    px[i..i + 4].copy_from_slice(&[r, g, b, a]);
                }
            }
        }
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(&px, RULE_W, h, image::ExtendedColorType::Rgba8)
            .expect("PNG 编码");
        out
    }
}

/// 横线图的认法：(颜色, 样式, 图高, 线占的行数, 线在上)。
pub(super) type LineKey = (u32, u8, u32, u32, bool);

/// 框住块的单格表格。xochitl 普通块上的边框不画（外链、行内都不画），表格、单元格**行内** `style` 写的边框画（分边、点线虚线、颜色
/// 照写），外链、类规则里给表格写的不认；单元格里块的外边距、背景色不认；表格行内的外边距、`width` 属性会和书里 `table{margin:0 auto}`
/// 这类规则搅在一起（2026-10-13 测试书「测试-竖线」1–10、《雪国》制作说明页）。所以框画在单元格的行内，表格满宽，外面再包一层 div
/// 管左右外边距（`eink-box.css` 的类规则，普通块的外边距认），块的内边距写成单元格的。
pub(in crate::wash) fn frame_open(class: usize, td_style: &str, table: &Option<(String, &'static str)>) -> String {
    // 窄的定宽块（卷名竖排）：表格宽写占外层的百分比、靠哪边写 `align`——百分比相对页宽、不随阅读字号变（em 外边距会，2026-10-15 真机
    // 字号 0.8：《梁羽生》卷名框变宽、一行 4 个字不再竖排）；表格不会窄过里面最宽的一个字，一字宽的竖条写多小都是一字宽
    let (width, align) = match table {
        Some((pct, align)) => (format!("{pct}%"), format!(r#" align="{align}""#)),
        None => ("100%".to_string(), String::new()),
    };
    format!(
        r#"<div class="eink-frame eink-b{class}"><table class="eink-frame" cellspacing="0" width="{width}"{align}><tbody><tr><td style="{td_style}">"#
    )
}
pub(super) const FRAME_CLOSE: &str = "</td></tr></tbody></table></div>";

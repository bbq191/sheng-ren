//! 阅读器怎么排（设备模型）：照 Kindle 改写的 pass 拿 [`crate::kindle_model`] 算出 Kindle 上的样子，再按这里算出这台阅读器拿到
//! 同样的 CSS 会排成什么样，不一样的才改写（写成这台认的写法）。
//!
//! 内容全从阅读模式来（profile 里认不认某种 CSS 的字段，加上测试书量出来的尺寸 `[heading_sizes]`、`[page_top]`），算法里不写
//! 设备名、不写死数字。完全照 CSS 排的阅读器是 [`ReaderModel::default`]，什么都不用改。2026-10-13 从各个 pass 里收拢。

/// 这台阅读器和 CSS 不一样的地方。
#[derive(Clone, Debug, PartialEq)]
pub struct ReaderModel {
    /// 认 `display:none`（不认的：藏起来的元素露出来，要去掉）。
    pub display_none: bool,
    /// 块末尾的 `<br/>` 多排一行（Kindle 不排）。
    pub trailing_br_line: bool,
    /// `<p>` 不认外链、`<style>` 里的 `line-height`（行内的认），别的块按阅读器缺省行距的倍数认（[`ReaderModel::line_height`]）。
    pub line_height_except_p: bool,
    /// 插了下边框横线图的块上行距倍数的下限（低了字伸出行盒、压住横线，见 profile `line_height_floor`）。
    pub line_height_floor: Option<f64>,
    /// 标题（h1–h6）的 `font-size` 只认关键字（`None`＝照 CSS）。
    pub heading_sizes: Option<HeadingSizes>,
    /// 不认 `margin-top`，页首要垫图（`None`＝认）。
    pub page_top: Option<profile::PageTop>,
    /// 行内 `style`、`<style>`、规则最后一条不带分号又紧挨 `}` 的声明里 em 照 CSS 排（`false`：要挪进外链样式表）。
    pub inline_css_lengths: bool,
    /// 认内边距（`false`：并进外边距、折进相邻块的间距）。
    pub padding: bool,
    /// 画边框（`false`：上下边框插横线图）。
    pub borders: bool,
    /// 认标题的上下外边距（`false`：写到相邻的块上）。
    pub heading_margins: bool,
    /// 左右外边距的 px 和 Kindle 一样大（`false`：折成 em）。
    pub px_margins: bool,
    /// 左右外边距的 % 按包含块宽（`false`：折成 em）。
    pub percent_margins: bool,
    /// 认块的 `width`（`false`：折成左右外边距）。
    pub block_width: bool,
    /// 认负的外边距（`false`：页中间相邻两块的间距照 Kindle 写成非负的）。
    pub negative_margins: bool,
    /// 单元格缺省竖直居中（`false`：Kindle 上居中的单元格写 `vertical-align:middle`）。
    pub cell_middle: bool,
    /// 认行内元素上 `vertical-align` 的上标、下标（`false`：里面包 `<sup>`/`<sub>`）。
    pub inline_valign: bool,
    /// 按单元格的 CSS 宽度把格子压窄（`true`：列宽百分比写进行内 `style`；`false`：写成 `width` 属性）。
    pub cell_width: bool,
    /// 按 `margin:auto` 让定宽块居中（`true`：Kindle 不，auto 写成 0）。
    pub margin_auto: bool,
    /// 认 `float`（`true`：Kindle 不浮动，标题上的写成 `none`）。
    pub float: bool,
    /// 左右边距的 em 照 Kindle 按页宽换算（`true`：乘这台一行的 em 数 ÷ 32）。
    pub horizontal_scale: bool,
    /// em 一律按根字号算（`true`：字号、边距、缩进的 em 换成按根的值，`wash/root_em.rs`）。
    pub em_root: bool,
    /// [`ReaderModel::em_root`] 的阅读器上 1em 是几个屏幕像素（字号写的 px 是屏幕像素，`wash/root_em.rs` 按它折）。
    pub em_root_px: Option<f64>,
    /// 标题自己没写对齐时居中（`true`：照 Kindle 继承来的写明，`wash/block_style.rs`）。
    pub heading_center: bool,
    /// 认 `rem`（`false`：改成 em，字号的折成 px）。
    pub rem: bool,
    /// 认 `!important`（`false`：带它的声明不起作用，要去掉、胜出的值写进行内）。
    pub important: bool,
    /// 不认不带单位的 `line-height`、只认长度的阅读器上，缺省行距是 em 的几倍（`None`＝照 CSS；见 [`ReaderModel::line_height_len`]）。
    pub line_em: Option<f64>,
}

impl Default for ReaderModel {
    fn default() -> Self {
        ReaderModel { display_none: true, trailing_br_line: false, line_height_except_p: false, line_height_floor: None, heading_sizes: None, page_top: None, inline_css_lengths: true, padding: true, borders: true, heading_margins: true, px_margins: true, percent_margins: true, block_width: true, negative_margins: true, cell_middle: true, inline_valign: true, cell_width: true, margin_auto: true, float: true, horizontal_scale: false, em_root: false, em_root_px: None, heading_center: false, rem: true, important: true, line_em: None }
    }
}

impl ReaderModel {
    pub fn for_profile(p: &profile::Profile) -> ReaderModel {
        ReaderModel {
            display_none: p.css.display_none,
            trailing_br_line: p.css.trailing_br_line,
            line_height_except_p: p.css.line_height_except_p,
            line_height_floor: p.css.line_height_floor.filter(|_| p.css.line_height_except_p),
            heading_sizes: p.css.heading_sizes.as_ref().filter(|_| p.css.heading_font_keywords).map(HeadingSizes::from_profile),
            page_top: p.css.page_top.filter(|_| !p.css.margin_top),
            inline_css_lengths: p.css.inline_css_lengths,
            padding: p.css.padding,
            borders: p.css.borders,
            heading_margins: p.css.heading_margins,
            px_margins: p.css.px_margins,
            percent_margins: p.css.percent_margins,
            block_width: p.css.block_width,
            negative_margins: p.css.negative_margins,
            cell_middle: p.css.cell_middle,
            inline_valign: p.css.inline_valign,
            cell_width: p.css.cell_width,
            margin_auto: p.css.margin_auto,
            float: p.css.float,
            horizontal_scale: p.css.horizontal_scale,
            em_root: p.css.em_root,
            em_root_px: p.em_px.filter(|_| p.css.em_root),
            heading_center: p.css.heading_center,
            rem: p.css.rem,
            important: p.css.important,
            line_em: (!p.css.line_height_numbers).then(|| f64::from(p.line_px.unwrap_or(0)) / p.em_px.unwrap_or(1.0)),
        }
    }

    /// 进指纹用的写法（字段和量出来的数都在里面，同样的模型同样的字）；完全照 CSS 排的（[`ReaderModel::default`]）是空串。
    pub fn signature(&self) -> String {
        if *self == ReaderModel::default() {
            return String::new();
        }
        let b = |v: bool| u8::from(v);
        let mut s = format!("d{}b{}l{}", b(self.display_none), b(self.trailing_br_line), b(self.line_height_except_p));
        if let Some(h) = &self.heading_sizes {
            let kws: Vec<String> = h.keywords.iter().map(|(k, v)| format!("{k}={v}")).collect();
            let tags: Vec<String> = h.tags.iter().map(f64::to_string).collect();
            s.push_str(&format!("h{};{}", kws.join(","), tags.join(",")));
        }
        if let Some(t) = &self.page_top {
            s.push_str(&format!("t{},{},{}", t.gap_heading_per_em, t.gap_other, t.strut_base_per_em));
        }
        if !self.inline_css_lengths {
            s.push_str("i0");
        }
        for (on, c) in [(self.padding, 'p'), (self.borders, 'r'), (self.heading_margins, 'm'), (self.px_margins, 'x'), (self.percent_margins, '%'), (self.block_width, 'w'), (self.negative_margins, 'g'), (self.cell_middle, 'v'), (self.inline_valign, 'u'), (self.cell_width, 'c'), (self.margin_auto, 'a'), (self.float, 'o'), (self.rem, 'q'), (self.important, 'k')] {
            if !on {
                s.push(c);
                s.push('0');
            }
        }
        if self.horizontal_scale {
            s.push_str("h1");
        }
        if self.heading_center {
            s.push_str("j1");
        }
        if let Some(f) = self.line_height_floor {
            s.push_str(&format!("f{f}"));
        }
        if self.em_root {
            s.push_str(&format!("e{}", self.em_root_px.unwrap_or(0.0)));
        }
        if let Some(l) = self.line_em {
            s.push_str(&format!("n{}", (l * 10000.0).round() / 10000.0));
        }
        s
    }

    /// 块在这台上的行距（阅读器缺省行距的倍数）：`lh` 是层叠出来的行高（元素字号的倍数，`None`＝没写或 `normal`，算 1）。
    /// [`ReaderModel::line_height_except_p`] 的阅读器上 `<p>` 只认行内 `style` 写的（`inline`：生效的那条写在行内），别的是 1。
    pub fn line_height(&self, tag: &str, lh: Option<f64>, inline: bool) -> f64 {
        if self.line_height_except_p && tag == "p" && !inline {
            1.0
        } else {
            lh.unwrap_or(1.0)
        }
    }

    /// 块在这台上的字号（根 em）：标题按 [`HeadingSizes`]（`decl` 是层叠出来的 `font-size` 声明），别的块照算出来的 `computed`。
    pub fn font_size(&self, tag: &str, decl: Option<&str>, computed: f64) -> f64 {
        match &self.heading_sizes {
            Some(h) => h.on_reader(tag, decl).unwrap_or(computed),
            None => computed,
        }
    }
}

/// 标题只认关键字的阅读器上标题的字号（正文 1em＝1）。
#[derive(Clone, Debug, PartialEq)]
pub struct HeadingSizes {
    /// 认的关键字和大小，按大小从小到大。
    keywords: Vec<(String, f64)>,
    /// h1–h6 按标签固定的大小。
    tags: [f64; 6],
}

impl HeadingSizes {
    fn from_profile(h: &profile::HeadingSizes) -> HeadingSizes {
        let mut keywords: Vec<(String, f64)> = h.keywords.iter().map(|(k, v)| (k.to_ascii_lowercase(), *v)).collect();
        keywords.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        HeadingSizes { keywords, tags: h.tags }
    }

    /// 关键字的大小（不认的 `None`）；`!important` 不管。
    pub fn keyword_size(&self, v: &str) -> Option<f64> {
        let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
        self.keywords.iter().find(|(k, _)| *k == v).map(|(_, s)| *s)
    }

    /// 标签固定的大小（不是标题的 `None`）。
    pub fn tag_size(&self, tag: &str) -> Option<f64> {
        let n = ["h1", "h2", "h3", "h4", "h5", "h6"].iter().position(|h| *h == tag)?;
        Some(self.tags[n])
    }

    /// 标题在这台上的字号：写了认的关键字照写的，没写按标签（不是标题的 `None`）。
    pub fn on_reader(&self, tag: &str, decl: Option<&str>) -> Option<f64> {
        let def = self.tag_size(tag)?;
        Some(decl.and_then(|v| self.keyword_size(v)).unwrap_or(def))
    }

    /// 和 `want` 比例最接近的关键字。
    pub fn nearest(&self, want: f64) -> (&str, f64) {
        let (k, s) = self.keywords.iter().min_by(|a, b| (a.1 / want).ln().abs().total_cmp(&(b.1 / want).ln().abs())).expect("profile 校验过 keywords 非空");
        (k.as_str(), *s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_profile_model() {
        let m = ReaderModel::for_profile(profile::fixture("xochitl").unwrap());
        assert!(!m.display_none && m.trailing_br_line && m.line_height_except_p && m.page_top.is_some());
        let h = m.heading_sizes.as_ref().unwrap();
        assert_eq!((h.keyword_size("x-large !important"), h.keyword_size("smaller")), (Some(1.514), None));
        assert_eq!((h.tag_size("h6"), h.tag_size("h7"), h.tag_size("p")), (Some(1.0), None, None));
        assert_eq!(h.nearest(2.0).0, "xx-large");
        assert_eq!(m.font_size("h2", Some("2em"), 2.0), 1.514);
        assert_eq!(m.font_size("div", None, 2.0), 2.0);
        assert_eq!((m.line_height("p", Some(2.0), false), m.line_height("p", Some(2.0), true), m.line_height("h1", Some(2.0), false), m.line_height("div", None, false)), (1.0, 2.0, 2.0, 1.0));
        assert_eq!(ReaderModel::for_profile(profile::get("ireader").unwrap()), ReaderModel { px_margins: false, rem: false, important: false, cell_middle: false, inline_valign: false, heading_center: true, line_em: Some(58.0 / 37.5), ..ReaderModel::default() });
        assert_eq!(ReaderModel::default().signature(), "");
        assert_eq!(
            m.signature(),
            "d0b1l1hsmall=0.784,medium=1,large=1.216,x-large=1.514,xx-large=2.018;2.018,1.514,1.216,1,0.784,1t91,46,44i0p0r0m0x0%0w0g0v0c0o0h1f1e37.8"
        );
    }
}

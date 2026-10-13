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
    /// 只有 `<p>` 不认 `line-height`，别的块按阅读器缺省行距的倍数认（[`ReaderModel::line_height`]）。
    pub line_height_except_p: bool,
    /// 标题（h1–h6）的 `font-size` 只认关键字（`None`＝照 CSS）。
    pub heading_sizes: Option<HeadingSizes>,
    /// 不认 `margin-top`，页首要垫图（`None`＝认）。
    pub page_top: Option<profile::PageTop>,
}

impl Default for ReaderModel {
    fn default() -> Self {
        ReaderModel { display_none: true, trailing_br_line: false, line_height_except_p: false, heading_sizes: None, page_top: None }
    }
}

impl ReaderModel {
    pub fn for_profile(p: &profile::Profile) -> ReaderModel {
        ReaderModel {
            display_none: p.display_none,
            trailing_br_line: p.trailing_br_line,
            line_height_except_p: p.line_height_except_p,
            heading_sizes: p.heading_sizes.as_ref().filter(|_| p.heading_font_keywords).map(HeadingSizes::from_profile),
            page_top: p.page_top.filter(|_| !p.margin_top),
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
        s
    }

    /// 块在这台上的行距（阅读器缺省行距的倍数）：`lh` 是层叠出来的行高（元素字号的倍数，`None`＝没写或 `normal`，算 1）。
    /// [`ReaderModel::line_height_except_p`] 的阅读器上 `<p>` 不认（总是 1）。
    pub fn line_height(&self, tag: &str, lh: Option<f64>) -> f64 {
        if self.line_height_except_p && tag == "p" {
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
        let m = ReaderModel::for_profile(profile::get("xochitl").unwrap());
        assert!(!m.display_none && m.trailing_br_line && m.line_height_except_p && m.page_top.is_some());
        let h = m.heading_sizes.as_ref().unwrap();
        assert_eq!((h.keyword_size("x-large !important"), h.keyword_size("smaller")), (Some(1.514), None));
        assert_eq!((h.tag_size("h6"), h.tag_size("h7"), h.tag_size("p")), (Some(1.0), None, None));
        assert_eq!(h.nearest(2.0).0, "xx-large");
        assert_eq!(m.font_size("h2", Some("2em"), 2.0), 1.514);
        assert_eq!(m.font_size("div", None, 2.0), 2.0);
        assert_eq!((m.line_height("p", Some(2.0)), m.line_height("h1", Some(2.0)), m.line_height("div", None)), (1.0, 2.0, 1.0));
        assert_eq!(ReaderModel::for_profile(profile::get("ireader").unwrap()), ReaderModel { display_none: true, ..ReaderModel::default() });
        assert_eq!(ReaderModel::default().signature(), "");
        assert_eq!(
            m.signature(),
            "d0b1l1hsmall=0.784,medium=1,large=1.216,x-large=1.514,xx-large=2.018;2.018,1.514,1.216,1,0.784,1t91,46,44"
        );
    }
}

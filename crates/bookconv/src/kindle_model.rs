//! Kindle 上排出来的样子（Send to Kindle 的规则）：KFX 写出器和掌阅、Move 的 `kindle_rules` 系列共用的一份。
//!
//! - 常量：没写行高时的倍数、行高下限、百分比折算的页宽、空段折叠系数；
//! - 全书正文的字号、行距、字体怎么取（按字数加权的众数），行高怎么按正文归一；
//! - 竖直长度换成根 em；
//! - 一章的元素表 [`Chapter`]：层叠（样式表、`style`、表现属性）只在这里写一遍，`<body>` 里的元素按文档序编号，和标签扫描的
//!   开始标签一一对应（对得上才知道改源码里的哪个标签）。
//!
//! 2026-10-13 收拢：以前写出器和清洗层的 7 个 pass 各写一份常量、各自解析样式表、各走一遍 DOM，写出器改了规则两边会悄悄对不上。
use crate::cascade::{Computed, Len, MediaEnv, Origin, Rules, Sheet};
use crate::epubzip::Entry;
use ego_tree::NodeId;
use scraper::{ElementRef, Html, Node};
use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub use crate::cascade::{body_font_size, weighted_mode, PT_PER_EM};

/// 没写 `line-height`（或 `normal`）时当作的倍数；也是 KFX `lh` 单位折合的 em（样本里文档缺省行高 1.2em）。
pub const LH_EM: f64 = 1.2;

/// 行高（正文行高的倍数）的下限（Send to Kindle 同样）。
pub const MIN_LINE_HEIGHT: f64 = 0.6;

/// 百分比换算成 em 时假定的页宽（样本里 `margin-top:30%` → 9.6em）。
pub const PAGE_WIDTH_EM: f64 = 32.0;

/// 只有空白的段落（`<p>&nbsp;</p>`、`<p>　</p>`、`<p><br/></p>`、段落之间单独的 `<br/>`）不出节点，折成下一块上边距多出的
/// 本元素行高的这么多倍（Send to Kindle 同样，2026-10-08 量的：《绝叫》空段 0.594 行高、《金庸》`<p><br/></p>` 0.595 × 1.7em、
/// 段落间单独的 `<br/>` 0.72em ＝ 0.6 × 缺省行高 1.2em）。
pub const BLANK_LINE_FOLD: f64 = 0.6;

/// 统计用的键：千分之一取整。
pub fn key(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

/// 全书正文的行高（元素字号的倍数）：每段字按所在块的行高（没写按 [`LH_EM`]）计字数，取最多的。Send to Kindle 把它当成阅读器
/// 行距设置的 1.0（《绍宋》正文 `line-height:1.5em`）。算出来不在 1–3 之间的不信，用 [`LH_EM`]。
pub fn base_line_height(count: &HashMap<i64, usize>) -> f64 {
    weighted_mode(count).filter(|v| (1.0..=3.0).contains(v)).unwrap_or(LH_EM)
}

/// 元素在 Kindle 上的行高（正文行高的倍数）：本元素行高 ÷ 正文行高 `base`，不低于 [`MIN_LINE_HEIGHT`]。
pub fn line_height_ratio(lh: Option<f64>, base: f64) -> f64 {
    (lh.unwrap_or(LH_EM) / base).max(MIN_LINE_HEIGHT)
}

/// 正文字体：字体名（已经是小写，没写的 `None`）→ 字数里最多的；字数一样多的取名字小的，没写字体的排最前（结果和 `HashMap` 的
/// 遍历顺序无关）。
pub fn body_font(count: &HashMap<Option<String>, usize>) -> Option<String> {
    count.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).and_then(|(f, _)| f.clone())
}

/// 竖直长度换成根 em：em 乘本元素字号 `fs`（根 em），pt 按 [`PT_PER_EM`]，百分比按页宽 [`PAGE_WIDTH_EM`]，没写是 0。
pub fn vertical_rem(l: Option<Len>, fs: f64) -> f64 {
    match l {
        Some(Len::Em(n)) => n * fs,
        Some(Len::Pt(n)) => n / PT_PER_EM,
        Some(Len::Percent(p)) => p / 100.0 * PAGE_WIDTH_EM,
        None => 0.0,
    }
}

/// 相邻的上下外边距折叠（同 CSS：两个都正取大，都负取小，一正一负相加）。
pub fn collapse(a: f64, b: f64) -> f64 {
    if a >= 0.0 && b >= 0.0 {
        a.max(b)
    } else if a < 0.0 && b < 0.0 {
        a.min(b)
    } else {
        a + b
    }
}

/// 全书的样式表按阅读模式的 `media` 解析好（书内路径 → 规则）；不是 UTF-8 的按 Latin-1 读。
pub fn book_css<'e>(entries: &'e [Entry], media: Option<&MediaEnv>) -> HashMap<&'e str, Arc<Rules>> {
    entries
        .iter()
        .filter(|e| crate::wash::is_css_name(&e.name))
        .map(|e| {
            let text = match std::str::from_utf8(&e.data) {
                Ok(t) => Cow::Borrowed(t),
                Err(_) => Cow::Owned(crate::util::latin1_decode(&e.data)),
            };
            (e.name.as_str(), Rules::parse(&text, &e.name, media))
        })
        .collect()
}

/// 单元格内容的自然宽度（CSS 像素，1em＝16px）：一行排开的字宽（中日韩、全角 1em，别的半个），加 10px（Send to Kindle 的列宽
/// 照这个算出来对得上：「右格」两个字 42px，2026-10-14 怪癖测试书 Q127–Q129）。
pub fn natural_px(td: &ElementRef, fs: f64) -> f64 {
    let em: f64 = td.text().flat_map(str::chars).filter(|c| !c.is_whitespace()).map(|c| if (c as u32) >= 0x2E80 { 1.0 } else { 0.5 }).sum();
    em * 16.0 * fs + 10.0
}

/// 表格的列宽（百分比，从第一列起、到第一个没写宽度的列为止）：第一行单元格写了的宽度换成 CSS 像素，除以写了的宽度加上没写的
/// 单元格的自然宽度（Send to Kindle 同样：`td{width:1em}` 配一格「右格」写 27.5862%＝16/(16+42)、`width="30"` 写 41.6667%＝30/(30+42)，
/// Q127–Q129；以前照写 em，Kindle 上那一格窄到一个字、字竖着排，Amazon 版横排）。写了百分比的照写。
pub fn column_percents(widths: &[Option<Len>], naturals: &[f64]) -> Vec<f64> {
    let px = |w: &Len, i: usize| match *w {
        Len::Em(n) => n * 16.0,
        Len::Pt(n) => n / 0.75,
        Len::Percent(_) => naturals.get(i).copied().unwrap_or(0.0),
    };
    let total: f64 = widths.iter().enumerate().map(|(i, w)| w.as_ref().map_or(naturals.get(i).copied().unwrap_or(0.0), |w| px(w, i))).sum();
    widths
        .iter()
        .enumerate()
        .map_while(|(i, w)| w.as_ref().map(|w| (i, *w)))
        .map(|(i, w)| match w {
            Len::Percent(p) => p,
            w if total > 0.0 => (px(&w, i) / total * 100.0 * 10000.0).round() / 10000.0,
            _ => 0.0,
        })
        .collect()
}

/// 标题的字号按阅读器改写成关键字时（Move 只认关键字，`wash/block_style.rs`），原来在 Kindle 上的字号（根 em）记在这个属性里：
/// 后面照 Kindle 算位置的 pass 按它算，不按关键字的 CSS 字号。
pub const KINDLE_FS_ATTR: &str = "data-eink-fs";

/// `<body>` 里的一个元素。
pub struct El {
    pub node: NodeId,
    /// 父元素在 [`Chapter::els`] 里的下标（父元素是 `<body>` 的 `None`）。
    pub parent: Option<usize>,
    /// 计算值。
    pub comp: Rc<Computed>,
    /// 层叠出来的声明（含 `style`、表现属性），给要看"写没写""写的什么"的用。
    pub decls: HashMap<String, String>,
    /// 生效的声明不是外链样式表里的普通声明的：属性 → (来源, 带没带 `!important`)。表现属性（`align` 之类）不在里面。
    pub origin: HashMap<String, (Origin, bool)>,
    /// 块级（[`crate::cascade::is_block`]）。
    pub is_block: bool,
    /// 最近的块（自己是块就是自己；外面没有块、直接在 `<body>` 里的 `None`）。
    pub block: Option<usize>,
    /// 自己或外面的元素 `display:none`。
    pub hidden: bool,
}

/// 一章：解析好的文档、`<body>` 的计算值、`<body>` 里的元素表（文档序）。
pub struct Chapter {
    html: Html,
    body_id: NodeId,
    pub body_comp: Rc<Computed>,
    /// `<body>` 上层叠出来的声明。
    pub body_decls: HashMap<String, String>,
    pub els: Vec<El>,
    index: HashMap<NodeId, usize>,
    /// 元素数和源码 `<body>` 里开始标签的扫描一致（解析器没补出 `tbody` 之类）：第 n 个元素就是第 n 个开始标签，改源码要先看这个。
    pub matches_tags: bool,
}

impl Chapter {
    /// 解析一章。`css` 是 [`book_css`] 给的全书样式表，`name` 是这章的书内路径（解析相对链接、找样式表用）。
    pub fn parse(name: &str, text: &str, media: Option<&MediaEnv>, css: &HashMap<&str, Arc<Rules>>) -> Option<Chapter> {
        let html = Html::parse_document(text);
        let sheet = Sheet::for_doc(&html, name, media, |path| css.get(path).cloned());
        let decls_of = |el: &ElementRef| {
            let mut d = sheet.cascade(el, name);
            crate::cascade::presentational_hints(el, &mut d);
            d
        };
        let with_origin = |el: &ElementRef| {
            let mut origin = HashMap::new();
            let mut d: HashMap<String, String> = sheet
                .cascade_with_origin(el, name)
                .into_iter()
                .map(|(k, (v, o, imp))| {
                    if o != Origin::Sheet || imp {
                        origin.insert(k.clone(), (o, imp));
                    }
                    (k, v)
                })
                .collect();
            crate::cascade::presentational_hints(el, &mut d);
            (d, origin)
        };
        let root = html.root_element();
        let body = root.children().filter_map(ElementRef::wrap).find(|el| el.value().name() == "body")?;
        let rc = Computed::derive(&Computed::root(), &decls_of(&root), root.value().name());
        let body_decls = decls_of(&body);
        let bc = Rc::new(Computed::derive(&rc, &body_decls, "body"));
        let mut els: Vec<El> = Vec::new();
        // 不递归（没关的标签能套上千层）：(节点, 父元素的计算值, 父元素下标, 父元素所在的块, 在不在隐藏的元素里)
        type Frame<'a> = (ego_tree::NodeRef<'a, Node>, Rc<Computed>, Option<usize>, Option<usize>, bool);
        let mut stack: Vec<Frame> = body.children().rev().map(|c| (c, bc.clone(), None, None, false)).collect();
        while let Some((n, parent, pi, block, hidden)) = stack.pop() {
            let Some(el) = ElementRef::wrap(n) else { continue };
            let (decls, origin) = with_origin(&el);
            let mut c = Computed::derive(&parent, &decls, el.value().name());
            // 照这台改过字号的标题：Kindle 上的字号（见 [`KINDLE_FS_ATTR`]）
            if let Some(fs) = el.value().attr(KINDLE_FS_ATTR).and_then(|v| v.parse::<f64>().ok()) {
                c.font_size = fs;
            }
            let c = Rc::new(c);
            let hidden = hidden || c.display.as_deref() == Some("none");
            let is_block = crate::cascade::is_block(&el, &c);
            let me = els.len();
            let block = if is_block { Some(me) } else { block };
            els.push(El { node: n.id(), parent: pi, comp: c.clone(), decls, origin, is_block, block, hidden });
            for ch in n.children().rev() {
                stack.push((ch, c.clone(), Some(me), block, hidden));
            }
        }
        let matches_tags = crate::html::body_range(text).is_some_and(|(bs, be)| crate::html::tags_in(text, bs, be).filter(|t| t.is_start()).count() == els.len());
        let index = els.iter().enumerate().map(|(i, e)| (e.node, i)).collect();
        let body_id = body.id();
        drop(sheet);
        Some(Chapter { html, body_id, body_comp: bc, body_decls, els, index, matches_tags })
    }

    /// 第 `i` 个元素。
    pub fn el(&self, i: usize) -> ElementRef<'_> {
        ElementRef::wrap(self.html.tree.get(self.els[i].node).expect("元素表里的节点")).expect("元素表里只有元素")
    }

    pub fn body(&self) -> ElementRef<'_> {
        ElementRef::wrap(self.html.tree.get(self.body_id).expect("body")).expect("body 是元素")
    }

    /// 节点在元素表里的下标（不在 `<body>` 里的 `None`）。
    pub fn index_of(&self, id: NodeId) -> Option<usize> {
        self.index.get(&id).copied()
    }

    /// 节点的计算值（`<body>` 自己也给）。
    pub fn comp_of(&self, id: NodeId) -> Option<&Rc<Computed>> {
        if id == self.body_id {
            return Some(&self.body_comp);
        }
        self.index_of(id).map(|i| &self.els[i].comp)
    }

    /// 第 `i` 个元素的子元素里是块的（下标，文档序）。
    pub fn block_children(&self, i: usize) -> Vec<usize> {
        self.el(i).children().filter_map(ElementRef::wrap).filter_map(|c| self.index_of(c.id())).filter(|&j| self.els[j].is_block).collect()
    }

    /// 块 `i` 在 Kindle 上的上（`top`）/ 下外边距（根 em）：连同第一个（最后一个）子块，中间没有内边距、边框隔着的，相邻的折叠
    /// （[`collapse`]）。有负的 `None`（拿不准）。
    pub fn margin_edge(&self, i: usize, top: bool) -> Option<f64> {
        let (m, p) = if top { (0, 0) } else { (2, 2) };
        let mut cur = i;
        let mut acc = 0.0f64;
        loop {
            let c = &self.els[cur].comp;
            let v = vertical_rem(c.margin[m], c.font_size);
            if v < 0.0 {
                return None;
            }
            acc = collapse(acc, v);
            if vertical_rem(c.padding[p], c.font_size) > 0.0 || c.border[p].is_some() {
                return Some(acc);
            }
            let kids = self.block_children(cur);
            match if top { kids.first() } else { kids.last() } {
                Some(&k) => cur = k,
                None => return Some(acc),
            }
        }
    }

    /// 元素直接含的字数（不算空白）：只数自己的文字子节点。
    pub fn own_chars(&self, i: usize) -> usize {
        self.el(i).children().filter_map(|c| if let Node::Text(t) = c.value() { Some(t.chars().filter(|c| !c.is_whitespace()).count()) } else { None }).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(body: &str, css: &str) -> Chapter {
        let entries = vec![Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() }];
        let rules = book_css(&entries, None);
        let text = format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#);
        Chapter::parse("O/c.xhtml", &text, None, &rules).unwrap()
    }

    /// 文档序编号、父元素、最近的块、隐藏、写了的声明；`tbody` 是解析器补的，对不上标签扫描。
    #[test]
    fn elements_in_document_order() {
        let c = chapter(r#"<div class="h"><p>一<span>二</span></p></div><h1 style="line-height:2">三</h1>"#, ".h{display:none}");
        let names: Vec<&str> = (0..c.els.len()).map(|i| c.el(i).value().name()).collect();
        assert_eq!(names, ["div", "p", "span", "h1"]);
        assert_eq!(c.els[2].parent, Some(1));
        assert_eq!(c.els[2].block, Some(1));
        assert!(c.els[2].hidden && !c.els[3].hidden);
        assert_eq!(c.els[3].decls.get("line-height").map(String::as_str), Some("2"));
        assert_eq!((c.els[3].origin.get("line-height"), c.els[0].origin.get("display")), (Some(&(Origin::Inline, false)), None));
        assert_eq!(c.own_chars(1), 1);
        assert!(c.matches_tags);
        assert!(!chapter("<table><tr><td>x</td></tr></table>", "").matches_tags);
    }

    #[test]
    fn kindle_rules() {
        assert_eq!(vertical_rem(Some(Len::Percent(50.0)), 2.0), 16.0);
        assert_eq!(vertical_rem(Some(Len::Em(2.0)), 1.5), 3.0);
        assert_eq!(vertical_rem(Some(Len::Pt(24.0)), 3.0), 2.0);
        assert_eq!(line_height_ratio(Some(0.5), 1.5), MIN_LINE_HEIGHT);
        assert_eq!(base_line_height(&HashMap::from([(key(5.0), 9)])), LH_EM);
        let fonts = HashMap::from([(Some("b".to_string()), 3), (None, 3), (Some("a".to_string()), 2)]);
        assert_eq!(body_font(&fonts), None);
    }
}

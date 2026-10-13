//! Kindle 上排出来的样子（Send to Kindle 的规则）：KFX 写出器和掌阅、Move 的 `kindle_rules` 系列共用的一份。
//!
//! - 常量：没写行高时的倍数、行高下限、百分比折算的页宽、空段折叠系数；
//! - 全书正文的字号、行距、字体怎么取（按字数加权的众数），行高怎么按正文归一；
//! - 竖直长度换成根 em；
//! - 一章的元素表 [`Chapter`]：层叠（样式表、`style`、表现属性）只在这里写一遍，`<body>` 里的元素按文档序编号，和标签扫描的
//!   开始标签一一对应（对得上才知道改源码里的哪个标签）。
//!
//! 2026-10-13 收拢：以前写出器和清洗层的 7 个 pass 各写一份常量、各自解析样式表、各走一遍 DOM，写出器改了规则两边会悄悄对不上。
use crate::cascade::{Computed, Len, MediaEnv, Rules, Sheet};
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

/// `<body>` 里的一个元素。
pub struct El {
    pub node: NodeId,
    /// 父元素在 [`Chapter::els`] 里的下标（父元素是 `<body>` 的 `None`）。
    pub parent: Option<usize>,
    /// 计算值。
    pub comp: Rc<Computed>,
    /// 层叠出来的声明（含 `style`、表现属性），给要看"写没写""写的什么"的用。
    pub decls: HashMap<String, String>,
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
        let root = html.root_element();
        let body = root.children().filter_map(ElementRef::wrap).find(|el| el.value().name() == "body")?;
        let rc = Computed::derive(&Computed::root(), &decls_of(&root), root.value().name());
        let bc = Rc::new(Computed::derive(&rc, &decls_of(&body), "body"));
        let mut els: Vec<El> = Vec::new();
        // 不递归（没关的标签能套上千层）：(节点, 父元素的计算值, 父元素下标, 父元素所在的块, 在不在隐藏的元素里)
        type Frame<'a> = (ego_tree::NodeRef<'a, Node>, Rc<Computed>, Option<usize>, Option<usize>, bool);
        let mut stack: Vec<Frame> = body.children().rev().map(|c| (c, bc.clone(), None, None, false)).collect();
        while let Some((n, parent, pi, block, hidden)) = stack.pop() {
            let Some(el) = ElementRef::wrap(n) else { continue };
            let decls = decls_of(&el);
            let c = Rc::new(Computed::derive(&parent, &decls, el.value().name()));
            let hidden = hidden || c.display.as_deref() == Some("none");
            let is_block = crate::cascade::is_block(&el, &c);
            let me = els.len();
            let block = if is_block { Some(me) } else { block };
            els.push(El { node: n.id(), parent: pi, comp: c.clone(), decls, is_block, block, hidden });
            for ch in n.children().rev() {
                stack.push((ch, c.clone(), Some(me), block, hidden));
            }
        }
        let matches_tags = crate::html::body_range(text).is_some_and(|(bs, be)| crate::html::tags_in(text, bs, be).filter(|t| t.is_start()).count() == els.len());
        let index = els.iter().enumerate().map(|(i, e)| (e.node, i)).collect();
        let body_id = body.id();
        drop(sheet);
        Some(Chapter { html, body_id, body_comp: bc, els, index, matches_tags })
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

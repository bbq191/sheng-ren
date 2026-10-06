//! EPUB → KFX 写出器（最小版）。
//!
//! 输入应是已经按设备优化过的 EPUB；这里只做格式转换，不改内容。
//! 写的片段是删除实验（`docs/kfx.md#删除实验`）定下的最小集合；分词、`$550`/`$621` 位置点、`$597`、`$585` 不写。
//! 支持的内容见 `docs/kfx.md#写出器`（段落、标题、行内样式、图片、链接与注释弹窗、目录、嵌入字体、列表、表格、边框）。

use crate::container::{Body, Entity};
use crate::css::{Border, BorderWidth, Computed, Len, Sheet};
use crate::ion::{self, Item, Value};
use crate::yj::*;
use bookconv::epubbook::{self, Loaded};
use bookconv::epubzip::resolve_link;
use bookconv::util::fnv64;
use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node};
use std::collections::{HashMap, HashSet};

/// 写出器版本：改了产物字节的修改要加一。只进书库指纹，**不写进书里**（见 [`FILE_CREATOR_VERSION`]）。
/// - 7（2026-10-06）：`rgb()`/`rgba()` 的百分比按百分比算；负字号作废；算不出有限值的长度（字号 0 时除以 0）写 0，不写 NaN。
///   没有文字的元素（`<span id>`、`<div id>`）、图片自己的 id 当锚点（挂到下一个块开头，文末的挂到最后一个块末尾），
///   以前丢掉、链接和目录退回文件开头。23 本测试书只有《福尔摩斯探案全集》变了（1101 个锚点、目录、115 个版面多配上注释弹窗），
///   其余逐字节不变；未真机验证。
/// - 8（2026-10-06）：书库生成时唯一 ID 取书 id（以前取不到，退回 OPF 唯一标识符的哈希，见 `library` 的 `kfx_id`）：**书库的 kindle 产物全部变了**
///   （容器 id、content_id、book_id；Kindle 上进度清零，用户接受）。`@media` 按阅读模式的阅读范围、屏幕求值（以前含 screen 就整块收），
///   `<link>`/`<style>` 的 `media` 属性同样处理；选择器优先级不再把伪类括号里的字计成标签；颜色认 `#rgba`/`#rrggbbaa`；声明用
///   `bookconv::html::css_decls` 切。同一份优化后 EPUB、同一个 `--id`，20 本测试书和一卷漫画新旧写出器逐字节相同（书里都没用到这些写法）。
/// - 9（2026-10-06）：图片节点写 CSS 的百分比宽度（`$56`，单位 `$314`，同表格宽度的写法；没有图片宽度的样本，按表格样本推的）。
///   以前图片一律不写宽度，Kindle 按图自身大小显示；优化器 v50 起给带图注的竖长图写 `width:P%`，图和图注排在同一页。
pub const WRITER_VERSION: &str = "9";

/// 写进书里的创建器版本（`creator_version`、`kfxgen_package_version`），固定不变：Kindle 发现文件字节变了就把书当新书、
/// 阅读进度清零（2026-10-06 真机：只差版本号的《绍宋》覆盖后进度没了，逐字节相同的《嘯風山莊》覆盖后进度还在）。
/// 写出器升版本后内容没变的书要生成逐字节相同的文件，所以这里不跟 [`WRITER_VERSION`] 走。
const FILE_CREATOR_VERSION: &str = "1";

/// 第一个本地符号的编号：系统表 9 个 + `YJ_symbols` v10 的 859 个。
const FIRST_LOCAL_SID: u32 = 10 + 859;
const YJ_SYMBOLS_MAX_ID: i64 = 859;

/// KFX 的 `lh` 单位折合多少个 em（样本里文档缺省行高 1.2em，长度都按它换算）。
const LH_EM: f64 = 1.2;
/// 百分比换算成 em 时假定的页宽（样本里 `margin-top:30%` → 9.6em）。
const PAGE_WIDTH_EM: f64 = 32.0;
/// 资源路径符号和图片字节实体符号的编号差（正好是 Ion 系统符号的个数）。
const SID_GAP: u32 = 9;
/// 元数据 `cover_image` 写的名字，符号编号＝封面资源 + [`SID_GAP`]。
const COVER_REF: &str = "cover-ref";
/// 文档级附加数据的名字；封面图实体叫它加 `-ad`。
const COVER_AUX: &str = "kfxdoc";

#[derive(Clone, Debug, Default)]
pub struct Opts {
    /// 固定唯一 ID（书库用书的 id）；`None` 按书的 OPF 唯一标识符派生。
    pub fixed_id: Option<u64>,
    /// 求值 `@media` 特性条件（`min-width` 等）用的阅读范围、屏幕（[`crate::css::MediaEnv::for_profile`]）；
    /// `None` 时带特性条件的一律不收，只看媒体类型。
    pub media: Option<crate::css::MediaEnv>,
}

// ---------------------------------------------------------------- 中间结构

/// 竖直方向长度，单位：根字号的 em。
type Vert = f64;

/// 水平方向长度：根 em + 百分比。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Horiz {
    em: f64,
    pct: f64,
}

#[derive(Clone, Debug)]
struct Run {
    start: usize,
    len: usize,
    /// 和所在段落不同的样式；`None`＝只是链接。
    comp: Option<Computed>,
    /// 书内链接的目标（文件, 锚点）。
    link: Option<(String, String)>,
    /// 注释引用（点了弹窗）：见 [`mark_notes`]。
    note_ref: bool,
}

#[derive(Clone, Debug)]
enum Kind {
    Text { text: String, runs: Vec<Run> },
    Image { src: String },
    Container(Vec<Block>),
}

#[derive(Clone, Debug)]
struct Block {
    kind: Kind,
    comp: Computed,
    heading: Option<u8>,
    margin_top: Vert,
    margin_bottom: Vert,
    margin_left: Horiz,
    margin_right: Horiz,
    padding: [Vert; 4],
    /// 本块里元素的 id → 字符偏移（目录、锚点用）。
    ids: Vec<(String, usize)>,
    /// 注释正文（弹窗里显示的内容）：见 [`mark_notes`]。
    note: bool,
    /// 节点类型（`None`＝文字 `$269`／容器 `$270`）：列表、列表项、表格各层、水平线。
    ty: Option<u32>,
    /// 节点上的字段（列表符号、表格边框合并等）。
    attrs: Vec<(u32, Value)>,
    /// 额外的样式属性（单元格跨行跨列、竖直对齐，表格宽度等）。
    extra: Vec<(u32, Value)>,
}

const BLOCK_TAGS: &[&str] = &[
    "address", "article", "aside", "blockquote", "body", "center", "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset", "figcaption",
    "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "li", "main", "menu", "nav", "ol", "p", "pre",
    "section", "summary", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "ul", "caption",
];

fn is_block_el(el: &ElementRef, comp: &Computed) -> bool {
    match comp.display.as_deref() {
        Some("block" | "list-item" | "table" | "table-row" | "table-cell" | "flex") => true,
        Some("inline" | "inline-block") => false,
        _ => BLOCK_TAGS.contains(&el.value().name()),
    }
}

fn to_vert(l: Option<Len>, fs: f64) -> Vert {
    match l {
        Some(Len::Em(n)) => n * fs,
        Some(Len::Pt(n)) => n / 12.0,
        Some(Len::Percent(p)) => p / 100.0 * PAGE_WIDTH_EM,
        None => 0.0,
    }
}

fn to_horiz(l: Option<Len>, fs: f64) -> Horiz {
    match l {
        Some(Len::Em(n)) => Horiz { em: n * fs, pct: 0.0 },
        Some(Len::Pt(n)) => Horiz { em: n / 12.0, pct: 0.0 },
        Some(Len::Percent(p)) => Horiz { em: 0.0, pct: p },
        None => Horiz::default(),
    }
}

/// CSS 外边距折叠：两个相邻外边距合成一个。
fn collapse(a: Vert, b: Vert) -> Vert {
    match (a >= 0.0, b >= 0.0) {
        (true, true) => a.max(b),
        (false, false) => a.min(b),
        _ => a + b,
    }
}

struct Doc<'a> {
    path: &'a str,
    sheet: Sheet,
    lang: Option<String>,
    /// 还没落到内容上的锚点：没有文字的元素（`<span id="x"></span>`、`<div id="x"></div>`）的 id。挂到文档顺序里
    /// 下一个生成的块的开头（[`Doc::take_pending`]）。以前直接丢掉，指向它们的链接、目录项退回到文件开头
    /// （《福尔摩斯探案全集》目录页「第一册」指向页末插图前的空锚点，点了停在目录页开头；全书 1102 处）。
    pending: std::cell::RefCell<Vec<String>>,
}

/// 收集行内内容：文字（空白按 CSS 折叠）、`<br>`、行内元素的样式区间、遇到图片就切开。
struct Inline {
    text: String,
    chars: usize,
    runs: Vec<Run>,
    ids: Vec<(String, usize)>,
    pending_space: bool,
}

impl Inline {
    fn new() -> Self {
        Inline { text: String::new(), chars: 0, runs: Vec::new(), ids: Vec::new(), pending_space: false }
    }

    fn push_text(&mut self, s: &str, pre: bool) {
        for c in s.chars() {
            if !pre && c.is_ascii_whitespace() {
                self.pending_space = self.chars > 0 && !self.text.ends_with('\n');
                continue;
            }
            if self.pending_space {
                self.text.push(' ');
                self.chars += 1;
                self.pending_space = false;
            }
            self.text.push(c);
            self.chars += 1;
        }
    }

    fn push_break(&mut self) {
        self.pending_space = false;
        self.text.push('\n');
        self.chars += 1;
    }

    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    fn take(&mut self) -> Inline {
        std::mem::replace(self, Inline::new())
    }
}

/// 文档末尾没落到内容上的锚点（`…</p><span id="x"></span></body>`）挂到最后一个块的末尾。
fn attach_trailing(blocks: &mut [Block], ids: Vec<String>) {
    let Some(last) = blocks.last_mut() else { return };
    match &mut last.kind {
        Kind::Container(c) if !c.is_empty() => attach_trailing(c, ids),
        Kind::Text { text, .. } => {
            let end = text.chars().count();
            last.ids.extend(ids.into_iter().map(|i| (i, end)));
        }
        _ => last.ids.extend(ids.into_iter().map(|i| (i, 0))),
    }
}

/// 把 `lead` 里的锚点放到块开头（字符偏移 0）。
fn prepend_ids(ids: &mut Vec<(String, usize)>, lead: Vec<String>) {
    ids.splice(0..0, lead.into_iter().map(|i| (i, 0)));
}

impl Doc<'_> {
    /// 取走还没落到内容上的锚点（见 [`Doc::pending`]）。
    fn take_pending(&self) -> Vec<String> {
        self.pending.take()
    }

    fn comp(&self, el: &ElementRef, parent: &Computed) -> Computed {
        let decls = self.sheet.cascade(el);
        let mut c = Computed::derive(parent, &decls, el.value().name());
        if let Some(l) = el.value().attr("xml:lang").or_else(|| el.value().attr("lang")) {
            c.lang = Some(l.to_string());
        }
        c
    }

    /// 把一个块级元素（计算值 `comp` 由调用方算好，层叠不重复做）展开成块序列。
    fn block(&self, el: ElementRef, comp: Computed, out: &mut Vec<Block>) {
        if comp.display.as_deref() == Some("none") {
            return;
        }
        let name = el.value().name();
        if matches!(name, "head" | "script" | "style" | "title") {
            return;
        }
        // 本元素之前攒下的锚点落在本元素的开头。
        let pre = self.take_pending();
        match name {
            "ul" | "ol" => {
                // 列表符号看列表项（`li` 上写的优先，《雪国》把 `cjk-ideographic` 写在 `li` 上）；列表项写成
                // `display:block` 的不显示符号（《啸风山庄》`li {display:block}`），和 `list-style:none` 一样当普通块。
                let first = el.children().filter_map(ElementRef::wrap).find(|c| c.value().name() == "li").map(|li| self.comp(&li, &comp));
                let mut comp = comp.clone();
                if let Some(li) = &first {
                    if li.list_style.is_some() {
                        comp.list_style = li.list_style.clone();
                    }
                }
                let marker = first.as_ref().is_none_or(|li| li.display.as_deref().is_none_or(|d| d == "list-item"));
                if marker && comp.list_style.as_deref() != Some("none") {
                    let mut b = self.list(el, comp);
                    prepend_ids(&mut b.ids, pre);
                    return out.push(b);
                }
            }
            "table" => {
                let mut b = self.table(el, comp);
                prepend_ids(&mut b.ids, pre);
                return out.push(b);
            }
            "hr" => {
                let mut b = self.boxed(Kind::Container(Vec::new()), comp, el.value().attr("id"));
                b.ty = Some(NODE_HR);
                // UA 样式：上下 0.5em。
                if b.comp.margin[0].is_none() {
                    b.margin_top = 0.5 * b.comp.font_size;
                }
                if b.comp.margin[2].is_none() {
                    b.margin_bottom = 0.5 * b.comp.font_size;
                }
                prepend_ids(&mut b.ids, pre);
                return out.push(b);
            }
            _ => {}
        }
        let fs = comp.font_size;
        let heading = bookconv::html::heading_level_of(name);
        let mut children = Vec::new();
        self.children(el, &comp, &mut children);
        let id = el.value().attr("id").map(str::to_string);
        // 开头的锚点：之前攒下的 + 本元素自己的 id。
        let mut lead = pre;
        lead.extend(id);
        if children.is_empty() {
            // 没有内容的元素：锚点（连同里面攒下的）留给后面的内容。
            let inner = self.take_pending();
            lead.extend(inner);
            *self.pending.borrow_mut() = lead;
            return;
        }
        let margin_top = to_vert(comp.margin[0], fs);
        let margin_bottom = to_vert(comp.margin[2], fs);
        let mut ml = to_horiz(comp.margin[3], fs);
        let mr = to_horiz(comp.margin[1], fs);
        let padding = [0, 1, 2, 3].map(|i| to_vert(comp.padding[i], fs));
        // 不显示符号的列表（`list-style:none`）当普通块，照 Amazon 缩进 1.5em（测试书 L04：`$48` 4.688%）。
        if matches!(name, "ul" | "ol") && comp.margin[3].is_none() && comp.padding[3].is_none() {
            ml.em += 1.5;
        }
        // 自己只包着一个文字块（普通段落）：本元素就是这个块。
        // 有背景或内边距的块写成容器套文字（样本里带背景色的 h1 就是这样；背景、内边距、负外边距直接放在文字段落上，
        // Kindle 上长标题会溢出屏幕，2026-10-05 真机）。
        let boxed = comp.background.is_some() || comp.bg_image.is_some() || padding.iter().any(|p| *p != 0.0) || comp.has_border();
        let own = !boxed && children.len() == 1 && matches!(children[0].kind, Kind::Text { .. }) && children[0].is_anonymous();
        if own {
            let mut b = children.pop().unwrap_or_else(|| unreachable!());
            b.comp = comp;
            b.heading = heading;
            b.margin_top = margin_top;
            b.margin_bottom = margin_bottom;
            b.margin_left = ml;
            b.margin_right = mr;
            b.padding = padding;
            prepend_ids(&mut b.ids, lead);
            out.push(b);
            return;
        }
        if boxed {
            out.push(Block {
                kind: Kind::Container(children),
                comp,
                heading,
                margin_top,
                margin_bottom,
                margin_left: ml,
                margin_right: mr,
                padding,
                ids: lead.into_iter().map(|i| (i, 0)).collect(),
                note: false,
                ty: None,
                attrs: Vec::new(),
                extra: Vec::new(),
            });
            return;
        }
        // 没有背景的包裹层摊平：竖直外边距、内边距并进首尾子块，水平外边距加到每个子块上。
        let n = children.len();
        let mut lead = Some(lead);
        for (i, mut c) in children.into_iter().enumerate() {
            if i == 0 {
                c.margin_top = collapse(margin_top + padding[0], c.margin_top);
                prepend_ids(&mut c.ids, lead.take().unwrap_or_default());
                if c.heading.is_none() {
                    c.heading = heading;
                }
            }
            if i + 1 == n {
                c.margin_bottom = collapse(margin_bottom + padding[2], c.margin_bottom);
            }
            c.margin_left.em += ml.em;
            c.margin_left.pct += ml.pct;
            c.margin_right.em += mr.em;
            c.margin_right.pct += mr.pct;
            out.push(c);
        }
    }

    /// 自己的外边距、内边距照计算值的块（列表、表格、单元格、水平线用；不摊平、不并进子块）。
    fn boxed(&self, kind: Kind, comp: Computed, id: Option<&str>) -> Block {
        let fs = comp.font_size;
        let mut b = Block::anonymous(kind, comp, id.map(|i| vec![(i.to_string(), 0)]).unwrap_or_default());
        b.margin_top = to_vert(b.comp.margin[0], fs);
        b.margin_bottom = to_vert(b.comp.margin[2], fs);
        b.margin_left = to_horiz(b.comp.margin[3], fs);
        b.margin_right = to_horiz(b.comp.margin[1], fs);
        b.padding = [0, 1, 2, 3].map(|i| to_vert(b.comp.padding[i], fs));
        b
    }

    /// 元素的子块；只有一个匿名文字块时直接用它当 `ty` 类型的节点（列表项、表格标题里的文字），否则包成容器。
    fn wrap_children(&self, el: ElementRef, comp: Computed, ty: Option<u32>) -> Block {
        let mut kids = Vec::new();
        self.children(el, &comp, &mut kids);
        let id = el.value().attr("id");
        if ty.is_some() && kids.len() == 1 && matches!(kids[0].kind, Kind::Text { .. }) && kids[0].is_anonymous() {
            if let Some(k) = kids.pop() {
                let mut b = self.boxed(k.kind, comp, id);
                b.ids.extend(k.ids);
                b.ty = ty;
                return b;
            }
        }
        let mut b = self.boxed(Kind::Container(kids), comp, id);
        b.ty = ty;
        b
    }

    /// `<ul>`/`<ol>` → `$276` 列表，`<li>` → `$277` 列表项（只有文字的列表项本身就是文字节点）。
    /// 列表符号的缺省按标签和嵌套层数（同浏览器：圆点 → 空心圆 → 方块），Amazon 转出来也是这样（2026-10-05 测试书）。
    fn list(&self, el: ElementRef, comp: Computed) -> Block {
        let ordered = el.value().name() == "ol";
        let depth = el.ancestors().filter_map(ElementRef::wrap).filter(|a| a.value().name() == "ul").count();
        let style = match comp.list_style.as_deref() {
            Some("disc") => LIST_DISC,
            Some("circle") => LIST_CIRCLE,
            Some("square") => LIST_SQUARE,
            Some("decimal") => LIST_DECIMAL,
            Some("decimal-leading-zero") => LIST_DECIMAL_ZERO,
            Some("lower-roman") => LIST_LOWER_ROMAN,
            Some("upper-roman") => LIST_UPPER_ROMAN,
            Some("lower-alpha" | "lower-latin") => LIST_LOWER_ALPHA,
            Some("upper-alpha" | "upper-latin") => LIST_UPPER_ALPHA,
            Some("lower-greek") => LIST_LOWER_GREEK,
            Some("cjk-ideographic" | "simp-chinese-informal" | "trad-chinese-informal" | "cjk-decimal") => LIST_CJK,
            _ if ordered => LIST_DECIMAL,
            _ => [LIST_DISC, LIST_CIRCLE, LIST_SQUARE][depth.min(2)],
        };
        let mut items = Vec::new();
        for child in el.children().filter_map(ElementRef::wrap) {
            let c = self.comp(&child, &comp);
            if c.display.as_deref() == Some("none") {
                continue;
            }
            let mut item = self.wrap_children(child, c, Some(NODE_LIST_ITEM));
            if let Some(v) = child.value().attr("value").and_then(|v| v.trim().parse::<i64>().ok()) {
                item.attrs.push((LIST_START, Value::Int(v)));
            }
            items.push(item);
        }
        let inside = comp.list_inside;
        let mut b = self.boxed(Kind::Container(items), comp, el.value().attr("id"));
        b.ty = Some(NODE_LIST);
        b.attrs.push((LIST_STYLE, Value::Symbol(style)));
        if let Some(n) = el.value().attr("start").and_then(|v| v.trim().parse::<i64>().ok()) {
            b.attrs.push((LIST_START, Value::Int(n)));
        }
        if inside {
            b.attrs.push((LIST_POSITION, Value::Symbol(LIST_INSIDE)));
        }
        b
    }

    /// `<table>` → `$278` 表 → `$151`/`$454`/`$455` 表头/表体/表脚 → `$279` 行 → `$270` 单元格（2026-10-05 《绍宋》样本和测试书对照）。
    /// 跨列、跨行、竖直对齐写在单元格的样式里；`<caption>` 是 `$269` 节点（`$615: $453`）套文字。
    fn table(&self, el: ElementRef, comp: Computed) -> Block {
        let mut parts = Vec::new();
        let mut col_widths: Option<Vec<Len>> = None;
        let spacing = |l: Option<Len>| match l {
            Some(Len::Pt(n)) => n * 0.6,
            Some(Len::Em(n)) => n * 12.0 * comp.font_size,
            _ => 0.9,
        };
        for part in el.children().filter_map(ElementRef::wrap) {
            let pc = self.comp(&part, &comp);
            if pc.display.as_deref() == Some("none") {
                continue;
            }
            match part.value().name() {
                "caption" => {
                    let mut cap = self.wrap_children(part, pc, None);
                    cap.ty = Some(NODE_TEXT);
                    cap.attrs.push((NOTE_CONTENT, Value::Symbol(CAPTION)));
                    cap.extra.push((P_LAYOUT_HINTS, Value::List(vec![Value::Symbol(CAPTION)])));
                    parts.push(cap);
                }
                tag @ ("thead" | "tbody" | "tfoot") => {
                    let ty = match tag {
                        "thead" => NODE_THEAD,
                        "tfoot" => NODE_TFOOT,
                        _ => NODE_TBODY,
                    };
                    let mut rows = Vec::new();
                    for tr in part.children().filter_map(ElementRef::wrap).filter(|e| e.value().name() == "tr") {
                        let rc = self.comp(&tr, &pc);
                        let mut cells = Vec::new();
                        let mut widths = Vec::new();
                        for td in tr.children().filter_map(ElementRef::wrap).filter(|e| matches!(e.value().name(), "td" | "th")) {
                            let mut cc = self.comp(&td, &rc);
                            if cc.width.is_none() {
                                cc.width = td.value().attr("width").and_then(crate::css::parse_len);
                            }
                            // UA 样式：单元格内边距 1px。
                            for p in cc.padding.iter_mut() {
                                p.get_or_insert(Len::Pt(0.75));
                            }
                            widths.push(cc.width);
                            let valign = match cc.valign.as_deref() {
                                Some("top" | "text-top") => VALIGN_TOP,
                                Some("bottom" | "text-bottom") => VALIGN_BOTTOM,
                                _ => ALIGN_CENTER,
                            };
                            let mut cell = self.wrap_children(td, cc, None);
                            cell.extra.push((P_CELL_VALIGN, Value::Symbol(valign)));
                            for (attr, key) in [("colspan", P_COLSPAN), ("rowspan", P_ROWSPAN)] {
                                if let Some(n) = td.value().attr(attr).and_then(|v| v.trim().parse::<i64>().ok()).filter(|n| *n > 1) {
                                    cell.extra.push((key, Value::Int(n)));
                                }
                            }
                            cells.push(cell);
                        }
                        if col_widths.is_none() {
                            col_widths = Some(widths.into_iter().map_while(|w| w).collect());
                        }
                        let mut row = Block::anonymous(Kind::Container(cells), rc.inherited(), Vec::new());
                        row.ty = Some(NODE_ROW);
                        rows.push(row);
                    }
                    let mut p = Block::anonymous(Kind::Container(rows), pc.inherited(), Vec::new());
                    p.ty = Some(ty);
                    parts.push(p);
                }
                _ => {}
            }
        }
        let mut attrs = vec![
            (TABLE_COLLAPSE, Value::Bool(comp.border_collapse)),
            (TABLE_SPACING_H, num(spacing(comp.border_spacing), U_PT)),
            (TABLE_SPACING_V, num(spacing(comp.border_spacing), U_PT)),
        ];
        let cols: Vec<Value> = col_widths.unwrap_or_default().into_iter().filter_map(|w| width_value(w, comp.font_size)).map(|v| Value::Struct(vec![(P_WIDTH, v)])).collect();
        if !cols.is_empty() {
            attrs.push((TABLE_COLUMNS, Value::List(cols)));
        }
        let mut b = self.boxed(Kind::Container(parts), comp, el.value().attr("id"));
        b.ty = Some(NODE_TABLE);
        b.attrs = attrs;
        b
    }

    /// 块级元素的子节点：连续的行内内容合成匿名文字块，块级子元素递归，图片单独成块。
    fn children(&self, el: ElementRef, comp: &Computed, out: &mut Vec<Block>) {
        let mut inl = Inline::new();
        for child in el.children() {
            self.inline_or_block(child, comp, comp, &mut inl, out);
        }
        self.flush(&mut inl, comp, out);
    }

    fn flush(&self, inl: &mut Inline, comp: &Computed, out: &mut Vec<Block>) {
        let taken = inl.take();
        if taken.is_empty() {
            // 没有文字：里面的锚点留给后面的内容。
            self.pending.borrow_mut().extend(taken.ids.into_iter().map(|(i, _)| i));
            return;
        }
        let mut text = taken.text;
        // 末尾的换行、空格去掉（字符数不变的前提下只能删尾部，不影响区间起点）。
        while text.ends_with([' ', '\n']) {
            text.pop();
        }
        let chars = text.chars().count();
        let runs = taken
            .runs
            .into_iter()
            .filter_map(|mut r| {
                r.len = r.len.min(chars.saturating_sub(r.start));
                (r.len > 0).then_some(r)
            })
            .collect();
        let mut ids = taken.ids.into_iter().map(|(i, o)| (i, o.min(chars))).collect();
        prepend_ids(&mut ids, self.take_pending());
        out.push(Block::anonymous(Kind::Text { text, runs }, comp.inherited(), ids));
    }

    fn inline_or_block(&self, node: NodeRef<Node>, block_comp: &Computed, parent: &Computed, inl: &mut Inline, out: &mut Vec<Block>) {
        match node.value() {
            Node::Text(t) => inl.push_text(t, parent.pre),
            Node::Element(_) => {
                let Some(el) = ElementRef::wrap(node) else { return };
                let name = el.value().name();
                if name == "br" {
                    inl.push_break();
                    return;
                }
                if let Some(src) = image_src(&el) {
                    self.flush(inl, block_comp, out);
                    let comp = self.comp(&el, parent);
                    // 图片自己的 id（`<img id="filepos152">`）也是锚点
                    let mut lead = self.take_pending();
                    lead.extend(el.value().attr("id").map(str::to_string));
                    let ids = lead.into_iter().map(|i| (i, 0)).collect();
                    out.push(Block::anonymous(Kind::Image { src: resolve_link(self.path, &src).0 }, comp, ids));
                    return;
                }
                let comp = self.comp(&el, parent);
                if comp.display.as_deref() == Some("none") || matches!(name, "script" | "style" | "head") {
                    return;
                }
                if is_block_el(&el, &comp) {
                    self.flush(inl, block_comp, out);
                    self.block(el, comp, out);
                    return;
                }
                if let Some(id) = el.value().attr("id") {
                    inl.ids.push((id.to_string(), inl.chars));
                }
                let start = inl.chars;
                let run_at = inl.runs.len();
                for c in el.children() {
                    self.inline_or_block(c, block_comp, &comp, inl, out);
                }
                let link = (name == "a")
                    .then(|| el.value().attr("href"))
                    .flatten()
                    .filter(|h| !h.contains("://") && !h.starts_with("mailto:"))
                    .map(|h| {
                        let (path, frag) = resolve_link(self.path, h);
                        (if path.is_empty() { self.path.to_string() } else { path }, frag.unwrap_or_default())
                    });
                let styled = run_differs(&comp, block_comp);
                if inl.chars > start && (styled || link.is_some()) {
                    inl.runs.insert(run_at, Run { start, len: inl.chars - start, comp: styled.then_some(comp), link, note_ref: false });
                }
            }
            _ => {}
        }
    }
}

impl Block {
    fn anonymous(kind: Kind, comp: Computed, ids: Vec<(String, usize)>) -> Block {
        Block {
            kind,
            comp,
            heading: None,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: Horiz::default(),
            margin_right: Horiz::default(),
            padding: [0.0; 4],
            ids,
            note: false,
            ty: None,
            attrs: Vec::new(),
            extra: Vec::new(),
        }
    }

    fn is_anonymous(&self) -> bool {
        self.margin_top == 0.0 && self.margin_bottom == 0.0 && self.heading.is_none() && self.margin_left == Horiz::default() && self.ty.is_none()
    }
}

fn image_src(el: &ElementRef) -> Option<String> {
    let v = el.value();
    match v.name() {
        "img" => v.attr("src").map(str::to_string),
        "image" => v.attr("xlink:href").or_else(|| v.attr("href")).map(str::to_string),
        _ => None,
    }
}

fn run_differs(a: &Computed, b: &Computed) -> bool {
    a.bold != b.bold
        || a.italic != b.italic
        || a.color != b.color
        || (a.font_size - b.font_size).abs() > 1e-6
        || a.font_family != b.font_family
        || a.superscript != b.superscript
        || a.subscript != b.subscript
        || a.decoration != b.decoration
        || a.small_caps != b.small_caps
        || a.letter_spacing != b.letter_spacing
        || a.background.is_some()
        || a.has_border()
}

/// 解析好的一个文档：(spine 下标, 路径, 语言, 块)。
type ParsedDoc<'a> = (usize, &'a str, Option<String>, Vec<Block>);

/// 全书的文字块按深度优先编号（含容器里的），`f` 拿到 (文档下标, 编号, 块)。
fn each_text_block(docs: &mut [ParsedDoc], mut f: impl FnMut(usize, usize, &mut Block)) {
    fn walk(blocks: &mut [Block], di: usize, n: &mut usize, f: &mut dyn FnMut(usize, usize, &mut Block)) {
        for b in blocks {
            match &mut b.kind {
                Kind::Container(c) => walk(c, di, n, f),
                Kind::Text { .. } => {
                    f(di, *n, b);
                    *n += 1;
                }
                Kind::Image { .. } => {}
            }
        }
    }
    let mut n = 0;
    for (di, d) in docs.iter_mut().enumerate() {
        walk(&mut d.3, di, &mut n, &mut f);
    }
}

/// 注释配对（同 Amazon 的转换：原书多半只是普通链接，没有 `epub:type`）：正文链接指到后面的一个块，那个块**开头**的链接
/// 指回这个正文链接所在的位置（链接自己或包着它的元素的 id），正文链接就是注释引用（`$616: $617`，Kindle 点了弹窗），目标块是注释正文
/// （`$615: $618`）。优化器 `kindle` 模式保留回链（`note_backlinks`）、把注释搬到引用它的那一章（文件）末尾，正好配得上。
/// 返回配上了几条。
fn mark_notes(docs: &mut [ParsedDoc]) -> usize {
    // 第一遍：每个块的 id、每个链接区间（块编号, 区间下标, 目标, 区间起点处的 id）
    let mut id_block: HashMap<(String, String), usize> = HashMap::new();
    // (块编号, 区间下标, 目标, 区间起点处的 id)
    type Link = (usize, usize, (String, String), Vec<String>);
    let mut links: Vec<Link> = Vec::new();
    let paths: Vec<String> = docs.iter().map(|d| d.1.to_string()).collect();
    each_text_block(docs, |di, n, b| {
        for (id, _) in &b.ids {
            id_block.entry((paths[di].clone(), id.clone())).or_insert(n);
        }
        if let Kind::Text { runs, .. } = &b.kind {
            for (ri, r) in runs.iter().enumerate() {
                if let Some(t) = &r.link {
                    let src: Vec<String> = b.ids.iter().filter(|(_, o)| *o >= r.start && *o <= r.start + r.len).map(|(i, _)| i.clone()).collect();
                    links.push((n, ri, t.clone(), src));
                }
            }
        }
    });
    // 每个块开头（第一个字起）的链接链回到哪里：注释正文的回链在段首（「[1]Queen of Sheba…」）
    let mut back: HashMap<usize, Vec<(String, String)>> = HashMap::new();
    let mut run_start: HashMap<(usize, usize), usize> = HashMap::new();
    each_text_block(docs, |_, n, b| {
        if let Kind::Text { runs, .. } = &b.kind {
            for (ri, r) in runs.iter().enumerate() {
                run_start.insert((n, ri), r.start);
            }
        }
    });
    for (n, ri, t, _) in &links {
        if run_start.get(&(*n, *ri)).is_some_and(|&s| s == 0) {
            back.entry(*n).or_default().push(t.clone());
        }
    }
    let block_path: HashMap<usize, usize> = {
        let mut m = HashMap::new();
        each_text_block(docs, |di, n, _| {
            m.insert(n, di);
        });
        m
    };
    let mut refs: HashSet<(usize, usize)> = HashSet::new();
    let mut notes: HashSet<usize> = HashSet::new();
    for (n, ri, target, src) in &links {
        // 注释在引用后面（优化器把注释搬到引用它的那一章（文件）末尾）；反过来那条是回链。
        let Some(&tb) = id_block.get(target) else { continue };
        if tb <= *n {
            continue;
        }
        let src_path = &paths[block_path[n]];
        let points_back = back.get(&tb).is_some_and(|ts| ts.iter().any(|(p, f)| p == src_path && src.contains(f)));
        if points_back {
            refs.insert((*n, *ri));
            notes.insert(tb);
        }
    }
    each_text_block(docs, |_, n, b| {
        if notes.contains(&n) {
            b.note = true;
        }
        if let Kind::Text { runs, .. } = &mut b.kind {
            for (ri, r) in runs.iter_mut().enumerate() {
                if refs.contains(&(n, ri)) {
                    r.note_ref = true;
                }
            }
        }
    });
    refs.len()
}

/// 正文字体（字数最多的那个）没有嵌入时不写进样式：Kindle 遇到不认识的字体名会换成别的字体，和没写字体的表格、
/// 阅读器设置里选的字体都对不上（《啸风山庄》正文写着没嵌入的「AR MingU30 DemiBold」，2026-10-05 真机）。
/// 正文本来就该用阅读器的字体（用户定的字体规矩，见 typesetting.md）。
fn body_font_to_drop(docs: &mut [ParsedDoc], book: &Loaded) -> Option<String> {
    let mut count: HashMap<Option<String>, usize> = HashMap::new();
    each_text_block(docs, |_, _, b| {
        if let Kind::Text { text, .. } = &b.kind {
            *count.entry(b.comp.font_family.as_ref().map(|f| f.to_lowercase())).or_default() += text.chars().count();
        }
    });
    let body = count.into_iter().max_by_key(|(_, n)| *n)?.0?;
    let files: HashSet<&str> = book.fonts.iter().map(|(p, _)| p.as_str()).collect();
    let embedded = book.css.iter().flat_map(|(p, t)| crate::css::font_faces(t, p)).any(|f| f.family.to_lowercase() == body && files.contains(f.path.as_str()));
    (!embedded).then_some(body)
}

/// 相邻块的外边距折叠（同一层）。
fn collapse_siblings(blocks: &mut [Block]) {
    for i in 1..blocks.len() {
        let prev_bottom = blocks[i - 1].margin_bottom;
        blocks[i].margin_top = collapse(prev_bottom, blocks[i].margin_top);
        blocks[i - 1].margin_bottom = 0.0;
    }
    for b in blocks.iter_mut() {
        if let Kind::Container(c) = &mut b.kind {
            collapse_siblings(c);
        }
    }
}

// ---------------------------------------------------------------- 写 KFX

fn num(v: f64, unit: u32) -> Value {
    // 保留 6 位有效数字左右（和样本一样的精度，避免 0.8333333333 这种长尾）。
    // 字号 0 的元素换算长度时会除以 0：算不出有限值的写 0，不往书里写 NaN、无穷大。
    let r = if v.is_finite() { (v * 1e6).round() / 1e6 } else { 0.0 };
    Value::Struct(vec![(VALUE, Value::F64(if r == 0.0 { 0.0 } else { r })), (UNIT, Value::Symbol(unit))])
}

struct Res {
    name: String,
    location: String,
    format: u32,
    mime: &'static str,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

struct Builder {
    locals: Vec<String>,
    local_index: HashMap<String, u32>,
    next_eid: i64,
    /// 样式去重：属性编码 → 样式名的符号。
    styles: HashMap<Vec<u8>, u32>,
    style_entities: Vec<(String, Vec<(u32, Value)>)>,
    resources: Vec<Res>,
    res_by_path: HashMap<String, usize>,
    images: HashMap<String, (Vec<u8>, &'static str)>,
    warnings: Vec<String>,
    /// 链接、目录用到的锚点：(文件, 锚点) → 锚点名，按出现顺序。
    anchors: Vec<((String, String), u32)>,
    anchor_index: HashMap<(String, String), u32>,
    /// 标题（级别, 节点 id），按书里的顺序。
    headings: Vec<(u8, i64)>,
    /// 样式里用到的字体名（嵌入字体只嵌这些）。
    used_fonts: std::collections::BTreeSet<String>,
    /// 不写进样式的字体名：没嵌入的正文字体（见 [`body_font_to_drop`]）。
    drop_font: Option<String>,
    /// 求值 `@media` 用（见 [`Opts::media`]）。
    media: Option<crate::css::MediaEnv>,
}

impl Builder {
    fn sym(&mut self, name: &str) -> u32 {
        if let Some(&s) = self.local_index.get(name) {
            return s;
        }
        let s = FIRST_LOCAL_SID + self.locals.len() as u32;
        self.locals.push(name.to_string());
        self.local_index.insert(name.to_string(), s);
        s
    }

    fn anchor(&mut self, key: (String, String)) -> u32 {
        if let Some(&a) = self.anchor_index.get(&key) {
            return a;
        }
        let a = self.sym(&format!("anchor{}", self.anchors.len()));
        self.anchor_index.insert(key.clone(), a);
        self.anchors.push((key, a));
        a
    }

    fn eid(&mut self) -> i64 {
        let e = self.next_eid;
        self.next_eid += 1;
        e
    }

    /// 样式去重：属性一样的共用一个样式片段。
    fn style(&mut self, mut props: Vec<(u32, Value)>) -> u32 {
        props.sort_by_key(|(k, _)| *k);
        if let Some(d) = &self.drop_font {
            props.retain(|(k, v)| !(*k == P_FONT_FAMILY && matches!(v, Value::String(f) if f.eq_ignore_ascii_case(d))));
        }
        for (k, v) in &props {
            if let (P_FONT_FAMILY, Value::String(f)) = (*k, v) {
                self.used_fonts.insert(f.clone());
            }
        }
        // 键：每个属性的编号 + 值的 Ion 编码（Ion 值自带长度，拼起来不会混淆）。
        let mut key = Vec::new();
        for (k, v) in &props {
            key.extend(k.to_le_bytes());
            ion::encode_value(&mut key, v);
        }
        if let Some(&s) = self.styles.get(&key) {
            return s;
        }
        let name = format!("style{}", self.style_entities.len());
        let s = self.sym(&name);
        self.styles.insert(key, s);
        self.style_entities.push((name, props));
        s
    }

    fn resource(&mut self, path: &str) -> Option<usize> {
        if let Some(&i) = self.res_by_path.get(path) {
            return Some(i);
        }
        let (format, mime) = match self.images.get(path).map(|(_, m)| *m) {
            Some("image/jpeg") => (FORMAT_JPG, "image/jpg"),
            Some("image/png") => (FORMAT_PNG, "image/png"),
            Some("image/gif") => (FORMAT_GIF, "image/gif"),
            Some(other) => {
                self.warnings.push(format!("图片格式 {other} 不支持：{path}"));
                return None;
            }
            None => {
                self.warnings.push(format!("图片找不到或格式不支持：{path}"));
                return None;
            }
        };
        // 字节搬进资源，不复制（大漫画几百 MB）；同一路径以后走上面的 `res_by_path`，不会再来取。
        let (bytes, _) = self.images.remove(path)?;
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok())
            .unwrap_or((0, 0));
        let i = self.resources.len();
        self.resources.push(Res { name: format!("img{i}"), location: format!("resource/img{i}"), format, mime, width, height, bytes });
        // 图片字节的实体名另起（样本里是 `…-ad`，和 `$165` 的资源路径不同名；用 `resource/…` 当实体名时
        // Kindle 不显示图片，2026-10-05 真机）。
        self.res_by_path.insert(path.to_string(), i);
        Some(i)
    }

    /// 块的样式属性。`parent_fs`：KFX 里父节点的字号（根 em），字号写成相对它的倍数。
    fn block_props(&mut self, b: &Block, parent_fs: f64, doc_lang: &Option<String>) -> Vec<(u32, Value)> {
        let c = &b.comp;
        let fs = c.font_size;
        let mut p = text_props(c, parent_fs);
        if let Some(l) = c.lang.as_ref().or(doc_lang.as_ref()) {
            p.push((P_LANG, Value::String(l.to_ascii_lowercase())));
        }
        if let Some(a) = c.text_align.as_deref() {
            let v = match a {
                "center" => ALIGN_CENTER,
                "right" | "end" => ALIGN_RIGHT,
                "justify" => ALIGN_JUSTIFY,
                _ => ALIGN_LEFT,
            };
            p.push((P_TEXT_ALIGN, Value::Symbol(v)));
        }
        match c.text_indent {
            Some(Len::Em(n)) => p.push((P_TEXT_INDENT, num(n, U_EM))),
            Some(Len::Pt(n)) => p.push((P_TEXT_INDENT, num(n / 12.0 / fs, U_EM))),
            Some(Len::Percent(n)) => p.push((P_TEXT_INDENT, num(n, U_PERCENT))),
            None => {}
        }
        let lh = c.line_height.unwrap_or(LH_EM) / LH_EM;
        p.push((P_LINE_HEIGHT, num(lh, U_LH)));
        let vert = |v: Vert| num(v / fs / LH_EM, U_LH);
        if b.margin_top != 0.0 {
            p.push((P_MARGIN_TOP, vert(b.margin_top)));
        }
        if b.margin_bottom != 0.0 {
            p.push((P_MARGIN_BOTTOM, vert(b.margin_bottom)));
        }
        let horiz = |h: Horiz| if h.em == 0.0 { num(h.pct, U_PERCENT) } else { num((h.em + h.pct / 100.0 * PAGE_WIDTH_EM) / fs, U_EM) };
        if b.margin_left != Horiz::default() {
            p.push((P_MARGIN_LEFT, horiz(b.margin_left)));
        }
        if b.margin_right != Horiz::default() {
            p.push((P_MARGIN_RIGHT, horiz(b.margin_right)));
        }
        for (i, k) in [P_PADDING_TOP, P_PADDING_RIGHT, P_PADDING_BOTTOM, P_PADDING_LEFT].into_iter().enumerate() {
            if b.padding[i] != 0.0 {
                p.push((k, if i % 2 == 0 { vert(b.padding[i]) } else { num(b.padding[i] / fs, U_EM) }));
            }
        }
        if let Some(bg) = c.background {
            p.push((P_BACKGROUND, Value::Int(i64::from(bg))));
        }
        p.extend(border_props(c));
        // 图片只认百分比宽度（写出器 9）：优化器给带图注的竖长图写的 `width:P%`（`bookconv::capfit`）、书里样式表的 `width:40%`；
        // 别的单位（em、px）没对过样本，不写
        let image_pct = matches!(b.kind, Kind::Image { .. }) && matches!(c.width, Some(Len::Percent(_)));
        if matches!(b.ty, Some(NODE_TABLE | NODE_HR)) || image_pct {
            if let Some(v) = c.width.and_then(|w| width_value(w, fs)) {
                p.push((P_WIDTH, v));
            }
        }
        p.extend(b.extra.iter().cloned());
        p
    }

    /// 生成一个块的节点（递归），同时登记位置、id。
    fn node(&mut self, b: &Block, parent_fs: f64, ctx: &mut SectionCtx) -> Option<Value> {
        let props = self.block_props(b, parent_fs, &ctx.lang);
        match &b.kind {
            Kind::Text { text, runs } => {
                let eid = self.eid();
                let chars = text.chars().count();
                for (id, off) in &b.ids {
                    ctx.ids.push((id.clone(), eid, *off));
                }
                ctx.order.push((eid, chars));
                let idx = ctx.texts.len();
                ctx.texts.push(Value::String(text.clone()));
                let style = self.style(props);
                let mut f = vec![(EID, Value::Int(eid)), (STYLE_REF, Value::Symbol(style)), (NODE_TYPE, Value::Symbol(b.ty.unwrap_or(NODE_TEXT)))];
                f.extend(b.attrs.iter().cloned());
                if let Some(h) = b.heading {
                    f.push((HEADING_LEVEL, Value::Int(i64::from(h))));
                }
                if b.note {
                    f.push((NOTE_CONTENT, Value::Symbol(NOTE_CONTENT_FOOTNOTE)));
                }
                if let Some(h) = b.heading {
                    self.headings.push((h, eid));
                }
                let run_values: Vec<Value> = runs
                    .iter()
                    .map(|r| {
                        let mut f = vec![(OFFSET, Value::Int(r.start as i64)), (LENGTH, Value::Int(r.len as i64))];
                        if r.note_ref {
                            f.push((NOTE_REF, Value::Symbol(NOTE_REF_POPUP)));
                        }
                        if let Some(key) = &r.link {
                            f.push((LINK_TO, Value::Symbol(self.anchor(key.clone()))));
                        }
                        if let Some(rc) = &r.comp {
                            let mut rp = text_props(rc, b.comp.font_size);
                            if rc.superscript && !b.comp.superscript {
                                rp.push((P_VERTICAL_ALIGN, Value::Symbol(VALIGN_SUPER)));
                            } else if rc.subscript && !b.comp.subscript {
                                rp.push((P_VERTICAL_ALIGN, Value::Symbol(VALIGN_SUB)));
                            }
                            rp.extend(border_props(rc));
                            if let Some(bg) = rc.background {
                                rp.push((P_INLINE_BACKGROUND, Value::Int(i64::from(bg))));
                            }
                            f.push((STYLE_REF, Value::Symbol(self.style(rp))));
                        }
                        Value::Struct(f)
                    })
                    .collect();
                if !run_values.is_empty() {
                    f.push((RUNS, Value::List(run_values)));
                }
                f.push((TEXT_REF, Value::Struct(vec![(NAME, Value::Symbol(ctx.pool)), (TEXT_INDEX, Value::Int(idx as i64))])));
                Some(Value::Struct(f))
            }
            Kind::Image { src } => {
                let r = self.resource(src)?;
                let eid = self.eid();
                for (id, _) in &b.ids {
                    ctx.ids.push((id.clone(), eid, 0));
                }
                ctx.order.push((eid, 1));
                ctx.resources.push(r);
                let style = self.style(props);
                let res = self.sym(&self.resources[r].name.clone());
                Some(Value::Struct(vec![
                    (EID, Value::Int(eid)),
                    (STYLE_REF, Value::Symbol(style)),
                    (NODE_TYPE, Value::Symbol(NODE_IMAGE)),
                    (RESOURCE_REF, Value::Symbol(res)),
                ]))
            }
            Kind::Container(children) => {
                let eid = self.eid();
                for (id, _) in &b.ids {
                    ctx.ids.push((id.clone(), eid, 0));
                }
                ctx.order.push((eid, 1));
                if let Some(h) = b.heading {
                    self.headings.push((h, eid));
                }
                let ty = b.ty.unwrap_or(NODE_CONTAINER);
                let mut props = props;
                // 背景图：样式里指向图片资源（`$479`），同《绍宋》样本 `body.juan` 等
                if let Some(r) = b.comp.bg_image.as_deref().and_then(|src| self.resource(src)) {
                    ctx.resources.push(r);
                    props.push((P_BG_IMAGE, Value::Symbol(self.sym(&self.resources[r].name.clone()))));
                    if b.comp.bg_no_repeat {
                        props.push((P_BG_REPEAT, Value::Symbol(BG_NO_REPEAT)));
                    }
                    if b.comp.bg_fixed {
                        props.push((P_BG_ATTACHMENT, Value::Symbol(BG_FIXED)));
                    }
                    let len = |l: Len| match l {
                        Len::Percent(p) => num(p, U_PERCENT),
                        Len::Em(n) => num(n, U_EM),
                        Len::Pt(n) => num(n, U_PT),
                    };
                    for (v, k) in b.comp.bg_position.iter().zip([P_BG_POS_X, P_BG_POS_Y]).chain(b.comp.bg_size.iter().zip([P_BG_SIZE_W, P_BG_SIZE_H])) {
                        if let Some(l) = v {
                            props.push((k, len(*l)));
                        }
                    }
                }
                // 表体、行这些结构层没有样式（同样本）。
                let styled = !matches!(ty, NODE_THEAD | NODE_TBODY | NODE_TFOOT | NODE_ROW);
                let style = styled.then(|| self.style(props));
                let kids: Vec<Value> = children.iter().filter_map(|c| self.node(c, b.comp.font_size, ctx)).collect();
                let mut f = vec![(EID, Value::Int(eid))];
                if ty == NODE_CONTAINER {
                    f.push((TMPL_FIT, Value::Symbol(CONTAINER_LAYOUT)));
                }
                if let Some(style) = style {
                    f.push((STYLE_REF, Value::Symbol(style)));
                }
                f.extend(b.attrs.iter().cloned());
                if let Some(h) = b.heading {
                    f.push((HEADING_LEVEL, Value::Int(i64::from(h))));
                }
                f.push((NODE_TYPE, Value::Symbol(ty)));
                if !kids.is_empty() {
                    f.push((CHILDREN, Value::List(kids)));
                }
                Some(Value::Struct(f))
            }
        }
    }
}

/// 宽度：百分比照写，em/pt 换成 em。
fn width_value(l: Len, fs: f64) -> Option<Value> {
    match l {
        Len::Percent(p) => Some(num(p, U_PERCENT)),
        Len::Em(n) if n > 0.0 => Some(num(n, U_EM)),
        Len::Pt(n) if n > 0.0 => Some(num(n / 12.0 / fs, U_EM)),
        _ => None,
    }
}

/// 边框、圆角（块和行内区间共用）。四边样式、宽度、颜色都一样时写「全部」那一组，否则按边写（上、左、下、右）。
fn border_props(c: &Computed) -> Vec<(u32, Value)> {
    let mut p = Vec::new();
    let bw = |w: BorderWidth| match w {
        BorderWidth::Pt(n) => num(n, U_PT),
        BorderWidth::Em(n) => num(n, U_EM),
    };
    let style = |b: &Border| match b.style.as_str() {
        "dashed" => BORDER_DASHED,
        "dotted" => BORDER_DOTTED,
        "double" => BORDER_DOUBLE,
        "groove" => BORDER_GROOVE,
        "ridge" => BORDER_RIDGE,
        "inset" => BORDER_INSET,
        "outset" => BORDER_OUTSET,
        _ => BORDER_SOLID,
    };
    let zero = |b: &Border| matches!(b.width, BorderWidth::Pt(n) | BorderWidth::Em(n) if n == 0.0);
    let sides: Vec<Option<&Border>> = c.border.iter().map(|b| b.as_ref().filter(|b| !zero(b))).collect();
    let mut emit = |slot: usize, b: &Border| {
        p.push((P_BORDER_STYLE[slot], Value::Symbol(style(b))));
        p.push((P_BORDER_WIDTH[slot], bw(b.width)));
        if let Some(col) = b.color {
            p.push((P_BORDER_COLOR[slot], Value::Int(i64::from(col))));
        }
    };
    if let (Some(first), true) = (sides[0], sides.iter().all(|s| *s == sides[0])) {
        emit(0, first);
    } else {
        // CSS 顺序（上、右、下、左）→ KFX 顺序（上、左、下、右）的下标
        for (css_i, slot) in [(0, 1), (3, 2), (2, 3), (1, 4)] {
            if let Some(b) = sides[css_i] {
                emit(slot, b);
            }
        }
    }
    if c.radius.iter().any(Option::is_some) {
        // 左上、右上、右下、左下 → `$459`、`$460`、`$462`、`$461`
        for (i, slot) in [(0, 0), (1, 1), (2, 3), (3, 2)] {
            let v = match c.radius[i] {
                Some(Len::Em(n)) => num(n, U_EM),
                Some(Len::Pt(n)) => num(n * 0.6, U_PT),
                Some(Len::Percent(n)) => num(n, U_PERCENT),
                None => num(0.0, U_PT),
            };
            p.push((P_BORDER_RADIUS[slot], v));
        }
    }
    p
}

/// 字体、字号、粗体、斜体、颜色（块和行内区间共用）。
fn text_props(c: &Computed, parent_fs: f64) -> Vec<(u32, Value)> {
    let mut p = Vec::new();
    if let Some(f) = &c.font_family {
        p.push((P_FONT_FAMILY, Value::String(f.clone())));
    }
    let rel = if parent_fs > 0.0 { c.font_size / parent_fs } else { c.font_size };
    p.push((P_FONT_SIZE, num(rel, U_FONT_EM)));
    p.push((P_FONT_WEIGHT, Value::Symbol(if c.bold { WEIGHT_BOLD } else { WEIGHT_NORMAL })));
    if c.italic {
        p.push((P_FONT_STYLE, Value::Symbol(STYLE_ITALIC)));
    }
    if let Some(col) = c.color {
        p.push((P_COLOR, Value::Int(i64::from(col))));
    }
    for (on, k) in c.decoration.iter().zip([P_UNDERLINE, P_LINE_THROUGH, P_OVERLINE]) {
        if *on {
            p.push((k, Value::Symbol(BORDER_SOLID)));
        }
    }
    if c.small_caps {
        p.push((P_FONT_VARIANT, Value::Symbol(SMALL_CAPS)));
    }
    if let Some(ls) = c.letter_spacing {
        p.push((P_LETTER_SPACING, num(ls, U_EM)));
    }
    p
}

struct SectionCtx {
    pool: u32,
    texts: Vec<Value>,
    /// 节点顺序与各自占的位置数。
    order: Vec<(i64, usize)>,
    ids: Vec<(String, i64, usize)>,
    resources: Vec<usize>,
    lang: Option<String>,
}

struct SectionOut {
    name: u32,
    length: usize,
    eids: Vec<i64>,
    pid_map: Vec<Value>,
    resources: Vec<usize>,
    first_eid: i64,
}

/// 32 进制大写（容器 id、book_id 用）。
/// 唯一 ID → 容器 id（`CR!` + 28 个字符）。书里存 4 处（见 `Container::set_container_id`），都写这一个值。
pub fn container_id(id: u64) -> String {
    format!("CR!{}", base32(id, 28))
}

fn base32(mut n: u64, len: usize) -> String {
    const A: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut s = Vec::with_capacity(len);
    for _ in 0..len {
        s.push(A[(n % 32) as usize]);
        n = (n / 32) ^ n.rotate_left(13);
    }
    String::from_utf8(s).unwrap_or_default()
}

/// 实体头：样本里都是 `{$410: 0, $411: 0}`。
fn entity_header() -> Vec<Item> {
    vec![Item::Bvm, Item::Value(Value::Struct(vec![(410, Value::Int(0)), (411, Value::Int(0))]))]
}

fn ent(id: u32, ty: u32, v: Value) -> Entity {
    Entity { id, ty, version: 1, header: entity_header(), body: Body::Ion(vec![Item::Bvm, Item::Value(v)]) }
}

/// `$264` 的写法：升序 id，连续的写成 `[起点, 个数]`。
fn compress_ids(mut ids: Vec<i64>) -> Value {
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j + 1 < ids.len() && ids[j + 1] == ids[j] + 1 {
            j += 1;
        }
        if j > i {
            out.push(Value::List(vec![Value::Int(ids[i]), Value::Int((j - i + 1) as i64)]));
        } else {
            out.push(Value::Int(ids[i]));
        }
        i = j + 1;
    }
    Value::List(out)
}

/// `$609` 的写法：`[前进, 新 id]`，id 正好加一时只写前进量，最后 `[前进, 0]`。
fn pid_map(order: &[(i64, usize)]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut prev: Option<(i64, usize)> = None;
    for &(eid, len) in order {
        let adv = prev.map_or(0, |p| p.1) as i64;
        match prev {
            Some((pe, _)) if eid == pe + 1 => out.push(Value::Int(adv)),
            _ => out.push(Value::List(vec![Value::Int(adv), Value::Int(eid)])),
        }
        prev = Some((eid, len));
    }
    if let Some((_, len)) = prev {
        out.push(Value::List(vec![Value::Int(len as i64), Value::Int(0)]));
    }
    out
}

/// EPUB → KFX。返回 KFX 字节和警告。
pub fn epub_to_kfx(epub: &[u8], opts: &Opts) -> Result<(Vec<u8>, Vec<String>), String> {
    epub_to_kfx_from(std::io::Cursor::new(epub), opts)
}

/// 同 [`epub_to_kfx`]，从可定位的读取器（如打开的文件）读 EPUB：不用先把整本读进内存，峰值少一份压缩包大小。
pub fn epub_to_kfx_from<R: std::io::Read + std::io::Seek>(epub: R, opts: &Opts) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut book = epubbook::load_from(epub, &mut warnings)?;
    // 图片字节从书里搬出来（写出器只在这里用到它们），不复制。
    let images = std::mem::take(&mut book.images).into_iter().map(|i| (i.path, (i.bytes, i.mime))).collect();
    let mut b = Builder {
        locals: Vec::new(),
        local_index: HashMap::new(),
        next_eid: 1,
        styles: HashMap::new(),
        style_entities: Vec::new(),
        resources: Vec::new(),
        res_by_path: HashMap::new(),
        images,
        warnings,
        anchors: Vec::new(),
        anchor_index: HashMap::new(),
        headings: Vec::new(),
        used_fonts: Default::default(),
        drop_font: None,
        media: opts.media,
    };
    // 封面资源最先登记，紧跟着排好 `cover_image` 要用的名字：元数据 `cover_image` 也是按「这个名字的符号编号 − 9」
    // 找封面资源的（6 本样本的 `e6` 减 9 都正好是封面 JPEG 的 `$164`；书架缩略图靠它，2026-10-05 真机）。
    if let Some(i) = book.cover.as_deref().and_then(|c| b.resource(c)) {
        let name = b.resources[i].name.clone();
        let res = b.sym(&name);
        for k in 1..SID_GAP {
            b.sym(&format!("pad-cover-{k}"));
        }
        let cover_ref = b.sym(COVER_REF);
        debug_assert_eq!(cover_ref, res + SID_GAP);
    }
    let id = opts.fixed_id.unwrap_or(book.meta.stable_id);
    let out = build(&book, &mut b, id)?;
    Ok((out, b.warnings))
}

fn build(book: &Loaded, b: &mut Builder, id: u64) -> Result<Vec<u8>, String> {
    let css: HashMap<&str, &str> = book.css.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    let book_lang = (!book.meta.language.is_empty()).then(|| book.meta.language.clone());
    // 固定版式（漫画，优化器 `comicfxl` 写的 `fixed-layout`/`original-resolution`）：画布宽高。见 docs/kfx.md#固定版式。
    let fixed_canvas: Option<(i64, i64)> = (!book.meta.fixed_layout.is_empty()).then(|| {
        book.meta
            .fixed_layout
            .iter()
            .find(|(n, _)| *n == 126)
            .and_then(|(_, v)| v.split_once('x'))
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
            .unwrap_or((0, 0))
    });
    // 翻页方向：`$557` 从左往右、`$559` 从右往左（2026-10-05 测试漫画 LTR/RTL 两本只差这一处）。
    let direction = if book.meta.rtl { DIR_RTL } else { DIR_LTR };
    let mut sections: Vec<SectionOut> = Vec::new();
    let mut entities: Vec<Entity> = Vec::new();
    let mut id_map: HashMap<(String, String), (i64, usize)> = HashMap::new();
    let mut cover_tmpl: Option<i64> = None;
    // 没有可见内容的文件（只有隐藏标题之类）：目录项、链接改指到下一个版面的开头。
    let mut empty_docs: Vec<String> = Vec::new();

    // 先把所有文档解析成块（注释配对要看全书），再逐个生成版面。
    let mut parsed: Vec<ParsedDoc> = Vec::new();
    // 外部样式表按路径只解析一次（大合集几百个文档共用一份样式表）。
    let mut sheets: HashMap<String, std::rc::Rc<crate::css::Rules>> = HashMap::new();
    for (si, doc) in book.docs.iter().enumerate() {
        let html = Html::parse_document(&doc.html);
        let mut sheet = Sheet::default();
        let mut order = 0;
        for el in html.select(&scraper::Selector::parse("link, style").unwrap_or_else(|_| unreachable!())) {
            // `<link>`/`<style>` 的 `media` 属性和 `@media` 同一口径
            if el.value().attr("media").is_some_and(|m| !crate::css::media_ok(m, b.media.as_ref())) {
                continue;
            }
            if el.value().name() == "style" {
                order = sheet.add_at(&el.text().collect::<String>(), order, &doc.path, b.media.as_ref());
            } else if el.value().attr("rel").is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet")) {
                if let Some(h) = el.value().attr("href") {
                    let path = resolve_link(&doc.path, h).0;
                    if let Some(c) = css.get(path.as_str()) {
                        let rules = sheets.entry(path).or_insert_with_key(|p| crate::css::Rules::parse(c, p, b.media.as_ref())).clone();
                        order = sheet.add_rules(rules, order);
                    }
                }
            }
        }
        let root = html.root_element();
        let lang = root.value().attr("xml:lang").or_else(|| root.value().attr("lang")).map(str::to_string).or_else(|| book_lang.clone());
        let d = Doc { path: &doc.path, sheet, lang: lang.clone(), pending: Default::default() };
        let mut blocks = Vec::new();
        let root_comp = d.comp(&root, &Computed::root());
        if let Some(body) = root.children().filter_map(ElementRef::wrap).find(|e| e.value().name() == "body") {
            // body 也当一层包裹：它的水平外边距加到每个块上（样本里 body 的 5pt 边距出现在每个段落上）。
            d.block(body, d.comp(&body, &root_comp), &mut blocks);
        }
        attach_trailing(&mut blocks, d.take_pending());
        collapse_siblings(&mut blocks);
        parsed.push((si, &doc.path, d.lang.clone(), blocks));
    }
    mark_notes(&mut parsed);
    b.drop_font = body_font_to_drop(&mut parsed, book);
    for (si, path, lang, blocks) in parsed {
        if blocks.is_empty() {
            empty_docs.push(path.to_string());
            continue;
        }
        let sec_name = b.sym(&format!("sec{si}"));
        let story_name = b.sym(&format!("story{si}"));
        let pool = b.sym(&format!("text{si}"));
        let tmpl_eid = b.eid();
        let mut ctx = SectionCtx { pool, texts: Vec::new(), order: vec![(tmpl_eid, 1)], ids: Vec::new(), resources: Vec::new(), lang };
        // 只有一张图的文档写成整页图片版面（封面、插图页），和样本一样。
        let single_image = blocks.len() == 1 && matches!(blocks[0].kind, Kind::Image { .. });
        let nodes: Vec<Value> = if let (true, Some((cw, ch)), Kind::Image { src }) = (single_image, fixed_canvas, &blocks[0].kind) {
            // 固定版式的一页，照 Amazon 转的异形页样本：整页容器（画布宽高、`$476`、position relative）里放一个绝对定位的
            // 图片节点（宽高、上、左）。比画布小的图有的页整页空白是资源符号顺序造成的，不是这里（见下面分配资源符号处）。
            let Some(r) = b.resource(src) else { continue };
            let (cid, iid) = (b.eid(), b.eid());
            ctx.order.push((cid, 1));
            ctx.order.push((iid, 1));
            ctx.resources.push(r);
            let (w, h) = fixed_image_size((i64::from(b.resources[r].width), i64::from(b.resources[r].height)), (cw, ch));
            let res = b.sym(&b.resources[r].name.clone());
            if cw > 0 {
                let cstyle = b.style(vec![
                    (P_WIDTH, Value::F64(cw as f64)),
                    (P_HEIGHT, Value::F64(ch as f64)),
                    (P_SIZING, Value::Symbol(SIZING_VALUE)),
                    (P_CLIP, Value::Bool(true)),
                    (P_POSITION, Value::Symbol(POSITION_RELATIVE)),
                ]);
                let istyle = b.style(vec![
                    (P_WIDTH, Value::F64(w as f64)),
                    (P_HEIGHT, Value::F64(h as f64)),
                    (P_SIZING, Value::Symbol(SIZING_VALUE)),
                    (P_TOP, Value::F64(((ch - h) / 2) as f64)),
                    (P_LEFT, Value::F64(((cw - w) / 2) as f64)),
                    (P_POSITION, Value::Symbol(POSITION_ABSOLUTE)),
                ]);
                let img = Value::Struct(vec![(EID, Value::Int(iid)), (STYLE_REF, Value::Symbol(istyle)), (NODE_TYPE, Value::Symbol(NODE_IMAGE)), (RESOURCE_REF, Value::Symbol(res))]);
                vec![Value::Struct(vec![
                    (EID, Value::Int(cid)),
                    (TMPL_FIT, Value::Symbol(CONTAINER_LAYOUT)),
                    (STYLE_REF, Value::Symbol(cstyle)),
                    (NODE_TYPE, Value::Symbol(NODE_CONTAINER)),
                    (CHILDREN, Value::List(vec![img])),
                ])]
            } else {
                let style = b.style(vec![(P_WIDTH, Value::F64(w as f64)), (P_HEIGHT, Value::F64(h as f64)), (P_SIZING, Value::Symbol(SIZING_VALUE))]);
                vec![Value::Struct(vec![(EID, Value::Int(iid)), (STYLE_REF, Value::Symbol(style)), (NODE_TYPE, Value::Symbol(NODE_IMAGE)), (RESOURCE_REF, Value::Symbol(res))])]
            }
        } else {
            blocks.iter().filter_map(|bl| b.node(bl, 1.0, &mut ctx)).collect()
        };
        if nodes.is_empty() {
            empty_docs.push(path.to_string());
            continue;
        }
        let mut tmpl = vec![(EID, Value::Int(tmpl_eid)), (STORYLINE_REF, Value::Symbol(story_name))];
        if let (true, Some((cw, ch))) = (single_image, fixed_canvas.filter(|c| c.0 > 0)) {
            tmpl.push((TMPL_WIDTH, Value::Int(cw)));
            tmpl.push((TMPL_HEIGHT, Value::Int(ch)));
            tmpl.push((FIXED_PAGE_FIT, Value::Symbol(FIXED_PAGE_FIT_VALUE)));
            tmpl.push((P_FONT_SIZE, Value::F64(16.0)));
            tmpl.push((DOC_DIRECTION, Value::Symbol(direction)));
            tmpl.push((DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)));
            tmpl.push((TMPL_FIT, Value::Symbol(TMPL_FIT_VALUE)));
            tmpl.push((TMPL_ALIGN, Value::Symbol(ALIGN_CENTER)));
            tmpl.push((NODE_TYPE, Value::Symbol(NODE_CONTAINER)));
            if book.cover.as_deref() == b.res_by_path.iter().find(|(_, &i)| i == ctx.resources[0]).map(|(p, _)| p.as_str()) {
                cover_tmpl.get_or_insert(tmpl_eid);
            }
        } else if single_image {
            let r = &b.resources[ctx.resources[0]];
            tmpl.push((TMPL_WIDTH, Value::Int(i64::from(r.width))));
            tmpl.push((TMPL_HEIGHT, Value::Int(i64::from(r.height))));
            tmpl.push((TMPL_FIT, Value::Symbol(TMPL_FIT_VALUE)));
            tmpl.push((TMPL_ALIGN, Value::Symbol(ALIGN_CENTER)));
            tmpl.push((NODE_TYPE, Value::Symbol(NODE_CONTAINER)));
            if book.cover.as_deref() == b.res_by_path.iter().find(|(_, &i)| i == ctx.resources[0]).map(|(p, _)| p.as_str()) {
                cover_tmpl.get_or_insert(tmpl_eid);
            }
        } else {
            tmpl.push((NODE_TYPE, Value::Symbol(NODE_TEXT)));
        }
        entities.push(ent(sec_name, T_SECTION, Value::Struct(vec![(SECTION_REF, Value::Symbol(sec_name)), (TEMPLATES, Value::List(vec![Value::Struct(tmpl)]))])));
        entities.push(ent(story_name, T_STORYLINE, Value::Struct(vec![(STORYLINE_REF, Value::Symbol(story_name)), (CHILDREN, Value::List(nodes))])));
        if !ctx.texts.is_empty() {
            entities.push(ent(pool, T_TEXT_POOL, Value::Struct(vec![(NAME, Value::Symbol(pool)), (CHILDREN, Value::List(std::mem::take(&mut ctx.texts)))])));
        }
        let first_eid = ctx.order.get(1).map_or(tmpl_eid, |o| o.0);
        id_map.insert((path.to_string(), String::new()), (first_eid, 0));
        for p in empty_docs.drain(..) {
            id_map.insert((p, String::new()), (first_eid, 0));
        }
        for (i, e, o) in &ctx.ids {
            id_map.entry((path.to_string(), i.clone())).or_insert((*e, *o));
        }
        let length = ctx.order.iter().map(|o| o.1).sum();
        sections.push(SectionOut {
            name: sec_name,
            length,
            eids: ctx.order.iter().map(|o| o.0).collect(),
            pid_map: pid_map(&ctx.order),
            resources: std::mem::take(&mut ctx.resources),
            first_eid,
        });
    }
    if sections.is_empty() {
        return Err("书里没有可显示的内容".into());
    }
    if let Some(last) = sections.last() {
        for p in empty_docs.drain(..) {
            id_map.insert((p, String::new()), (last.first_eid, 0));
        }
    }
    // 封面图即使没出现在正文里也要带上（书架缩略图）。
    let cover_res = book.cover.as_deref().and_then(|c| b.resource(c));
    // 图片字节的实体名。封面的要叫「文档数据 `$538.$597.$614` 的名字 + `-ad`」：Kindle 书架缩略图按这个找
    // （样本里元数据的 `cover_image` 一律写 `e6`，常常指到别的插图甚至锚点，缩略图照样对；
    // 把这个实体改名缩略图就没了，2026-10-05 真机）。
    let raw_name = |i: usize, name: &str| if Some(i) == cover_res { format!("{COVER_AUX}-ad") } else { format!("{name}-ad") };

    // 样式、资源。
    for (name, props) in std::mem::take(&mut b.style_entities) {
        let s = b.sym(&name);
        let mut f = props;
        f.push((STYLE_NAME, Value::Symbol(s)));
        entities.push(ent(s, T_STYLE, Value::Struct(f)));
    }
    let res_list: Vec<(String, String, u32, &'static str, u32, u32)> =
        b.resources.iter().map(|r| (r.name.clone(), r.location.clone(), r.format, r.mime, r.width, r.height)).collect();
    // 嵌入字体：样式里用到、书里有 `@font-face` 和字体文件的（正文、批注的字体优化器已经去掉，不会出现在这里）。
    let mut faces: HashMap<String, crate::css::FontFace> = HashMap::new();
    for (path, text) in &book.css {
        for f in crate::css::font_faces(text, path) {
            faces.entry(f.family.to_lowercase()).or_insert(f);
        }
    }
    let font_files: HashMap<&str, &Vec<u8>> = book.fonts.iter().map(|(p, b)| (p.as_str(), b)).collect();
    let fonts: Vec<(String, crate::css::FontFace, Vec<u8>)> = b
        .used_fonts
        .iter()
        .filter_map(|f| {
            let face = faces.get(&f.to_lowercase())?;
            let bytes = font_files.get(face.path.as_str())?;
            Some((f.clone(), face.clone(), (*bytes).clone()))
        })
        .collect();
    // 阅读顺序、位置映射。
    let order_list = Value::List(sections.iter().map(|s| Value::Symbol(s.name)).collect());
    let reading_orders =
        Value::List(vec![Value::Struct(vec![(READING_ORDER_NAME, Value::Symbol(DEFAULT_READING_ORDER)), (SECTIONS, order_list)])]);
    entities.push(ent(NO_NAME, T_READING_ORDERS, Value::Struct(vec![(READING_ORDERS, reading_orders.clone())])));
    entities.push(ent(
        NO_NAME,
        T_SECTION_EIDS,
        Value::List(sections.iter().map(|s| Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (LIST, compress_ids(s.eids.clone()))])).collect()),
    ));
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for s in &sections {
        ranges.push(Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (START, Value::Int(pos as i64)), (LENGTH, Value::Int(s.length as i64))]));
        pos += s.length;
    }
    entities.push(ent(NO_NAME, T_SECTION_RANGES, Value::Struct(vec![(LIST, Value::List(ranges))])));
    for s in &sections {
        entities.push(ent(s.name, T_SECTION_PID_MAP, Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (LIST, Value::List(s.pid_map.clone()))])));
    }

    // 目录：锚点 + 导航。
    let mut toc_stack: Vec<(u32, Vec<Value>)> = vec![(0, Vec::new())];
    for t in &book.toc {
        let Some(&(eid, off)) = id_map.get(&(t.path.clone(), t.frag.clone())).or_else(|| id_map.get(&(t.path.clone(), String::new()))) else {
            b.warnings.push(format!("目录项找不到位置：{}", t.label));
            continue;
        };
        let level = t.level + 1;
        while toc_stack.len() > 1 && toc_stack.last().is_some_and(|(l, _)| *l >= level) {
            close_level(&mut toc_stack);
        }
        b.anchor((t.path.clone(), t.frag.clone()));
        let target = Value::Struct(vec![(EID, Value::Int(eid)), (OFFSET, Value::Int(off as i64))]);
        let entry = Value::Struct(vec![(NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String(t.label.clone()))])), (NAV_TARGET, target)]);
        if let Some(last) = toc_stack.last_mut() {
            last.1.push(entry);
        }
        toc_stack.push((level, Vec::new()));
    }
    while toc_stack.len() > 1 {
        close_level(&mut toc_stack);
    }
    let mut toc_entries = toc_stack.pop().map(|t| t.1).unwrap_or_default();
    if toc_entries.is_empty() {
        // 没有目录：每个版面一项，标签用序号。
        toc_entries = sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Value::Struct(vec![
                    (NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String((i + 1).to_string()))])),
                    (NAV_TARGET, Value::Struct(vec![(EID, Value::Int(s.first_eid)), (OFFSET, Value::Int(0))])),
                ])
            })
            .collect();
    }
    // 锚点（链接、目录的目标）。找不到目标的指向目标文件开头，文件也找不到就指向书的开头。
    let book_start = (sections[0].first_eid, 0usize);
    for (key, an) in b.anchors.clone() {
        let pos = id_map.get(&key).or_else(|| id_map.get(&(key.0.clone(), String::new()))).copied().unwrap_or_else(|| {
            b.warnings.push(format!("链接目标找不到：{}#{}", key.0, key.1));
            book_start
        });
        let target = Value::Struct(vec![(EID, Value::Int(pos.0)), (OFFSET, Value::Int(pos.1 as i64))]);
        entities.push(ent(an, T_ANCHOR, Value::Struct(vec![(ANCHOR_NAME, Value::Symbol(an)), (ANCHOR_POSITION, target)])));
    }
    let mut nav_names = Vec::new();
    // 标题导航：每个标题级别一组（样本里按级别第一次出现的顺序排）。
    if !b.headings.is_empty() {
        let mut levels: Vec<u8> = Vec::new();
        for (l, _) in &b.headings {
            if !levels.contains(l) {
                levels.push(*l);
            }
        }
        let unit = |eid: i64| {
            vec![
                (NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String("heading-nav-unit".into()))])),
                (NAV_TARGET, Value::Struct(vec![(EID, Value::Int(eid)), (OFFSET, Value::Int(0))])),
            ]
        };
        let groups: Vec<Value> = levels
            .iter()
            .map(|&l| {
                let eids: Vec<i64> = b.headings.iter().filter(|(hl, _)| *hl == l).map(|h| h.1).collect();
                let mut f = vec![(NAV_LANDMARK_TYPE, Value::Symbol(HEADING_LEVEL_1 + u32::from(l) - 1))];
                f.extend(unit(eids[0]));
                f.push((NAV_ENTRIES, Value::List(eids.iter().map(|&e| Value::Struct(unit(e))).collect())));
                Value::Struct(f)
            })
            .collect();
        let n = b.sym("nav-headings");
        nav_names.push(Value::Symbol(n));
        entities.push(ent(n, T_NAV_CONTAINER, Value::Struct(vec![(NAV_TYPE, Value::Symbol(NAV_TYPE_HEADINGS)), (NAV_NAME, Value::Symbol(n)), (NAV_ENTRIES, Value::List(groups))])));
    }
    if let Some(ct) = cover_tmpl {
        let n = b.sym("nav-landmarks");
        nav_names.push(Value::Symbol(n));
        entities.push(ent(
            n,
            T_NAV_CONTAINER,
            Value::Struct(vec![
                (NAV_TYPE, Value::Symbol(NAV_TYPE_LANDMARKS)),
                (NAV_NAME, Value::Symbol(n)),
                (
                    NAV_ENTRIES,
                    Value::List(vec![Value::Struct(vec![
                        (NAV_LANDMARK_TYPE, Value::Symbol(LANDMARK_COVER)),
                        (NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String("cover-nav-unit".into()))])),
                        (NAV_TARGET, Value::Struct(vec![(EID, Value::Int(ct)), (OFFSET, Value::Int(0))])),
                    ])]),
                ),
            ]),
        ));
    }
    let toc_name = b.sym("nav-toc");
    nav_names.push(Value::Symbol(toc_name));
    entities.push(ent(
        toc_name,
        T_NAV_CONTAINER,
        Value::Struct(vec![(NAV_TYPE, Value::Symbol(NAV_TYPE_TOC)), (NAV_NAME, Value::Symbol(toc_name)), (NAV_ENTRIES, Value::List(toc_entries))]),
    ));
    entities.push(ent(
        NO_NAME,
        T_NAV_ROOTS,
        Value::List(vec![Value::Struct(vec![(READING_ORDER_NAME, Value::Symbol(DEFAULT_READING_ORDER)), (NAV_CONTAINERS, Value::List(nav_names))])]),
    ));
    entities.push(ent(NO_NAME, T_NAV_EMPTY, Value::Struct(vec![(NAV_ENTRIES, Value::List(Vec::new()))])));

    // 元数据。
    let content_id = format!("{:016X}{:016X}", id, fnv64(&id.to_le_bytes()));
    let container_id = container_id(id);
    let book_id = base32(id.rotate_left(17), 23);
    let kv = |k: &str, v: Value| Value::Struct(vec![(META_KEY, Value::String(k.into())), (META_VALUE, v)]);
    let s = |v: &str| Value::String(v.to_string());
    let mut title_meta = vec![
        kv("book_id", s(&book_id)),
        kv("title", s(&book.meta.title)),
        kv("publisher", s(&book.meta.publisher)),
        kv("language", s(&meta_language(book_lang.as_deref()))),
        kv("issue_date", s(book.meta.date.get(..10).unwrap_or("2000-01-01"))),
        kv("content_id", s(&content_id)),
        kv("cde_content_type", s("PDOC")),
        kv("ASIN", s(&content_id)),
        kv("is_sample", Value::Bool(false)),
        kv("asset_id", s(&container_id)),
    ];
    for a in &book.meta.authors {
        title_meta.push(kv("author", s(a)));
    }
    if cover_res.is_some() {
        title_meta.push(kv("cover_image", s(COVER_REF)));
    }
    let group = |name: &str, entries: Vec<Value>| Value::Struct(vec![(META_GROUP_NAME, s(name)), (META_ENTRIES, Value::List(entries))]);
    entities.push(ent(
        NO_NAME,
        T_METADATA,
        Value::Struct(vec![(
            META_GROUPS,
            Value::List(vec![
                if fixed_canvas.is_some() {
                    // 固定版式（同 Amazon 转的测试漫画）：锁竖屏、声明固定版式
                    let lock = book.meta.fixed_layout.iter().find(|(n, _)| *n == 124).map_or("portrait", |(_, v)| v.as_str());
                    group("kindle_ebook_metadata", vec![kv("book_orientation_lock", s(lock))])
                } else {
                    group("kindle_ebook_metadata", vec![kv("nested_span", s("enabled")), kv("selection", s("enabled"))])
                },
                group("kindle_title_metadata", title_meta),
                group("kindle_audit_metadata", vec![kv("creator_version", s(FILE_CREATOR_VERSION)), kv("file_creator", s("epub-to-kfx"))]),
            ]
            .into_iter()
            .chain(fixed_canvas.map(|_| group("kindle_capability_metadata", vec![kv("yj_fixed_layout", Value::Int(1)), kv("continuous_popup_progression", Value::Int(0))])))
            .collect()),
        )]),
    ));

    // 文档数据（照样本的值，含义还没全弄清）。
    let mut doc_data = if fixed_canvas.is_some() {
        vec![(DOC_DIRECTION, Value::Symbol(direction)), (DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)), (DOC_FIXED, Value::Symbol(DOC_FIXED_VALUE))]
    } else {
        vec![
            (112, Value::Symbol(383)),
            (P_FONT_SIZE, num(1.0, U_EM)),
            (DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)),
            (DOC_DIRECTION, Value::Symbol(direction)),
            (436, Value::Symbol(441)),
        ]
    };
    if cover_res.is_some() {
        let aux = b.sym(COVER_AUX);
        doc_data.push((DOC_AUX, Value::Struct(vec![(DOC_AUX_NAME, Value::Symbol(aux))])));
    }
    doc_data.push((ion::SID_MAX_ID, Value::Int(b.next_eid)));
    if fixed_canvas.is_none() {
        doc_data.push((P_LINE_HEIGHT, num(LH_EM, U_EM)));
    }
    doc_data.extend([(477, Value::Symbol(56)), (READING_ORDERS, reading_orders)]);
    entities.push(ent(NO_NAME, T_DOCUMENT_DATA, Value::Struct(doc_data)));

    // 资源（字节实体名、资源路径）的符号最后分配：书里不能有实体的 id 排在资源路径的符号后面。以前资源先分配、目录锚点和
    // 导航后分配，固定版式里比画布小的图有的页整页空白（哪几页随实体集合变）；Amazon 转的书从来不这样排，给它加上排在后面的
    // 锚点也出空白页（2026-10-06 真机，42 本测试书对照，见 docs/kfx.md）。
    // 字节实体名、资源路径：图片在前、字体在后，一起按组分配符号。
    let mut raws: Vec<(String, String)> = res_list.iter().enumerate().map(|(i, (name, loc, ..))| (raw_name(i, name), loc.clone())).collect();
    raws.extend((0..fonts.len()).map(|i| (format!("font{i}-ad"), format!("resource/font{i}"))));
    // 图片字节实体和资源路径的符号要隔 9 个：Kindle 按「`$165` 资源路径的符号编号 − 9」找图片字节
    // （6 本样本 443 个资源全是这样；只改资源路径或只给字节实体改名，书架缩略图就没了，2026-10-05 真机）。
    // 每 9 个资源一组：先这组的字节实体名，不满 9 个用占位符号补齐，再这组的资源路径。
    let mut raw_sids = vec![0u32; raws.len()];
    for (g, chunk) in raws.chunks(SID_GAP as usize).enumerate() {
        let base = g * SID_GAP as usize;
        for (k, (raw, _)) in chunk.iter().enumerate() {
            raw_sids[base + k] = b.sym(raw);
        }
        for k in chunk.len()..SID_GAP as usize {
            b.sym(&format!("pad{g}-{k}"));
        }
        for (k, (_, loc)) in chunk.iter().enumerate() {
            let l = b.sym(loc);
            debug_assert_eq!(l, raw_sids[base + k] + SID_GAP);
        }
    }
    for (i, (family, face, bytes)) in fonts.into_iter().enumerate() {
        let (raw, loc) = &raws[res_list.len() + i];
        entities.push(ent(
            NO_NAME,
            T_FONT,
            Value::Struct(vec![
                (P_FONT_FAMILY, Value::String(family)),
                (P_FONT_STYLE, Value::Symbol(if face.italic { STYLE_ITALIC } else { FONT_NORMAL })),
                (P_FONT_WEIGHT, Value::Symbol(if face.bold { WEIGHT_BOLD } else { WEIGHT_NORMAL })),
                (P_FONT_STRETCH, Value::Symbol(FONT_NORMAL)),
                (RES_LOCATION, Value::String(loc.clone())),
            ]),
        ));
        let id = b.local_index[raw];
        entities.push(Entity { id, ty: T_RAW_FONT, version: 1, header: entity_header(), body: Body::Raw(bytes) });
    }
    for (i, (name, loc, format, mime, w, h)) in res_list.iter().enumerate() {
        let n = b.sym(name);
        let l = raw_sids[i];
        entities.push(ent(
            n,
            T_RESOURCE,
            Value::Struct(vec![
                (RES_FORMAT, Value::Symbol(*format)),
                (RES_MIME, Value::String(mime.to_string())),
                (RES_LOCATION, Value::String(loc.clone())),
                (RES_WIDTH, Value::Int(i64::from(*w))),
                (RESOURCE_REF, Value::Symbol(n)),
                (RES_HEIGHT, Value::Int(i64::from(*h))),
            ]),
        ));
        let bytes = std::mem::take(&mut b.resources[i].bytes);
        entities.push(Entity { id: l, ty: T_RAW_MEDIA, version: 1, header: entity_header(), body: Body::Raw(bytes) });
    }

    // 按类型排序（和样本一样），清单放最后。
    entities.sort_by_key(|e| e.ty);
    let mut deps = Vec::new();
    for s in &sections {
        if !s.resources.is_empty() {
            // 样本里每个资源列两次（含义不明，照抄）。
            let mut names: Vec<Value> = Vec::new();
            for &r in &s.resources {
                let v = Value::Symbol(b.local_index[&b.resources[r].name]);
                if !names.contains(&v) {
                    names.push(v.clone());
                    names.push(v);
                }
            }
            deps.push(Value::Struct(vec![(EID, Value::Symbol(s.name)), (MANIFEST_DEP_LIST, Value::List(names))]));
        }
    }
    for (i, r) in b.resources.iter().enumerate() {
        deps.push(Value::Struct(vec![
            (EID, Value::Symbol(b.local_index[&r.name])),
            (MANIFEST_DEP_LIST, Value::List(vec![Value::Symbol(b.local_index[&raw_name(i, &r.name)])])),
        ]));
    }
    let all_ids = Value::List(entities.iter().map(|e| Value::Symbol(e.id)).collect());
    let manifest = Value::Annotated(
        vec![T_MANIFEST],
        Box::new(Value::Struct(vec![
            (MANIFEST_CONTAINERS, Value::List(vec![Value::Struct(vec![(EID, s(&container_id)), (LIST, all_ids)])])),
            (MANIFEST_DEPS, Value::List(deps)),
        ])),
    );
    entities.push(ent(NO_NAME, T_MANIFEST, manifest));

    // 容器。
    let symtab = vec![
        Item::Bvm,
        Item::Value(Value::Annotated(
            vec![ion::SID_ION_SYMBOL_TABLE],
            Box::new(Value::Struct(vec![
                (
                    ion::SID_IMPORTS,
                    Value::List(vec![Value::Struct(vec![
                        (ion::SID_NAME, s("YJ_symbols")),
                        (ion::SID_VERSION, Value::Int(10)),
                        (ion::SID_MAX_ID, Value::Int(YJ_SYMBOLS_MAX_ID)),
                    ])]),
                ),
                (ion::SID_SYMBOLS, Value::List(b.locals.iter().map(|l| s(l)).collect())),
            ])),
        )),
    ];
    let caps = vec![
        Item::Bvm,
        Item::Value(Value::Annotated(
            vec![CAPABILITIES],
            Box::new(Value::List(vec![
                Value::Struct(vec![(META_KEY, s("kfxgen.positionMaps")), (ion::SID_VERSION, Value::Int(2))]),
                Value::Struct(vec![(META_KEY, s("kfxgen.pidMapWithOffset")), (ion::SID_VERSION, Value::Int(1))]),
            ])),
        )),
    ];
    use crate::container::*;
    let info = Value::Struct(vec![
        (SID_CONTAINER_ID, s(&container_id)),
        (SID_COMPRESSION, Value::Int(0)),
        (SID_DRM_SCHEME, Value::Int(0)),
        (SID_INDEX_OFFSET, Value::Int(0)),
        (SID_INDEX_LENGTH, Value::Int(0)),
        (SID_SYMTAB_OFFSET, Value::Int(0)),
        (SID_SYMTAB_LENGTH, Value::Int(0)),
        (SID_CHUNK_SIZE, Value::Int(4096)),
        (SID_CAPS_OFFSET, Value::Int(0)),
        (SID_CAPS_LENGTH, Value::Int(0)),
    ]);
    let kfxgen = format!(
        "[{{key:\"kfxgen_package_version\",value:\"epub-to-kfx {FILE_CREATOR_VERSION}\"}},{{key:\"kfxgen_payload_sha1\",value:\"{}\"}},{{key:\"kfxgen_acr\",value:\"{container_id}\"}}]",
        "0".repeat(40)
    );
    let c = Container { version: 2, info, symtab, capabilities: caps, kfxgen: kfxgen.into_bytes(), entities };
    Ok(c.to_bytes())
}

/// 元数据里写的语言。中文书写 `en`：Kindle 只看元数据语言决定开不开「Aa → 间距」里的段间距、字间距、字符间距，
/// 中文书不开；正文样式里的 `$10` 仍是书本身的语言，中文排版（字体、避头尾、标点挤压）不受影响。代价是长按查词
/// 默认弹英文词典，要在右下角手动切到中文词典（用户 2026-10-05 选的，真机「封面结构63」）。
fn meta_language(book_lang: Option<&str>) -> String {
    match book_lang {
        Some(l) if l.to_ascii_lowercase().starts_with("zh") => "en".to_string(),
        Some(l) => l.to_string(),
        None => "en".to_string(),
    }
}

fn close_level(stack: &mut Vec<(u32, Vec<Value>)>) {
    let Some((_, kids)) = stack.pop() else { return };
    if kids.is_empty() {
        return;
    }
    if let Some(Value::Struct(f)) = stack.last_mut().and_then(|p| p.1.last_mut()) {
        f.push((NAV_ENTRIES, Value::List(kids)));
    }
}

/// 固定版式一页里图片的显示宽高：保持比例缩进画布，**不超过图自己的尺寸**——节点比图片资源大时 Kindle 不放大、整页空白
/// （2026-10-06 真机：954×1272 的图写成画布 1272×1696 → 空白页）。所以比例和画布一致（`comicfxl::fills_canvas`，误差 1px 内）
/// 且不比画布小的写画布大小，比画布小的、比例不一致的（装饰小图、窄页、跨页）按缩放后的尺寸，由调用方居中。
/// 以前一律写画布大小：比例不一致的被拉伸，小的空白。画布宽高未知（0）或图的尺寸取不到时退回另一方。
fn fixed_image_size((iw, ih): (i64, i64), (cw, ch): (i64, i64)) -> (i64, i64) {
    if cw <= 0 || ch <= 0 {
        return (iw, ih);
    }
    if iw <= 0 || ih <= 0 || (iw >= cw && bookconv::comicfxl::fills_canvas((iw as u32, ih as u32), cw as u32, ch as u32)) {
        return (cw, ch);
    }
    let s = (cw as f64 / iw as f64).min(ch as f64 / ih as f64).min(1.0);
    (((iw as f64 * s).round() as i64).max(1), ((ih as f64 * s).round() as i64).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一本小 EPUB（封面、章节、粗体、换行、书内链接、两级目录）走一遍，核对真机定下来的几条规矩。
    #[test]
    fn end_to_end_rules() {
        use crate::container::Container;
        use crate::ion::SymbolTable;
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(4, 6, image::Luma([128]))).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("OEBPS/content.opf", r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>书</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="img" href="c.jpg" media-type="image/jpeg" properties="cover-image"/><item id="cv" href="cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="cv"/><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#.as_bytes()).unwrap();
        w.put("OEBPS/c.jpg", &jpeg).unwrap();
        w.put("OEBPS/nav.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="c1.xhtml">第一章</a><ol><li><a href="c2.xhtml#s">1</a></li></ol></li></ol></nav></body></html>"#.as_bytes()).unwrap();
        w.put("OEBPS/cover.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="c.jpg"/></div></body></html>"#).unwrap();
        w.put("OEBPS/c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{text-indent:2em}</style></head><body><h1>第一章</h1><p>甲<b>乙</b>丙<br/>丁 <a href="c2.xhtml#s">跳</a></p></body></html>"#.as_bytes()).unwrap();
        w.put("OEBPS/c2.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h3 id="s">1</h3><p>戊</p></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();

        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = Container::parse(&kfx).unwrap();
        let syms: SymbolTable = c.symbols();
        let of = |ty: u32| c.entities.iter().filter(move |e| e.ty == ty);
        // 文字一个不多一个不少，`<br>` 是 `\n`。
        let texts: Vec<String> = of(T_TEXT_POOL).flat_map(|e| e.value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().iter().map(|v| v.as_str().unwrap().to_string())).collect();
        assert_eq!(texts, ["第一章", "甲乙丙\n丁 跳", "1", "戊"]);
        // 图片字节：资源路径的符号编号 − 9 = 字节实体（且就是清单里的依赖）。
        let res = of(T_RESOURCE).next().unwrap().value().unwrap().clone();
        let loc = syms.sid(res.field(RES_LOCATION).unwrap().as_str().unwrap()).unwrap();
        let raw = of(T_RAW_MEDIA).next().unwrap();
        assert_eq!(raw.id, loc - SID_GAP);
        // 封面：`cover_image` 的符号编号 − 9 = 封面资源；字节实体叫 `$538.$597.$614` + `-ad`。
        let meta = format!("{:?}", of(T_METADATA).next().unwrap().value().unwrap());
        assert!(meta.contains(&format!("String({COVER_REF:?})")), "{meta}");
        // 中文书的元数据语言写 en（开间距设置），正文样式里仍是 zh。
        assert!(meta.contains(r#"String("language")), (307, String("en"))"#), "{meta}");
        let styles = format!("{:?}", of(T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(styles.contains(r#"(10, String("zh"))"#), "{styles}");
        assert_eq!(syms.sid(COVER_REF).unwrap() - SID_GAP, res.field(RESOURCE_REF).unwrap().as_symbol().unwrap());
        let aux = of(T_DOCUMENT_DATA).next().unwrap().value().unwrap().field(DOC_AUX).unwrap().field(DOC_AUX_NAME).unwrap().as_symbol().unwrap();
        assert_eq!(syms.display(raw.id), format!("{}-ad", syms.display(aux)));
        // 链接指向的锚点存在；目录两级。
        let anchors: Vec<u32> = of(T_ANCHOR).map(|e| e.id).collect();
        let story = format!("{:?}", of(T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(anchors.iter().any(|a| story.contains(&format!("({LINK_TO}, Symbol({a}))"))), "{story}");
        let toc = of(T_NAV_CONTAINER).find(|e| e.value().unwrap().field(NAV_TYPE) == Some(&Value::Symbol(NAV_TYPE_TOC))).unwrap();
        let top = toc.value().unwrap().field(NAV_ENTRIES).unwrap().as_list().unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].field(NAV_ENTRIES).unwrap().as_list().unwrap().len(), 1);
        // 位置：各版面长度之和 = `$609` 游程之和。
        let ranges = of(T_SECTION_RANGES).next().unwrap().value().unwrap().field(LIST).unwrap().as_list().unwrap().to_vec();
        for (r, m) in ranges.iter().zip(of(T_SECTION_PID_MAP)) {
            let sum: i64 = m.value().unwrap().field(LIST).unwrap().as_list().unwrap().iter().map(|x| x.as_list().map_or_else(|| x.as_int().unwrap(), |p| p[0].as_int().unwrap())).sum();
            assert_eq!(sum, r.field(LENGTH).unwrap().as_int().unwrap());
        }
    }

    /// 注释配对：正文链接 → 章末注释，注释开头的回链指回正文链接。只有正文那条算注释引用，回链不算。
    #[test]
    fn notes_paired_by_backlink() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文<a id="r1" href="#n1"><sup>[1]</sup></a>接着</p><p>别的<a href="#x">链接</a></p><p id="x">目标</p><p class="fn"><a id="n1" href="#r1">[1]</a>注释正文</p></body></html>"##.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert_eq!(story.matches(&format!("({NOTE_REF}, Symbol({NOTE_REF_POPUP}))")).count(), 1, "{story}");
        assert_eq!(story.matches(&format!("({NOTE_CONTENT}, Symbol({NOTE_CONTENT_FOOTNOTE}))")).count(), 1, "{story}");
    }

    /// 列表、表格、边框按 2026-10-05 测试书（Amazon 转出的 KFX）对照出来的写法。
    #[test]
    fn lists_tables_borders_like_amazon() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>td{border:1px solid #000;vertical-align:top} .b{border-top:2px dashed red;padding-left:0.5em} li.x{list-style-type:lower-roman}</style></head><body>
            <ul><li>甲<ul><li>乙</li></ul></li></ul>
            <ol start="3"><li class="x">丙</li><li value="9"><p>丁</p><p>戊</p></li></ol>
            <ul style="list-style:none"><li>己</li></ul>
            <table style="border-collapse:collapse;width:80%"><caption>表题</caption><tr><td colspan="2" style="width:30%">庚</td></tr><tr><td rowspan="2">辛</td><td></td></tr></table>
            <div class="b"><p>壬</p></div><hr/><p>前<u>癸</u></p></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        let has = |k: u32, v: u32| story.contains(&format!("({k}, Symbol({v}))"));
        // 列表：嵌套的 ul 换空心圆；li 上写的符号优先；start/value；不显示符号的不是列表
        assert!(has(LIST_STYLE, LIST_DISC) && has(LIST_STYLE, LIST_CIRCLE) && has(LIST_STYLE, LIST_LOWER_ROMAN), "{story}");
        assert_eq!(story.matches(&format!("({NODE_TYPE}, Symbol({NODE_LIST}))")).count(), 3, "{story}");
        assert!(story.contains(&format!("({LIST_START}, Int(3))")) && story.contains(&format!("({LIST_START}, Int(9))")), "{story}");
        // 表格：表 → 表体 → 行 → 单元格，标题；跨列跨行、竖直对齐、列宽、边框合并
        for t in [NODE_TABLE, NODE_TBODY, NODE_ROW] {
            assert!(has(NODE_TYPE, t), "{t}: {story}");
        }
        assert!(has(NOTE_CONTENT, CAPTION) && story.contains(&format!("({TABLE_COLLAPSE}, Bool(true))")) && story.contains(&format!("({TABLE_COLUMNS}, ")), "{story}");
        assert!(styles.contains(&format!("({P_COLSPAN}, Int(2))")) && styles.contains(&format!("({P_ROWSPAN}, Int(2))")), "{styles}");
        assert!(styles.contains(&format!("({P_CELL_VALIGN}, Symbol({VALIGN_TOP}))")), "{styles}");
        // 边框：四边一样写「全部」（1px＝0.45pt），只有上边写上边；左内边距是 `$53`
        assert!(styles.contains(&format!("({}, Symbol({BORDER_SOLID}))", P_BORDER_STYLE[0])) && styles.contains("(307, F64(0.45))"), "{styles}");
        assert!(styles.contains(&format!("({}, Symbol({BORDER_DASHED}))", P_BORDER_STYLE[1])) && styles.contains(&format!("({}, Int({}))", P_BORDER_COLOR[1], 0xFFFF0000u32)), "{styles}");
        assert!(styles.contains(&format!("({P_PADDING_LEFT}, ")), "{styles}");
        assert!(has(NODE_TYPE, NODE_HR) && styles.contains(&format!("({P_UNDERLINE}, Symbol({BORDER_SOLID}))")), "{story}");
        // 文字一个不多一个不少
        let texts: Vec<String> = c.entities.iter().filter(|e| e.ty == T_TEXT_POOL).flat_map(|e| e.value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
        assert_eq!(texts.concat(), "甲乙丙丁戊己表题庚辛壬前癸");
    }

    /// 固定版式（漫画）：照 Amazon 转的测试漫画写元数据、文档数据和每页的画布版面；从右往左翻写 `$559`。
    #[test]
    fn fixed_layout_odd_shaped_images_keep_aspect() {
        assert_eq!(fixed_image_size((1272, 1696), (1272, 1696)), (1272, 1696));
        assert_eq!(fixed_image_size((954, 1272), (1272, 1696)), (954, 1272), "同比例的小图不放大（Kindle 不放大、会空白）");
        assert_eq!(fixed_image_size((1440, 1920), (1272, 1696)), (1272, 1696), "同比例的大图缩到画布");
        assert_eq!(fixed_image_size((2544, 1696), (1272, 1696)), (1272, 848), "跨页按宽缩进画布");
        assert_eq!(fixed_image_size((300, 200), (1272, 1696)), (300, 200), "小图不放大");
        assert_eq!(fixed_image_size((400, 3392), (1272, 1696)), (200, 1696), "窄长图按高缩");
        assert_eq!(fixed_image_size((0, 0), (1272, 1696)), (1272, 1696));
        assert_eq!(fixed_image_size((300, 200), (0, 0)), (300, 200));
    }

    #[test]
    fn fixed_layout_comic_like_amazon() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(1272, 1696, image::Luma([128]))).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><meta name="fixed-layout" content="true"/><meta name="original-resolution" content="1272x1696"/><meta name="orientation-lock" content="portrait"/></metadata><manifest><item id="i" href="i.jpg" media-type="image/jpeg"/><item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/></manifest><spine page-progression-direction="rtl"><itemref idref="p1"/><itemref idref="p2"/></spine></package>"#).unwrap();
        w.put("i.jpg", &jpeg).unwrap();
        for p in ["p1.xhtml", "p2.xhtml"] {
            w.put(p, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="i.jpg" style="width:1272px;height:1696px"/></div></body></html>"#).unwrap();
        }
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let dump = |ty: u32| format!("{:?}", c.entities.iter().filter(|e| e.ty == ty).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        let meta = dump(T_METADATA);
        assert!(meta.contains(r#"String("yj_fixed_layout")), (307, Int(1))"#) && meta.contains(r#"String("book_orientation_lock")), (307, String("portrait"))"#), "{meta}");
        let doc = dump(T_DOCUMENT_DATA);
        assert!(doc.contains(&format!("({DOC_FIXED}, Symbol({DOC_FIXED_VALUE}))")) && doc.contains(&format!("({DOC_DIRECTION}, Symbol({DIR_RTL}))")), "{doc}");
        let secs = dump(T_SECTION);
        assert_eq!(secs.matches(&format!("({TMPL_WIDTH}, Int(1272)), ({TMPL_HEIGHT}, Int(1696)), ({FIXED_PAGE_FIT}, Symbol({FIXED_PAGE_FIT_VALUE}))")).count(), 2, "{secs}");
        assert!(dump(T_STYLE).contains(&format!("({P_WIDTH}, F64(1272.0)), ({P_HEIGHT}, F64(1696.0))")), "{}", dump(T_STYLE));
        // 没有实体的 id 排在资源路径的符号后面（否则 Kindle 上比画布小的图有的页空白）
        let syms = c.symbols();
        let max_loc = c.entities.iter().filter(|e| e.ty == T_RESOURCE).filter_map(|e| e.value()?.field(RES_LOCATION)?.as_str().and_then(|l| syms.sid(l))).max().unwrap();
        let max_ent = c.entities.iter().map(|e| e.id).max().unwrap();
        assert!(max_ent < max_loc, "最大实体 id {max_ent} 不能排在资源路径符号 {max_loc} 后面");
    }

    /// 没嵌入的正文字体不写进样式（Kindle 会换成别的字体，和表格、阅读器设置都对不上）；嵌入的装饰字体照写。
    #[test]
    fn unembedded_body_font_dropped() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{font-family:"AR MingU30"} h1{font-family:"黑体"}</style></head><body><h1>标题</h1><p>很长很长的正文一</p><p>很长很长的正文二</p><table><tr><td>表格</td></tr></table></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(!styles.contains("AR MingU30") && styles.contains("黑体"), "{styles}");
    }

    /// 背景图：同《绍宋》样本，写在包住整页的容器上（`$479` 指向资源、不重复、固定、位置、尺寸）；`url()` 按样式表路径解析。
    #[test]
    fn background_image_like_amazon() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="s" href="S/a.css" media-type="text/css"/><item id="b" href="I/bg.png" media-type="image/png"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("O/S/a.css", br#"body.j{background:url("../I/bg.png") bottom / cover no-repeat fixed rgba(117, 0, 0, 1)}"#).unwrap();
        w.put("O/I/bg.png", &png).unwrap();
        w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../S/a.css"/></head><body class="j"><p>x</p></body></html>"#).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(styles.contains(&format!("({P_BG_IMAGE}, Symbol(")) && styles.contains(&format!("({P_BG_REPEAT}, Symbol({BG_NO_REPEAT}))")) && styles.contains(&format!("({P_BG_ATTACHMENT}, Symbol({BG_FIXED}))")), "{styles}");
        assert!(styles.contains(&format!("({P_BG_SIZE_H}, ")) && styles.contains(&format!("({P_BACKGROUND}, Int({}))", 0xFF750000u32)), "{styles}");
        assert!(c.entities.iter().any(|e| e.ty == T_RESOURCE), "背景图登记成资源");
    }

    /// 图片的百分比宽度写进样式（`$56`，单位百分比，同表格宽度）；别的单位不写（写出器 9）。
    #[test]
    fn image_percent_width() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 8, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
let page = |body: &str| format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>img.l{{width:40%}} img.e{{width:10em}}</style></head><body><p>甲</p>{body}<p>乙</p></body></html>"#);
        let styles_of = |body: &str| {
            let mut w2 = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
            w2.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
            w2.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="i" href="i.png" media-type="image/png"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
            w2.put("i.png", &png).unwrap();
            w2.put("c1.xhtml", page(body).as_bytes()).unwrap();
            let epub = w2.finish().unwrap().into_inner();
            let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            let c = crate::container::Container::parse(&kfx).unwrap();
            format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>())
        };
        let want = |p: f64| format!("({P_WIDTH}, {:?})", num(p, U_PERCENT));
        assert!(styles_of(r#"<div><img src="i.png" style="width:61%"/></div>"#).contains(&want(61.0)), "行内百分比");
        assert!(styles_of(r#"<div><img class="l" src="i.png"/></div>"#).contains(&want(40.0)), "样式表百分比");
        assert!(!styles_of(r#"<div><img class="e" src="i.png"/></div>"#).contains(&format!("({P_WIDTH}, ")), "em 宽度不写");
        assert!(!styles_of(r#"<div><img src="i.png"/></div>"#).contains(&format!("({P_WIDTH}, ")), "没写宽度");
    }

    #[test]
    fn pid_map_matches_sample_shape() {
        // 样本《ABC谋杀案》第 2 个版面：[[0,3090],[1,325],[1,40],[4,333],9,14,6,9,15,[22,0]]
        let order = [(3090, 1), (325, 1), (40, 4), (333, 9), (334, 14), (335, 6), (336, 9), (337, 15), (338, 22)];
        let m = pid_map(&order);
        let expect = Value::List(vec![
            Value::List(vec![Value::Int(0), Value::Int(3090)]),
            Value::List(vec![Value::Int(1), Value::Int(325)]),
            Value::List(vec![Value::Int(1), Value::Int(40)]),
            Value::List(vec![Value::Int(4), Value::Int(333)]),
            Value::Int(9),
            Value::Int(14),
            Value::Int(6),
            Value::Int(9),
            Value::Int(15),
            Value::List(vec![Value::Int(22), Value::Int(0)]),
        ]);
        assert_eq!(Value::List(m), expect);
    }

    #[test]
    fn compress_ids_matches_sample_shape() {
        let ids = vec![3088, 31, 267, 268, 269, 270, 271];
        assert_eq!(
            compress_ids(ids),
            Value::List(vec![Value::Int(31), Value::List(vec![Value::Int(267), Value::Int(5)]), Value::Int(3088)])
        );
    }

    /// 没有文字的元素的 id 也是锚点：挂到文档顺序里下一个块的开头，文末的挂到最后一个块的末尾；图片自己的 id 也算。
    /// 以前丢掉，链接退回文件开头（《福尔摩斯探案全集》目录页「第一册」点了停在目录页开头）。
    #[test]
    fn empty_element_ids_anchor_to_next_block() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="i" href="i.png" media-type="image/png"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("i.png", &png).unwrap();
        w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p><a href="#a">1</a><a href="#b">2</a><a href="#c">3</a><a href="#d">4</a></p><p>甲</p><p><span id="a"></span></p><p>乙</p><div id="b"></div><p><img id="c" src="i.png"/></p><p>丙丁</p><span id="d"></span></body></html>"##.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        // 节点 id：按文字找文字节点，图片节点按类型找
        let story = c.entities.iter().find(|e| e.ty == T_STORYLINE).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
        let pool = c.entities.iter().find(|e| e.ty == T_TEXT_POOL).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
        let eid_of_text = |t: &str| {
            let idx = pool.iter().position(|v| v.as_str() == Some(t)).unwrap() as i64;
            story.iter().find(|n| n.field(TEXT_REF).and_then(|r| r.field(TEXT_INDEX)).and_then(Value::as_int) == Some(idx)).unwrap().field(EID).unwrap().as_int().unwrap()
        };
        let img = story.iter().find(|n| n.field(NODE_TYPE) == Some(&Value::Symbol(NODE_IMAGE))).unwrap().field(EID).unwrap().as_int().unwrap();
        // 链接按出现顺序配锚点：anchor0..3 → #a #b #c #d
        let syms = c.symbols();
        let target = |name: &str| {
            let sid = syms.sid(name).unwrap();
            let a = c.entities.iter().find(|e| e.ty == T_ANCHOR && e.id == sid).unwrap().value().unwrap().field(ANCHOR_POSITION).unwrap().clone();
            (a.field(EID).unwrap().as_int().unwrap(), a.field(OFFSET).unwrap().as_int().unwrap())
        };
        assert_eq!(target("anchor0"), (eid_of_text("乙"), 0), "空段落里的 span → 下一段开头");
        assert_eq!(target("anchor1"), (img, 0), "空 div → 下一块（图片）");
        assert_eq!(target("anchor2"), (img, 0), "图片自己的 id");
        assert_eq!(target("anchor3"), (eid_of_text("丙丁"), 2), "文末的 → 最后一块末尾");
    }

    #[test]
    fn non_finite_lengths_written_as_zero() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(num(v, U_EM), num(0.0, U_EM));
        }
        assert_eq!(num(0.8333333333, U_LH), Value::Struct(vec![(VALUE, Value::F64(0.833333)), (UNIT, Value::Symbol(U_LH))]));
    }

    #[test]
    fn margin_collapse() {
        assert_eq!(collapse(1.0, 1.5), 1.5);
        assert_eq!(collapse(-1.0, 2.0), 1.0);
        assert_eq!(collapse(-1.0, -2.0), -2.0);
    }
}

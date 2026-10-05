//! EPUB → KFX 写出器（最小版）。
//!
//! 输入应是已经按设备优化过的 EPUB；这里只做格式转换，不改内容。
//! 写的片段是删除实验（`docs/kfx.md#删除实验`）定下的最小集合；分词、`$550`/`$621` 位置点、`$597`、`$585` 不写。
//! 现在支持：段落、标题、行内样式（粗体、斜体、颜色、字号、字体、上标）、`<br>`、块级图片和整页图片、目录。
//! 还不支持：链接与注释跳转、表格、列表编号、边框、嵌入字体（`@font-face` 去掉，字体交给阅读器）。

use crate::container::{Body, Entity};
use crate::css::{Computed, Len, Sheet};
use crate::ion::{self, Item, Value};
use crate::yj::*;
use bookconv::epubbook::{self, Loaded};
use bookconv::epubzip::resolve_link;
use bookconv::util::fnv64;
use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node};
use std::collections::{BTreeMap, HashMap};

/// 写出器版本：改了产物字节的修改要加一。
pub const WRITER_VERSION: &str = "1";

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

impl Doc<'_> {
    fn comp(&self, el: &ElementRef, parent: &Computed) -> Computed {
        let decls = self.sheet.cascade(el);
        let mut c = Computed::derive(parent, &decls, el.value().name());
        if let Some(l) = el.value().attr("xml:lang").or_else(|| el.value().attr("lang")) {
            c.lang = Some(l.to_string());
        }
        c
    }

    /// 把一个块级元素展开成块序列。
    fn block(&self, el: ElementRef, parent: &Computed, out: &mut Vec<Block>) {
        let comp = self.comp(&el, parent);
        if comp.display.as_deref() == Some("none") {
            return;
        }
        let name = el.value().name();
        if matches!(name, "head" | "script" | "style" | "title") {
            return;
        }
        let fs = comp.font_size;
        let heading = bookconv::html::heading_level_of(name);
        let mut children = Vec::new();
        self.children(el, &comp, &mut children);
        if children.is_empty() {
            return;
        }
        let margin_top = to_vert(comp.margin[0], fs);
        let margin_bottom = to_vert(comp.margin[2], fs);
        let ml = to_horiz(comp.margin[3], fs);
        let mr = to_horiz(comp.margin[1], fs);
        let padding = [0, 1, 2, 3].map(|i| to_vert(comp.padding[i], fs));
        let id = el.value().attr("id").map(str::to_string);
        // 自己只包着一个文字块（普通段落）：本元素就是这个块。
        // 有背景或内边距的块写成容器套文字（样本里带背景色的 h1 就是这样；背景、内边距、负外边距直接放在文字段落上，
        // Kindle 上长标题会溢出屏幕，2026-10-05 真机）。
        let boxed = comp.background.is_some() || padding.iter().any(|p| *p != 0.0);
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
            if let Some(id) = id {
                b.ids.insert(0, (id, 0));
            }
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
                ids: id.map(|i| vec![(i, 0)]).unwrap_or_default(),
            });
            return;
        }
        // 没有背景的包裹层摊平：竖直外边距、内边距并进首尾子块，水平外边距加到每个子块上。
        let n = children.len();
        for (i, mut c) in children.into_iter().enumerate() {
            if i == 0 {
                c.margin_top = collapse(margin_top + padding[0], c.margin_top);
                if let Some(id) = &id {
                    c.ids.insert(0, (id.clone(), 0));
                }
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
        let ids = taken.ids.into_iter().map(|(i, o)| (i, o.min(chars))).collect();
        out.push(Block::anonymous(Kind::Text { text, runs }, comp.inherited(), ids));
    }

    fn inline_or_block(&self, node: NodeRef<Node>, block_comp: &Computed, parent: &Computed, inl: &mut Inline, out: &mut Vec<Block>) {
        match node.value() {
            Node::Text(t) => inl.push_text(t, false),
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
                    out.push(Block::anonymous(Kind::Image { src: resolve_link(self.path, &src).0 }, comp, Vec::new()));
                    return;
                }
                let comp = self.comp(&el, parent);
                if comp.display.as_deref() == Some("none") || matches!(name, "script" | "style" | "head") {
                    return;
                }
                if is_block_el(&el, &comp) {
                    self.flush(inl, block_comp, out);
                    self.block(el, parent, out);
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
                    inl.runs.insert(run_at, Run { start, len: inl.chars - start, comp: styled.then_some(comp), link });
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
        }
    }

    fn is_anonymous(&self) -> bool {
        self.margin_top == 0.0 && self.margin_bottom == 0.0 && self.heading.is_none() && self.margin_left == Horiz::default()
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
        || a.background.is_some()
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
    let r = (v * 1e6).round() / 1e6;
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
    styles: BTreeMap<String, String>,
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
        for (k, v) in &props {
            if let (P_FONT_FAMILY, Value::String(f)) = (*k, v) {
                self.used_fonts.insert(f.clone());
            }
        }
        let mut key = Vec::new();
        ion::encode_value(&mut key, &Value::Struct(props.clone()));
        let key: String = key.iter().map(|b| format!("{b:02x}")).collect();
        if let Some(name) = self.styles.get(&key).cloned() {
            return self.sym(&name);
        }
        let name = format!("style{}", self.style_entities.len());
        self.styles.insert(key, name.clone());
        self.style_entities.push((name.clone(), props));
        self.sym(&name)
    }

    fn resource(&mut self, path: &str) -> Option<usize> {
        if let Some(&i) = self.res_by_path.get(path) {
            return Some(i);
        }
        let Some((bytes, mime)) = self.images.get(path).cloned() else {
            self.warnings.push(format!("图片找不到或格式不支持：{path}"));
            return None;
        };
        let (format, mime) = match mime {
            "image/jpeg" => (FORMAT_JPG, "image/jpg"),
            "image/png" => (FORMAT_PNG, "image/png"),
            "image/gif" => (FORMAT_GIF, "image/gif"),
            other => {
                self.warnings.push(format!("图片格式 {other} 不支持：{path}"));
                return None;
            }
        };
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
                let mut f = vec![(EID, Value::Int(eid)), (STYLE_REF, Value::Symbol(style)), (NODE_TYPE, Value::Symbol(NODE_TEXT))];
                if let Some(h) = b.heading {
                    f.push((HEADING_LEVEL, Value::Int(i64::from(h))));
                }
                if let Some(h) = b.heading {
                    self.headings.push((h, eid));
                }
                let run_values: Vec<Value> = runs
                    .iter()
                    .map(|r| {
                        let mut f = vec![(OFFSET, Value::Int(r.start as i64)), (LENGTH, Value::Int(r.len as i64))];
                        if let Some(key) = &r.link {
                            f.push((LINK_TO, Value::Symbol(self.anchor(key.clone()))));
                        }
                        if let Some(rc) = &r.comp {
                            let mut rp = text_props(rc, b.comp.font_size);
                            if rc.superscript && !b.comp.superscript {
                                rp.push((P_VERTICAL_ALIGN, Value::Symbol(VALIGN_SUPER)));
                            }
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
                let style = self.style(props);
                let kids: Vec<Value> = children.iter().filter_map(|c| self.node(c, b.comp.font_size, ctx)).collect();
                let mut f = vec![(EID, Value::Int(eid)), (TMPL_FIT, Value::Symbol(CONTAINER_LAYOUT)), (STYLE_REF, Value::Symbol(style))];
                if let Some(h) = b.heading {
                    f.push((HEADING_LEVEL, Value::Int(i64::from(h))));
                }
                f.push((NODE_TYPE, Value::Symbol(NODE_CONTAINER)));
                f.push((CHILDREN, Value::List(kids)));
                Some(Value::Struct(f))
            }
        }
    }
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
    let mut warnings = Vec::new();
    let book = epubbook::load(epub, &mut warnings)?;
    let mut b = Builder {
        locals: Vec::new(),
        local_index: HashMap::new(),
        next_eid: 1,
        styles: BTreeMap::new(),
        style_entities: Vec::new(),
        resources: Vec::new(),
        res_by_path: HashMap::new(),
        images: book.images.iter().map(|i| (i.path.clone(), (i.bytes.clone(), i.mime))).collect(),
        warnings,
        anchors: Vec::new(),
        anchor_index: HashMap::new(),
        headings: Vec::new(),
        used_fonts: Default::default(),
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
    let mut sections: Vec<SectionOut> = Vec::new();
    let mut entities: Vec<Entity> = Vec::new();
    let mut id_map: HashMap<(String, String), (i64, usize)> = HashMap::new();
    let mut cover_tmpl: Option<i64> = None;
    // 没有可见内容的文件（只有隐藏标题之类）：目录项、链接改指到下一个版面的开头。
    let mut empty_docs: Vec<String> = Vec::new();

    for (si, doc) in book.docs.iter().enumerate() {
        let html = Html::parse_document(&doc.html);
        let mut sheet = Sheet::default();
        let mut order = 0;
        for el in html.select(&scraper::Selector::parse("link, style").unwrap_or_else(|_| unreachable!())) {
            if el.value().name() == "style" {
                order = sheet.add(&el.text().collect::<String>(), order);
            } else if el.value().attr("rel").is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet")) {
                if let Some(h) = el.value().attr("href") {
                    if let Some(c) = css.get(resolve_link(&doc.path, h).0.as_str()) {
                        order = sheet.add(c, order);
                    }
                }
            }
        }
        let root = html.root_element();
        let lang = root.value().attr("xml:lang").or_else(|| root.value().attr("lang")).map(str::to_string).or_else(|| book_lang.clone());
        let d = Doc { path: &doc.path, sheet, lang: lang.clone() };
        let mut blocks = Vec::new();
        let root_comp = d.comp(&root, &Computed::root());
        if let Some(body) = root.children().filter_map(ElementRef::wrap).find(|e| e.value().name() == "body") {
            // body 也当一层包裹：它的水平外边距加到每个块上（样本里 body 的 5pt 边距出现在每个段落上）。
            d.block(body, &root_comp, &mut blocks);
        }
        collapse_siblings(&mut blocks);
        if blocks.is_empty() {
            empty_docs.push(doc.path.clone());
            continue;
        }
        let sec_name = b.sym(&format!("sec{si}"));
        let story_name = b.sym(&format!("story{si}"));
        let pool = b.sym(&format!("text{si}"));
        let tmpl_eid = b.eid();
        let mut ctx = SectionCtx { pool, texts: Vec::new(), order: vec![(tmpl_eid, 1)], ids: Vec::new(), resources: Vec::new(), lang: d.lang.clone() };
        // 只有一张图的文档写成整页图片版面（封面、插图页），和样本一样。
        let single_image = blocks.len() == 1 && matches!(blocks[0].kind, Kind::Image { .. });
        let nodes: Vec<Value> = blocks.iter().filter_map(|bl| b.node(bl, 1.0, &mut ctx)).collect();
        if nodes.is_empty() {
            empty_docs.push(doc.path.clone());
            continue;
        }
        let mut tmpl = vec![(EID, Value::Int(tmpl_eid)), (STORYLINE_REF, Value::Symbol(story_name))];
        if single_image {
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
        id_map.insert((doc.path.clone(), String::new()), (first_eid, 0));
        for p in empty_docs.drain(..) {
            id_map.insert((p, String::new()), (first_eid, 0));
        }
        for (i, e, o) in &ctx.ids {
            id_map.entry((doc.path.clone(), i.clone())).or_insert((*e, *o));
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
    let container_id = format!("CR!{}", base32(id, 28));
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
                group("kindle_ebook_metadata", vec![kv("nested_span", s("enabled")), kv("selection", s("enabled"))]),
                group("kindle_title_metadata", title_meta),
                group("kindle_audit_metadata", vec![kv("creator_version", s(WRITER_VERSION)), kv("file_creator", s("epub-to-kfx"))]),
            ]),
        )]),
    ));

    // 文档数据（照样本的值，含义还没全弄清）。
    let mut doc_data = vec![
        (112, Value::Symbol(383)),
        (P_FONT_SIZE, num(1.0, U_EM)),
        (192, Value::Symbol(376)),
        (560, Value::Symbol(557)),
        (436, Value::Symbol(441)),
    ];
    if cover_res.is_some() {
        let aux = b.sym(COVER_AUX);
        doc_data.push((DOC_AUX, Value::Struct(vec![(DOC_AUX_NAME, Value::Symbol(aux))])));
    }
    doc_data.extend([
        (ion::SID_MAX_ID, Value::Int(b.next_eid)),
        (P_LINE_HEIGHT, num(LH_EM, U_EM)),
        (477, Value::Symbol(56)),
        (READING_ORDERS, reading_orders),
    ]);
    entities.push(ent(NO_NAME, T_DOCUMENT_DATA, Value::Struct(doc_data)));

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
        "[{{key:\"kfxgen_package_version\",value:\"epub-to-kfx {WRITER_VERSION}\"}},{{key:\"kfxgen_payload_sha1\",value:\"{}\"}},{{key:\"kfxgen_acr\",value:\"{container_id}\"}}]",
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

        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1) }).unwrap();
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

    #[test]
    fn margin_collapse() {
        assert_eq!(collapse(1.0, 1.5), 1.5);
        assert_eq!(collapse(-1.0, 2.0), 1.0);
        assert_eq!(collapse(-1.0, -2.0), -2.0);
    }
}

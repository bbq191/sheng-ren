//! 解析：XHTML 文档按 CSS 层叠成块（[`Block`]）序列——行内内容收成文字和样式区间，块级元素展开、摊平或写成容器，列表、表格，
//! 外边距折叠、空段折叠；全书文档多线程解析（[`parse_docs`]），元素套得太深的书报错（[`MAX_NESTING`]）。

use super::*;

/// 竖直方向长度，单位：根字号的 em。
pub(super) type Vert = f64;

/// 水平方向长度：根 em + 百分比。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Horiz {
    pub(super) em: f64,
    pub(super) pct: f64,
}

#[derive(Clone, Debug)]
pub(super) struct Run {
    pub(super) start: usize,
    pub(super) len: usize,
    /// 和所在段落不同的样式；`None`＝只是链接。
    pub(super) comp: Option<Computed>,
    /// 书内链接的目标（文件, 锚点）。
    pub(super) link: Option<(String, String)>,
    /// 注释引用（点了弹窗）：见 [`mark_notes`]。
    pub(super) note_ref: bool,
    /// 是 `<a>`（有没有 href 都算）：颜色写成链接的颜色（`$576`/`$577`），见 [`Builder::node`]。
    pub(super) anchor: bool,
}

#[derive(Clone, Debug)]
pub(super) enum Kind {
    Text { text: String, runs: Vec<Run> },
    Image { src: String },
    Container(Vec<Block>),
}

#[derive(Clone, Debug)]
pub(super) struct Block {
    pub(super) kind: Kind,
    pub(super) comp: Computed,
    pub(super) heading: Option<u8>,
    pub(super) margin_top: Vert,
    pub(super) margin_bottom: Vert,
    pub(super) margin_left: Horiz,
    pub(super) margin_right: Horiz,
    pub(super) padding: [Vert; 4],
    /// 左右内边距（右、左）：百分比照写成百分比（Send to Kindle 同样；以前按页宽 32em 换成 em，字号调大以后
    /// 《绍宋》章名横幅 `padding: 0.3em 33%` 的两边内边距跟着变宽，挤掉文字）。上面 `padding` 的左右两项只用来判断有没有内边距。
    pub(super) padding_h: [Horiz; 2],
    /// 本块里元素的 id → 字符偏移（目录、锚点用）。
    pub(super) ids: Vec<(String, usize)>,
    /// 注释正文（弹窗里显示的内容）：见 [`mark_notes`]。
    pub(super) note: bool,
    /// 节点类型（`None`＝文字 `$269`／容器 `$270`）：列表、列表项、表格各层、水平线。
    pub(super) ty: Option<u32>,
    /// 节点上的字段（列表符号、表格边框合并等）。
    pub(super) attrs: Vec<(u32, Value)>,
    /// 额外的样式属性（单元格跨行跨列、竖直对齐，表格宽度等）。
    pub(super) extra: Vec<(u32, Value)>,
    /// 容器（带背景、边框的块，列表项、单元格）里只有本元素文字的匿名文字块：样式只写对齐，别的从容器继承
    /// （Send to Kindle 同样：《罗杰疑案》带背景的章标题，字体、字号、颜色都写在容器上，里面的文字只有 `text-align`）。
    pub(super) bare: bool,
    /// 直接由行内内容合成的文字块（[`Doc::flush`]），不是哪个块级元素自己：包着它的元素可以把它当成自己的文字。
    /// 以前只看有没有边距，`<li><p class="footnote">` 这种没写边距的 `<p>` 也被当成匿名的，样式被 `<li>` 的盖掉（《春雪》注释的字号、缩进、行高）。
    inline: bool,
    /// 前面折掉的空段（[`Doc::gap`]）：外边距折叠之后加到上边距上（[`apply_gaps`]）。
    gap_before: Vert,
}

/// 只有空白的段落（`<p>&nbsp;</p>`、`<p>　</p>`、`<p><br/></p>`、段落之间单独的 `<br/>`）不出节点，折成下一块上边距多出的
/// 本元素行高的这么多倍（Send to Kindle 同样，2026-10-08 量的：《绝叫》空段 0.594 行高、《金庸》`<p><br/></p>` 0.595 × 1.7em、
/// 段落间单独的 `<br/>` 0.72em ＝ 0.6 × 缺省行高 1.2em）。以前整段丢掉、不折，场景空行在 Kindle 上没了。
const BLANK_LINE_FOLD: f64 = 0.6;

/// 绝对长度换成根 em：em 乘本元素字号 `fs`（根 em），pt 按 1em＝12pt；百分比要看包含块，`None`。
pub(super) fn rem_len(l: Len, fs: f64) -> Option<f64> {
    match l {
        Len::Em(n) => Some(n * fs),
        Len::Pt(n) => Some(n / 12.0),
        Len::Percent(_) => None,
    }
}

/// 竖直长度：百分比按页宽 [`PAGE_WIDTH_EM`]。
fn to_vert(l: Option<Len>, fs: f64) -> Vert {
    match l {
        Some(Len::Percent(p)) => p / 100.0 * PAGE_WIDTH_EM,
        Some(l) => rem_len(l, fs).unwrap_or_default(),
        None => 0.0,
    }
}

/// 水平长度：百分比照留百分比。
fn to_horiz(l: Option<Len>, fs: f64) -> Horiz {
    match l {
        Some(Len::Percent(p)) => Horiz { em: 0.0, pct: p },
        Some(l) => Horiz { em: rem_len(l, fs).unwrap_or_default(), pct: 0.0 },
        None => Horiz::default(),
    }
}

/// `xml:lang`（没有就 `lang`）。
fn lang_attr<'a>(el: &ElementRef<'a>) -> Option<&'a str> {
    el.value().attr("xml:lang").or_else(|| el.value().attr("lang"))
}

/// 块的外边距、内边距换算成根 em（竖直）和根 em + 百分比（水平），字段同 [`Block`]。
struct BoxLens {
    margin_top: Vert,
    margin_bottom: Vert,
    margin_left: Horiz,
    margin_right: Horiz,
    padding: [Vert; 4],
    padding_h: [Horiz; 2],
}

impl BoxLens {
    fn of(c: &Computed) -> BoxLens {
        let fs = c.font_size;
        BoxLens {
            margin_top: to_vert(c.margin[0], fs),
            margin_bottom: to_vert(c.margin[2], fs),
            margin_left: to_horiz(c.margin[3], fs),
            margin_right: to_horiz(c.margin[1], fs),
            padding: [0, 1, 2, 3].map(|i| to_vert(c.padding[i], fs)),
            padding_h: [to_horiz(c.padding[1], fs), to_horiz(c.padding[3], fs)],
        }
    }
}

/// CSS 外边距折叠：两个相邻外边距合成一个。
pub(super) fn collapse(a: Vert, b: Vert) -> Vert {
    match (a >= 0.0, b >= 0.0) {
        (true, true) => a.max(b),
        (false, false) => a.min(b),
        _ => a + b,
    }
}

pub(super) struct Doc<'a> {
    pub(super) path: &'a str,
    pub(super) sheet: Sheet,
    pub(super) lang: Option<String>,
    /// 还没落到内容上的锚点：没有文字的元素（`<span id="x"></span>`、`<div id="x"></div>`）的 id。挂到文档顺序里
    /// 下一个生成的块的开头（[`Doc::take_pending`]）。以前直接丢掉，指向它们的链接、目录项退回到文件开头
    /// （《福尔摩斯探案全集》目录页「第一册」指向页末插图前的空锚点，点了停在目录页开头；全书 1102 处）。
    pub(super) pending: std::cell::RefCell<Vec<String>>,
    /// 还没落到块上的空段折出来的高度（根 em，见 [`BLANK_LINE_FOLD`]）：和 `pending` 一样给下一个生成的块。
    pub(super) gap: std::cell::Cell<Vert>,
    /// 包着当前位置、还没走完的行内链接（`<a href>`）的目标，外层在前：里面的块级元素（`<a href><p>…</p></a>`）另起行内缓冲时
    /// 接着开（[`Doc::inline`]）。以前链接只在 `<a>` 自己的缓冲里，里面块的文字没有链接（写出器 15）。
    pub(super) links: std::cell::RefCell<Vec<(String, String)>>,
}

/// 一个还开着的行内区间（[`Inline::open`]）。
#[derive(Clone)]
struct OpenRun {
    start: usize,
    /// 插到 `runs` 的哪个下标（外层的区间排在里层的前面）。
    run_at: usize,
    run: Run,
    /// 开的时候前面有个还没写出的空格（`foo <a>bar</a>`）：这个空格写出来时正好落在起点上，起点后挪一格，空格不算区间里的
    /// （以前算进去：offset 3 len 4，写出器 15）。元素的 id 同样后挪（[`Inline::push_id`]），注释配对按区间起点找 id，两边要一致。
    after_space: bool,
}

/// 收集行内内容：文字（空白按 CSS 折叠）、`<br>`、行内元素的样式区间、遇到图片就切开。
pub(super) struct Inline {
    text: String,
    chars: usize,
    runs: Vec<Run>,
    ids: Vec<(String, usize)>,
    /// `ids` 里登记时前面有个还没写出的空格的（同 [`OpenRun::after_space`]：空格写出来时锚点后挪一格，落在元素的第一个字上）。
    space_ids: Vec<usize>,
    pending_space: bool,
    /// 还没走完的行内元素的区间（外层在前，长度待定）。
    /// 走到一半遇到块或图片要切开（[`Doc::flush`]）时，区间在切开处截断，到新缓冲从 0 接着开（见 [`Inline::cut_open`]）。
    open: Vec<OpenRun>,
}

impl Inline {
    fn new() -> Self {
        Inline { text: String::new(), chars: 0, runs: Vec::new(), ids: Vec::new(), space_ids: Vec::new(), pending_space: false, open: Vec::new() }
    }

    /// 行内元素的 id：锚点在它的第一个字上。
    fn push_id(&mut self, id: &str) {
        if self.pending_space {
            self.space_ids.push(self.ids.len());
        }
        self.ids.push((id.to_string(), self.chars));
    }

    /// 开一个行内区间（行内元素进来时）。
    fn open_run(&mut self, run: Run) {
        self.open.push(OpenRun { start: self.chars, run_at: self.runs.len(), run, after_space: self.pending_space });
    }

    /// 一个行内元素走完（或在切开处截断）：有字的话把区间插进 `runs`。
    fn close_run(&mut self, o: OpenRun) {
        if self.chars > o.start {
            self.runs.insert(o.run_at, Run { start: o.start, len: self.chars - o.start, ..o.run });
        }
    }

    /// 切开之前：还开着的区间（从里层到外层，外层插在前面）截断在这里；返回给新缓冲用的、从 0 重新开始的区间。
    fn cut_open(&mut self) -> Vec<OpenRun> {
        let open = std::mem::take(&mut self.open);
        for o in open.iter().rev() {
            self.close_run(o.clone());
        }
        open.into_iter().map(|o| OpenRun { start: 0, run_at: 0, run: o.run, after_space: false }).collect()
    }

    fn push_text(&mut self, s: &str, pre: bool) {
        for c in s.chars() {
            if !pre && c.is_ascii_whitespace() {
                self.pending_space = self.chars > 0 && !self.text.ends_with('\n');
                continue;
            }
            if self.pending_space {
                for o in self.open.iter_mut().filter(|o| o.after_space && o.start == self.chars) {
                    o.start += 1;
                }
                for i in self.space_ids.drain(..) {
                    if self.ids[i].1 == self.chars {
                        self.ids[i].1 += 1;
                    }
                }
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
        self.space_ids.clear();
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

    /// 新的行内缓冲：包着它的行内链接（[`Doc::links`]）接着开，只带链接（样式已经由计算值继承下来）。
    fn inline(&self) -> Inline {
        let mut inl = Inline::new();
        for link in self.links.borrow().iter() {
            inl.open_run(Run { start: 0, len: 0, comp: None, link: Some(link.clone()), note_ref: false, anchor: false });
        }
        inl
    }

    pub(super) fn comp(&self, el: &ElementRef, parent: &Computed) -> Computed {
        let mut decls = self.sheet.cascade(el, self.path);
        crate::css::presentational_hints(el, &mut decls);
        let mut c = Computed::derive(parent, &decls, el.value().name());
        if let Some(l) = lang_attr(el) {
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
        let pre_gap = self.gap.take();
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
                    return out.push(self.list(el, comp, first).led_by(pre, pre_gap));
                }
            }
            "table" => return out.push(self.table(el, comp).led_by(pre, pre_gap)),
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
                return out.push(b.led_by(pre, pre_gap));
            }
            _ => {}
        }
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
            self.gap.set(self.gap.get() + pre_gap);
            return;
        }
        let BoxLens { margin_top, margin_bottom, margin_left: mut ml, margin_right: mr, padding, padding_h } = BoxLens::of(&comp);
        // 不显示符号的列表（`list-style:none`）当普通块，照 Amazon 缩进 1.5em（测试书 L04：`$48` 4.688%）。
        if matches!(name, "ul" | "ol") && comp.margin[3].is_none() && comp.padding[3].is_none() {
            ml.em += 1.5;
        }
        // 自己只包着一个文字块（普通段落）：本元素就是这个块。
        // 有背景或内边距的块写成容器套文字（样本里带背景色的 h1 就是这样；背景、内边距、负外边距直接放在文字段落上，
        // Kindle 上长标题会溢出屏幕，2026-10-05 真机）。
        let boxed = comp.background.is_some() || comp.bg_image.is_some() || padding.iter().any(|p| *p != 0.0) || comp.has_border() || comp.box_shadow.is_some();
        let own = !boxed && children.len() == 1 && matches!(children[0].kind, Kind::Text { .. }) && children[0].is_anonymous() && children[0].inline;
        // 有宽度的包裹层也写成容器，宽度写在容器上；只有自己文字的块宽度写在文字上（Send to Kindle 同样：《金庸》60% 宽的 div、
        // 《平凡的世界》`width:100%` 的章标题）
        let boxed = boxed || (!own && comp.width.is_some());
        if own {
            let mut b = children.pop().unwrap_or_else(|| unreachable!());
            // 整段只有一个带样式的行内元素（`<p><span class="大字">版权信息</span></p>`、`<p><b>注：…</b></p>`）：它的文字样式并进段落
            // （Send to Kindle 同样：《人生海海》字号 1.833 写在段落上、行高按它算；《金庸》整段的 `<b>` 是段落的 bolder）
            let mut comp = comp;
            if let Kind::Text { text, runs } = &mut b.kind {
                // 几层都覆盖整段时（`<span class="大字"><span class="bold">目录</span></span>`）取最里层，它的计算值已经含外层；链接照留
                let n = text.chars().count();
                let whole = !runs.is_empty() && runs.iter().all(|r| r.start == 0 && r.len == n && !r.note_ref);
                let inner = runs.iter().rev().find_map(|r| r.comp.as_ref()).cloned();
                if let (true, Some(rc)) = (whole, inner) {
                    if !rc.superscript && !rc.subscript && !rc.has_border() && rc.background.is_none() {
                        // `<a>` 的颜色是链接颜色，留在链接区间上（Send to Kindle 写成 `$576`/`$577`），不并进段落
                        let anchor = runs.iter().any(|r| r.anchor);
                        let color = comp.color;
                        comp = merge_text_style(&comp, &rc);
                        if anchor {
                            comp.color = color;
                        }
                        runs.retain_mut(|r| {
                            if !(r.anchor && r.comp.as_ref().is_some_and(|c| c.color != color)) {
                                r.comp = None;
                            }
                            r.link.is_some() || r.comp.is_some()
                        });
                    }
                }
            }
            b.comp = comp;
            // 现在它是本元素自己的块了，外层元素不能再把它当成自己的文字
            b.inline = false;
            b.heading = heading;
            b.margin_top = margin_top;
            b.margin_bottom = margin_bottom;
            b.margin_left = ml;
            b.margin_right = mr;
            b.padding = padding;
            b.padding_h = padding_h;
            out.push(b.led_by(lead, pre_gap));
            return;
        }
        if boxed {
            // 页面一级的 `cover` 背景：照 Send to Kindle 写整页范围（`$645`），背景铺满一页而不是只在内容范围里画（《绍宋》卷首语，真机 ✓）。
            // 2026-10-08 试过也给 `fixed` 写了尺寸的写（制作说明），真机没有效果，用户说不改了，撤回。只认 body：别的块上没见过样本。
            let page_bg = comp.bg_cover;
            let attrs = if name == "body" && page_bg && comp.bg_image.is_some() {
                let b = BG_PAGE_BOUNDS_KEYS.iter().zip([0.0, 0.0, 100.0, 100.0]).map(|(&k, v)| (k, num(v, U_PERCENT))).collect();
                vec![(BG_PAGE_BOUNDS, Value::Struct(b))]
            } else {
                Vec::new()
            };
            let mut children = children;
            mark_bare(&mut children, &comp);
            out.push(Block {
                kind: Kind::Container(children),
                comp,
                heading,
                margin_top,
                margin_bottom,
                margin_left: ml,
                margin_right: mr,
                padding,
                padding_h,
                ids: lead.into_iter().map(|i| (i, 0)).collect(),
                note: false,
                ty: None,
                attrs,
                extra: Vec::new(),
            bare: false,
            inline: false,
            gap_before: pre_gap,
            });
            return;
        }
        // 没有背景的包裹层摊平：竖直外边距、内边距并进首尾子块，水平外边距加到每个子块上。
        let n = children.len();
        let mut lead = Some(lead);
        for (i, mut c) in children.into_iter().enumerate() {
            if i == 0 {
                c.margin_top = collapse(margin_top + padding[0], c.margin_top);
                c.gap_before += pre_gap;
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
        let l = BoxLens::of(&comp);
        let mut b = Block::anonymous(kind, comp, id.map(|i| vec![(i.to_string(), 0)]).unwrap_or_default());
        (b.margin_top, b.margin_bottom, b.margin_left, b.margin_right, b.padding, b.padding_h) =
            (l.margin_top, l.margin_bottom, l.margin_left, l.margin_right, l.padding, l.padding_h);
        b
    }

    /// 元素的子块；只有一个匿名文字块时直接用它当 `ty` 类型的节点（列表项、表格标题里的文字），否则包成容器。
    fn wrap_children(&self, el: ElementRef, comp: Computed, ty: Option<u32>) -> Block {
        let mut kids = Vec::new();
        self.children(el, &comp, &mut kids);
        let id = el.value().attr("id");
        if ty.is_some() && kids.len() == 1 && matches!(kids[0].kind, Kind::Text { .. }) && kids[0].is_anonymous() {
            if let Some(k) = kids.pop() {
                // 文字的样式用里面那个块的（`<li><p class="footnote">` 的字号、缩进、行高、字体，以前丢了，用的是 `<li>` 的），
                // 盒子（边距、内边距、背景、边框）用本元素的
                let text_comp = with_box(&k.comp, &comp);
                let mut b = self.boxed(k.kind, comp, id);
                b.comp = text_comp;
                b.ids.extend(k.ids);
                b.ty = ty;
                return b;
            }
        }
        mark_bare(&mut kids, &comp);
        let mut b = self.boxed(Kind::Container(kids), comp, id);
        b.ty = ty;
        b
    }

    /// `<ul>`/`<ol>` → `$276` 列表，`<li>` → `$277` 列表项（只有文字的列表项本身就是文字节点）。
    /// 列表符号的缺省按标签和嵌套层数（同浏览器：圆点 → 空心圆 → 方块），Amazon 转出来也是这样（2026-10-05 测试书）。
    /// `first_li`：第一个 `<li>` 的计算值（调用方判断要不要当列表时已经层叠过，不再算一遍；父元素只差列表符号，
    /// 而 `<li>` 的列表符号本来就继承或自己写，结果一样）。
    /// 直接放在 `<ul>`/`<ol>` 里的文字、行内元素、图片（不在 `<li>` 里）照浏览器当匿名块排在列表里、不带符号（以前丢掉）。
    fn list(&self, el: ElementRef, comp: Computed, mut first_li: Option<Computed>) -> Block {
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
        let mut inl = self.inline();
        for node in el.children() {
            // 文字和图片走行内收集（连续的合成一个匿名文字块，图片单独成块）；别的元素照旧当列表项
            let Some(child) = ElementRef::wrap(node).filter(|e| image_src(e).is_none()) else {
                self.inline_or_block(node, &comp, &comp, &mut inl, &mut items);
                continue;
            };
            self.flush(&mut inl, &comp, &mut items);
            let c = match first_li.take_if(|_| child.value().name() == "li") {
                Some(c) => c,
                None => self.comp(&child, &comp),
            };
            if c.display.as_deref() == Some("none") {
                continue;
            }
            let mut item = self.wrap_children(child, c, Some(NODE_LIST_ITEM));
            if let Some(v) = child.value().attr("value").and_then(|v| v.trim().parse::<i64>().ok()) {
                item.attrs.push((LIST_START, Value::Int(v)));
            }
            items.push(item);
        }
        self.flush(&mut inl, &comp, &mut items);
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
    pub(super) fn children(&self, el: ElementRef, comp: &Computed, out: &mut Vec<Block>) {
        let mut inl = self.inline();
        for child in el.children() {
            self.inline_or_block(child, comp, comp, &mut inl, out);
        }
        self.flush(&mut inl, comp, out);
    }

    fn flush(&self, inl: &mut Inline, comp: &Computed, out: &mut Vec<Block>) {
        let reopened = inl.cut_open();
        let taken = inl.take();
        inl.open = reopened;
        if taken.is_empty() {
            // 没有文字：里面的锚点留给后面的内容。只有 `&nbsp;`、全角空格、`<br/>` 的（按 CSS 排出一个空行；只有
            // 普通空白的排不出行，不算）折成下一块的上边距。
            if !taken.text.is_empty() {
                self.gap.set(self.gap.get() + BLANK_LINE_FOLD * comp.line_height.unwrap_or(LH_EM) * comp.font_size);
            }
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
        let mut b = Block::anonymous(Kind::Text { text, runs }, comp.inherited(), ids);
        b.inline = true;
        b.gap_before = self.gap.take();
        out.push(b);
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
                    let mut b = Block::anonymous(Kind::Image { src: resolve_link(self.path, &src).0 }, comp, ids);
                    b.gap_before = self.gap.take();
                    out.push(b);
                    return;
                }
                let comp = self.comp(&el, parent);
                if comp.display.as_deref() == Some("none") || matches!(name, "script" | "style" | "head") {
                    return;
                }
                if crate::css::is_block(&el, &comp) {
                    self.flush(inl, block_comp, out);
                    self.block(el, comp, out);
                    return;
                }
                if let Some(id) = el.value().attr("id") {
                    inl.push_id(id);
                }
                let link = (name == "a")
                    .then(|| el.value().attr("href"))
                    .flatten()
                    .filter(|h| !bookconv::html::is_external(h))
                    .map(|h| {
                        let (path, frag) = resolve_link(self.path, h);
                        (if path.is_empty() { self.path.to_string() } else { path }, frag.unwrap_or_default())
                    });
                let styled = run_differs(&comp, block_comp);
                // 区间先登记成「开着的」：子节点里遇到块或图片切开时，`flush` 在切开处截断它、到新缓冲从 0 接着开
                // （以前事后按进来时的下标插，切开过的话缓冲已经换了：下标越界 panic，或区间错位到别的字上）
                let opened = styled || link.is_some();
                let linked = link.is_some();
                if linked {
                    self.links.borrow_mut().extend(link.clone());
                }
                if opened {
                    inl.open_run(Run { start: 0, len: 0, comp: styled.then(|| comp.clone()), link, note_ref: false, anchor: name == "a" });
                }
                for c in el.children() {
                    self.inline_or_block(c, block_comp, &comp, inl, out);
                }
                // 子节点进出成对、切开时开着的个数不变，弹出来的就是本元素的
                if let Some(o) = opened.then(|| inl.open.pop()).flatten() {
                    inl.close_run(o);
                }
                if linked {
                    self.links.borrow_mut().pop();
                }
            }
            _ => {}
        }
    }
}

impl Block {
    pub(super) fn anonymous(kind: Kind, comp: Computed, ids: Vec<(String, usize)>) -> Block {
        Block {
            kind,
            comp,
            heading: None,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: Horiz::default(),
            margin_right: Horiz::default(),
            padding: [0.0; 4],
            padding_h: [Horiz::default(); 2],
            ids,
            note: false,
            ty: None,
            attrs: Vec::new(),
            extra: Vec::new(),
            bare: false,
            inline: false,
            gap_before: 0.0,
        }
    }

    /// 前面攒下的锚点 `lead` 放到块开头、折掉的空段高度 `gap` 加到上边距（[`Doc::block`] 里本元素整个成一块时）。
    fn led_by(mut self, lead: Vec<String>, gap: Vert) -> Block {
        prepend_ids(&mut self.ids, lead);
        self.gap_before += gap;
        self
    }

    fn is_anonymous(&self) -> bool {
        self.margin_top == 0.0 && self.margin_bottom == 0.0 && self.heading.is_none() && self.margin_left == Horiz::default() && self.ty.is_none()
    }
}

fn image_src(el: &ElementRef) -> Option<String> {
    let v = el.value();
    match v.name() {
        "img" => v.attr("src").map(str::to_string),
        // SVG 的 `<image xlink:href>`：属性在 xlink 命名空间里，`attr()` 只认无命名空间的，按本地名找（《绍宋》封面页
        // `<svg><image xlink:href="../Images/cover.png"/></svg>`；以前找不到、封面页整页丢掉——优化器以前把 SVG 封面换成 `<img>`，
        // 只修复模式不换了才露出来，2026-10-08 真机：封面没了）。
        "image" => v.attrs().find(|(k, _)| *k == "href").map(|(_, val)| val.to_string()),
        _ => None,
    }
}

fn run_differs(a: &Computed, b: &Computed) -> bool {
    a.bold != b.bold
        || a.semibold != b.semibold
        || a.bolder != b.bolder
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
pub(super) type ParsedDoc<'a> = (usize, &'a str, Option<String>, Vec<Block>);

/// 有没有块（含容器里的）的左（`left`）或右外边距是负的。
fn has_negative(blocks: &[Block], left: bool) -> bool {
    blocks.iter().any(|b| {
        let h = if left { b.margin_left } else { b.margin_right };
        h.em + h.pct / 100.0 * PAGE_WIDTH_EM < -1e-9 || matches!(&b.kind, Kind::Container(c) if has_negative(c, left))
    })
}

/// 顶层块的左右百分比长度（书里相对 body 内容宽度）折算成相对整页：乘 `s`。
fn scale_percent(blocks: &mut [Block], s: f64) {
    for b in blocks {
        let [p0, p1] = &mut b.padding_h;
        for h in [&mut b.margin_left, &mut b.margin_right, p0, p1] {
            h.pct *= s;
        }
        if let Some(Len::Percent(p)) = &mut b.comp.text_indent {
            *p *= s;
        }
    }
}

/// 相邻块的外边距折叠（同一层）。合成的边距放在后一块的上边距；后一块没有上边距时留在前一块的下边距
/// （Send to Kindle 同样：段距写在每段的上边距，「目录」标题的下边距、《绍宋》章号 `p.j11` 的 `margin-bottom:2px` 留在自己身上，2026-10-08）。
/// 折掉的空段加到下一块的上边距上（外边距折叠之后：多出的高度不参与折叠）。文件末尾的空段没有下一块，不折。
pub(super) fn apply_gaps(blocks: &mut [Block]) {
    for b in blocks.iter_mut() {
        b.margin_top += b.gap_before;
        b.gap_before = 0.0;
        if let Kind::Container(c) = &mut b.kind {
            apply_gaps(c);
        }
    }
}

pub(super) fn collapse_siblings(blocks: &mut [Block]) {
    for i in 1..blocks.len() {
        let prev_bottom = blocks[i - 1].margin_bottom;
        if blocks[i].margin_top == 0.0 {
            continue;
        }
        blocks[i].margin_top = collapse(prev_bottom, blocks[i].margin_top);
        blocks[i - 1].margin_bottom = 0.0;
    }
    for b in blocks.iter_mut() {
        if let Kind::Container(c) = &mut b.kind {
            collapse_siblings(c);
        }
    }
}

/// 段落的计算值换上行内元素的文字样式（字体、字号、字重、斜体、颜色、行高、装饰、字间距）；盒子（边距、背景、边框）照段落的。
fn merge_text_style(block: &Computed, run: &Computed) -> Computed {
    Computed {
        font_family: run.font_family.clone(),
        font_fallbacks: run.font_fallbacks.clone(),
        font_size: run.font_size,
        bold: run.bold,
        semibold: run.semibold,
        bolder: run.bolder,
        weight_declared: run.weight_declared,
        italic: run.italic,
        line_height: run.line_height,
        line_height_abs: run.line_height_abs,
        color: run.color,
        decoration: run.decoration,
        small_caps: run.small_caps,
        letter_spacing: run.letter_spacing,
        ..block.clone()
    }
}

/// 标出容器里只有本元素文字的匿名文字块（见 [`Block::bare`]）。
fn mark_bare(children: &mut [Block], comp: &Computed) {
    let inherited = comp.inherited();
    for c in children {
        // 行内区间的样式相对段落写，段落样式空着照样成立
        if matches!(c.kind, Kind::Text { .. }) && c.is_anonymous() && c.inline && c.comp == inherited {
            c.bare = true;
        }
    }
}

/// `text` 的计算值换上 `boxc` 的盒子属性（外边距、内边距、背景、边框、宽高、显示方式、单元格竖直对齐）。
fn with_box(text: &Computed, boxc: &Computed) -> Computed {
    Computed {
        display: boxc.display.clone(),
        margin: boxc.margin,
        padding: boxc.padding,
        background: boxc.background,
        border: boxc.border.clone(),
        radius: boxc.radius,
        width: boxc.width,
        min_height: boxc.min_height,
        valign: boxc.valign.clone(),
        bg_image: boxc.bg_image.clone(),
        bg_no_repeat: boxc.bg_no_repeat,
        bg_fixed: boxc.bg_fixed,
        bg_position: boxc.bg_position,
        bg_size: boxc.bg_size,
        bg_cover: boxc.bg_cover,
        ..text.clone()
    }
}

/// 块的最大嵌套层数（不递归）。
pub(super) fn block_depth(blocks: &[Block]) -> usize {
    let mut max = 0;
    let mut stack: Vec<(&[Block], usize)> = vec![(blocks, 1)];
    while let Some((bs, d)) = stack.pop() {
        for b in bs {
            max = max.max(d);
            if let Kind::Container(c) = &b.kind {
                stack.push((c, d + 1));
            }
        }
    }
    max
}

/// 书的语言（OPF 里的；空的算没写）。
pub(super) fn book_language(book: &Loaded) -> Option<String> {
    (!book.meta.language.is_empty()).then(|| book.meta.language.clone())
}

/// 先把所有文档解析成块（注释配对要看全书），再逐个生成版面。各文档独立，多线程解析（`bookconv::util::par_map`，按原顺序收回）。
/// 外部样式表按路径只解析一次（大合集几百个文档共用一份样式表），各线程共用。`faithful`：按 CSS 原样算（[`epub_text_styles`]），
/// body 的左右边距照算。
///
/// 元素套得太深（超过 [`MAX_NESTING`]）的书报错；套得较深的文档在单独的大栈线程里解析（见 [`DEEP_NESTING`]）。
pub(super) fn parse_docs(book: &Loaded, media: Option<crate::css::MediaEnv>, faithful: bool) -> Result<Vec<ParsedDoc<'_>>, String> {
    let css: HashMap<&str, &str> = book.css.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    let book_lang = book_language(book);
    let sheets: std::sync::Mutex<HashMap<String, std::sync::Arc<crate::css::Rules>>> = Default::default();
    let docs: Vec<(usize, &bookconv::epubbook::Doc)> = book.docs.iter().enumerate().collect();
    let parse_blocks = |html: &Html, doc: &bookconv::epubbook::Doc| -> (Option<String>, Vec<Block>) {
        let sheet = Sheet::for_doc(html, &doc.path, media.as_ref(), |path| {
            let c = css.get(path)?;
            let cached = sheets.lock().unwrap_or_else(|e| e.into_inner()).get(path).cloned();
            // 没解析过的在锁外解析（几个线程同时碰上同一份时各解析一次，结果一样，留先放进去的那份）
            Some(cached.unwrap_or_else(|| {
                let r = crate::css::Rules::parse(c, path, media.as_ref());
                sheets.lock().unwrap_or_else(|e| e.into_inner()).entry(path.to_string()).or_insert(r).clone()
            }))
        });
        let root = html.root_element();
        let lang = lang_attr(&root).map(str::to_string).or_else(|| book_lang.clone());
        let d = Doc { path: &doc.path, sheet, lang: lang.clone(), pending: Default::default(), gap: Default::default(), links: Default::default() };
        let mut blocks = Vec::new();
        let root_comp = d.comp(&root, &Computed::root());
        if let Some(body) = root.children().filter_map(ElementRef::wrap).find(|e| e.value().name() == "body") {
            // body 的左右外边距、内边距不写（Send to Kindle 同样：《人生海海》《ABC谋杀案》body `margin:0 5pt`、《绍宋》新版
            // `padding:1em` 都没有出现在段落上，2026-10-08；更早的《ABC谋杀案》样本加到了每段上，那是另一版 EPUB）。
            // 书里按 body 内容宽度算的百分比折算成整页的（《雪国》body 左右 1%，段落的 1% 写 0.98%）。
            // 例外：文件里有块的左（右）外边距是负的，这一侧 body 的边距照旧加到每个块上（《罗杰疑案》《ABC谋杀案》章标题
            // `margin:-2em` 两侧都留；《平凡的世界》只有左边 `-1em` 的那几个文件只留左边）。
            let mut bc = d.comp(&body, &root_comp);
            if faithful {
                d.block(body, bc, &mut blocks);
                attach_trailing(&mut blocks, d.take_pending());
                collapse_siblings(&mut blocks);
                return (d.lang.clone(), blocks);
            }
            let fs = bc.font_size;
            let side = |i: usize| {
                let (m, p) = (to_horiz(bc.margin[i], fs), to_horiz(bc.padding[i], fs));
                Horiz { em: m.em + p.em, pct: m.pct + p.pct }
            };
            let (left, right) = (side(3), side(1));
            for i in [1, 3] {
                bc.margin[i] = None;
                bc.padding[i] = None;
            }
            d.block(body, bc, &mut blocks);
            let (neg_l, neg_r) = (has_negative(&blocks, true), has_negative(&blocks, false));
            let rem = |h: Horiz| h.em + h.pct / 100.0 * PAGE_WIDTH_EM;
            let width = PAGE_WIDTH_EM - if neg_l { 0.0 } else { rem(left) } - if neg_r { 0.0 } else { rem(right) };
            if width > 1.0 && width < PAGE_WIDTH_EM {
                scale_percent(&mut blocks, width / PAGE_WIDTH_EM);
            }
            for b in &mut blocks {
                if neg_l {
                    b.margin_left.em += left.em;
                    b.margin_left.pct += left.pct;
                }
                if neg_r {
                    b.margin_right.em += right.em;
                    b.margin_right.pct += right.pct;
                }
            }
        }
        attach_trailing(&mut blocks, d.take_pending());
        collapse_siblings(&mut blocks);
        apply_gaps(&mut blocks);
        (d.lang.clone(), blocks)
    };
    let parse_doc = |&(si, doc): &(usize, &bookconv::epubbook::Doc)| -> Result<(usize, Option<String>, Vec<Block>), String> {
        let html = Html::parse_document(&doc.html);
        let depth = nesting_depth(&html);
        if depth > MAX_NESTING {
            return Err(format!("{}：元素套了 {depth} 层，超过上限 {MAX_NESTING} 层（多半是标签没关），不转换", doc.path));
        }
        let (lang, blocks) = if depth > DEEP_NESTING {
            // DOM 不能跨线程（`Html` 不是 `Send`），到大栈线程里重新解析一遍；只有少见的深文档多花这一遍
            drop(html);
            on_big_stack(|| parse_blocks(&Html::parse_document(&doc.html), doc))?
        } else {
            parse_blocks(&html, doc)
        };
        Ok((si, lang, blocks))
    };
    // 一批批解析，每批的结果在本线程复制一份、工作线程分配的那份随即释放：块是在工作线程里边解析 DOM 边分配的，和已经释放的 DOM
    // 交错着留在那几个线程的分配区里，本线程后面生成版面时用不上那些空洞（glibc 按线程分区），不复制的话《阿加莎全集》峰值
    // 从 0.47GB 涨到 0.63GB；复制一遍只多花约 0.1 秒，工作线程那份整块释放掉。
    // 本线程复制这一批的同时，下一批已经在解析（复制和解析错开，不干等）。
    let mut parsed: Vec<ParsedDoc> = Vec::with_capacity(docs.len());
    let batches: Vec<_> = docs.chunks(64).collect();
    std::thread::scope(|s| {
        let mut got = batches.first().map(|b| bookconv::util::par_map(b, parse_doc));
        for k in 0..batches.len() {
            let next = batches.get(k + 1).map(|b| s.spawn(|| bookconv::util::par_map(b, parse_doc)));
            for g in got.into_iter().flatten() {
                let (si, l, b) = g?;
                // 套得深的（少见）不复制：复制、释放都是递归的，本线程的栈不一定够
                let b = if block_depth(&b) > DEEP_NESTING { b } else { b.clone() };
                parsed.push((si, book.docs[si].path.as_str(), l, b));
            }
            got = next.map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)));
        }
        Ok::<_, String>(())
    })?;
    Ok(parsed)
}

/// 元素最多套几层：超过的书报错不转换（真书十几层；几百上千层的是没关的标签一路套下去，解析、生成版面都是递归的，
/// 再深就要栈溢出、整个进程中止——书库在进程内调写出器，会带走整个 sync）。
pub(super) const MAX_NESTING: usize = 1000;

/// 套得比这深的文档在单独的大栈线程里解析（[`on_big_stack`]）：解析每层约 10KB 栈，`par_map` 的工作线程只有 2MB
/// （以前 300 层没关的 `<div>` 就栈溢出中止）。浅的照旧在工作线程里，不多开线程。
pub(super) const DEEP_NESTING: usize = 32;

/// 大栈线程的栈：[`MAX_NESTING`] 层的解析、版面、编码都够（只保留地址空间，用到多少占多少）。
const BIG_STACK: usize = 256 << 20;

/// DOM 的最大嵌套层数（不递归）。
fn nesting_depth(html: &Html) -> usize {
    let (mut depth, mut max) = (0usize, 0usize);
    for edge in html.tree.root().traverse() {
        match edge {
            ego_tree::iter::Edge::Open(_) => {
                depth += 1;
                max = max.max(depth);
            }
            ego_tree::iter::Edge::Close(_) => depth -= 1,
        }
    }
    max
}

/// `deep` 时在大栈线程里跑 `f`（[`on_big_stack`]），否则就在本线程跑。
/// 浅的书不开线程：在别的线程里分配，glibc 另开分配区，《阿加莎全集》峰值内存多约 40MB。
pub(super) fn maybe_big_stack<T: Send>(deep: bool, f: impl FnOnce() -> T + Send) -> Result<T, String> {
    if deep { on_big_stack(f) } else { Ok(f()) }
}

/// 在栈大小 [`BIG_STACK`] 的线程里跑 `f`，等它跑完（panic 照样传出来）。
fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new().stack_size(BIG_STACK).spawn_scoped(s, f).map_err(|e| format!("开线程失败：{e}"))?;
        Ok(h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
    })
}

/// 正文里哪儿都没用到封面图（原书只在 OPF 标了 `cover-image`、spine 里没有封面页）时，在最前面补一个只有封面图的文档，
/// 排成整页图片版面、封面地标指向它。照 Send to Kindle：《绍宋》原书没有封面页，Amazon 版第一个版面就是整页封面
/// （2026-10-09 真机：我们的打开没有封面）。正文里用到了的不补，那些书的产物逐字节不变。
pub(super) fn prepend_cover_page(book: &Loaded, parsed: &mut Vec<ParsedDoc>) {
    fn uses(blocks: &[Block], src: &str) -> bool {
        blocks.iter().any(|bl| match &bl.kind {
            Kind::Image { src: s } => s == src,
            Kind::Container(c) => uses(c, src),
            Kind::Text { .. } => false,
        })
    }
    let Some(cover) = book.cover.as_deref() else { return };
    if parsed.iter().any(|(_, _, _, blocks)| uses(blocks, cover)) {
        return;
    }
    // 序号只用来起实体名，取一个文档用不到的；路径不是任何文档的，没有链接会指到它。
    parsed.insert(0, (book.docs.len(), "\0cover", None, vec![Block::anonymous(Kind::Image { src: cover.to_string() }, Computed::root(), Vec::new())]));
}

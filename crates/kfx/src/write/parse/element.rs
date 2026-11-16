//! 块级元素的展开：一个块的展开（[`Doc::block`]、[`Doc::block_inner`]、[`Doc::special_block`] 和收尾用的
//! [`own_text_block`]、[`container_block`]、[`flatten_wrapper`]）。`impl Doc<'_>` 按职责分到 `element/` 下的子模块：
//! 计算值与图片尺寸在 `computed`，行内收集和收块在 `flow`，列表和表格在 `list_table`；这里只留块的入口和分派。

use super::*;

mod computed;
mod flow;
mod list_table;

/// 文档末尾没落到内容上的锚点（`…</p><span id="x"></span></body>`）挂到最后一个块的末尾。
pub(super) fn attach_trailing(blocks: &mut [Block], ids: Vec<String>) {
    let Some(last) = blocks.last_mut() else {
        return;
    };
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
pub(super) fn prepend_ids(ids: &mut Vec<(String, usize)>, lead: Vec<String>) {
    ids.splice(0..0, lead.into_iter().map(|i| (i, 0)));
}

/// [`Doc::special_block`] 的结果。只是交接用的返回值（拿到立刻拆开），两个变体大小不同无所谓，不为它装箱。
#[allow(clippy::large_enum_variant)]
enum Special {
    /// 列表、表格、水平线自成的一块。
    Own(Block),
    /// 不是这几种：交回计算值，照普通块处理。
    Normal(Computed),
}

/// 块级元素只包着一段文字（[`Doc::block`] 判的 `own`）：这段文字就是本元素的块，换上本元素的计算值、标题级别和盒子。
/// 按这台阅读器的排法（[`crate::css::DeviceLine`]）改层叠出来的 `line-height`、em。只给 `--styles --device=` 的分析用。
fn device_line_height(
    el: &ElementRef,
    decls: &mut crate::css::Decls,
    line: crate::css::DeviceLine,
) {
    let name = el.value().name();
    let v = decls
        .get("line-height")
        .map(|v| v.trim().to_ascii_lowercase());
    let number = v
        .as_deref()
        .is_some_and(|v| v == "normal" || v.parse::<f64>().is_ok());
    if let Some(d) = line.length_em {
        // 不带单位的数、`normal` 当没写；全书缺省是 d 倍（随字号，写成数字往下继承）
        if number {
            decls.remove("line-height");
        }
        if name == "html" && !decls.contains_key("line-height") {
            decls.insert("line-height".into(), d.to_string());
        }
    }
    // 标题和标题里的不换：Move 的标题只认字号关键字、不认上下外边距（`block_style`、`box_fix` 管，`wash/root_em.rs` 同样跳过）
    let in_heading = el
        .ancestors()
        .filter_map(ElementRef::wrap)
        .chain(std::iter::once(*el))
        .any(|a| bookconv::html::heading_level_of(a.value().name()).is_some());
    if line.em_root && !in_heading {
        root_em(decls);
    }
    if line.except_p && line.length_em.is_none() {
        let inline = el
            .value()
            .attr("style")
            .is_some_and(|s| s.to_ascii_lowercase().contains("line-height"));
        if (name == "p" && !inline)
            || v.as_deref() == Some("normal")
            || (name == "html" && v.is_none())
        {
            decls.insert("line-height".into(), "1".into());
        }
    }
}

/// em 一律按根字号算的阅读器（[`crate::css::DeviceLine::em_root`]）：字号的 em、% 写成 rem（px 按屏幕像素折成 rem），外边距、
/// 内边距、首行缩进的 em 写成 pt（12pt＝根 em）。只给 `--styles --device=` 的分析用。
fn root_em(decls: &mut crate::css::Decls) {
    let conv = |v: &str, font: bool| -> Option<String> {
        let mut changed = false;
        let out: Vec<String> = v
            .split_whitespace()
            .map(|t| {
                let lower = t.to_ascii_lowercase();
                let num = |suffix: &str| {
                    lower
                        .strip_suffix(suffix)
                        .and_then(|n| n.parse::<f64>().ok())
                };
                match (num("rem"), num("em"), num("%")) {
                    (Some(_), _, _) => t.to_string(),
                    // 字号的 px 是屏幕像素（同盒子长度，[`crate::css::device_px_em`]）
                    _ if font && num("px").is_some() && crate::css::device_px_em().is_some() => {
                        changed = true;
                        format!(
                            "{}rem",
                            num("px").unwrap_or(0.0) / crate::css::device_px_em().unwrap_or(1.0)
                        )
                    }
                    (None, Some(n), _) => {
                        changed = true;
                        if font {
                            format!("{n}rem")
                        } else {
                            format!("{}pt", n * crate::css::PT_PER_EM)
                        }
                    }
                    (None, None, Some(n)) if font => {
                        changed = true;
                        format!("{}rem", n / 100.0)
                    }
                    _ => t.to_string(),
                }
            })
            .collect();
        changed.then(|| out.join(" "))
    };
    for (k, v) in decls.iter_mut() {
        let font = k == "font-size";
        let boxy = k == "text-indent" || k.starts_with("margin") || k.starts_with("padding");
        if font || boxy {
            if let Some(n) = conv(v, font) {
                *v = n;
            }
        }
    }
}

fn own_text_block(mut b: Block, comp: Computed, heading: Option<u8>, lens: BoxLens) -> Block {
    b.comp = absorb_whole_span_style(&mut b.kind, comp);
    // 现在它是本元素自己的块了，外层元素不能再把它当成自己的文字
    b.inline = false;
    b.heading = heading;
    b.margin_top = lens.margin_top;
    b.margin_bottom = lens.margin_bottom;
    b.margin_left = lens.margin_left;
    b.margin_right = lens.margin_right;
    b.padding = lens.padding;
    b.padding_h = lens.padding_h;
    b
}

/// 整段只有一个带样式的行内元素（`<p><span class="大字">版权信息</span></p>`、`<p><b>注：…</b></p>`）：它的文字样式并进段落
/// （Send to Kindle 同样：《人生海海》字号 1.833 写在段落上、行高按它算；《金庸》整段的 `<b>` 是段落的 bolder）。
/// 返回段落的计算值；并进去的区间去掉样式（只剩链接的照留）。不是这种段落的原样返回 `comp`。
fn absorb_whole_span_style(kind: &mut Kind, comp: Computed) -> Computed {
    let mut comp = comp;
    if let Kind::Text { text, runs, .. } = kind {
        // 几层都覆盖整段时（`<span class="大字"><span class="bold">目录</span></span>`）取最里层，它的计算值已经含外层；链接照留
        let n = text.chars().count();
        let whole = !runs.is_empty()
            && runs
                .iter()
                .all(|r| r.start == 0 && r.len == n && !r.note_ref);
        let inner = runs.iter().rev().find_map(|r| r.comp.as_ref()).cloned();
        if let (true, Some(rc)) = (whole, inner) {
            if !rc.superscript && !rc.subscript && !rc.has_border() && rc.background.is_none() {
                // `<a>` 的颜色是链接颜色，留在链接区间上（Send to Kindle 写成 `$576`/`$577`），不并进段落
                let anchor = runs.iter().any(|r| r.anchor);
                let color = comp.color;
                let (old_fs, old_lh) =
                    (comp.font_size, comp.line_height.map(|l| l * comp.font_size));
                comp = merge_text_style(&comp, &rc);
                // 段落的首行缩进按段落自己的字号（em 换成并进来的字号的）；段落的行高是明确的值（自己写的、继承来的）时取段落和行内
                // 元素两者大的（《人生海海》大字 span 按 span，《消失的爱人》整段 0.75em 的 span 照段落继承的 1.2），`normal` 的照行内
                // 元素的（《福尔摩斯》整段 `<font size>` 的小字行高跟着小）；2026-10-14 重推的样本
                if let Some(Len::Em(n)) = comp.text_indent {
                    comp.text_indent = Some(Len::Em(n * old_fs / comp.font_size));
                }
                let run_lh = comp.line_height.unwrap_or(LH_EM) * comp.font_size;
                if let Some(old_lh) = old_lh.filter(|&l| l > run_lh + 1e-9) {
                    comp.line_height = Some(old_lh / comp.font_size);
                    comp.line_height_abs = None;
                }
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
    comp
}

/// 有背景、边框、内边距或宽度的块级元素（[`Doc::block`] 判的 `boxed`）：写成容器套子块，盒子在容器上。
fn container_block(
    name: &str,
    comp: Computed,
    children: Vec<Block>,
    heading: Option<u8>,
    lens: BoxLens,
    lead: Vec<String>,
    pre_gap: Gap,
) -> Block {
    // 页面一级的 `cover` 背景：照 Send to Kindle 写整页范围（`$645`），背景铺满一页而不是只在内容范围里画（《绍宋》卷首语，真机 ✓）。
    // 2026-10-08 试过也给 `fixed` 写了尺寸的写（制作说明），真机没有效果，维护者说不改了，撤回。只认 body：别的块上没见过样本。
    let page_bg = comp.bg_cover;
    let attrs = if name == "body" && page_bg && comp.bg_image.is_some() {
        let b = BG_PAGE_BOUNDS_KEYS
            .iter()
            .zip([0.0, 0.0, 100.0, 100.0])
            .map(|(&k, v)| (k, num(v, U_PERCENT)))
            .collect();
        vec![(BG_PAGE_BOUNDS, Value::Struct(b))]
    } else {
        Vec::new()
    };
    let mut children = children;
    mark_bare(&mut children, &comp);
    Block {
        kind: Kind::Container(children),
        comp,
        heading,
        margin_top: lens.margin_top,
        margin_bottom: lens.margin_bottom,
        margin_left: lens.margin_left,
        margin_right: lens.margin_right,
        padding: lens.padding,
        padding_h: lens.padding_h,
        ids: lead.into_iter().map(|i| (i, 0)).collect(),
        note: false,
        ty: None,
        attrs,
        extra: Vec::new(),
        bare: false,
        inline: false,
        gap_before: pre_gap,
    }
}

/// 没有背景的包裹层摊平：竖直外边距、内边距并进首尾子块（开头的锚点、空段留下的间距、标题级别给第一个子块），
/// 水平外边距加到每个子块上。
fn flatten_wrapper(
    children: Vec<Block>,
    heading: Option<u8>,
    lens: BoxLens,
    lead: Vec<String>,
    pre_gap: Gap,
    out: &mut Vec<Block>,
) {
    let BoxLens {
        margin_top,
        margin_bottom,
        margin_left: ml,
        margin_right: mr,
        padding,
        ..
    } = lens;
    let n = children.len();
    let mut lead = Some(lead);
    let mut pre_gap = Some(pre_gap);
    for (i, mut c) in children.into_iter().enumerate() {
        if i == 0 {
            c.margin_top = collapse(margin_top + padding[0], c.margin_top);
            c.gap_before.prepend(pre_gap.take().unwrap_or_default());
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

impl Doc<'_> {
    /// 取走还没落到内容上的锚点（见 [`Doc::pending`]）。
    pub(super) fn take_pending(&self) -> Vec<String> {
        self.pending.take()
    }

    /// 把一个块级元素（计算值 `comp` 由调用方算好，层叠不重复做）展开成块序列。
    ///
    /// 分四种写法：列表、表格、水平线自成一块（[`Doc::special_block`]）；只包着一段文字的就是这段文字（[`own_text_block`]）；
    /// 有背景、边框、内边距或宽度的写成容器（[`container_block`]）；其余的包裹层摊平到子块上（[`flatten_wrapper`]）。
    pub(super) fn block(&self, el: ElementRef, comp: Computed, out: &mut Vec<Block>) {
        if comp.display.as_deref() == Some("none") {
            return;
        }
        // 强制分页：记号像锚点一样落在下一个生成的块开头，解析完在那里另起一节（[`split_at_breaks`]）
        let after = comp.break_after;
        if comp.break_before {
            self.pending.borrow_mut().insert(0, BREAK_MARK.to_string());
        }
        self.block_inner(el, comp, out);
        if after {
            self.pending.borrow_mut().push(BREAK_MARK.to_string());
        }
    }

    fn block_inner(&self, el: ElementRef, comp: Computed, out: &mut Vec<Block>) {
        let name = el.value().name();
        if matches!(name, "head" | "script" | "style" | "title") {
            return;
        }
        // 本元素之前攒下的锚点落在本元素的开头。
        let pre = self.take_pending();
        let pre_gap = self.gap.take();
        let inner_from = self.gap.borrow().h;
        let comp = match self.special_block(el, comp) {
            Special::Own(b) => return out.push(b.led_by(pre, pre_gap)),
            Special::Normal(comp) => comp,
        };
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
            // 里面折出了空段（`<p><br/></p>`）：本元素的上下外边距照留（Send to Kindle 同样，见 [`Gap`]）
            if self.gap.borrow().h > inner_from {
                let l = BoxLens::of(&comp);
                self.gap
                    .borrow_mut()
                    .margins
                    .push((l.margin_top, l.margin_bottom));
            }
            self.gap.borrow_mut().prepend(pre_gap);
            return;
        }
        let mut lens = BoxLens::of(&comp);
        // 不显示符号的列表（`list-style:none`）当普通块，照 Amazon 缩进 1.5em（测试书 L04：`$48` 4.688%）。
        if matches!(name, "ul" | "ol") && comp.margin[3].is_none() && comp.padding[3].is_none() {
            lens.margin_left.em += 1.5;
        }
        // 没有背景、边框、阴影的块：左右内边距并进外边距，不为它包容器（Send to Kindle 同样：《绍宋》章名 `margin:0 auto;padding:0 4px`
        // 写成文字块的左右外边距 0.469%，2026-10-14 重推的样本；显示一样）
        if comp.background.is_none()
            && comp.bg_image.is_none()
            && !comp.has_border()
            && comp.box_shadow.is_none()
        {
            lens.margin_left = Horiz {
                em: lens.margin_left.em + lens.padding_h[1].em,
                pct: lens.margin_left.pct + lens.padding_h[1].pct,
            };
            lens.margin_right = Horiz {
                em: lens.margin_right.em + lens.padding_h[0].em,
                pct: lens.margin_right.pct + lens.padding_h[0].pct,
            };
            lens.padding_h = [Horiz::default(); 2];
            lens.padding[1] = 0.0;
            lens.padding[3] = 0.0;
        }
        // 自己只包着一个文字块（普通段落）：本元素就是这个块。
        // 有背景或内边距的块写成容器套文字（样本里带背景色的 h1 就是这样；背景、内边距、负外边距直接放在文字段落上，
        // Kindle 上长标题会溢出屏幕，2026-10-05 真机）。
        let boxed = comp.background.is_some()
            || comp.bg_image.is_some()
            || lens.padding.iter().any(|p| *p != 0.0)
            || comp.has_border()
            || comp.box_shadow.is_some();
        let own = !boxed
            && children.len() == 1
            && matches!(children[0].kind, Kind::Text { .. })
            && children[0].is_anonymous()
            && children[0].inline;
        // 有宽度的包裹层也写成容器，宽度写在容器上；只有自己文字的块宽度写在文字上（Send to Kindle 同样：《金庸》60% 宽的 div、
        // 《平凡的世界》`width:100%` 的章标题）
        let boxed = boxed || (!own && comp.width.is_some());
        if own {
            let b = children.pop().unwrap_or_else(|| unreachable!());
            out.push(own_text_block(b, comp, heading, lens).led_by(lead, pre_gap));
            return;
        }
        if boxed {
            out.push(container_block(
                name, comp, children, heading, lens, lead, pre_gap,
            ));
            return;
        }
        flatten_wrapper(children, heading, lens, lead, pre_gap, out);
    }

    /// 列表、表格、水平线：自成一块（自己的外边距、内边距照计算值，不摊平），开头的锚点由调用方接上。
    /// 别的元素（以及当普通块处理的列表：不显示符号的）原样交回 [`Special::Normal`]，照普通块往下走。
    fn special_block(&self, el: ElementRef, comp: Computed) -> Special {
        match el.value().name() {
            "ul" | "ol" => {
                // 列表符号看列表项（`li` 上写的优先，《雪国》把 `cjk-ideographic` 写在 `li` 上）；列表项写成
                // `display:block` 的不显示符号（《啸风山庄》`li {display:block}`），和 `list-style:none` 一样当普通块。
                let first = el
                    .children()
                    .filter_map(ElementRef::wrap)
                    .find(|c| c.value().name() == "li")
                    .map(|li| self.comp(&li, &comp));
                // 列表项上的符号只用来写列表；当普通块往下走时交回的是没改过的计算值
                let mut comp = comp.clone();
                if let Some(li) = &first {
                    if li.list_style.is_some() {
                        comp.list_style = li.list_style.clone();
                    }
                }
                let marker = first
                    .as_ref()
                    .is_none_or(|li| li.display.as_deref().is_none_or(|d| d == "list-item"));
                if marker && comp.list_style.as_deref() != Some("none") {
                    return Special::Own(self.list(el, comp, first));
                }
            }
            "table" => return Special::Own(self.table(el, comp)),
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
                return Special::Own(b);
            }
            _ => {}
        }
        Special::Normal(comp)
    }

    /// 自己的外边距、内边距照计算值的块（列表、表格、单元格、水平线用；不摊平、不并进子块）。
    fn boxed(&self, kind: Kind, comp: Computed, id: Option<&str>) -> Block {
        let l = BoxLens::of(&comp);
        let mut b = Block::anonymous(
            kind,
            comp,
            id.map(|i| vec![(i.to_string(), 0)]).unwrap_or_default(),
        );
        (
            b.margin_top,
            b.margin_bottom,
            b.margin_left,
            b.margin_right,
            b.padding,
            b.padding_h,
        ) = (
            l.margin_top,
            l.margin_bottom,
            l.margin_left,
            l.margin_right,
            l.padding,
            l.padding_h,
        );
        b
    }

    /// 元素的子块；只有一个匿名文字块时直接用它当 `ty` 类型的节点（列表项、表格标题里的文字），否则包成容器。
    fn wrap_children(&self, el: ElementRef, comp: Computed, ty: Option<u32>) -> Block {
        let mut kids = Vec::new();
        self.children(el, &comp, &mut kids);
        let id = el.value().attr("id");
        if ty.is_some()
            && kids.len() == 1
            && matches!(kids[0].kind, Kind::Text { .. })
            && kids[0].is_anonymous()
        {
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
}

fn image_src(el: &ElementRef) -> Option<String> {
    let v = el.value();
    match v.name() {
        "img" => v.attr("src").map(str::to_string),
        // SVG 的 `<image xlink:href>`：属性在 xlink 命名空间里，`attr()` 只认无命名空间的，按本地名找（《绍宋》封面页
        // `<svg><image xlink:href="../Images/cover.png"/></svg>`；以前找不到、封面页整页丢掉——优化器以前把 SVG 封面换成 `<img>`，
        // 只修复模式不换了才露出来，2026-10-08 真机：封面没了）。
        "image" => v
            .attrs()
            .find(|(k, _)| *k == "href")
            .map(|(_, val)| val.to_string()),
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
        || inline_box(a)
        || a.baseline_shift.is_some()
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
        if matches!(c.kind, Kind::Text { .. })
            && c.is_anonymous()
            && c.inline
            && c.comp == inherited
        {
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

//! 版面：块生成故事线节点（[`Builder::node`]），逐个文档写版面、故事线、文字池（[`lay_out`]），固定版式的整页图片。

use super::*;

pub(super) struct SectionCtx {
    pool: u32,
    texts: Vec<Value>,
    /// 节点顺序与各自占的位置数、从节点里的第几个字起（行内图片把文字节点切成几段，见 [`pid_map`]）。
    order: Vec<(i64, usize, usize)>,
    ids: Vec<(String, i64, usize)>,
    resources: Vec<usize>,
    lang: Option<String>,
    /// 还没落到节点上的锚点：取不到的图片上的 id，挂到下一个节点的开头（同解析时没有文字的元素，见 [`Doc::pending`]），
    /// 文档末尾的挂到最后一个节点末尾。以前随图片一起丢掉，指向它的链接退回文件开头（写出器 15）。
    pending: Vec<String>,
    /// 最后登记的节点和它的末尾（文字节点是字数，别的是 0）。
    last_end: Option<(i64, usize)>,
}

impl SectionCtx {
    /// 登记一个节点：占 `len` 个位置，锚点 `ids`（文字节点 `text` 按各自的字符偏移，别的都在开头），前面攒下的锚点落在它开头。
    fn place(&mut self, eid: i64, len: usize, ids: &[(String, usize)], text: bool) {
        for id in self.pending.drain(..) {
            self.ids.push((id, eid, 0));
        }
        for (id, off) in ids {
            self.ids.push((id.clone(), eid, if text { *off } else { 0 }));
        }
        self.order.push((eid, len, 0));
        self.last_end = Some((eid, if text { len } else { 0 }));
    }

    /// 文字节点里夹着行内图片（字符偏移 `at`、图片节点 id）：位置照 Send to Kindle 切段——图片前的字、图片处一个空段、
    /// 图片节点占一个位置、图片后的字接着从文字节点的偏移数（测试书 `[12, 8, 12], 0, [1, 8, 13]`）。
    fn split_at_images(&mut self, eid: i64, len: usize, images: &[(usize, i64)]) {
        let Some(first) = self.order.pop() else { return };
        debug_assert_eq!(first.0, eid);
        let mut cur = 0;
        for &(at, img) in images {
            if at > cur {
                self.order.push((eid, at - cur, cur));
            }
            self.order.push((eid, 0, at));
            self.order.push((img, 1, 0));
            cur = at + 1;
        }
        if cur < len {
            self.order.push((eid, len - cur, cur));
        }
    }
}

pub(super) struct SectionOut {
    pub(super) name: u32,
    pub(super) length: usize,
    pub(super) eids: Vec<i64>,
    pub(super) pid_map: Vec<Value>,
    pub(super) resources: Vec<usize>,
    pub(super) first_eid: i64,
}

/// 固定版式（漫画，优化器 `comicfxl` 写的 `fixed-layout`/`original-resolution`）：画布宽高（取不到写 0）。见 docs/kfx.md#固定版式。
pub(super) fn fixed_canvas(book: &Loaded) -> Option<(i64, i64)> {
    (!book.meta.fixed_layout.is_empty()).then(|| {
        book.meta
            .fixed_layout
            .iter()
            .find(|(n, _)| *n == 126)
            .and_then(|(_, v)| v.split_once('x'))
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
            .unwrap_or((0, 0))
    })
}

/// 版面阶段的结果。
pub(super) struct Layout {
    pub(super) sections: Vec<SectionOut>,
    /// 版面、故事线、文字池实体。
    pub(super) entities: Vec<Entity>,
    /// (文件, 锚点) → (节点 id, 字符偏移)；锚点空的是文件开头。
    pub(super) id_map: HashMap<(String, String), (i64, usize)>,
    /// 封面图那一页的版面模板节点 id（导航的封面地标指向它）。
    pub(super) cover_tmpl: Option<i64>,
}

/// 逐个文档生成版面（`$260`）、故事线（`$259`）、文字池（`$145`），登记每个 id 的位置。
pub(super) fn lay_out(book: &Loaded, b: &mut Builder, parsed: Vec<ParsedDoc>, fixed_canvas: Option<(i64, i64)>, direction: u32) -> Result<Layout, String> {
    let mut sections: Vec<SectionOut> = Vec::new();
    let mut entities: Vec<Entity> = Vec::new();
    let mut id_map: HashMap<(String, String), (i64, usize)> = HashMap::new();
    let mut cover_tmpl: Option<i64> = None;
    // 没有可见内容的文件（只有隐藏标题之类）：目录项、链接改指到下一个版面的开头。
    let mut empty_docs: Vec<String> = Vec::new();
    let is_cover = |b: &Builder, r: usize| book.cover.as_deref().is_some_and(|c| b.res.index_of(c) == Some(r));
    for (si, path, lang, blocks) in parsed {
        if blocks.is_empty() {
            empty_docs.push(path.to_string());
            continue;
        }
        let sec_name = b.syms.sym(&format!("sec{si}"));
        let story_name = b.syms.sym(&format!("story{si}"));
        let pool = b.syms.sym(&format!("text{si}"));
        let tmpl_eid = b.eid();
        let mut ctx = SectionCtx { pool, texts: Vec::new(), order: vec![(tmpl_eid, 1, 0)], ids: Vec::new(), resources: Vec::new(), lang, pending: Vec::new(), last_end: None };
        // 只有一张图的文档写成整页图片版面（封面、插图页），和样本一样。
        let single_image = blocks.len() == 1 && matches!(blocks[0].kind, Kind::Image { .. });
        let nodes: Vec<Value> = if let (true, Some((cw, ch)), Kind::Image { src, .. }) = (single_image, fixed_canvas, &blocks[0].kind) {
            // 固定版式的一页，照 Amazon 转的异形页样本：整页容器（画布宽高、`$476`、position relative）里放一个绝对定位的
            // 图片节点（宽高、上、左）。比画布小的图有的页整页空白是资源符号顺序造成的，不是这里（见下面分配资源符号处）。
            // 图片取不到的这一页同没有内容的文件：目录项、链接改指到下一个版面（以前没登记，指到它的退回书的开头）。
            let Some(r) = b.resource(src, ImageUse::Content) else {
                empty_docs.push(path.to_string());
                continue;
            };
            let (cid, iid) = (b.eid(), b.eid());
            ctx.order.push((cid, 1, 0));
            ctx.order.push((iid, 1, 0));
            ctx.resources.push(r);
            let (w, h) = fixed_image_size((i64::from(b.res[r].width), i64::from(b.res[r].height)), (cw, ch));
            let res = b.res_sym(r);
            // 宽高照写像素数（不带单位，同样本）
            let sized = |w: i64, h: i64| {
                let mut p = StyleProps::new();
                p.float(P_WIDTH, w as f64);
                p.float(P_HEIGHT, h as f64);
                p.symbol(P_SIZING, SIZING_VALUE);
                p
            };
            if cw > 0 {
                let mut cp = sized(cw, ch);
                cp.flag(P_CLIP, true);
                cp.symbol(P_POSITION, POSITION_RELATIVE);
                let cstyle = b.style(cp);
                let mut ip = sized(w, h);
                ip.float(P_TOP, ((ch - h) / 2) as f64);
                ip.float(P_LEFT, ((cw - w) / 2) as f64);
                ip.symbol(P_POSITION, POSITION_ABSOLUTE);
                let istyle = b.style(ip);
                let img = image_node(iid, istyle, res);
                vec![Value::Struct(vec![
                    (EID, Value::Int(cid)),
                    (TMPL_FIT, Value::Symbol(CONTAINER_LAYOUT)),
                    (STYLE_REF, Value::Symbol(cstyle)),
                    (NODE_TYPE, Value::Symbol(NODE_CONTAINER)),
                    (CHILDREN, Value::List(vec![img])),
                ])]
            } else {
                let style = b.style(sized(w, h));
                vec![image_node(iid, style, res)]
            }
        } else {
            let nodes = blocks.iter().filter_map(|bl| b.node(bl, &Parent::root(b.stats.base_fs), &mut ctx)).collect();
            // 文档末尾取不到的图片上的锚点挂到最后一个节点末尾
            if let Some((eid, end)) = ctx.last_end {
                for id in std::mem::take(&mut ctx.pending) {
                    ctx.ids.push((id, eid, end));
                }
            }
            nodes
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
            if is_cover(b, ctx.resources[0]) {
                cover_tmpl.get_or_insert(tmpl_eid);
            }
        } else if single_image {
            let r = &b.res[ctx.resources[0]];
            tmpl.push((TMPL_WIDTH, Value::Int(i64::from(r.width))));
            tmpl.push((TMPL_HEIGHT, Value::Int(i64::from(r.height))));
            tmpl.push((TMPL_FIT, Value::Symbol(TMPL_FIT_VALUE)));
            tmpl.push((TMPL_ALIGN, Value::Symbol(ALIGN_CENTER)));
            tmpl.push((NODE_TYPE, Value::Symbol(NODE_CONTAINER)));
            if is_cover(b, ctx.resources[0]) {
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
        for (i, e, o) in ctx.ids {
            id_map.entry((path.to_string(), i)).or_insert((e, o));
        }
        let length = ctx.order.iter().map(|o| o.1).sum();
        sections.push(SectionOut {
            name: sec_name,
            length,
            eids: ctx.order.iter().map(|o| o.0).collect(),
            pid_map: pid_map(&ctx.order),
            resources: ctx.resources,
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
    Ok(Layout { sections, entities, id_map, cover_tmpl })
}

/// 固定版式一页里图片的显示宽高：保持比例缩进画布，**不超过图自己的尺寸**——节点比图片资源大时 Kindle 不放大、整页空白
/// （2026-10-06 真机：954×1272 的图写成画布 1272×1696 → 空白页）。所以比例和画布一致（`comicfxl::fills_canvas`，误差 1px 内）
/// 且不比画布小的写画布大小，比画布小的、比例不一致的（装饰小图、窄页、跨页）按缩放后的尺寸，由调用方居中。
/// 以前一律写画布大小：比例不一致的被拉伸，小的空白。画布宽高未知（0）或图的尺寸取不到时退回另一方。
pub(super) fn fixed_image_size((iw, ih): (i64, i64), (cw, ch): (i64, i64)) -> (i64, i64) {
    if cw <= 0 || ch <= 0 {
        return (iw, ih);
    }
    if iw <= 0 || ih <= 0 || (iw >= cw && bookconv::comicfxl::fills_canvas((iw as u32, ih as u32), cw as u32, ch as u32)) {
        return (cw, ch);
    }
    let s = (cw as f64 / iw as f64).min(ch as f64 / ih as f64).min(1.0);
    (((iw as f64 * s).round() as i64).max(1), ((ih as f64 * s).round() as i64).max(1))
}

impl Builder {
    /// 生成一个块的节点（递归），同时登记位置、id。
    fn node(&mut self, b: &Block, parent: &Parent, ctx: &mut SectionCtx) -> Option<Value> {
        let col = self.styles.text_color(&b.comp);
        let props = self.block_props(b, parent, &ctx.lang, col);
        // 本节点给子节点（行内区间、容器里的块）的「父节点」
        let me = Parent { weight: weight_of(&b.comp), color: shown_color(col, &b.comp, parent), avail: inner_width(b, parent.avail), base_fs: parent.base_fs };
        match &b.kind {
            Kind::Text { text, runs, images } => {
                let eid = self.eid();
                let chars = text.chars().count();
                ctx.place(eid, chars, &b.ids, true);
                let style = self.style(props);
                // 夹着行内图片的：文字和图片节点直接放在节点内容里（同 Send to Kindle），不进文字池
                let content = if images.is_empty() {
                    let idx = ctx.texts.len();
                    ctx.texts.push(Value::String(text.clone()));
                    (TEXT_REF, Value::Struct(vec![(NAME, Value::Symbol(ctx.pool)), (TEXT_INDEX, Value::Int(idx as i64))]))
                } else {
                    (CHILDREN, Value::List(self.inline_content(text, images, b, eid, chars, ctx)))
                };
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
                            let mut rp = text_props(&mut self.styles, rc, &me);
                            // 字号和段落一样的不写（Send to Kindle 同样；写了的话 Kindle 按区间的字号另算行高，基线偏移、空的
                            // inline-block 把整行撑高，2026-10-14 怪癖测试书 Q122、Q125 真机对照）
                            if (rc.font_size - b.comp.font_size).abs() < 1e-6 {
                                rp.take(P_FONT_SIZE);
                            }
                            // 空的 inline-block 插的零宽空格：没有字，不写文字颜色（Send to Kindle 同样）
                            if text.chars().skip(r.start).take(r.len).all(|c| c == '\u{200B}') {
                                rp.take(P_COLOR);
                            }
                            if rc.superscript && !b.comp.superscript {
                                rp.symbol(P_VERTICAL_ALIGN, VALIGN_SUPER);
                            } else if rc.subscript && !b.comp.subscript {
                                rp.symbol(P_VERTICAL_ALIGN, VALIGN_SUB);
                            }
                            rp.extend(border_props(rc));
                            rp.extend(inline_box_props(rc));
                            // `<a>` 的颜色写成链接（未访问、已访问）的颜色（Send to Kindle 同样：《人生海海》目录 `color:#00C`）
                            if r.anchor {
                                if let Some(col) = rp.take(P_COLOR) {
                                    for k in [P_LINK_UNVISITED, P_LINK_VISITED] {
                                        rp.set(k, Value::Struct(vec![(P_COLOR, col.clone())]));
                                    }
                                }
                            }
                            if let Some(bg) = rc.background {
                                rp.color(P_INLINE_BACKGROUND, bg);
                            }
                            f.push((STYLE_REF, Value::Symbol(self.style(rp))));
                        }
                        Value::Struct(f)
                    })
                    .collect();
                if !run_values.is_empty() {
                    f.push((RUNS, Value::List(run_values)));
                }
                f.push(content);
                Some(Value::Struct(f))
            }
            Kind::Image { src, .. } => {
                let Some(r) = self.resource(src, ImageUse::Content) else {
                    ctx.pending.extend(b.ids.iter().map(|(id, _)| id.clone()));
                    return None;
                };
                let eid = self.eid();
                ctx.place(eid, 1, &b.ids, false);
                ctx.resources.push(r);
                let style = self.style(props);
                let res = self.res_sym(r);
                Some(image_node(eid, style, res))
            }
            Kind::Container(children) => {
                let eid = self.eid();
                ctx.place(eid, 1, &b.ids, false);
                if let Some(h) = b.heading {
                    self.headings.push((h, eid));
                }
                let ty = b.ty.unwrap_or(NODE_CONTAINER);
                let mut props = props;
                // 背景图：样式里指向图片资源（`$479`），同《绍宋》样本 `body.juan` 等
                if let Some(r) = b.comp.bg_image.as_deref().and_then(|src| self.resource(src, ImageUse::Background)) {
                    ctx.resources.push(r);
                    props.symbol(P_BG_IMAGE, self.res_sym(r));
                    if b.comp.bg_no_repeat {
                        props.symbol(P_BG_REPEAT, BG_NO_REPEAT);
                    }
                    if b.comp.bg_fixed {
                        props.symbol(P_BG_ATTACHMENT, BG_FIXED);
                    }
                    let len = |l: Len| match l {
                        Len::Percent(p) => num(p, U_PERCENT),
                        Len::Em(n) => num(n, U_EM),
                        Len::Pt(n) => num(n, U_PT),
                    };
                    for (v, k) in b.comp.bg_position.iter().zip([P_BG_POS_X, P_BG_POS_Y]).chain(b.comp.bg_size.iter().zip([P_BG_SIZE_W, P_BG_SIZE_H])) {
                        if let Some(l) = v {
                            props.set(k, len(*l));
                        }
                    }
                }
                // 表体、行这些结构层没有样式（同样本）。
                let styled = !matches!(ty, NODE_THEAD | NODE_TBODY | NODE_TFOOT | NODE_ROW);
                let style = styled.then(|| self.style(props));
                let kids: Vec<Value> = children.iter().filter_map(|c| self.node(c, &me, ctx)).collect();
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

impl Builder {
    /// 夹着行内图片的文字节点的内容：文字在图片处切开，图片写成行内图片节点；位置按段登记（[`SectionCtx::split_at_images`]）。
    /// 取不到的图片在文字里换成一个空格（字符偏移不变，区间、锚点照旧对得上）。
    fn inline_content(&mut self, text: &str, images: &[InlineImage], b: &Block, eid: i64, chars: usize, ctx: &mut SectionCtx) -> Vec<Value> {
        let mut out = Vec::new();
        let mut placed = Vec::new();
        let mut piece = String::new();
        let mut images = images.iter().peekable();
        for (i, ch) in text.chars().enumerate() {
            let Some(img) = images.next_if(|im| im.at == i) else {
                piece.push(ch);
                continue;
            };
            let Some(r) = self.resource(&img.src, ImageUse::Content) else {
                piece.push(' ');
                continue;
            };
            if !piece.is_empty() {
                out.push(Value::String(std::mem::take(&mut piece)));
            }
            let iid = self.eid();
            placed.push((i, iid));
            ctx.resources.push(r);
            let props = self.inline_image_props(img, r, b.comp.font_size);
            let style = self.style(props);
            let res = self.res_sym(r);
            let Value::Struct(mut f) = image_node(iid, style, res) else { unreachable!() };
            f.push((ALT_TEXT, Value::String(img.alt.clone())));
            f.push((RENDER, Value::Symbol(RENDER_INLINE)));
            out.push(Value::Struct(f));
        }
        if !piece.is_empty() {
            out.push(Value::String(piece));
        }
        if !placed.is_empty() {
            ctx.split_at_images(eid, chars, &placed);
        }
        out
    }

    /// 行内图片的样式（照 Send to Kindle，2026-10-12 测试书）：宽、高都写 em（相对所在段落的字号），只写了一边的另一边按图的比例；
    /// 都没写的按图的像素（1px＝1/16em）。写了尺寸的加 `$546: $377`，宽度用 em 写的另加最大宽度 100%；`vertical-align` 照写。
    fn inline_image_props(&self, img: &InlineImage, r: usize, block_fs: f64) -> StyleProps {
        let (w, h) = inline_size(img, (self.res[r].width, self.res[r].height));
        let fs = if block_fs > 0.0 { block_fs } else { 1.0 };
        let mut p = StyleProps::new();
        p.set(P_WIDTH, num(w / fs, U_EM));
        p.set(P_HEIGHT, num(h / fs, U_EM));
        if img.width.is_some() || img.height.is_some() {
            p.symbol(P_SIZING, SIZING_VALUE);
        }
        if matches!(img.width, Some(Len::Em(_))) {
            p.set(P_MAX_WIDTH, num(100.0, U_PERCENT));
        }
        let va = match img.comp.valign.as_deref() {
            Some("baseline") => Some(VALIGN_BASELINE),
            Some("super") => Some(VALIGN_SUPER),
            Some("sub") => Some(VALIGN_SUB),
            Some("middle") => Some(ALIGN_CENTER),
            Some("top") => Some(VALIGN_TOP),
            Some("text-bottom") => Some(VALIGN_TEXT_BOTTOM),
            _ => None,
        };
        if let Some(v) = va {
            p.symbol(P_VERTICAL_ALIGN, v);
        }
        p
    }
}

/// 行内图片显示的宽、高（根 em，见 [`Builder::inline_image_props`]）：只写了一边的另一边按图的比例，都没写的按像素（1px＝1/16em）。
/// `--styles` 导出（`analyze::dump_blocks`）也用它，`tools/kfx/unify.py` 拿掌阅、Move 的 EPUB 算出来的和 KFX 比。
pub(super) fn inline_size(img: &InlineImage, (iw, ih): (u32, u32)) -> (f64, f64) {
    let (iw, ih) = (f64::from(iw.max(1)), f64::from(ih.max(1)));
    let rem = |l: Len| match l {
        Len::Percent(p) => p / 100.0 * PAGE_WIDTH_EM,
        l => rem_len(l, img.comp.font_size).unwrap_or_default(),
    };
    match (img.width.map(rem), img.height.map(rem)) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ih / iw),
        (None, Some(h)) => (h * iw / ih, h),
        (None, None) => (iw / 16.0, ih / 16.0),
    }
}

/// 图片节点：节点 id、样式、图片资源。
fn image_node(eid: i64, style: u32, res: u32) -> Value {
    Value::Struct(vec![(EID, Value::Int(eid)), (STYLE_REF, Value::Symbol(style)), (NODE_TYPE, Value::Symbol(NODE_IMAGE)), (RESOURCE_REF, Value::Symbol(res))])
}

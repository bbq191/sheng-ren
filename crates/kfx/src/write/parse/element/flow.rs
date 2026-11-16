//! 行内收集和收块：行内缓冲怎么开、连续的行内内容怎么收成文字块、行内元素和块级元素怎么分派。

use super::*;

impl Doc<'_> {
    /// 新的行内缓冲：包着它的行内链接（[`Doc::links`]）接着开，只带链接（样式已经由计算值继承下来）。
    pub(super) fn inline(&self) -> Inline {
        let mut inl = Inline::new();
        for link in self.links.borrow().iter() {
            inl.open_run(Run {
                start: 0,
                len: 0,
                comp: None,
                link: Some(link.clone()),
                note_ref: false,
                anchor: false,
            });
        }
        inl
    }

    /// 块级元素的子节点：连续的行内内容合成匿名文字块，块级子元素递归，图片单独成块。
    pub(in crate::write) fn children(&self, el: ElementRef, comp: &Computed, out: &mut Vec<Block>) {
        let mut inl = self.inline();
        for child in el.children() {
            self.inline_or_block(child, comp, comp, &mut inl, out);
        }
        self.flush(&mut inl, comp, out);
    }

    pub(super) fn flush(&self, inl: &mut Inline, comp: &Computed, out: &mut Vec<Block>) {
        let reopened = inl.cut_open();
        let taken = inl.take();
        inl.open = reopened;
        // 独立成块的图片：整段没有字的（插图段落），或者所在的行（按 `<br/>` 分）没有字、后面跟着换行的（`<p><img/><br/>图注</p>`）；
        // 别的嵌在字里（见 [`InlineImage`]）。Send to Kindle 同样（2026-10-12 测试书：图后换行接图注的独立成块，
        // 「文字<br/>小图」段末单独一行的小图照样是行内的）。
        let chars: Vec<char> = taken.text.chars().collect();
        let textless = taken.is_empty();
        let mut alone = Vec::new();
        let mut line = 0;
        for i in 0..=chars.len() {
            if i == chars.len() || chars[i] == '\n' {
                let seg = &chars[line..i];
                let broken = i < chars.len();
                if !seg.iter().any(|c| !c.is_whitespace() && *c != OBJ) && (textless || broken) {
                    alone.extend(
                        taken
                            .images
                            .iter()
                            .filter(|im| im.at >= line && im.at < i)
                            .map(|im| im.at),
                    );
                }
                line = i + 1;
            }
        }
        if alone.is_empty() {
            return self.flush_piece(taken, comp, out);
        }
        let mut cur = 0;
        for &at in &alone {
            self.flush_piece(taken.slice(&chars, cur, at), comp, out);
            let Some(img) = taken.images.iter().find(|im| im.at == at) else {
                continue;
            };
            let mut b = Block::anonymous(
                Kind::Image {
                    src: img.src.clone(),
                    linked: img.linked,
                },
                img.comp.clone(),
                self.take_pending().into_iter().map(|i| (i, 0)).collect(),
            );
            b.gap_before = self.gap.take();
            out.push(b);
            cur = at + 1;
        }
        self.flush_piece(taken.slice(&chars, cur, chars.len()), comp, out);
    }

    /// 一段行内内容写成文字块；没有字的不出块：里面的锚点留给后面的内容，只有 `&nbsp;`、全角空格、`<br/>` 的
    /// （按 CSS 排出一个空行；只有普通空白的排不出行，不算）折成下一块的上边距。
    fn flush_piece(&self, taken: Inline, comp: &Computed, out: &mut Vec<Block>) {
        if taken.is_empty() {
            if !taken.text.is_empty() {
                self.gap.borrow_mut().h +=
                    BLANK_LINE_FOLD * comp.line_height.unwrap_or(LH_EM) * comp.font_size;
            }
            self.pending
                .borrow_mut()
                .extend(taken.ids.into_iter().map(|(i, _)| i));
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
        let mut ids = taken
            .ids
            .into_iter()
            .map(|(i, o)| (i, o.min(chars)))
            .collect();
        prepend_ids(&mut ids, self.take_pending());
        let mut b = Block::anonymous(
            Kind::Text {
                text,
                runs,
                images: taken.images,
            },
            comp.inherited(),
            ids,
        );
        b.inline = true;
        b.gap_before = self.gap.take();
        out.push(b);
    }

    pub(super) fn inline_or_block(
        &self,
        node: NodeRef<Node>,
        block_comp: &Computed,
        parent: &Computed,
        inl: &mut Inline,
        out: &mut Vec<Block>,
    ) {
        match node.value() {
            Node::Text(t) => inl.push_text(t, parent.pre),
            Node::Element(_) => {
                let Some(el) = ElementRef::wrap(node) else {
                    return;
                };
                let name = el.value().name();
                if name == "br" {
                    inl.push_break();
                    return;
                }
                if let Some(src) = image_src(&el) {
                    let comp = self.comp(&el, parent);
                    let linked = el
                        .ancestors()
                        .filter_map(ElementRef::wrap)
                        .any(|a| a.value().name() == "a" && a.value().attr("href").is_some());
                    let src = resolve_link(self.path, &src).0;
                    if comp.display.as_deref() == Some("block") {
                        self.flush(inl, block_comp, out);
                        // 图片自己的 id（`<img id="filepos152">`）也是锚点
                        let mut lead = self.take_pending();
                        lead.extend(el.value().attr("id").map(str::to_string));
                        let mut b = Block::anonymous(
                            Kind::Image { src, linked },
                            comp,
                            lead.into_iter().map(|i| (i, 0)).collect(),
                        );
                        b.gap_before = self.gap.take();
                        out.push(b);
                        return;
                    }
                    // 先当行内图片收着：同一段里有字就嵌在字里，没有字的在 `flush` 里独立成块
                    if let Some(id) = el.value().attr("id") {
                        inl.push_id(id);
                    }
                    let (width, height) = self.image_size(&el);
                    let alt = el.value().attr("alt").unwrap_or_default().to_string();
                    inl.push_image(InlineImage {
                        at: 0,
                        src,
                        alt,
                        comp,
                        linked,
                        width,
                        height,
                    });
                    return;
                }
                let comp = self.comp(&el, parent);
                if comp.display.as_deref() == Some("none")
                    || matches!(name, "script" | "style" | "head")
                {
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
                // 空的 inline-block 小方块（《狼厅》微信读书注释图标：19px 黑底圆，注释文字只在属性里）：Send to Kindle 插一个零宽空格，
                // 区间写背景、圆角、竖直对齐，外边距和宽度（写成右内边距）按 0.75pt 一 px、高度按 0.45pt 写成最小高度，Kindle 上画出
                // 黑圆点（2026-10-14 怪癖测试书 Q125 真机）
                if comp.display.as_deref() == Some("inline-block")
                    && el.text().all(|t| t.trim().is_empty())
                    && el.children().all(|c| !c.value().is_element())
                    && (comp.background.is_some() || comp.width.is_some())
                {
                    let fs = comp.font_size;
                    let mut rc = comp.clone();
                    for side in [1, 3] {
                        rc.margin[side] = comp.margin[side].map(|l| match l {
                            Len::Pt(n) => {
                                Len::Em(n / 0.45 * 0.75 / bookconv::cascade::PT_PER_EM / fs)
                            }
                            l => l,
                        });
                    }
                    rc.padding[1] = comp.width.map(|l| match l {
                        Len::Pt(n) => Len::Em(n / bookconv::cascade::PT_PER_EM / fs),
                        l => l,
                    });
                    rc.min_height = comp.min_height.map(|l| match l {
                        Len::Pt(n) => Len::Pt(n * 0.6),
                        l => l,
                    });
                    inl.open_run(Run {
                        start: 0,
                        len: 0,
                        comp: Some(rc),
                        link: None,
                        note_ref: false,
                        anchor: false,
                    });
                    inl.push_char('\u{200B}');
                    if let Some(o) = inl.open.pop() {
                        inl.close_run(o);
                    }
                    return;
                }
                let link = (name == "a")
                    .then(|| el.value().attr("href"))
                    .flatten()
                    .filter(|h| !bookconv::html::is_external(h))
                    .map(|h| {
                        let (path, frag) = resolve_link(self.path, h);
                        (
                            if path.is_empty() {
                                self.path.to_string()
                            } else {
                                path
                            },
                            frag.unwrap_or_default(),
                        )
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
                    inl.open_run(Run {
                        start: 0,
                        len: 0,
                        comp: styled.then(|| comp.clone()),
                        link,
                        note_ref: false,
                        anchor: name == "a",
                    });
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

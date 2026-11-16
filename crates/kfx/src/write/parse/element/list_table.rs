//! 列表和表格：`<ul>`/`<ol>` → 列表、列表项，`<table>` → 表、表头/表体/表脚、行、单元格。

use super::*;

impl Doc<'_> {
    /// `<ul>`/`<ol>` → `$276` 列表，`<li>` → `$277` 列表项（只有文字的列表项本身就是文字节点）。
    /// 列表符号的缺省按标签和嵌套层数（同浏览器：圆点 → 空心圆 → 方块），Amazon 转出来也是这样（2026-10-05 测试书）。
    /// `first_li`：第一个 `<li>` 的计算值（调用方判断要不要当列表时已经层叠过，不再算一遍；父元素只差列表符号，
    /// 而 `<li>` 的列表符号本来就继承或自己写，结果一样）。
    /// 直接放在 `<ul>`/`<ol>` 里的文字、行内元素、图片（不在 `<li>` 里）照浏览器当匿名块排在列表里、不带符号（以前丢掉）。
    pub(super) fn list(&self, el: ElementRef, comp: Computed, mut first_li: Option<Computed>) -> Block {
        let ordered = el.value().name() == "ol";
        let depth = el
            .ancestors()
            .filter_map(ElementRef::wrap)
            .filter(|a| a.value().name() == "ul")
            .count();
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
            Some(
                "cjk-ideographic"
                | "simp-chinese-informal"
                | "trad-chinese-informal"
                | "cjk-decimal",
            ) => LIST_CJK,
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
            if let Some(v) = child
                .value()
                .attr("value")
                .and_then(|v| v.trim().parse::<i64>().ok())
            {
                item.attrs.push((LIST_START, Value::Int(v)));
            }
            items.push(item);
        }
        self.flush(&mut inl, &comp, &mut items);
        let inside = comp.list_inside;
        let mut b = self.boxed(Kind::Container(items), comp, el.value().attr("id"));
        b.ty = Some(NODE_LIST);
        b.attrs.push((LIST_STYLE, Value::Symbol(style)));
        if let Some(n) = el
            .value()
            .attr("start")
            .and_then(|v| v.trim().parse::<i64>().ok())
        {
            b.attrs.push((LIST_START, Value::Int(n)));
        }
        if inside {
            b.attrs.push((LIST_POSITION, Value::Symbol(LIST_INSIDE)));
        }
        b
    }

    /// `<table>` → `$278` 表 → `$151`/`$454`/`$455` 表头/表体/表脚 → `$279` 行 → `$270` 单元格（2026-10-05 《绍宋》样本和测试书对照）。
    /// 跨列、跨行、竖直对齐写在单元格的样式里；`<caption>` 是 `$269` 节点（`$615: $453`）套文字。
    pub(super) fn table(&self, el: ElementRef, comp: Computed) -> Block {
        let mut parts = Vec::new();
        let mut col_widths: Option<Vec<f64>> = None;
        let spacing = |l: Option<Len>| match l {
            Some(Len::Pt(n)) => n * KFX_PT_PER_CSS_PT,
            Some(Len::Em(n)) => n * PT_PER_EM * comp.font_size,
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
                    cap.extra
                        .push((P_LAYOUT_HINTS, Value::List(vec![Value::Symbol(CAPTION)])));
                    parts.push(cap);
                }
                tag @ ("thead" | "tbody" | "tfoot") => {
                    let ty = match tag {
                        "thead" => NODE_THEAD,
                        "tfoot" => NODE_TFOOT,
                        _ => NODE_TBODY,
                    };
                    let mut rows = Vec::new();
                    for tr in part
                        .children()
                        .filter_map(ElementRef::wrap)
                        .filter(|e| e.value().name() == "tr")
                    {
                        let rc = self.comp(&tr, &pc);
                        let mut cells = Vec::new();
                        let mut widths = Vec::new();
                        let mut naturals = Vec::new();
                        for td in tr
                            .children()
                            .filter_map(ElementRef::wrap)
                            .filter(|e| matches!(e.value().name(), "td" | "th"))
                        {
                            let mut cc = self.comp(&td, &rc);
                            if cc.width.is_none() {
                                // HTML 属性不带单位的是像素（`width="30"`，Send to Kindle 按 30px 算列宽，Q129）
                                cc.width = td.value().attr("width").and_then(|w| {
                                    match w.trim().parse::<f64>() {
                                        Ok(n) => Some(Len::Pt(n * 0.75)),
                                        Err(_) => crate::css::parse_len(w),
                                    }
                                });
                            }
                            // UA 样式：单元格内边距 1px（按 0.45pt，Send to Kindle 写 0.031lh、0.117%，2026-10-14 怪癖测试书 Q97）。
                            for p in cc.padding.iter_mut() {
                                p.get_or_insert(Len::Pt(0.45));
                            }
                            widths.push(cc.width);
                            naturals.push(bookconv::kindle_model::natural_px(&td, cc.font_size));
                            // 单元格自己不写宽度、只写 `$546: $377`，宽度写成表格的列宽（Send to Kindle 同样，Q127–Q129）
                            let sized = cc.width.take().is_some();
                            let valign = match cc.valign.as_deref() {
                                Some("top" | "text-top") => VALIGN_TOP,
                                Some("bottom" | "text-bottom") => VALIGN_BOTTOM,
                                _ => ALIGN_CENTER,
                            };
                            let mut cell = self.wrap_children(td, cc, None);
                            cell.extra.push((P_CELL_VALIGN, Value::Symbol(valign)));
                            if sized {
                                cell.extra.push((P_SIZING, Value::Symbol(SIZING_VALUE)));
                            }
                            for (attr, key) in [("colspan", P_COLSPAN), ("rowspan", P_ROWSPAN)] {
                                if let Some(n) = td
                                    .value()
                                    .attr(attr)
                                    .and_then(|v| v.trim().parse::<i64>().ok())
                                    .filter(|n| *n > 1)
                                {
                                    cell.extra.push((key, Value::Int(n)));
                                }
                            }
                            cells.push(cell);
                        }
                        if col_widths.is_none() {
                            col_widths =
                                Some(bookconv::kindle_model::column_percents(&widths, &naturals));
                        }
                        let mut row =
                            Block::anonymous(Kind::Container(cells), rc.inherited(), Vec::new());
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
        let cols: Vec<Value> = col_widths
            .unwrap_or_default()
            .into_iter()
            .map(|p| Value::Struct(vec![(P_WIDTH, num(p, U_PERCENT))]))
            .collect();
        if !cols.is_empty() {
            attrs.push((TABLE_COLUMNS, Value::List(cols)));
        }
        let mut b = self.boxed(Kind::Container(parts), comp, el.value().attr("id"));
        b.ty = Some(NODE_TABLE);
        b.attrs = attrs;
        b
    }
}

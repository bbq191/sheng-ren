//! [`plan`] 的各段：一章要改的元素、横线图、行内要删的属性（[`Plan`](super::Plan)）。
use super::line::LineStyle;
use super::page_top_rule_px;
use super::rewrite::{big_top_pad, list_indent_extra_px, NARROW_SHARE};
use super::{fmt, heading, skipped, BoxGeometry, Line, Plan};
use crate::cascade::{Border, BorderWidth, Computed, Len, MediaEnv};
use crate::kindle_model::{self as km, Chapter};
use crate::reader_model::ReaderModel;
use crate::wash::Entry;
use scraper::{ElementRef, Node};
use std::collections::HashMap;
use std::rc::Rc;

/// `plan` 的只读上下文：一章、这台阅读器、盒尺寸、文档序里每个元素前面有没有内容。
struct Cx<'a> {
    ch: &'a Chapter,
    reader: &'a ReaderModel,
    g: BoxGeometry,
    content_before: Vec<bool>,
}

impl Cx<'_> {
    fn tag(&self, i: usize) -> String {
        km::reader_tag(&self.ch.el(i))
    }

    fn comp(&self, i: usize) -> &Rc<Computed> {
        &self.ch.els[i].comp
    }

    fn has_bg(&self, i: usize) -> bool {
        self.comp(i).background.is_some_and(|c| c >> 24 != 0) || self.comp(i).bg_image.is_some()
    }

    // 水平长度换成本元素的 em：pt 按 12pt 一根 em，% 按这台的内容区宽（Move 的 % 外边距是坏的，折成 em 写）
    fn to_em(&self, l: Option<Len>, fs: f64) -> f64 {
        match l {
            Some(Len::Em(n)) => n,
            Some(Len::Pt(n)) => n / km::PT_PER_EM / fs,
            Some(Len::Percent(p)) => p / 100.0 * self.g.area_w / (self.g.em_px * self.g.h_scale * fs),
            None => 0.0,
        }
    }

    fn pad_rem(&self, i: usize, side: usize) -> f64 {
        km::vertical_rem(self.comp(i).padding[side], self.comp(i).font_size).max(0.0)
    }

    fn visible(&self, i: usize, side: usize) -> Option<&Border> {
        self.comp(i).border[side].as_ref().filter(|b| {
            b.width != BorderWidth::Pt(0.0)
                && b.width != BorderWidth::Em(0.0)
                && b.color.is_none_or(|c| c >> 24 != 0)
        })
    }

    fn usable(&self, i: usize) -> bool {
        self.ch.els[i].is_block && !self.ch.els[i].hidden && !skipped(&self.tag(i))
    }

    // 里面有没有内容（字、不是我们垫的图）：空块（只有 id 的分页 div）不配对
    fn has_content(&self, i: usize) -> bool {
        self.ch.el(i).descendants().any(|d| match d.value() {
            Node::Text(t) => !t.trim().is_empty(),
            Node::Element(v) => {
                matches!(v.name(), "img" | "svg" | "image") && !v.classes().any(|c| c == "eink-sp")
            }
            _ => false,
        })
    }

    fn ours(&self, i: usize) -> bool {
        self.ch.el(i).value().classes().any(|c| c == "eink-sp")
    }

    // 一条看得见的边框写成单元格行内的 CSS（宽度照横线图折成屏幕像素）
    fn border_css(&self, i: usize, side: usize) -> Option<String> {
        let b = self.visible(i, side)?;
        let c = self.comp(i);
        let rem = match b.width {
            BorderWidth::Pt(n) => n / km::PT_PER_EM,
            BorderWidth::Em(n) => n * c.font_size,
        };
        let style = if matches!(b.style.as_str(), "dotted" | "dashed" | "double") {
            b.style.as_str()
        } else {
            "solid"
        };
        let color = (b.color.or(c.color).unwrap_or(0xFF00_0000) | 0xFF00_0000) & 0xFF_FFFF;
        let name = ["top", "right", "bottom", "left"][side];
        Some(format!(
            "border-{name}:{}px {style} #{color:06x}",
            fmt((rem * self.g.v_rem_px).max(1.0).round())
        ))
    }

    // 块在 Kindle 上能用的宽（屏幕像素）：内容区宽减去祖先的左右外边距、内边距
    fn avail_px(&self, i: usize) -> f64 {
        let mut w = self.g.area_w;
        let mut p = self.ch.els[i].parent;
        while let Some(a) = p {
            let c = self.comp(a);
            w -= (self.to_em(c.margin[3], c.font_size)
                + self.to_em(c.margin[1], c.font_size)
                + self.to_em(c.padding[3], c.font_size)
                + self.to_em(c.padding[1], c.font_size))
            .max(0.0)
                * c.font_size
                * self.g.em_px
                * self.g.h_scale;
            p = self.ch.els[a].parent;
        }
        w.max(self.g.area_w * 0.1)
    }

    // 不认块宽度的阅读器（Move）：定了宽度的块照 Kindle 折成左右外边距（本元素的 em），多出来的放右边（Kindle 不按 auto 居中）
    // 窄的定宽块（占能用宽的 [`NARROW_SHARE`] 以内）：不折成 em 外边距（跟着字号变），包进单格表格、表格宽写百分比
    // （元素 → (表格宽百分比, 靠哪边, 块自己的左右外边距 em)）
    fn width_to_margins(
        &self,
        width_margins: &mut HashMap<usize, [f64; 2]>,
        narrow: &mut HashMap<usize, (f64, &'static str, [f64; 2])>,
    ) {
        if !self.reader.block_width {
            for i in 0..self.ch.els.len() {
                let c = self.comp(i);
                let Some(w) = c.width.filter(|_| self.usable(i)) else {
                    continue;
                };
                let unit = c.font_size * self.g.em_px * self.g.h_scale;
                let avail = self.avail_px(i);
                let w_px = match w {
                    Len::Percent(p) => p / 100.0 * avail,
                    l => self.to_em(Some(l), c.font_size) * unit,
                };
                let side = |s: usize| {
                    if c.margin_auto[s] {
                        0.0
                    } else {
                        self.to_em(c.margin[s], c.font_size) * unit
                    }
                };
                let pad = (self.to_em(c.padding[3], c.font_size)
                    + self.to_em(c.padding[1], c.font_size))
                    * unit;
                let extra = avail - w_px - pad - side(3) - side(1);
                if w_px <= 0.0 || extra <= 1.0 {
                    continue;
                }
                // 多出来的放右边：Kindle 不按 margin:auto 居中（Q124 真机，Send to Kindle 写的块对齐不起作用）；只有左边 auto 的靠右
                // （写出器写成块的对齐 `$580` 靠右，Kindle 照排：《梁羽生》卷名 bigtitle 2026-10-14 真机），多出来的放左边
                let m = if c.margin_auto[3] && !c.margin_auto[1] {
                    [(side(3) + extra) / unit, side(1) / unit]
                } else {
                    [side(3) / unit, (side(1) + extra) / unit]
                };
                width_margins.insert(i, m);
                if w_px + pad < NARROW_SHARE * avail {
                    // 靠哪边同上面折外边距的算法：Kindle 不按 margin:auto 居中，多出来的放右边；只有左边 auto 的靠右
                    let align = if c.margin_auto[3] && !c.margin_auto[1] {
                        "right"
                    } else {
                        "left"
                    };
                    narrow.insert(
                        i,
                        (
                            (w_px + pad) / avail * 100.0,
                            align,
                            [side(3) / unit, side(1) / unit],
                        ),
                    );
                }
            }
        }
    }

    // 认块宽度、按 margin:auto 居中的阅读器（掌阅，Q124）：Kindle 不居中，定了宽度、两边都是 auto 的块左边的 auto 写成 0（只有左边
    // auto 的 Kindle 靠右，同掌阅，不动）
    fn margin_auto_center(&self, out: &mut Plan) {
        if self.reader.margin_auto && self.reader.block_width {
            for i in 0..self.ch.els.len() {
                if self.usable(i)
                    && self.comp(i).width.is_some_and(|w| !w.is_zero())
                    && self.comp(i).margin_auto[3]
                    && self.comp(i).margin_auto[1]
                {
                    out.decls.entry(i).or_default().push("margin-left:0".into());
                }
            }
        }
    }

    // 文件里前面还没有内容的块上的「前面分页」：新文件本来就从新的一页开始，Kindle 不再多翻一页；Move 照排、多出一张空白页
    // （《人生海海》「第一部」`<div></div><h1 style="page-break-before:always">`，2026-10-15 真机）。行内末尾接 auto（同一个 style 里
    // 后写的胜；不走类规则：类规则会连带把行内的外边距搬进类规则，Move 上下一章的间距跟着乱了，同日实测）
    fn page_top_break(&self, out: &mut Plan) {
        for (i, el) in self.ch.els.iter().enumerate() {
            let pb = |k: &str| {
                el.decls
                    .get(k)
                    .map(|v| v.trim().to_ascii_lowercase())
                    .unwrap_or_default()
            };
            let forced = matches!(
                pb("page-break-before").as_str(),
                "always" | "left" | "right"
            ) || matches!(
                pb("break-before").as_str(),
                "page" | "left" | "right" | "recto" | "verso"
            );
            if forced && el.is_block && !el.hidden && !self.content_before[i] {
                out.inline
                    .entry(i)
                    .or_default()
                    .push("page-break-before:auto".into());
            }
        }
    }

    // 认 float 的阅读器（掌阅）：Kindle 不浮动（Q133、Q134 真机），标题、段落上的写成 none（段落 Q148：《梁羽生》版本号，KINDLE_RULES 39；
    // 别的元素上的没量过，不动）
    fn float_off(&self, out: &mut Plan) {
        if self.reader.float {
            for (i, el) in self.ch.els.iter().enumerate() {
                let f = el
                    .decls
                    .get("float")
                    .map(|v| v.trim().to_ascii_lowercase())
                    .unwrap_or_default();
                if self.usable(i)
                    && (heading(&self.tag(i)) || self.tag(i) == "p")
                    && matches!(f.as_str(), "left" | "right")
                {
                    out.decls.entry(i).or_default().push("float:none".into());
                }
            }
        }
    }

    // 页首（文件里前面还没有内容）的块，负的上外边距配上内边距（《梁羽生》卷名 `padding-top:6em;margin-top:-6em`）：Kindle 把盒子往上
    // 推出页面、看不见的那截不占地方，认负外边距的掌阅却按整个盒子分页，竖排的卷名拆到三页（2026-10-14 真机）。照 Kindle 能看见的写：
    // 上外边距 0，上内边距减去推出去的
    fn negative_top_margin(&self, out: &mut Plan) {
        if self.reader.negative_margins && self.reader.padding {
            for (i, &before) in self.content_before.iter().enumerate() {
                let c = self.comp(i);
                let m = km::vertical_rem(c.margin[0], c.font_size);
                if !self.usable(i) || before || m >= 0.0 || c.margin_auto[0] {
                    continue;
                }
                let p = (self.pad_rem(i, 0) + m).max(0.0);
                let d = out.decls.entry(i).or_default();
                d.push("margin-top:0".into());
                d.push(format!("padding-top:{}em", fmt(p / c.font_size)));
            }
        }
    }

    // 左右有框的块（不画边框的阅读器）：包进单格表格；没背景图、宽度和居中不是 Kindle 的（折成外边距了的算）。列表（`ol`、`ul`）
    // 也包（《雪国》注释列表 `ol{border:1px dashed #366}`，Kindle 画框、Move 没有；KINDLE_RULES 35，维护者要修）：列表自己的左右内边距
    // 是放项目符号的，留给列表，单元格左右不加
    fn frame_side_borders(
        &self,
        out: &mut Plan,
        width_margins: &HashMap<usize, [f64; 2]>,
        narrow: &HashMap<usize, (f64, &'static str, [f64; 2])>,
        framed: &mut [bool],
    ) {
        if !self.reader.borders {
            let list = |i: usize| matches!(self.tag(i).as_str(), "ol" | "ul");
            for (i, f) in framed.iter_mut().enumerate() {
                let c = self.comp(i);
                let sized = width_margins.contains_key(&i);
                let frameable = self.usable(i)
                    || (list(i) && self.ch.els[i].is_block && !self.ch.els[i].hidden);
                if !frameable
                    || !(self.visible(i, 1).is_some()
                        || self.visible(i, 3).is_some()
                        || narrow.contains_key(&i))
                    || (!sized && (c.width.is_some() || c.margin_auto[1] || c.margin_auto[3]))
                    || (c.bg_image.is_some() && !narrow.contains_key(&i))
                {
                    continue;
                }
                *f = true;
                let px = |l: Option<Len>| {
                    (self.to_em(l, c.font_size) * c.font_size * self.g.em_px * self.g.h_scale)
                        .max(0.0)
                };
                // 外层 div 的左右外边距：块的外边距换成外层（父元素字号）的 em
                let parent_fs = self.ch.els[i]
                    .parent
                    .map_or(self.ch.body_comp.font_size, |p| self.comp(p).font_size);
                let m = narrow
                    .get(&i)
                    .map(|n| n.2)
                    .or_else(|| width_margins.get(&i).copied())
                    .unwrap_or([
                        self.to_em(c.margin[3], c.font_size),
                        self.to_em(c.margin[1], c.font_size),
                    ]);
                let pem = |v: f64| fmt((v * c.font_size / parent_fs).max(0.0));
                let wrap = format!(
                    "margin-left:{}em;margin-right:{}em;margin-top:0;margin-bottom:0;",
                    pem(m[0]),
                    pem(m[1])
                );
                let mut td: Vec<String> = (0..4).filter_map(|s| self.border_css(i, s)).collect();
                // 负的上外边距（不认负外边距的阅读器上没有）：Kindle 把框往上拉、上内边距再把字推下来，字还在原处（《梁羽生》卷名
                // `padding-top:6em;margin-top:-6em`，2026-10-14 探针 Q134），单元格的上内边距减去它
                let pull = if self.reader.negative_margins {
                    0.0
                } else {
                    km::vertical_rem(c.margin[0], c.font_size).min(0.0)
                };
                let pad = |side: usize| {
                    if side == 0 {
                        (self.pad_rem(i, 0) + pull).max(0.0) * self.g.v_rem_px
                    } else if side == 2 {
                        self.pad_rem(i, 2) * self.g.v_rem_px
                    } else if list(i) {
                        if side == 3 {
                            list_indent_extra_px(self.g.area_w)
                        } else {
                            0.0
                        }
                    } else {
                        px(c.padding[side])
                    }
                };
                td.push(format!(
                    "padding:{}px {}px {}px {}px",
                    fmt(pad(0).round()),
                    fmt(pad(1).round()),
                    fmt(pad(2).round()),
                    fmt(pad(3).round())
                ));
                // 单元格里块的背景色 xochitl 不画，写到单元格上
                if let Some(bg) = c.background.filter(|b| b >> 24 != 0) {
                    td.push(format!(
                        "background-color:#{:06x}",
                        crate::color::over_white(bg) & 0xFF_FFFF
                    ));
                }
                td.push("vertical-align:top".into());
                // 书里给 `table`、`td` 写的规则也落在我们的表格上（《雪国》`td{font-size:0.6em;line-height:100%}`、`table{text-align:center}`：
                // 注释框里的字只剩六成大，2026-10-15 unify）：单元格照外面那层的字号、行距、对齐、字体写回去（字号 1em 相对外层 div，
                // 外层同框外；Move 按根算的由 `root_em` 换算）
                let pc = self.ch.els[i]
                    .parent
                    .map_or(&*self.ch.body_comp, |p| self.comp(p));
                td.push("font-size:1em".into());
                td.push(match pc.line_height {
                    Some(f) => format!("line-height:{}em", fmt(f)),
                    None => "line-height:normal".into(),
                });
                td.push(format!(
                    "text-align:{}",
                    pc.text_align.as_deref().unwrap_or("left")
                ));
                if let Some(f) = &pc.font_family {
                    td.push(format!(
                        "font-family:{}",
                        if f.contains(' ') {
                            format!("'{f}'")
                        } else {
                            f.clone()
                        }
                    ));
                }
                out.frames.push((
                    i,
                    wrap,
                    td.join(";") + ";",
                    narrow
                        .get(&i)
                        .map(|n| (fmt((n.0 * 100.0).round() / 100.0), n.1)),
                ));
            }
        }
    }

    // 水平：左右内边距并进外边距；px、% 写的左右外边距折成 em（元素 → 改后的左右外边距 em，给横线图算宽度用）
    fn merge_h_padding(
        &self,
        out: &mut Plan,
        framed: &[bool],
        width_margins: &HashMap<usize, [f64; 2]>,
        h_margin: &mut HashMap<usize, [f64; 2]>,
    ) {
        if !(self.reader.padding
            && self.reader.px_margins
            && self.reader.percent_margins
            && self.reader.block_width)
        {
            // 声明的单位不对的（这台上 px、% 和 Kindle 不一样大）
            let wrong_unit = |i: usize, name: &str| {
                let v = self.ch.els[i]
                    .decls
                    .get(name)
                    .map(|v| v.trim().to_ascii_lowercase())
                    .unwrap_or_default();
                let nonzero = v
                    .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%')
                    .trim()
                    .parse::<f64>()
                    .is_ok_and(|n| n != 0.0);
                nonzero
                    && ((v.ends_with("px") && !self.reader.px_margins)
                        || (v.ends_with('%') && !self.reader.percent_margins))
            };
            for (i, _) in framed.iter().enumerate().filter(|(_, f)| !**f) {
                if !self.usable(i) {
                    continue;
                }
                let c = self.comp(i);
                let sized = width_margins.get(&i).copied();
                let mut m = sized.unwrap_or([
                    self.to_em(c.margin[3], c.font_size),
                    self.to_em(c.margin[1], c.font_size),
                ]);
                let mut changed = false;
                for (k, side, name) in [(0, 3, "margin-left"), (1, 1, "margin-right")] {
                    if c.margin_auto[side] && sized.is_none() {
                        continue;
                    }
                    // 带背景色的块不并内边距（并了色块就缩到字上）
                    let p = if self.reader.padding || self.has_bg(i) {
                        0.0
                    } else {
                        self.to_em(c.padding[side], c.font_size)
                    };
                    if p.abs() < 1e-6 && !wrong_unit(i, name) && sized.is_none() {
                        continue;
                    }
                    m[k] += p;
                    changed = true;
                    out.decls
                        .entry(i)
                        .or_default()
                        .push(format!("{name}:{}em", fmt(m[k])));
                }
                if changed {
                    h_margin.insert(i, m);
                }
                // 认内边距、px 又是屏幕像素的（掌阅）：px 写的左右内边距也照 Kindle 折成 em（Move 的内边距上面并进了外边距）
                if self.reader.padding && !self.reader.px_margins {
                    for (side, name) in [(3, "padding-left"), (1, "padding-right")] {
                        if wrong_unit(i, name) {
                            out.decls.entry(i).or_default().push(format!(
                                "{name}:{}em",
                                fmt(self.to_em(c.padding[side], c.font_size))
                            ));
                        }
                    }
                }
                // px 写的首行缩进同样是屏幕像素（掌阅 Q16、Q60；Move 2026-10-16 探针 Q151：居中标题 text-indent:48px 字右移 24px，KINDLE_RULES 40）：
                // 照 Kindle 折成 em
                if !self.reader.px_margins && wrong_unit(i, "text-indent") {
                    out.decls.entry(i).or_default().push(format!(
                        "text-indent:{}em",
                        fmt(self.to_em(c.text_indent, c.font_size))
                    ));
                }
            }
        }
    }

    // 单元格、行内元素、图上 px 写的左右边距（px 是屏幕像素的阅读器）：照 Kindle（外边距、内边距 0.45pt 一 px）折成 em。单元格的内边距两台都认
    // （类规则、行内都认，2026-10-13 探针 Q97–Q99）；行内元素、图的内外边距掌阅认、Move 一概不认（Q88–Q94），Move 上不改
    fn cell_px_margins(&self, out: &mut Plan) {
        if !self.reader.px_margins {
            let px_decl = |i: usize, name: &str| {
                self.ch.els[i].decls.get(name).is_some_and(|v| {
                    let v = v.trim().to_ascii_lowercase();
                    v.ends_with("px")
                        && v.trim_end_matches("px")
                            .trim()
                            .parse::<f64>()
                            .is_ok_and(|n| n != 0.0)
                })
            };
            for (i, el) in self.ch.els.iter().enumerate() {
                let t = self.tag(i);
                let cell = matches!(t.as_str(), "td" | "th");
                if el.hidden || !(cell || ((!el.is_block || t == "img") && self.reader.padding)) {
                    continue;
                }
                let c = self.comp(i);
                let sides = [
                    ("padding-top", 0),
                    ("padding-right", 1),
                    ("padding-bottom", 2),
                    ("padding-left", 3),
                ];
                if cell && sides.iter().any(|(n, _)| px_decl(i, n)) {
                    // 四边一起写（行内的内边距要删掉，没改的边照现在的值）
                    let d = out.decls.entry(i).or_default();
                    for (name, side) in sides {
                        let v = if side % 2 == 0 {
                            km::vertical_rem(c.padding[side], c.font_size) / c.font_size
                        } else {
                            self.to_em(c.padding[side], c.font_size)
                        };
                        d.push(format!("{name}:{}em", fmt(v.max(0.0))));
                    }
                }
                if !cell {
                    for (name, l) in [
                        ("margin-left", c.margin[3]),
                        ("margin-right", c.margin[1]),
                        ("padding-left", c.padding[3]),
                        ("padding-right", c.padding[1]),
                    ] {
                        if px_decl(i, name) {
                            out.decls.entry(i).or_default().push(format!(
                                "{name}:{}em",
                                fmt(self.to_em(l, c.font_size))
                            ));
                        }
                    }
                }
            }
        }
    }

    // 列表项的内边距（不认内边距的阅读器，Move：Q95 不认、Q96 上外边距认）：上下的并进同侧的外边距；左右的只在没有项目符号时并
    // （`list-style-type:none`、`display` 不是 list-item：有符号的并了会连符号一起挪）；带背景色的不动，有上下边框的上下不动
    fn list_item_padding(&self, out: &mut Plan) {
        if !self.reader.padding {
            for (i, el) in self.ch.els.iter().enumerate() {
                if self.tag(i) != "li" || el.hidden || self.has_bg(i) {
                    continue;
                }
                let c = self.comp(i);
                let mut d = Vec::new();
                let (pt, pb) = (self.pad_rem(i, 0), self.pad_rem(i, 2));
                if pt + pb > 1e-6
                    && self.visible(i, 0).is_none()
                    && self.visible(i, 2).is_none()
                {
                    d.push(format!(
                        "margin-top:{}em",
                        fmt((km::vertical_rem(c.margin[0], c.font_size) + pt) / c.font_size)
                    ));
                    d.push(format!(
                        "margin-bottom:{}em",
                        fmt((km::vertical_rem(c.margin[2], c.font_size) + pb) / c.font_size)
                    ));
                }
                let marker = c.list_style.as_deref() != Some("none")
                    && c.display
                        .as_deref()
                        .is_none_or(|v| v.eq_ignore_ascii_case("list-item"));
                if !marker {
                    for (side, name) in [(3, "margin-left"), (1, "margin-right")] {
                        let p = self.to_em(c.padding[side], c.font_size);
                        if p.abs() > 1e-6 && !c.margin_auto[side] {
                            d.push(format!(
                                "{name}:{}em",
                                fmt(self.to_em(c.margin[side], c.font_size) + p)
                            ));
                        }
                    }
                }
                if !d.is_empty() {
                    out.decls.entry(i).or_default().extend(d);
                }
            }
        }
    }

    // 横线图：看得见的上、下边框
    fn rule_images(
        &self,
        out: &mut Plan,
        framed: &[bool],
        h_margin: &HashMap<usize, [f64; 2]>,
        drawn: &mut [[bool; 2]],
    ) {
        if !self.reader.borders {
            // 书里的表格、单元格：看得见的边框照写进行内（xochitl 只认表格、单元格行内的边框）
            for i in 0..self.ch.els.len() {
                if !matches!(self.tag(i).as_str(), "table" | "td" | "th")
                    || self.ch.els[i].hidden
                {
                    continue;
                }
                let add: Vec<String> = (0..4).filter_map(|s| self.border_css(i, s)).collect();
                // 单元格有线的表格也要 cellspacing="0"（不然格子之间隔开、线成双）
                let lined_cells = self.tag(i) == "table"
                    && self
                        .ch
                        .el(i)
                        .descendants()
                        .filter_map(|d| self.ch.index_of(d.id()))
                        .any(|j| {
                            matches!(self.tag(j).as_str(), "td" | "th")
                                && (0..4).any(|s| self.visible(j, s).is_some())
                        });
                if !add.is_empty() || lined_cells {
                    out.cells.push((
                        i,
                        if add.is_empty() {
                            String::new()
                        } else {
                            add.join(";") + ";"
                        },
                    ));
                }
            }
            for i in 0..self.ch.els.len() {
                if !self.usable(i) || framed[i] {
                    continue;
                }
                let c = self.comp(i);
                for (k, side, top) in [(0, 0, true), (1, 2, false)] {
                    let Some(b) = self.visible(i, side) else { continue };
                    // 页首的块：垫图已经让出了横线图的高（`top_spacer.rs`，[`page_top_rule_px`]）；块里有行内垫图的，横线图插在它前面
                    // （块开始标签后面），窄图撑高的是横线下面那一行，要挪的也减去了横线图
                    if top
                        && !self.content_before[i]
                        && page_top_rule_px(c, &self.tag(i), self.reader, self.g.v_rem_px)
                            .is_none()
                    {
                        continue;
                    }
                    let style = match b.style.as_str() {
                        "dotted" => LineStyle::Dotted,
                        "dashed" => LineStyle::Dashed,
                        _ => LineStyle::Solid,
                    };
                    let border_rem = match b.width {
                        BorderWidth::Pt(n) => n / km::PT_PER_EM,
                        BorderWidth::Em(n) => n * c.font_size,
                    };
                    // 页首的块上内边距大到是拿来推字的：线只画线（见 [`page_top_rule_px`]）
                    let pad_px = if self.reader.padding
                        || (top && !self.content_before[i] && big_top_pad(c, self.reader))
                    {
                        0.0
                    } else {
                        self.pad_rem(i, side) * self.g.v_rem_px
                    };
                    let m = h_margin.get(&i).copied().unwrap_or([
                        self.to_em(c.margin[3], c.font_size),
                        self.to_em(c.margin[1], c.font_size),
                    ]);
                    let width_px = (self.g.area_w
                        - (m[0] + m[1]).max(0.0) * c.font_size * self.g.em_px * self.g.h_scale)
                        .clamp(self.g.area_w * 0.1, self.g.area_w);
                    let color = b.color.or(c.color).unwrap_or(0xFF00_0000) | 0xFF00_0000;
                    out.lines.push((
                        i,
                        Line {
                            color,
                            style,
                            border_px: (border_rem * self.g.v_rem_px).max(1.0),
                            pad_px,
                            width_px,
                            top,
                        },
                    ));
                    drawn[i][k] = true;
                    // 行距少出来的空白全从下面扣的阅读器（Move）：行距倍数低于下限时字伸出行盒，末尾的横线图压在字上（profile
                    // `line_height_floor`，2026-10-15 探针 Q143、《人生海海》版权页）。插了下边框的块行距不低于它（行内写的删掉、并进类规则）
                    if let Some(floor) = self.reader.line_height_floor.filter(|_| !top) {
                        let cur = self.ch.els[i]
                            .decls
                            .get("line-height")
                            .and_then(|v| v.trim().parse::<f64>().ok());
                        if cur.is_some_and(|v| v < floor) {
                            out.inline
                                .entry(i)
                                .or_default()
                                .push(format!("line-height:{}", fmt(floor)));
                        }
                    }
                }
            }
        }
    }

    // 单元格缺省不居中的阅读器（Move 顶端对齐，Q105、Q106）：Kindle 只看单元格自己的 `vertical-align`（top/text-top 顶、bottom/text-bottom
    // 底、别的和没写的居中，同 KFX 写出器），一行两格以上、Kindle 上居中而自己没写 middle 的写进行内（行内的压过书里 `tr`、类规则的）
    fn cell_middle(&self, out: &mut Plan) {
        if !self.reader.cell_middle {
            for i in 0..self.ch.els.len() {
                if !matches!(self.tag(i).as_str(), "td" | "th") || self.ch.els[i].hidden {
                    continue;
                }
                let v = self.comp(i).valign.as_deref().unwrap_or("");
                if matches!(v, "middle" | "top" | "text-top" | "bottom" | "text-bottom") {
                    continue;
                }
                let row = self.ch.el(i).parent().and_then(ElementRef::wrap);
                if row.is_none_or(|r| {
                    r.children()
                        .filter_map(ElementRef::wrap)
                        .filter(|c| matches!(c.value().name(), "td" | "th"))
                        .count()
                        < 2
                }) {
                    continue;
                }
                const MIDDLE: &str = "vertical-align:middle;";
                match out.cells.iter_mut().find(|(j, _)| *j == i) {
                    Some((_, add)) => add.push_str(MIDDLE),
                    None => out.cells.push((i, MIDDLE.to_string())),
                }
            }
        }
    }

    // 单元格的宽度照 Kindle：Send to Kindle 把它写成表格列宽的百分比（写了的宽度 ÷（写了的 + 没写的单元格的自然宽度），
    // [`km::column_percents`]），Kindle 上字照样横排（Q127–Q129 真机）。按宽度压窄格子的阅读器（掌阅：CSS、属性都认，`width:auto`
    // 不认）写成行内 `width:P%`、属性删掉；只认 `width` 属性的（Move）写成属性 `P%`
    fn cell_widths(&self, out: &mut Plan) {
        for i in 0..self.ch.els.len() {
            if self.tag(i) != "tr" || self.ch.els[i].hidden {
                continue;
            }
            let cells: Vec<usize> = self
                .ch
                .el(i)
                .children()
                .filter_map(|c| self.ch.index_of(c.id()))
                .filter(|&j| matches!(self.tag(j).as_str(), "td" | "th"))
                .collect();
            let width_of = |j: usize| {
                self.comp(j).width.or_else(|| {
                    self.ch
                        .el(j)
                        .value()
                        .attr("width")
                        .and_then(|w| match w.trim().parse::<f64>() {
                            Ok(n) => Some(Len::Pt(n * 0.75)),
                            Err(_) => crate::cascade::parse_len(w),
                        })
                })
            };
            let widths: Vec<Option<Len>> = cells.iter().map(|&j| width_of(j)).collect();
            if widths.iter().all(Option::is_none) {
                continue;
            }
            let naturals: Vec<f64> = cells
                .iter()
                .map(|&j| km::natural_px(&self.ch.el(j), self.comp(j).font_size))
                .collect();
            // 每一格各算（列宽只看第一行的是写出器的写法；这里每行一样的书居多）
            let total: f64 = widths
                .iter()
                .zip(&naturals)
                .map(|(w, n)| match w {
                    Some(Len::Em(e)) => e * 16.0,
                    Some(Len::Pt(p)) => p / 0.75,
                    _ => *n,
                })
                .sum();
            for (k, &j) in cells.iter().enumerate() {
                let pct = match widths[k] {
                    Some(Len::Percent(p)) => p,
                    Some(Len::Em(e)) if total > 0.0 => e * 16.0 / total * 100.0,
                    Some(Len::Pt(p)) if total > 0.0 => p / 0.75 / total * 100.0,
                    _ => continue,
                };
                let v = format!("{}%", fmt((pct * 100.0).round() / 100.0));
                if self.reader.cell_width {
                    out.cell_widths.push((j, None));
                    let add = format!("width:{v};");
                    match out.cells.iter_mut().find(|(x, _)| *x == j) {
                        Some((_, a)) => a.push_str(&add),
                        None => out.cells.push((j, add)),
                    }
                } else {
                    out.cell_widths.push((j, Some(v)));
                    if !out.cells.iter().any(|(x, _)| *x == j) {
                        out.cells.push((j, String::new()));
                    }
                }
            }
        }
    }

    // 包进表格的块：单元格里块的外边距不认，同标题一样把前后的间距写到框外相邻的块上（框里的内边距写在单元格上，不算）
    // 竖直：同一层相邻两块之间的距离
    fn vertical_gaps(&self, out: &mut Plan, framed: &[bool], drawn: &[[bool; 2]]) {
        let body = self.ch.body();
        let mut parents: Vec<ElementRef> = vec![body];
        parents.extend(
            (0..self.ch.els.len())
                .filter(|&i| self.ch.els[i].is_block && !self.ch.block_children(i).is_empty())
                .map(|i| self.ch.el(i)),
        );
        for parent in parents {
            // 这一层是块的序列（直接的文字只有空白，元素都是块；我们垫的图不算）才看
            let ok = parent.children().all(|c| match c.value() {
                Node::Text(t) => t.trim().is_empty(),
                Node::Element(v) => {
                    v.classes().any(|k| k == "eink-sp")
                        || self
                            .ch
                            .index_of(c.id())
                            .is_some_and(|j| self.ch.els[j].is_block)
                }
                _ => true,
            });
            if !ok {
                continue;
            }
            // 我们垫的页首空白图（`top_spacer`）不算块
            let kids: Vec<usize> = parent
                .children()
                .filter_map(|c| self.ch.index_of(c.id()))
                .filter(|&j| self.ch.els[j].is_block && !self.ch.els[j].hidden && !self.ours(j))
                .collect();
            for w in kids.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (skipped(&self.tag(a)) && !framed[a])
                    || (skipped(&self.tag(b)) && !framed[b])
                {
                    continue;
                }
                // 带框的块同标题：间距写到另一块上
                let (ha, hb) = (heading(&self.tag(a)) || framed[a], heading(&self.tag(b)) || framed[b]);
                // 两块都是标题、有空块的不动；后一块是标题而文件里它前面还没有内容的（页首）由垫图管
                if (ha && hb)
                    || !self.has_content(a)
                    || !self.has_content(b)
                    || (hb && !self.content_before[b])
                {
                    continue;
                }
                // 折叠链上有负外边距时 `margin_edge` 给不出；不认负外边距的阅读器改用这两块自己的（子块的不算）
                let own = |i: usize, side: usize| {
                    (!self.reader.negative_margins)
                        .then(|| km::vertical_rem(self.comp(i).margin[side], self.comp(i).font_size))
                };
                let (Some(a_mb), Some(b_mt)) = (
                    self.ch.margin_edge(a, false).or_else(|| own(a, 2)),
                    self.ch.margin_edge(b, true).or_else(|| own(b, 0)),
                ) else {
                    continue;
                };
                let a_pb = if self.reader.padding || drawn[a][1] || framed[a] {
                    0.0
                } else {
                    self.pad_rem(a, 2)
                };
                let b_pt = if self.reader.padding || drawn[b][0] || framed[b] {
                    0.0
                } else {
                    self.pad_rem(b, 0)
                };
                // 有子块又有内边距的：子块的外边距和它折不折叠说不准，不动
                if (a_pb > 0.0 && !self.ch.block_children(a).is_empty())
                    || (b_pt > 0.0 && !self.ch.block_children(b).is_empty())
                {
                    continue;
                }
                let gap = km::collapse(a_mb, b_mt) + a_pb + b_pt;
                let heading_involved = (!self.reader.heading_margins && (ha || hb))
                    || framed[a]
                    || framed[b];
                // 不认负外边距的阅读器（Move）：负的那条当 0，间距照 Kindle 折叠后的写成非负的（叠在一起的写 0）
                let neg = !self.reader.negative_margins && (a_mb < 0.0 || b_mt < 0.0);
                if a_pb + b_pt < 1e-6 && !heading_involved && !neg {
                    continue;
                }
                let gap = if neg { gap.max(0.0) } else { gap };
                if neg && !ha && !hb {
                    // 两边都写：只写一边的话另一边正的外边距和它折叠，取大的
                    out.decls
                        .entry(a)
                        .or_default()
                        .push(format!("margin-bottom:{}em", fmt(gap / self.comp(a).font_size)));
                    out.decls
                        .entry(b)
                        .or_default()
                        .push(format!("margin-top:{}em", fmt(gap / self.comp(b).font_size)));
                } else if !hb {
                    if (gap - b_mt).abs() > 1e-6 || ha {
                        out.decls
                            .entry(b)
                            .or_default()
                            .push(format!("margin-top:{}em", fmt(gap / self.comp(b).font_size)));
                    }
                } else if (gap - a_mb).abs() > 1e-6 || hb {
                    out.decls
                        .entry(a)
                        .or_default()
                        .push(format!("margin-bottom:{}em", fmt(gap / self.comp(a).font_size)));
                }
            }
        }
    }

    // 改了外边距、行内又写了外边距（或内边距）的：行内的删掉，没改的那几边照现在的值并进类规则
    fn strip_inline(&self, out: &mut Plan) {
        let sides = [
            ("margin-top", 0),
            ("margin-right", 1),
            ("margin-bottom", 2),
            ("margin-left", 3),
        ];
        for (&i, ds) in out.decls.iter_mut() {
            let inline = |pre: &str| {
                self.ch.els[i]
                    .origin
                    .iter()
                    .any(|(k, (o, _))| *o == crate::cascade::Origin::Inline && k.starts_with(pre))
            };
            let mut pre = Vec::new();
            if inline("margin") {
                pre.push("margin");
            }
            if (!self.reader.padding || ds.iter().any(|d| d.starts_with("padding-")))
                && inline("padding")
            {
                pre.push("padding");
            }
            if ds.iter().any(|d| d.starts_with("text-indent:")) && inline("text-indent") {
                pre.push("text-indent");
            }
            if pre.is_empty() {
                continue;
            }
            if pre.contains(&"margin") {
                let c = self.comp(i);
                for (name, side) in sides {
                    if ds.iter().any(|d| d.starts_with(&format!("{name}:"))) || c.margin_auto[side] {
                        continue;
                    }
                    let v = if side % 2 == 0 {
                        km::vertical_rem(c.margin[side], c.font_size) / c.font_size
                    } else {
                        self.to_em(c.margin[side], c.font_size)
                    };
                    ds.push(format!("{name}:{}em", fmt(v)));
                }
            }
            // 认内边距的阅读器上删了行内的内边距：没改的那几边照现在的值并进类规则
            if self.reader.padding && pre.contains(&"padding") {
                let c = self.comp(i);
                for (name, side) in [
                    ("padding-top", 0),
                    ("padding-right", 1),
                    ("padding-bottom", 2),
                    ("padding-left", 3),
                ] {
                    if ds.iter().any(|d| d.starts_with(&format!("{name}:"))) {
                        continue;
                    }
                    let v = if side % 2 == 0 {
                        km::vertical_rem(c.padding[side], c.font_size) / c.font_size
                    } else {
                        self.to_em(c.padding[side], c.font_size)
                    };
                    ds.push(format!("{name}:{}em", fmt(v)));
                }
            }
            out.strip.entry(i).or_default().extend(pre);
        }
    }

    // em 按根字号算的阅读器（Move）：上面写的 em 都是本元素的；`root_em` 换算别的元素，标题不管（见那里），标题上的这里换成根 em
    // （《梁羽生》卷名 3 倍字号，按本元素写的左外边距在 Move 上只挪三分之一，竖条没挤出来，2026-10-14 真机）
    fn em_root_headings(&self, out: &mut Plan) {
        if self.reader.em_root {
            for (&i, ds) in out.decls.iter_mut() {
                let fs = self.comp(i).font_size;
                if !heading(&self.tag(i)) || (fs - 1.0).abs() < 1e-6 {
                    continue;
                }
                for d in ds.iter_mut() {
                    if let Some((name, v)) = d.split_once(':') {
                        if let Some(n) = v.strip_suffix("em").and_then(|n| n.parse::<f64>().ok()) {
                            *d = format!("{name}:{}em", fmt(n * fs));
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn plan(
    e: &Entry,
    media: Option<&MediaEnv>,
    css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>,
    reader: &ReaderModel,
    g: BoxGeometry,
) -> Plan {
    let mut out = Plan::default();
    let Ok(text) = std::str::from_utf8(&e.data) else {
        return out;
    };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else {
        return out;
    };
    // 文档序里每个元素前面有没有内容（字、图）：前面没有的块不插上边框（页首的位置由垫图管）
    let mut content_before = vec![false; ch.els.len()];
    {
        let mut seen = false;
        for d in ch.body().descendants() {
            match d.value() {
                Node::Text(t) if !t.trim().is_empty() => seen = true,
                Node::Element(v) => {
                    if let Some(j) = ch.index_of(d.id()) {
                        content_before[j] = seen;
                    }
                    if matches!(v.name(), "img" | "svg" | "image")
                        && !v.classes().any(|c| c == "eink-sp")
                    {
                        seen = true;
                    }
                }
                _ => {}
            }
        }
    }
    let cx = Cx {
        ch: &ch,
        reader,
        g,
        content_before,
    };
    // 不认块宽度的阅读器（Move）：定了宽度的块照 Kindle 折成左右外边距（本元素的 em），多出来的放右边（Kindle 不按 auto 居中）
    let mut width_margins: HashMap<usize, [f64; 2]> = HashMap::new();
    // 窄的定宽块（占能用宽的 [`NARROW_SHARE`] 以内）：不折成 em 外边距（跟着字号变），包进单格表格、表格宽写百分比
    // （元素 → (表格宽百分比, 靠哪边, 块自己的左右外边距 em)）
    let mut narrow: HashMap<usize, (f64, &'static str, [f64; 2])> = HashMap::new();
    cx.width_to_margins(&mut width_margins, &mut narrow);
    cx.margin_auto_center(&mut out);
    cx.page_top_break(&mut out);
    cx.float_off(&mut out);
    cx.negative_top_margin(&mut out);
    // 左右有框的块（不画边框的阅读器）：包进单格表格（见 [`Cx::frame_side_borders`]）
    let mut framed = vec![false; ch.els.len()];
    cx.frame_side_borders(&mut out, &width_margins, &narrow, &mut framed);
    // 水平：左右内边距并进外边距；px、% 写的左右外边距折成 em（见 [`Cx::merge_h_padding`]）
    let mut h_margin: HashMap<usize, [f64; 2]> = HashMap::new();
    cx.merge_h_padding(&mut out, &framed, &width_margins, &mut h_margin);
    cx.cell_px_margins(&mut out);
    cx.list_item_padding(&mut out);
    // 横线图：看得见的上、下边框（见 [`Cx::rule_images`]）
    let mut drawn = vec![[false; 2]; ch.els.len()];
    cx.rule_images(&mut out, &framed, &h_margin, &mut drawn);
    cx.cell_middle(&mut out);
    cx.cell_widths(&mut out);
    cx.vertical_gaps(&mut out, &framed, &drawn);
    cx.strip_inline(&mut out);
    cx.em_root_headings(&mut out);
    out
}

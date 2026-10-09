//! 样式：块和行内区间的 KFX 样式属性（字体、字号、颜色与对比度、边距、边框、阴影、宽度），照 Send to Kindle 的规则（docs/kfx.md#send-to-kindle-的样式规则）。

use super::*;

/// 写一个节点的样式时 KFX 父节点的情况（Send to Kindle 只写和父节点不同的字重、颜色；水平长度按包含块宽度写成百分比）。
#[derive(Clone, Copy, Debug)]
pub(super) struct Parent {
    /// 实际显示的字重（`WEIGHT_*`）。
    pub(super) weight: u32,
    /// 实际显示的文字颜色（写进样式的；`None`＝阅读器缺省）。
    pub(super) color: Option<u32>,
    /// 包含块宽度（根 em，整页 [`PAGE_WIDTH_EM`]）。
    pub(super) avail: f64,
    /// 全书正文字号（根 em，见 [`base_font_size`]）：字号写成它的倍数。
    pub(super) base_fs: f64,
}

impl Parent {
    pub(super) fn root(base_fs: f64) -> Parent {
        Parent { weight: WEIGHT_NORMAL, color: None, avail: PAGE_WIDTH_EM, base_fs }
    }
}

/// 阴影：`{颜色, x, y, 模糊}`，长度 pt 或 em（Send to Kindle：`box-shadow:0 0 0.5em #aaa` → `$501: 0.5em`，`text-shadow` 的 3px → 1.35pt）。
fn shadow_value(s: &crate::css::Shadow, text: Option<u32>) -> Value {
    let len = |l: Len| match l {
        Len::Em(n) if n != 0.0 => num(n, U_EM),
        Len::Em(_) => num(0.0, U_PT),
        Len::Pt(n) => num(n, U_PT),
        Len::Percent(n) => num(n, U_PERCENT),
    };
    let col = s.color.or(text).unwrap_or(0xFF00_0000);
    Value::Struct(vec![(SHADOW_COLOR, Value::Int(i64::from(col))), (SHADOW_X, len(s.x)), (SHADOW_Y, len(s.y)), (SHADOW_BLUR, len(s.blur))])
}

/// 宽度：百分比照写，em/pt 换成 em。
pub(super) fn width_value(l: Len, fs: f64) -> Option<Value> {
    match l {
        Len::Percent(p) => Some(num(p, U_PERCENT)),
        Len::Em(n) if n > 0.0 => Some(num(n, U_EM)),
        Len::Pt(n) if n > 0.0 => Some(num(n / 12.0 / fs, U_EM)),
        _ => None,
    }
}

/// 边框、圆角（块和行内区间共用）。四边样式、宽度、颜色都一样时写「全部」那一组，否则按边写（上、左、下、右）。
pub(super) fn border_props(c: &Computed) -> Vec<(u32, Value)> {
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
        // 黑色不写（Send to Kindle 同样；文字也是黑色或缺省时边框本来就画成黑色）
        let black_text = c.color.is_none_or(|t| t & 0xFF_FFFF == 0);
        if let Some(col) = b.color.filter(|&col| !(col == 0xFF00_0000 && black_text)) {
            // 全透明的写「透明」（Send to Kindle 同样写 `$349`）
            p.push((P_BORDER_COLOR[slot], if col >> 24 == 0 { Value::Symbol(COLOR_TRANSPARENT) } else { Value::Int(i64::from(col)) }));
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

fn align_symbol(a: &str) -> u32 {
    match a {
        "center" => ALIGN_CENTER,
        "right" | "end" => ALIGN_RIGHT,
        "justify" => ALIGN_JUSTIFY,
        _ => ALIGN_LEFT,
    }
}

/// 显示的字重：`<b>`/`<strong>` 的 bolder、600 半粗、粗体、正常。
pub(super) fn weight_of(c: &Computed) -> u32 {
    if c.bolder {
        WEIGHT_BOLDER
    } else if c.semibold {
        WEIGHT_SEMIBOLD
    } else if c.bold {
        WEIGHT_BOLD
    } else {
        WEIGHT_NORMAL
    }
}

/// 近黑：三个分量都不超过 0x11（样本里见过的是纯黑和 `#111111`）。
fn near_black(col: u32) -> bool {
    col >> 24 == 0xFF && (col >> 16 & 0xFF) <= 0x11 && (col >> 8 & 0xFF) <= 0x11 && (col & 0xFF) <= 0x11
}

/// 这个节点实际显示的文字颜色（写进样式的，`None`＝阅读器缺省）。Send to Kindle 在没有背景的地方省掉近黑的文字颜色
/// （《绍宋》正文的黑、章号的 `#111111` 都不写；同样的颜色在有背景色的小注框、卷首语里照写，2026-10-08），
/// 父节点写了别的颜色时照写，免得继承父节点的颜色。
/// `col` 是 [`text_color`] 算好的（别再算一遍）。
pub(super) fn shown_color(col: Option<u32>, c: &Computed, parent: &Parent) -> Option<u32> {
    match col {
        Some(col) if near_black(col) && !c.on_background && c.background.is_none() && c.bg_image.is_none() && parent.color.is_none() => None,
        Some(col) => Some(col),
        None => parent.color,
    }
}

/// 按对比度调过的文字颜色（[`crate::css::ensure_contrast`]，背景是自己或祖先的背景色，没有按白页面）；没写颜色、缺省的黑字
/// 和背景对比度不够时也给一个颜色（深蓝底上的黑字写 `#e4e4e4`）。
/// 结果按颜色对缓存在 `b` 里（全书反复是那几对颜色，调整要逐级试）。
pub(super) fn text_color(b: &mut Builder, c: &Computed) -> Option<u32> {
    // 底下只有背景图、没有背景色：看不出对比度，照写
    if c.backdrop.is_none() && (c.on_image || c.bg_image.is_some()) {
        return c.color;
    }
    let bg = c.backdrop.unwrap_or(0xFFFF_FFFF);
    match c.color {
        Some(col) => Some(*b.contrast_fix.entry((col, bg)).or_insert_with(|| crate::css::ensure_contrast(col, bg))),
        None => *b
            .contrast_default
            .entry(bg)
            .or_insert_with(|| (crate::css::contrast(0xFF00_0000, bg) < 4.5).then(|| crate::css::ensure_contrast(0xFF00_0000, bg))),
    }
}

/// 容器给子节点的包含块宽度（根 em）：减去自己的左右外边距、内边距、边框（百分比按外面的包含块算）。
/// 表格按表格宽度算，不减边框（Send to Kindle：《绍宋》表格 `width:100%` 带 0.5px 边框，单元格的 0.2em 内边距写 0.375%＝0.12/32）。
pub(super) fn inner_width(b: &Block, avail: f64) -> f64 {
    let rem = |h: Horiz| h.em + h.pct / 100.0 * avail;
    if b.ty == Some(NODE_TABLE) {
        let w = match b.comp.width {
            Some(Len::Percent(p)) => p / 100.0 * avail,
            Some(l) => rem_len(l, b.comp.font_size).unwrap_or_default(),
            None => avail - rem(b.margin_left) - rem(b.margin_right),
        };
        return if w.is_finite() && w > 1.0 { w } else { avail };
    }
    let border = |s: usize| match b.comp.border[s].as_ref().map(|x| x.width) {
        Some(BorderWidth::Pt(n)) => n / 12.0,
        Some(BorderWidth::Em(n)) => n * b.comp.font_size,
        None => 0.0,
    };
    let w = avail - rem(b.margin_left) - rem(b.margin_right) - rem(b.padding_h[0]) - rem(b.padding_h[1]) - border(1) - border(3);
    if w.is_finite() && w > 1.0 { w } else { avail }
}

/// 字体、字号、粗体、斜体、颜色（块和行内区间共用）。字重、颜色只在和父节点显示的不一样时写（Send to Kindle 不写 normal 字重、
/// 不写没有背景处的近黑颜色，见 [`shown_color`]）。
pub(super) fn text_props(b: &mut Builder, c: &Computed, parent: &Parent) -> Vec<(u32, Value)> {
    let col = text_color(b, c);
    text_props_with(c, parent, col)
}

/// 同 [`text_props`]，文字颜色 `col`（[`text_color`]）由调用方算好。
fn text_props_with(c: &Computed, parent: &Parent, col: Option<u32>) -> Vec<(u32, Value)> {
    let mut p = Vec::new();
    if let Some(f) = &c.font_family {
        // 整串备选照写（Send to Kindle 同样，小写、逗号连接）；第一个在 `Builder::style` 里换成 `default` 或嵌入字体名
        let v = match &c.font_fallbacks {
            Some(rest) => format!("{f},{rest}"),
            None => f.clone(),
        };
        p.push((P_FONT_FAMILY, Value::String(v)));
    }
    // 字号是全书正文字号的倍数，相对根而不是父节点（Send to Kindle：《疯探》0.8 的容器里 1.2em 的作者行写 0.96＝1.2/1.25）
    let rel = if parent.base_fs > 0.0 { c.font_size / parent.base_fs } else { c.font_size };
    p.push((P_FONT_SIZE, num(rel, U_FONT_EM)));
    let w = weight_of(c);
    if w != WEIGHT_NORMAL || parent.weight != WEIGHT_NORMAL || c.weight_declared {
        p.push((P_FONT_WEIGHT, Value::Symbol(w)));
    }
    if c.italic {
        p.push((P_FONT_STYLE, Value::Symbol(STYLE_ITALIC)));
    }
    if let (Some(col), Some(_)) = (col, shown_color(col, c, parent)) {
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

impl Builder {
    /// 块的样式属性。`parent`：KFX 里的父节点（字号写成相对它的倍数，字重、颜色只在需要时写，水平长度按它的宽度换算）。
    /// `col`：本块的文字颜色（[`text_color`]，调用方算好）。
    pub(super) fn block_props(&mut self, b: &Block, parent: &Parent, doc_lang: &Option<String>, col: Option<u32>) -> Vec<(u32, Value)> {
        let c = &b.comp;
        let fs = c.font_size;
        if b.bare {
            return c.text_align.as_deref().map(|a| vec![(P_TEXT_ALIGN, Value::Symbol(align_symbol(a)))]).unwrap_or_default();
        }
        let mut p = text_props_with(c, parent, col);
        // 标题自己的样式带「标题」提示（Send to Kindle 同样）
        if b.heading.is_some() {
            p.push((P_LAYOUT_HINTS, Value::List(vec![Value::Symbol(HINT_HEADING)])));
        }
        if let Some(l) = c.lang.as_ref().or(doc_lang.as_ref()) {
            p.push((P_LANG, Value::String(l.to_ascii_lowercase())));
        }
        // 容器不写对齐（Send to Kindle 同样，对齐写在里面的文字上）
        if let (Some(a), false) = (c.text_align.as_deref(), matches!(b.kind, Kind::Container(_))) {
            p.push((P_TEXT_ALIGN, Value::Symbol(align_symbol(a))));
        }
        // 首行缩进：包含块是整页宽时照写 em，比整页窄（在有左右边距、内边距、边框的容器里）时写成包含块宽度的百分比
        // （Send to Kindle：正文 `text-indent:2em` 写 2em；《绍宋》信件框里的 2em 写 6.809%＝2/29.375，小注框里 6.78%，2026-10-08）
        let avail = parent.avail;
        // 自己或祖先（含 body，body 的左右边距本身不写）有左右边距、内边距、边框时写百分比（《人生海海》body `margin:0 5pt`、
        // 《揭露人性》`p.kindle-cn-ref{margin:0 1em 0 2em}` 里的 2em 都写 6.25%；没有的照写 em）
        let narrowed = avail < PAGE_WIDTH_EM - 1e-6 || c.in_hbox;
        let indent_rem = match c.text_indent {
            Some(Len::Percent(n)) => {
                p.push((P_TEXT_INDENT, num(n, U_PERCENT)));
                None
            }
            l => l.and_then(|l| rem_len(l, fs)),
        };
        if let Some(r) = indent_rem {
            p.push((P_TEXT_INDENT, if narrowed { num(r / avail * 100.0, U_PERCENT) } else { num(r / fs, U_EM) }));
        }
        // 行高按全书正文的行高归一（Send to Kindle 的做法，2026-10-08 对照《绍宋》：正文 `line-height:1.5em` 写成 1.0，
        // 行高 1em 的标题写成 0.667，没写行高的容器 0.8）：正文用的就是阅读器的行距设置，别的按比例。以前一律除以 1.2，
        // 正文行距比 Send to Kindle 的大 25%。
        // 下限 0.6（Send to Kindle 同样：《揭露人性》body `line-height:1.618em` 继承到 2.5 倍字号的卷号上只剩 0.4，写 0.6；
        // 《疯探》《克莱因壶》《占星术》的小字、目录同样落在 0.6）
        let lh = (c.line_height.unwrap_or(LH_EM) / self.base_lh).max(0.6);
        p.push((P_LINE_HEIGHT, num(lh, U_LH)));
        if c.nowrap {
            p.push((P_NOWRAP, Value::Bool(true)));
        }
        if c.break_all {
            p.push((P_WORD_BREAK, Value::Symbol(WORD_BREAK_ALL)));
        }
        match c.min_height.filter(|_| !matches!(b.kind, Kind::Image { .. })) {
            Some(Len::Em(n)) => p.push((P_MIN_HEIGHT, num(n, U_EM))),
            Some(Len::Pt(n)) => p.push((P_MIN_HEIGHT, num(n / 12.0 / fs, U_EM))),
            _ => {}
        }
        // `lh` 是本元素行高的倍数（Send to Kindle：没写行高的容器行高 0.8，同样 7% 的上边距写成 2.333 而不是 1.867；
        // 0.5em 的内边距在行高 0.667 的标题上写成 0.625）。以前按固定 1.2em 换算，行高不是 1 的块边距都偏了。
        let vert = |v: Vert| num(v / fs / (LH_EM * lh), U_LH);
        if b.margin_top != 0.0 {
            p.push((P_MARGIN_TOP, vert(b.margin_top)));
        }
        if b.margin_bottom != 0.0 {
            p.push((P_MARGIN_BOTTOM, vert(b.margin_bottom)));
        }
        // 左右外边距、内边距一律写成包含块宽度的百分比（Send to Kindle 同样：body 的 5pt → 1.302%、`list-style:none` 的 1.5em → 4.688%、
        // 《绍宋》`p.ganyan1` 的 0.5em → 1.484%、表格单元格的 0.2em → 0.375%；包含块按整页 32em 减去外层容器的左右边距、内边距、边框）
        let horiz = |h: Horiz| num(if h.em == 0.0 { h.pct } else { (h.em + h.pct / 100.0 * avail) / avail * 100.0 }, U_PERCENT);
        if b.margin_left != Horiz::default() {
            p.push((P_MARGIN_LEFT, horiz(b.margin_left)));
        }
        if b.margin_right != Horiz::default() {
            p.push((P_MARGIN_RIGHT, horiz(b.margin_right)));
        }
        for (i, k) in [P_PADDING_TOP, P_PADDING_RIGHT, P_PADDING_BOTTOM, P_PADDING_LEFT].into_iter().enumerate() {
            if i % 2 == 0 {
                if b.padding[i] != 0.0 {
                    p.push((k, vert(b.padding[i])));
                }
            } else if b.padding_h[i / 2] != Horiz::default() {
                p.push((k, horiz(b.padding_h[i / 2])));
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
        } else if b.ty.is_none() && !matches!(b.kind, Kind::Image { .. }) {
            // 有宽度的块（Send to Kindle 同样：宽度照写、带 `$546: $377`，em 宽度另加最大宽度 100%，左右外边距 auto 的按它对齐：
            // 《阿加莎》`h2{width:3em;margin:2.5em auto 1.8em -2em}` 靠左的小色块、《金庸》60% 宽的图注框）
            if let Some(v) = c.width.and_then(|w| width_value(w, fs)) {
                p.push((P_WIDTH, v));
                p.push((P_SIZING, Value::Symbol(SIZING_VALUE)));
                if !matches!(c.width, Some(Len::Percent(_))) {
                    p.push((P_MAX_WIDTH, num(100.0, U_PERCENT)));
                }
                let align = match (c.margin_auto[3], c.margin_auto[1]) {
                    (true, true) => Some(ALIGN_CENTER),
                    (false, true) => Some(ALIGN_LEFT),
                    (true, false) => Some(ALIGN_RIGHT),
                    _ => None,
                };
                if let Some(a) = align {
                    p.push((P_BOX_ALIGN, Value::Symbol(a)));
                }
            }
        }
        for (s, k) in [(c.box_shadow, P_BOX_SHADOW), (c.text_shadow, P_TEXT_SHADOW)] {
            if let Some(s) = s {
                p.push((k, shadow_value(&s, c.color)));
            }
        }
        p.extend(b.extra.iter().cloned());
        p
    }
}

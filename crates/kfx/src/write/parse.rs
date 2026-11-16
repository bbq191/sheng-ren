//! 解析：XHTML 文档按 CSS 层叠成块（[`Block`]）序列——行内内容收成文字和样式区间，块级元素展开、摊平或写成容器，列表、表格，
//! 外边距折叠、空段折叠；全书文档多线程解析（[`parse_docs`]），元素套得太深的书报错（[`MAX_NESTING`]）。

use super::*;

mod blocks;
mod docs;
mod element;
mod inline;

// 子模块按原 `parse.rs` 各段的职责拆开；`write` 通过 `use parse::*;` 拿到的项仍在这里再导出，
// 可见性沿用拆分前的 `pub(super)`（相对 `parse` 即 `write`）。
pub(super) use self::blocks::{
    apply_gaps, block_depth, collapse_siblings, has_negative, inline_box, scale_percent,
    split_at_breaks, starts_with_break, ParsedDoc, BREAK_MARK,
};
pub(super) use self::docs::{
    book_language, maybe_big_stack, parse_docs, prepend_cover_page, DEEP_NESTING,
};
pub(super) use self::element::{attach_trailing, prepend_ids};
pub(super) use self::inline::{Doc, Inline};

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

/// 行内图片在文字里占的那个字符（写出时文字在这里切开、插进图片节点，见 [`InlineImage`]）。
pub(super) const OBJ: char = '\u{FFFC}';

/// 夹在文字里的图片（图标注释号、文中小图标）：文字里占一个字符（[`OBJ`]），写成文字节点内容里的图片节点，
/// 不再切开文字另起一块（Send to Kindle 同样，2026-10-12 测试书；以前一律独立成块，《飘》图标注释号缩到一字高以后自己占一行）。
/// 整段只有图片、没有字的照旧独立成块（[`Doc::flush`]）。
#[derive(Clone, Debug)]
pub(super) struct InlineImage {
    /// 在文字里的字符偏移。
    pub(super) at: usize,
    pub(super) src: String,
    pub(super) alt: String,
    pub(super) comp: Computed,
    /// 图片在带 `href` 的 `<a>` 里（同 [`Kind::Image`]）。
    pub(super) linked: bool,
    /// CSS 的宽、高（没写的用 HTML 的 `width`/`height` 属性）：px 按 0.45pt（Send to Kindle 的口径：20px 写成 0.75em、`width="16"` 写成 0.6em）。
    pub(super) width: Option<Len>,
    pub(super) height: Option<Len>,
}

#[derive(Clone, Debug)]
pub(super) enum Kind {
    Text {
        text: String,
        runs: Vec<Run>,
        images: Vec<InlineImage>,
    },
    /// `linked`：图在带 `href` 的 `<a>` 里（图标注释号之类；只给 `--styles` 输出用，见 `analyze::dump_blocks`）。
    Image {
        src: String,
        linked: bool,
    },
    Container(Vec<Block>),
}

/// 折掉的空段（[`BLANK_LINE_FOLD`]）：高度和每个空段自己的上下外边距（根 em）。Send to Kindle 把空段排成一个高 0.6 行的空块，
/// 自己的外边距照留、照常和前后折叠（2026-10-14 重推的样本：《三体》`<p><br/></p>` 上下 1em、行高 1.618，下一段的上边距
/// 1 + 0.97 + 1 ≈ 2.94；《古龙》两个 `p.calibre10{margin:1em 0}` 4.47。以前丢了外边距，只加高度，差约 1 行）。
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Gap {
    /// 空段的高度合计。
    pub(super) h: Vert,
    /// 每个空段的 (上外边距, 下外边距)，按出现顺序。
    pub(super) margins: Vec<(Vert, Vert)>,
}

impl Gap {
    /// 前面再接一段（`before` 在本段之前）。
    fn prepend(&mut self, before: Gap) {
        let mut g = before;
        g.h += self.h;
        g.margins.append(&mut self.margins);
        *self = g;
    }

    /// `prev_bottom`（前一块的下外边距）到 `top`（本块的上外边距）之间夹着这些空段的外边距逐对折叠的总距离；没有空段的 `None`。
    pub(super) fn chain(&self, prev_bottom: Vert, top: Vert) -> Option<Vert> {
        let (first, rest) = self.margins.split_first()?;
        let mut total = collapse(prev_bottom, first.0);
        let mut bottom = first.1;
        for &(t, b) in rest {
            total += collapse(bottom, t);
            bottom = b;
        }
        Some(total + collapse(bottom, top))
    }
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
    gap_before: Gap,
}

/// 深度优先（先序）走遍块树，含容器里的块（容器本身先于里面的块）；`f` 返回 `ControlFlow::Break` 时停下。
/// 各处遍历整棵块树都用它（[`for_each_block`]、[`walk_blocks_mut`]、[`any_block`]），不再各写一份递归。
/// 递归的：套得深的书在大栈线程里调（见 [`DEEP_NESTING`]）。
pub(super) fn walk_blocks<'a>(
    blocks: &'a [Block],
    f: &mut impl FnMut(&'a Block) -> ControlFlow<()>,
) -> ControlFlow<()> {
    for b in blocks {
        f(b)?;
        if let Kind::Container(c) = &b.kind {
            walk_blocks(c, f)?;
        }
    }
    ControlFlow::Continue(())
}

/// 同 [`walk_blocks`]，走完不停。
pub(super) fn for_each_block<'a>(blocks: &'a [Block], mut f: impl FnMut(&'a Block)) {
    let _ = walk_blocks(blocks, &mut |b| {
        f(b);
        ControlFlow::Continue(())
    });
}

/// 同 [`for_each_block`]，可以改块（先改容器本身，再走它里面的块）。
pub(super) fn walk_blocks_mut(blocks: &mut [Block], f: &mut impl FnMut(&mut Block)) {
    for b in blocks {
        f(b);
        if let Kind::Container(c) = &mut b.kind {
            walk_blocks_mut(c, f);
        }
    }
}

/// 块树里（含容器里的）有没有满足 `pred` 的块。
pub(super) fn any_block(blocks: &[Block], mut pred: impl FnMut(&Block) -> bool) -> bool {
    walk_blocks(blocks, &mut |b| {
        if pred(b) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .is_break()
}

/// 只有空白的段落不出节点，折成下一块上边距（系数见 [`BLANK_LINE_FOLD`]，和掌阅、Move 的 `blank_fold` 共用）。以前整段丢掉、
/// 不折，场景空行在 Kindle 上没了。
use bookconv::kindle_model::BLANK_LINE_FOLD;

/// 绝对长度换成根 em：em 乘本元素字号 `fs`（根 em），pt 按 1em＝12pt（[`PT_PER_EM`]）；百分比要看包含块，`None`。
pub(super) fn rem_len(l: Len, fs: f64) -> Option<f64> {
    match l {
        Len::Em(n) => Some(n * fs),
        Len::Pt(n) => Some(n / PT_PER_EM),
        Len::Percent(_) => None,
    }
}

/// 竖直长度：百分比按页宽 [`PAGE_WIDTH_EM`]（同掌阅、Move，`bookconv::kindle_model::vertical_rem`）。
fn to_vert(l: Option<Len>, fs: f64) -> Vert {
    bookconv::kindle_model::vertical_rem(l, fs)
}

/// 水平长度：百分比照留百分比。
fn to_horiz(l: Option<Len>, fs: f64) -> Horiz {
    match l {
        Some(Len::Percent(p)) => Horiz { em: 0.0, pct: p },
        Some(l) => Horiz {
            em: rem_len(l, fs).unwrap_or_default(),
            pct: 0.0,
        },
        None => Horiz::default(),
    }
}

/// `xml:lang`（没有就 `lang`）。
fn lang_attr<'a>(el: &ElementRef<'a>) -> Option<&'a str> {
    el.value()
        .attr("xml:lang")
        .or_else(|| el.value().attr("lang"))
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

/// 元素最多套几层：超过的书报错不转换（真书十几层；几百上千层的是没关的标签一路套下去，解析、生成版面都是递归的，
/// 再深就要栈溢出、整个进程中止——书库在进程内调写出器，会带走整个 sync）。
pub(in crate::write) const MAX_NESTING: usize = 1000;

//! 块树的整形：`Block` 的构造与遍历辅助、分页记号、外边距与空段折叠。

use super::*;

impl Block {
    pub(in crate::write) fn anonymous(
        kind: Kind,
        comp: Computed,
        ids: Vec<(String, usize)>,
    ) -> Block {
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
            gap_before: Gap::default(),
        }
    }

    /// 前面攒下的锚点 `lead` 放到块开头、折掉的空段高度 `gap` 加到上边距（[`Doc::block`] 里本元素整个成一块时）。
    pub(super) fn led_by(mut self, lead: Vec<String>, gap: Gap) -> Block {
        prepend_ids(&mut self.ids, lead);
        self.gap_before.prepend(gap);
        self
    }

    pub(super) fn is_anonymous(&self) -> bool {
        self.margin_top == 0.0
            && self.margin_bottom == 0.0
            && self.heading.is_none()
            && self.margin_left == Horiz::default()
            && self.ty.is_none()
    }
}

/// 行内元素有左右外边距或内边距（Send to Kindle 写在行内区间上：2026-10-14 怪癖测试书 Q88–Q92，`margin-left:2em` 写 2em、`30px` 写
/// 13.5pt；以前丢了）。
pub(in crate::write) fn inline_box(c: &Computed) -> bool {
    [c.margin[1], c.margin[3], c.padding[1], c.padding[3]]
        .iter()
        .any(|l| l.is_some_and(|l| !l.is_zero()))
}
/// 解析好的一个文档：(spine 下标, 路径, 语言, 块)。
pub(in crate::write) type ParsedDoc<'a> = (usize, &'a str, Option<String>, Vec<Block>);

/// 强制分页的记号：当成锚点 id 落在分页后第一个块的开头（真的 id 里不会有 NUL）。
pub(in crate::write) const BREAK_MARK: &str = "\0break";

/// 块（或它最前面的子孙）开头有分页记号。
pub(in crate::write) fn starts_with_break(b: &Block) -> bool {
    b.ids.iter().any(|(i, o)| i == BREAK_MARK && *o == 0)
        || matches!(&b.kind, Kind::Container(c) if c.first().is_some_and(starts_with_break))
}

/// 去掉块树里的分页记号。
fn strip_breaks(blocks: &mut [Block]) {
    for b in blocks {
        b.ids.retain(|(i, _)| i != BREAK_MARK);
        if let Kind::Container(c) = &mut b.kind {
            strip_breaks(c);
        }
    }
}

/// 一个文件的块在强制分页处切成几节（Send to Kindle：`page-break-before` 处另起一节，2026-10-12《飘》版权页后的二维码页）。
/// 切在顶层；顶层只有一个容器（带背景的 body）时切它的子块，每节一份容器（背景照带，上外边距留在第一节、下外边距留在最后一节，
/// 同 Amazon）。文件开头的分页不用切。
pub(in crate::write) fn split_at_breaks(mut blocks: Vec<Block>) -> Vec<Vec<Block>> {
    let cut = |v: &[Block]| -> Vec<usize> {
        (1..v.len()).filter(|&i| starts_with_break(&v[i])).collect()
    };
    let mut parts: Vec<Vec<Block>> = Vec::new();
    let top = cut(&blocks);
    if !top.is_empty() {
        let mut rest = std::mem::take(&mut blocks);
        for &i in top.iter().rev() {
            parts.push(rest.split_off(i));
        }
        parts.push(rest);
        parts.reverse();
    } else if let [only] = blocks.as_mut_slice() {
        let inner = match &only.kind {
            Kind::Container(c) => cut(c),
            _ => Vec::new(),
        };
        if !inner.is_empty() {
            let shell = only.clone();
            let Kind::Container(mut rest) =
                std::mem::replace(&mut only.kind, Kind::Container(Vec::new()))
            else {
                unreachable!()
            };
            let mut chunks = Vec::new();
            for &i in inner.iter().rev() {
                chunks.push(rest.split_off(i));
            }
            chunks.push(rest);
            chunks.reverse();
            let n = chunks.len();
            for (k, c) in chunks.into_iter().enumerate() {
                let mut b = shell.clone();
                b.kind = Kind::Container(c);
                if k > 0 {
                    b.margin_top = Vert::default();
                    b.ids.clear();
                    b.gap_before = Gap::default();
                }
                if k + 1 < n {
                    b.margin_bottom = Vert::default();
                }
                parts.push(vec![b]);
            }
        }
    }
    if parts.is_empty() {
        parts.push(blocks);
    }
    for p in &mut parts {
        strip_breaks(p);
    }
    parts
}

/// 有没有块（含容器里的）的左（`left`）或右外边距是负的。
pub(super) fn has_negative(blocks: &[Block], left: bool) -> bool {
    any_block(blocks, |b| {
        let h = if left { b.margin_left } else { b.margin_right };
        h.em + h.pct / 100.0 * PAGE_WIDTH_EM < -1e-9
    })
}

/// 顶层块的左右百分比长度（书里相对 body 内容宽度）折算成相对整页：乘 `s`。
pub(super) fn scale_percent(blocks: &mut [Block], s: f64) {
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
pub(in crate::write) fn apply_gaps(blocks: &mut [Block]) {
    walk_blocks_mut(blocks, &mut |b| {
        b.margin_top += b.gap_before.h;
        b.gap_before = Gap::default();
    });
}

pub(in crate::write) fn collapse_siblings(blocks: &mut [Block]) {
    for i in 0..blocks.len() {
        let prev_bottom = if i > 0 {
            blocks[i - 1].margin_bottom
        } else {
            0.0
        };
        // 夹着空段的：外边距连同空段的逐对折叠（[`Gap::chain`]），合成的放在后一块的上边距
        if let Some(total) = blocks[i]
            .gap_before
            .chain(prev_bottom, blocks[i].margin_top)
        {
            blocks[i].margin_top = total;
            blocks[i].gap_before.margins.clear();
            if i > 0 {
                blocks[i - 1].margin_bottom = 0.0;
            }
            continue;
        }
        if i == 0 || blocks[i].margin_top == 0.0 {
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
/// 块的最大嵌套层数（不递归）。
pub(in crate::write) fn block_depth(blocks: &[Block]) -> usize {
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

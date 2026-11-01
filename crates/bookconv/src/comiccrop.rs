//! 漫画页裁边的两个加强（2026-10-30 维护者定，只 Kindle 开）：
//!
//! - **裁白边不让杂点挡住**（profile `comic_trim_specks`）：原来的裁白边（[`crate::imgopt::trim_page`]）一行里有一个不白的点就停，
//!   扫描灰尘挡住了不少页（《哆啦A夢》卷01 每页挡住的多是边上几个到二十来个暗点）。这里在裁过的图里找连通块，小块、离主体远、
//!   附近也没有别的小块的当杂点不算内容，按剩下的内容再收一次边。网点印的浅灰（一片小点，每点各自一块）不是杂点：小块成堆的算内容
//!   （第一版没分，裁掉了《北斗之拳》卷01 扉页的灰色泼墨、《哆啦A夢》卷01 版权页下面的淡色哆啦A梦）。比 [`DARK_BELOW`] 暗才算内容，
//!   淡线也算：KCC 把比 176 浅的都当背景，会裁进淡色的画。
//! - **裁掉页码**（profile `comic_page_numbers`，推翻 2026-10-06「不删原书任何内容」里的这一项）：照 KCC 在内容最底下找
//!   像页码的小块，再按 agy 的意见**整卷投票**——印的页码在一卷里位置固定（内容的左下、右下或正中），同一位置出现够多次才认，
//!   掉在页底的字、装饰只出现一两次，不裁（KCC 逐页判会删掉《死亡筆記》卷07 竖排的「月」字、切掉《火影》卷01 标题页的「◆」）。
//!   候选块上方还要留出一段空白。
//!
//! 坐标都在解码后的整页上（[`crate::imgopt::decode_page`] 摆正过 EXIF 方向），比例都按整页宽高算。

/// 比这个值暗（任一分量）才算内容：和裁白边一致——纸白（255）的页上，和首像素差超过 8 的就不算留白。淡线、浅网点都算内容
/// （设成 240 时《北斗之拳》卷01 扉页泼墨的浅色部分被当成留白裁掉）；纸发灰的页整页连成一块，等于不多裁。页码检测也用它。
const DARK_BELOW: u8 = 247;
/// 杂点：外框边长不超过短边的这个比例（2480×1687 的页约 16 像素；0.005 时《迷宮飯》卷01 页顶十几像素长的划痕挡住了裁白边）……
const SPECK_SIDE: f32 = 0.01;
/// ……而且离主体内容的外框超过短边的这个比例（约 25 像素）。挨着主体的小点可能是画的一部分，不动。
const SPECK_NEAR: f32 = 0.015;
/// 小块成堆算内容（网点印的浅灰、点画）：彼此不超过 [`SPECK_NEAR`] 的小块并成一堆，够这么多块……
const SPECK_CLUSTER: usize = 5;
/// ……或者一堆的外框边长超过杂点边长的这么多倍，就算内容。
const SPECK_CLUSTER_SPAN: u32 = 3;
/// 单边最多裁掉原图的这个比例（同裁白边的上限）：超过就不按杂点收边。
const MAX_TRIM: f32 = 0.35;

/// 在内容最底下这么高（占页高）里找最低的那块（不算太小的）：页码下面还有灰尘、小点时内容的底边在页码下面（《迷宮飯》卷01 好几页），
/// 像 KCC 只看最底下 2.5% 会找不到。
const PN_SEARCH: f32 = 0.06;
/// 相近的块合并：水平、竖直距离不超过页宽、页高的这个比例（同 KCC，一个数字的几笔、几位数字并成一块）。
const PN_MERGE: (f32, f32) = (0.01, 0.002);
/// 比这个小的块不算（同 KCC）。
const PN_MIN: (f32, f32) = (0.003, 0.006);
/// 页码块最大（同 KCC：三位数）。
const PN_MAX: (f32, f32) = (0.045, 0.02);
/// 页码块上方要空出这么高（占页高）、左右各扩三个块宽的范围里没有内容（agy 的意见：竖排字末尾的字上面紧挨着上一个字）。
const PN_GAP: f32 = 0.004;
/// 整卷投票按页码在内容里的位置分：中心在内容左边这个比例以内算左、右边以内算右……
const PN_SLOT_SIDE: f32 = 0.2;
/// ……离正中不超过内容宽的这个比例算中；别的位置不当候选。按内容算不按整页：Kmoe 每页扫描裁的宽不一样（《迷宮飯》卷01 1488–1743），
/// 页码占整页宽的比例散成一片（左下角 2.4%–7.9%），按整页比例聚会把同一个角拆成好几个位置、都凑不够票。
const PN_SLOT_CENTER: f32 = 0.1;
/// 一个位置至少这么多页、且不少于总页数的 [`PN_SLOT_SHARE`] 才认作这一卷的页码位置。
const PN_SLOT_MIN: usize = 5;
const PN_SLOT_SHARE: f32 = 0.02;

/// 一行里连续的暗像素 `[x0, x1)`。
#[derive(Clone, Copy)]
struct Run {
    y: u32,
    x0: u32,
    x1: u32,
}

/// 连通块（8 连通）的外框（含两端）和像素数。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Blob {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    area: u64,
}

impl Blob {
    fn w(&self) -> u32 {
        self.x1 - self.x0 + 1
    }
    fn h(&self) -> u32 {
        self.y1 - self.y0 + 1
    }
    fn union(&self, o: &Blob) -> Blob {
        Blob { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1), area: self.area + o.area }
    }
    /// 两个外框之间水平、竖直方向的空隙（重叠为 0）。
    fn gap(&self, o: &Blob) -> (u32, u32) {
        let dx = o.x0.saturating_sub(self.x1).max(self.x0.saturating_sub(o.x1));
        let dy = o.y0.saturating_sub(self.y1).max(self.y0.saturating_sub(o.y1));
        (dx, dy)
    }
}

/// 灰度图 `(w, h, 像素)` 里 `rows` 行的连通块（8 连通）。按行收暗像素段、和上一行相邻的段并成一块（并查集），内存只和段数有关。
fn blobs(px: &[u8], w: u32, rows: std::ops::Range<u32>) -> Vec<Blob> {
    let mut runs: Vec<Run> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut prev = 0..0;
    for y in rows {
        let row = &px[(y * w) as usize..((y + 1) * w) as usize];
        let start = runs.len();
        let mut x = 0;
        while x < w {
            if row[x as usize] < DARK_BELOW {
                let x0 = x;
                while x < w && row[x as usize] < DARK_BELOW {
                    x += 1;
                }
                runs.push(Run { y, x0, x1: x });
                parent.push(parent.len());
            } else {
                x += 1;
            }
        }
        // 和上一行的段比：x 区间放宽 1 有交叠（斜着挨着也算）就并起来；两边都按 x 排好了，双指针走一遍
        let mut j = prev.start;
        for i in start..runs.len() {
            let r = runs[i];
            while j < prev.end && runs[j].x1 < r.x0 {
                j += 1;
            }
            let mut k = j;
            while k < prev.end && runs[k].x0 <= r.x1 {
                let (a, b) = (find(&mut parent, i), find(&mut parent, k));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
                k += 1;
            }
        }
        prev = start..runs.len();
    }
    // 按根汇总（根的下标就是它所在块里最小的段，按段的下标存，不用哈希表）
    let mut acc: Vec<Option<Blob>> = vec![None; runs.len()];
    for (i, r) in runs.iter().enumerate() {
        let b = Blob { x0: r.x0, y0: r.y, x1: r.x1 - 1, y1: r.y, area: u64::from(r.x1 - r.x0) };
        let root = find(&mut parent, i);
        acc[root] = Some(acc[root].map_or(b, |o| o.union(&b)));
    }
    let mut v: Vec<Blob> = acc.into_iter().flatten().collect();
    v.sort_by_key(|b| (b.y0, b.x0));
    v
}

/// 杂点之外再收一次边（`comic_trim_specks`）：`px` 是已经裁过白边的灰度图（`w × h`），`full` 是整页原来的宽高（定比例、上限用），
/// `off` 是它在整页里的左上角。返回新的 `(左, 上, 宽, 高)`（相对 `px`），没得收 → `None`。
pub(crate) fn speck_bounds(px: &[u8], w: u32, h: u32, full: (u32, u32), off: (u32, u32)) -> Option<(u32, u32, u32, u32)> {
    let short = full.0.min(full.1) as f32;
    let side = ((short * SPECK_SIDE) as u32).max(2);
    let near = (short * SPECK_NEAR) as u32;
    let all = blobs(px, w, 0..h);
    let is_speck = |b: &Blob| b.w() <= side && b.h() <= side;
    let mut core = all.iter().filter(|b| !is_speck(b)).copied().reduce(|a, b| a.union(&b));
    // 小块按距离并成堆：成堆的（网点、点画）算内容，剩下零星的才是杂点。主体外框里面深过 `near` 的小块（格子里的网点，一页几万个）
    // 动不了边界，不参与
    let inner = core.filter(|c| c.w() > 2 * near && c.h() > 2 * near).map(|c| (c.x0 + near, c.y0 + near, c.x1 - near, c.y1 - near));
    let specks: Vec<Blob> = all.iter().filter(|b| is_speck(b) && !inner.is_some_and(|(x0, y0, x1, y1)| b.x0 > x0 && b.y0 > y0 && b.x1 < x1 && b.y1 < y1)).copied().collect();
    let mut lone = Vec::new();
    for (bbox, members) in speck_clusters(&specks, near) {
        if members >= SPECK_CLUSTER || bbox.w().max(bbox.h()) > side * SPECK_CLUSTER_SPAN {
            core = Some(core.map_or(bbox, |c| c.union(&bbox)));
        } else {
            lone.push(bbox);
        }
    }
    let core = core?;
    let mut keep = core;
    for b in &lone {
        let (dx, dy) = core.gap(b);
        if dx.max(dy) <= near {
            keep = keep.union(b);
        }
    }
    if (keep.x0, keep.y0, keep.x1, keep.y1) == (0, 0, w - 1, h - 1) {
        return None;
    }
    // 连同之前裁掉的，单边不超过上限
    let cap = |len: u32| (len as f32 * MAX_TRIM) as u32;
    let (cw, ch) = (cap(full.0), cap(full.1));
    let right = full.0 - (off.0 + keep.x1 + 1);
    let bottom = full.1 - (off.1 + keep.y1 + 1);
    if off.0 + keep.x0 > cw || right > cw || off.1 + keep.y0 > ch || bottom > ch {
        return None;
    }
    Some((keep.x0, keep.y0, keep.w(), keep.h()))
}

/// 小块按距离并成堆（外框之间的空隙不超过 `near`）：返回每堆的外框和块数。按 `near` 大小的格子分桶，只和相邻格子里的比，
/// 网点页上几万个小点也不会两两比一遍。
fn speck_clusters(specks: &[Blob], near: u32) -> Vec<(Blob, usize)> {
    let cell = near.max(1);
    let mut grid: std::collections::HashMap<(u32, u32), Vec<usize>> = std::collections::HashMap::new();
    for (i, b) in specks.iter().enumerate() {
        grid.entry((b.x0 / cell, b.y0 / cell)).or_default().push(i);
    }
    let mut parent: Vec<usize> = (0..specks.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for (i, b) in specks.iter().enumerate() {
        let (cx, cy) = (b.x0 / cell, b.y0 / cell);
        for gx in cx.saturating_sub(1)..=cx + 1 {
            for gy in cy.saturating_sub(1)..=cy + 1 {
                for &j in grid.get(&(gx, gy)).map_or(&[][..], |v| v.as_slice()) {
                    let (dx, dy) = b.gap(&specks[j]);
                    if j != i && dx.max(dy) <= near {
                        let (a, c) = (find(&mut parent, i), find(&mut parent, j));
                        if a != c {
                            parent[a.max(c)] = a.min(c);
                        }
                    }
                }
            }
        }
    }
    let mut out: std::collections::HashMap<usize, (Blob, usize)> = std::collections::HashMap::new();
    for (i, b) in specks.iter().enumerate() {
        let r = find(&mut parent, i);
        out.entry(r).and_modify(|(o, n)| {
            *o = o.union(b);
            *n += 1;
        })
        .or_insert((*b, 1));
    }
    let mut v: Vec<(Blob, usize)> = out.into_values().collect();
    v.sort_by_key(|(b, _)| (b.y0, b.x0));
    v
}

/// 页码在内容里的位置（整卷投票按它分，[`PN_SLOT_SIDE`]、[`PN_SLOT_CENTER`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Left,
    Center,
    Right,
}

/// 一页上像页码的候选（[`page_number_candidate`]）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageNumber {
    /// 裁到这里（整页坐标）：页码块的上沿，从这一行往下都切掉。
    pub cut: u32,
    /// 页码在内容里的哪边。
    pub slot: Slot,
}

/// 在裁过边的内容（灰度 `px`，`w × h`，在整页里的左上角 `off`，整页 `full`）最底下找像页码的块（同 KCC 的检测，另加上方要空的条件）：
/// 内容最底下 [`PN_SEARCH`] 里相近的暗块并起来、太小的不算，最低那块所在的一排只有它一块、大小像 1–3 位数字、上方 [`PN_GAP`] 高、
/// 左右各扩三个块宽的范围里没有内容，且在内容的左、右或正中（[`Slot`]）。它下面只剩太小的点（灰尘）时一起切掉。
pub(crate) fn page_number_candidate(px: &[u8], w: u32, h: u32, off: (u32, u32), full: (u32, u32)) -> Option<PageNumber> {
    let (fw, fh) = (full.0 as f32, full.1 as f32);
    let search = ((fh * PN_SEARCH) as u32).max(2);
    if h <= search * 2 {
        return None;
    }
    let mut boxes = blobs(px, w, h - search..h);
    // 相近的并起来，并到不再变
    let (mx, my) = ((fw * PN_MERGE.0) as u32, (fh * PN_MERGE.1) as u32);
    loop {
        let mut merged = false;
        'outer: for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                let (dx, dy) = boxes[i].gap(&boxes[j]);
                if dx <= mx && dy <= my {
                    boxes[i] = boxes[i].union(&boxes[j]);
                    boxes.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    boxes.retain(|b| b.w() as f32 >= fw * PN_MIN.0 && b.h() as f32 >= fh * PN_MIN.1);
    let bottom = boxes.iter().map(|b| b.y1).max()?;
    let lowest_top = boxes.iter().filter(|b| b.y1 == bottom).map(|b| b.y0).min()?;
    // 和它在同一排（下沿到了它的上沿以下）的只能有它一块（同 KCC：画面一角、别的字都算）
    let same: Vec<&Blob> = boxes.iter().filter(|b| b.y1 >= lowest_top).collect();
    let [b] = same.as_slice() else { return None };
    if b.w() as f32 > fw * PN_MAX.0 || b.h() as f32 > (fh * PN_MAX.1).max(3.0) {
        return None;
    }
    // 块的上方要空出来：竖排字的末字、画面掉下来的一角，上面紧挨着别的内容
    let gap = ((fh * PN_GAP) as u32).max(1);
    if b.y0 < gap {
        return None;
    }
    let (x0, x1) = (b.x0.saturating_sub(3 * b.w()), (b.x1 + 3 * b.w()).min(w - 1));
    let clear = (b.y0 - gap..b.y0).all(|y| px[(y * w + x0) as usize..=(y * w + x1) as usize].iter().all(|&v| v >= DARK_BELOW));
    if !clear {
        return None;
    }
    let rel = (b.x0 + b.x1) as f32 / 2.0 / w as f32;
    let slot = if rel < PN_SLOT_SIDE {
        Slot::Left
    } else if rel > 1.0 - PN_SLOT_SIDE {
        Slot::Right
    } else if (rel - 0.5).abs() <= PN_SLOT_CENTER {
        Slot::Center
    } else {
        return None;
    };
    Some(PageNumber { cut: off.1 + b.y0, slot })
}

/// 整卷投票（`comic_page_numbers`）：`cands[i]` 是第 i 页的候选。按在内容里的位置（[`Slot`]）分，够多页（[`PN_SLOT_MIN`]、
/// [`PN_SLOT_SHARE`]）的位置才认作这一卷的页码，返回每页要裁到哪里（不裁的 `None`）。
pub fn page_number_cuts(cands: &[Option<PageNumber>]) -> Vec<Option<u32>> {
    let need = PN_SLOT_MIN.max((cands.len() as f32 * PN_SLOT_SHARE).ceil() as usize);
    let count = |s: Slot| cands.iter().flatten().filter(|c| c.slot == s).count();
    let ok: Vec<Slot> = [Slot::Left, Slot::Center, Slot::Right].into_iter().filter(|&s| count(s) >= need).collect();
    cands.iter().map(|c| c.filter(|c| ok.contains(&c.slot)).map(|c| c.cut)).collect()
}

/// 黑底页（`comic_dark_bg`）：四条边各取这么宽的一圈（短边的比例，至少 2px）判底色。1% 在 1500px 宽的扫描页上是 15px，
/// 扫描页最外面常有一两像素的杂边，取太窄会被它带偏；取太宽会吃进画面。
const DARK_BAND: f32 = 0.01;
/// 黑底页：比这亮的不算底色。扫描的黑是深灰（《死亡筆記》卷07 每页最黑的中位数 22），JPEG 噪声在 ±10 上下，64 给足余量，又远低于
/// 网点印的灰（常在 100 以上）。
const DARK_BG_MAX: u8 = 64;
/// 黑底页：**四条边每一条**都要有这么多（千分比）的像素不亮于 [`DARK_BG_MAX`]。按条算、不按整圈算：出血画常是一两条边暗、
/// 角上一块暗，整圈平均会混过去，每条都要求就不会。
const DARK_SIDE_PER_MILLE: usize = 985;
/// 黑底页：四条边的底色（中位数）互相差不超过这么多。真黑底四边是同一种墨，出血画暗边深浅不一。
const DARK_SIDE_SPREAD: u8 = 16;
/// 黑底页：底色（四条边合起来的中位数）不亮于这个。比 [`DARK_BG_MAX`] 严：要求的是「黑底」，不是「偏暗的灰底」。
const DARK_BG_MEDIAN_MAX: u8 = 48;
/// 判黑底页：`px` 是整页（已裁过白边）的亮度平面（每像素最亮的分量），四条边一圈都是均匀的黑 / 深灰时返回底色（中位数），否则
/// `None`。保守：四条边每条都得几乎全是暗的（[`DARK_SIDE_PER_MILLE`]）、四边底色接近（[`DARK_SIDE_SPREAD`]）、底色够黑
/// （[`DARK_BG_MEDIAN_MAX`]），且页里得有亮的内容（整页全黑的不算）。只有一两条边暗的出血画、局部暗角都判不上。
pub fn dark_background(px: &[u8], w: u32, h: u32) -> Option<u8> {
    let band = ((w.min(h) as f32 * DARK_BAND).ceil() as u32).max(2);
    if w <= band * 4 || h <= band * 4 {
        return None;
    }
    let at = |x: u32, y: u32| px[(y * w + x) as usize];
    let sides: [Vec<u8>; 4] = [
        (0..band).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| at(x, y)).collect(),
        (h - band..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| at(x, y)).collect(),
        (0..h).flat_map(|y| (0..band).map(move |x| (x, y))).map(|(x, y)| at(x, y)).collect(),
        (0..h).flat_map(|y| (w - band..w).map(move |x| (x, y))).map(|(x, y)| at(x, y)).collect(),
    ];
    let median = |v: &[u8]| {
        let mut s = v.to_vec();
        s.sort_unstable();
        s[s.len() / 2]
    };
    let mut meds = [0u8; 4];
    for (i, s) in sides.iter().enumerate() {
        let dark = s.iter().filter(|&&v| v <= DARK_BG_MAX).count();
        if dark * 1000 < s.len() * DARK_SIDE_PER_MILLE {
            return None;
        }
        meds[i] = median(s);
    }
    let (lo, hi) = (*meds.iter().min()?, *meds.iter().max()?);
    if hi - lo > DARK_SIDE_SPREAD {
        return None;
    }
    let bg = median(&sides.concat());
    if bg > DARK_BG_MEDIAN_MAX {
        return None;
    }
    // 页里要有明显亮的内容（画、字），全黑页不算
    let bright = px.iter().filter(|&&v| v > DARK_BG_MAX.saturating_mul(2)).count();
    (bright * 100 >= px.len()).then_some(bg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 白底灰度图，`marks` 里的矩形 `(x, y, w, h)` 涂黑。
    fn page(w: u32, h: u32, marks: &[(u32, u32, u32, u32)]) -> Vec<u8> {
        let mut px = vec![255u8; (w * h) as usize];
        for &(x, y, mw, mh) in marks {
            for yy in y..y + mh {
                for xx in x..x + mw {
                    px[(yy * w + xx) as usize] = 0;
                }
            }
        }
        px
    }

    #[test]
    fn blobs_join_diagonal_neighbours_only() {
        // 两块斜着挨着算一块，隔一个像素的不算
        let mut px = page(10, 4, &[(1, 1, 1, 1), (2, 2, 1, 1), (5, 1, 1, 1)]);
        px[(3 * 10 + 7) as usize] = 0;
        let b = blobs(&px, 10, 0..4);
        assert_eq!(b.len(), 3, "{b:?}");
        assert_eq!((b[0].x0, b[0].y0, b[0].x1, b[0].y1, b[0].area), (1, 1, 2, 2, 2));
    }

    #[test]
    fn specks_far_from_content_are_ignored() {
        // 1000×1500：主体 100..900 × 100..1400，左上角一粒 3×3 的灰，主体右边挨着一个小点（离 5 像素，算画的一部分）
        let px = page(1000, 1500, &[(100, 100, 800, 1300), (10, 10, 3, 3), (905, 700, 3, 3)]);
        let b = speck_bounds(&px, 1000, 1500, (1000, 1500), (0, 0)).unwrap();
        assert_eq!(b, (100, 100, 808, 1300));
        // 主体左边隔开一片网点（一堆小点，每点各自一块）：是内容，留着
        let dots: Vec<(u32, u32, u32, u32)> = (0..6).flat_map(|i| (0..6).map(move |j| (10 + i * 8, 600 + j * 8, 3, 3))).collect();
        let mut marks = vec![(100, 100, 800, 1300)];
        marks.extend(dots);
        let tone = page(1000, 1500, &marks);
        assert_eq!(speck_bounds(&tone, 1000, 1500, (1000, 1500), (0, 0)), Some((10, 100, 890, 1300)));
        // 没有杂点、内容顶满：不收
        let full = page(50, 60, &[(0, 0, 50, 60)]);
        assert_eq!(speck_bounds(&full, 50, 60, (50, 60), (0, 0)), None);
    }

    #[test]
    fn page_number_needs_a_lone_small_block_with_space_above() {
        // 1000×1500：画面 0..1000 × 0..1400，页码 40×25 在 (480, 1475)，上面空 75 像素
        let (w, h) = (1000, 1500);
        let px = page(w, h, &[(0, 0, 1000, 1400), (480, 1475, 40, 25)]);
        let c = page_number_candidate(&px, w, h, (0, 0), (w, h)).unwrap();
        assert_eq!(c, PageNumber { cut: 1475, slot: Slot::Center });
        // 页码下面还有几粒灰尘：照样认，切口在页码上沿
        let dusty = page(w, h, &[(0, 0, 1000, 1400), (40, 1440, 40, 25), (600, 1495, 3, 3), (900, 1497, 2, 2)]);
        assert_eq!(page_number_candidate(&dusty, w, h, (0, 0), (w, h)), Some(PageNumber { cut: 1440, slot: Slot::Left }));
        // 在内容的四分之一处（不在左、右、正中）：不当候选
        let odd = page(w, h, &[(0, 0, 1000, 1400), (300, 1475, 40, 25)]);
        assert_eq!(page_number_candidate(&odd, w, h, (0, 0), (w, h)), None);
        // 页码上方紧挨着内容（竖排字的末字）：不算
        let tight = page(w, h, &[(0, 0, 1000, 1400), (480, 1440, 10, 30), (480, 1475, 40, 25)]);
        assert_eq!(page_number_candidate(&tight, w, h, (0, 0), (w, h)), None);
        // 最底下一排有两块（画面的一角）：不算
        let two = page(w, h, &[(0, 0, 1000, 1400), (100, 1475, 40, 25), (800, 1475, 40, 25)]);
        assert_eq!(page_number_candidate(&two, w, h, (0, 0), (w, h)), None);
        // 太宽（不像页码）：不算
        let wide = page(w, h, &[(0, 0, 1000, 1400), (300, 1475, 200, 25)]);
        assert_eq!(page_number_candidate(&wide, w, h, (0, 0), (w, h)), None);
    }

    #[test]
    fn only_slots_seen_on_enough_pages_are_cut() {
        let pn = |slot: Slot, cut: u32| Some(PageNumber { cut, slot });
        // 100 页：左下角 6 页、右下角 6 页、正中偶然 1 页（掉下来的字）
        let mut c = vec![None; 100];
        for i in 0..6 {
            c[2 * i] = pn(Slot::Left, 1400 + i as u32);
            c[2 * i + 1] = pn(Slot::Right, 1410);
        }
        c[50] = pn(Slot::Center, 1300);
        let cuts = page_number_cuts(&c);
        assert_eq!(cuts.iter().filter(|x| x.is_some()).count(), 12);
        assert_eq!((cuts[0], cuts[1], cuts[50]), (Some(1400), Some(1410), None));
        // 位置出现不到 5 次：都不裁
        let few: Vec<_> = (0..4).map(|_| pn(Slot::Center, 1400)).collect();
        assert!(page_number_cuts(&few).iter().all(Option::is_none));
    }

    fn dark_page(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| f(x, y)).collect()
    }

    #[test]
    fn dark_background_needs_every_side_dark_and_bright_content() {
        let inner = |x: u32, y: u32| (100..700).contains(&x) && (100..900).contains(&y);
        let framed = dark_page(800, 1000, |x, y| if inner(x, y) { 220 } else { 22 + ((x * 7 + y * 3) % 9) as u8 });
        assert_eq!(dark_background(&framed, 800, 1000), Some(26), "四边均匀深灰（带噪声）");
        // 两条边暗（左、上），右、下是画
        let two = dark_page(800, 1000, |x, y| if x >= 100 && y >= 100 { 220 } else { 22 });
        assert_eq!(dark_background(&two, 800, 1000), None);
        // 全黑页、偏亮的灰底都不算
        assert_eq!(dark_background(&dark_page(800, 1000, |_, _| 10), 800, 1000), None);
        assert_eq!(dark_background(&dark_page(800, 1000, |x, y| if inner(x, y) { 220 } else { 60 }), 800, 1000), None);
        // 一条边上 3% 是亮的（出血进边的画）
        let bleed = dark_page(800, 1000, |x, y| if inner(x, y) || (y < 20 && x < 30) { 220 } else { 22 });
        assert_eq!(dark_background(&bleed, 800, 1000), None);
    }

}

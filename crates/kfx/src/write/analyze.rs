//! 全书分析（版面要用）：注释配对、正文字号与行高、写成 `default` 的正文字体；按 CSS 原样导出文字块样式（[`dump_blocks`]）。

use super::*;

/// 全书的文字块按深度优先编号（含容器里的），`f` 拿到 (文档下标, 编号, 块)。
fn each_text_block(docs: &mut [ParsedDoc], mut f: impl FnMut(usize, usize, &mut Block)) {
    let mut n = 0;
    for (di, d) in docs.iter_mut().enumerate() {
        walk_blocks_mut(&mut d.3, &mut |b| {
            if matches!(b.kind, Kind::Text { .. }) {
                f(di, n, b);
                n += 1;
            }
        });
    }
}

/// 注释配对（同 Amazon 的转换：原书多半只是普通链接，没有 `epub:type`）：正文链接指到后面的一个块，那个块**开头**的链接
/// 指回这个正文链接所在的位置（链接自己或包着它的元素的 id），正文链接就是注释引用（`$616: $617`，Kindle 点了弹窗），目标块是注释正文
/// （`$615: $618`）。优化器 `kindle` 模式保留回链（`note_backlinks`）、把注释搬到引用它的那一章（文件）末尾，正好配得上。
/// 返回配上了几条。
fn mark_notes(docs: &mut [ParsedDoc]) -> usize {
    // 一遍只读：每个块的 id、每个链接区间（借用，不复制路径和 id）
    struct Link<'a> {
        /// 所在块的编号、文档下标，区间下标、起点
        n: usize,
        di: usize,
        ri: usize,
        start: usize,
        target: &'a (String, String),
        /// 区间起点处（含区间内、起点前紧挨着的空格处）的 id
        src: Vec<&'a str>,
    }
    let (refs, notes) = {
        let mut id_block: HashMap<(&str, &str), usize> = HashMap::new();
        let mut links: Vec<Link> = Vec::new();
        let mut n = 0;
        for (di, d) in docs.iter().enumerate() {
            let path = d.1;
            for_each_block(&d.3, |b| {
                let Kind::Text { text, runs } = &b.kind else { return };
                for (id, _) in &b.ids {
                    id_block.entry((path, id.as_str())).or_insert(n);
                }
                let mut chars: Option<Vec<char>> = None;
                for (ri, r) in runs.iter().enumerate() {
                    if let Some(target) = &r.link {
                        // 区间起点前紧挨着的空格也算（`<sup><span id="r"></span> <a href>…`：回链指向 span，空格写出器 15 起不算进区间）
                        let chars = chars.get_or_insert_with(|| text.chars().collect());
                        let from = if r.start > 0 && chars.get(r.start - 1) == Some(&' ') { r.start - 1 } else { r.start };
                        let src = b.ids.iter().filter(|(_, o)| *o >= from && *o <= r.start + r.len).map(|(i, _)| i.as_str()).collect();
                        links.push(Link { n, di, ri, start: r.start, target, src });
                    }
                }
                n += 1;
            });
        }
        // 每个块开头（第一个字起）的链接链回到哪里：注释正文的回链在段首（「[1]Queen of Sheba…」）
        let mut back: HashMap<usize, Vec<&(String, String)>> = HashMap::new();
        for l in links.iter().filter(|l| l.start == 0) {
            back.entry(l.n).or_default().push(l.target);
        }
        let mut refs: HashSet<(usize, usize)> = HashSet::new();
        let mut notes: HashSet<usize> = HashSet::new();
        for l in &links {
            // 注释在引用后面（优化器把注释搬到引用它的那一章（文件）末尾）；反过来那条是回链。
            let Some(&tb) = id_block.get(&(l.target.0.as_str(), l.target.1.as_str())) else { continue };
            if tb <= l.n {
                continue;
            }
            let src_path = docs[l.di].1;
            let points_back = back.get(&tb).is_some_and(|ts| ts.iter().any(|(p, f)| p == src_path && l.src.contains(&f.as_str())));
            if points_back {
                refs.insert((l.n, l.ri));
                notes.insert(tb);
            }
        }
        (refs, notes)
    };
    each_text_block(docs, |_, n, b| {
        if notes.contains(&n) {
            b.note = true;
        }
        if let Kind::Text { runs, .. } = &mut b.kind {
            for (ri, r) in runs.iter_mut().enumerate() {
                if refs.contains(&(n, ri)) {
                    r.note_ref = true;
                }
            }
        }
    });
    refs.len()
}

/// 全书文字块的字数统计（[`TextCounts::body_font`]、[`base_font_size`]、[`base_line_height`] 用），一遍走完。
pub(super) struct TextCounts {
    /// 字体名（原样，没写的 `None`）→ 字数（含空白）。
    fonts: HashMap<Option<String>, usize>,
    /// 字号（千分之一取整）→ 字数（不算空白）。
    font_size: HashMap<i64, usize>,
    /// 行高（同上）→ 字数（不算空白）。
    line_height: HashMap<i64, usize>,
}

impl TextCounts {
    pub(super) fn of(docs: &[ParsedDoc]) -> TextCounts {
        let mut t = TextCounts { fonts: HashMap::new(), font_size: HashMap::new(), line_height: HashMap::new() };
        for (_, _, _, blocks) in docs {
            for_each_block(blocks, |b| {
                let Kind::Text { text, .. } = &b.kind else { return };
                let (all, solid) = text.chars().fold((0, 0), |(a, s), c| (a + 1, s + usize::from(!c.is_whitespace())));
                // 同一个字体名的块多半挨着，按原样的名字计（不每块分配小写），取的时候再按小写合并
                match t.fonts.get_mut(&b.comp.font_family) {
                    Some(n) => *n += all,
                    None => {
                        t.fonts.insert(b.comp.font_family.clone(), all);
                    }
                }
                let key = |v: f64| (v * 1000.0).round() as i64;
                *t.font_size.entry(key(b.comp.font_size)).or_default() += solid;
                *t.line_height.entry(key(b.comp.line_height.unwrap_or(LH_EM))).or_default() += solid;
            });
        }
        t
    }

    /// 正文字体（字数最多的那个，小写），写成 `default`：没有嵌入时 Kindle 遇到不认识的字体名会换成别的字体，和没写字体的表格、
    /// 阅读器设置里选的字体都对不上（《啸风山庄》正文写着没嵌入的「AR MingU30 DemiBold」，2026-10-05 真机）。
    /// 正文本来就该用阅读器的字体（用户定的字体规矩，见 typesetting.md）。
    /// 写出器 11 起嵌入了的也一样（Send to Kindle：《绍宋》新版嵌入的 NotoSerifSCLight、《平凡的世界》的 FZLanTingSong 都写 `default`，2026-10-08）。
    pub(super) fn body_font(&self) -> Option<String> {
        let mut count: HashMap<Option<String>, usize> = HashMap::new();
        for (f, n) in &self.fonts {
            *count.entry(f.as_ref().map(|f| f.to_lowercase())).or_default() += n;
        }
        // 字数打平时取字体名小的（没写字体的排最前），结果和 HashMap 的遍历顺序无关（以前 `max_by_key` 打平时随遍历顺序，同一本书
        // 两次生成的 KFX 可能不一样，覆盖到 Kindle 上进度清零）。打破平局的口径同掌阅、Move 的 `kindle_rules`
        // （`book_facts`：那边按 DOM 文字数，这里按解析好的块数，计数的对象不同，只共用规则）。
        count.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))?.0
    }
}

/// 书里有字体文件的 `@font-face` 字体（小写）。
fn embedded_font_faces(book: &Loaded) -> HashSet<String> {
    let files: HashSet<&str> = book.fonts.iter().map(|(p, _)| p.as_str()).collect();
    book.css.iter().flat_map(|(p, t)| crate::css::font_faces(t, p)).filter(|f| files.contains(f.path.as_str())).map(|f| f.family.to_lowercase()).collect()
}

/// 全书正文的字号（根 em）：所有文字块按字数加权，最多的那个。Send to Kindle 把它当成阅读器字号设置的 1.0，别的字号按比例
/// （《啸风山庄》正文 `font-size:1.167em` 不写字号、1.083em 的写 0.928；《疯探》正文 1.25em，没写字号的容器写 0.8，2026-10-08）。
/// 算出来不在 0.5–3 之间的不信，用 1。
fn base_font_size(t: &TextCounts) -> f64 {
    crate::css::body_font_size(&t.font_size)
}

/// 全书正文的行高（元素字号的倍数）：所有文字块按字数加权，最多的那个行高；没写行高的按 normal（1.2）。
/// Send to Kindle 把它当成阅读器行距设置的 1.0（《绍宋》正文 `line-height:1.5em`）。算出来不在 1–3 之间的不信，用 1.2。
fn base_line_height(t: &TextCounts) -> f64 {
    crate::css::weighted_mode(&t.line_height).filter(|v| (1.0..=3.0).contains(v)).unwrap_or(LH_EM)
}

/// 一个文字块一行（见 [`epub_text_styles`]）。
/// 容器里只有本元素文字的块（[`Block::bare`]）用容器的边距（和 KFX 那边一样：边距在容器上）。
pub(super) fn dump_blocks(blocks: &[Block], out: &mut String) {
    dump_blocks_in(blocks, None, out)
}

fn dump_blocks_in(blocks: &[Block], parent: Option<&Block>, out: &mut String) {
    use std::fmt::Write;
    for b in blocks {
        match &b.kind {
            Kind::Container(c) => dump_blocks_in(c, Some(b), out),
            Kind::Text { text, .. } => {
                let c = &b.comp;
                let key: String = text.chars().filter(|ch| !ch.is_whitespace()).take(60).collect();
                if key.is_empty() {
                    continue;
                }
                let lh = c.line_height.unwrap_or(LH_EM) * c.font_size;
                let weight = if c.bolder { "bolder" } else if c.semibold { "semibold" } else if c.bold { "bold" } else { "normal" };
                let indent = match c.text_indent {
                    Some(Len::Em(n)) => format!("em:{n}"),
                    Some(Len::Pt(n)) => format!("pt:{n}"),
                    Some(Len::Percent(n)) => format!("%:{n}"),
                    None => "-".into(),
                };
                let m = match parent {
                    Some(p) if b.bare => p,
                    _ => b,
                };
                let _ = writeln!(
                    out,
                    "{key}\t{}\t{lh}\t{}\t{weight}\t{}\t{}\t{}\t{indent}\t{}\t{}\t{}+{}\t{}+{}",
                    c.font_size,
                    c.font_family.as_deref().unwrap_or("-"),
                    c.italic,
                    c.color.map_or("-".into(), |x| format!("{x:08x}")),
                    c.text_align.as_deref().unwrap_or("-"),
                    m.margin_top,
                    m.margin_bottom,
                    m.margin_left.em,
                    m.margin_left.pct,
                    m.margin_right.em,
                    m.margin_right.pct,
                );
            }
            Kind::Image { .. } => {}
        }
    }
}

/// 全书统计（[`analyze`] 算出来，之后各阶段只读）。
pub(super) struct BookStats {
    /// 写成 `default` 的字体名（小写）：正文字体（见 [`TextCounts::body_font`]）。
    pub(super) default_fonts: HashSet<String>,
    /// 书里嵌入了字体文件的字体名（小写）。
    pub(super) embedded_fonts: HashSet<String>,
    /// 全书正文的行高（元素字号的倍数，见 [`base_line_height`]）：KFX 的行高按它归一。
    pub(super) base_lh: f64,
    /// 全书正文的字号（根 em，见 [`base_font_size`]）。
    pub(super) base_fs: f64,
}

/// 全书范围的分析（版面要用）：注释配对（标在块上）、嵌入字体、写成 `default` 的正文字体、正文字号和行高。
pub(super) fn analyze(book: &Loaded, parsed: &mut [ParsedDoc]) -> BookStats {
    mark_notes(parsed);
    let counts = TextCounts::of(parsed);
    BookStats {
        // 缺字体文件的字体照写原名（Send to Kindle：《春雪》注释的 `ZY-KAITI` 照写；《绍宋》旧版的「宋体」写 `default` 是因为它是正文字体）
        embedded_fonts: embedded_font_faces(book),
        default_fonts: counts.body_font().into_iter().collect(),
        base_lh: base_line_height(&counts),
        base_fs: base_font_size(&counts),
    }
}

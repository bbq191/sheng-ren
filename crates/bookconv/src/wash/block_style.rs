//! 块的行距、标题字号照 Kindle（只修复、照 Send to Kindle 统一的文字书；2026-10-13 用户定）：Kindle 上的值按
//! [`crate::kindle_model`] 算，这台阅读器拿到原来的 CSS 排成多少按 [`ReaderModel`] 算，不一样的在行内 `style` 末尾接上这台认的写法
//! （行内的优先级最高）。一章只解析一遍，两样一起改。2026-10-13 由 `line_height.rs`、`heading_size.rs` 合成。
//!
//! **行距**（只有 `<p>` 不认 `line-height` 的阅读器：Move，profile `line_height_except_p`）。Kindle（KFX 写出器同 Send to Kindle，
//! `kfx::write::style` 的行高）：行高写成「本元素行高 ÷ 全书正文行高」（没写按 1.2，下限 0.6），正文就是阅读器的行距设置。
//! xochitl：`<p>` 外链、`<style>` 里的 `line-height` 不认（总是阅读器的行距），行内 `style` 写的认（同别的块，按缺省行距的倍数，
//! 2026-10-13 怪癖测试书 Q83：`line-height:2` 一行多 56px），别的块（标题、div、li、blockquote、td，行内 `style` 也算）按
//! 「阅读器缺省行距 × 倍数」排（2026-10-13 测试书「测试-行距」：body `line-height:1.618` 时 `<p>` 一行 56px、div 90px、2em 标题
//! 179px；`line-height:2`、`2em` 的 2em 标题 222px，`normal` 111px）。所以正文行距写了 1.618 的书，Move 上标题、div 的行距比 Kindle
//! 大六成（《三体》标题两行隔 179px，Kindle 上约 123）；单独写了别的行距的段落（诗、引文）Move 上是阅读器行距，书里行内写了的又
//! 没按正文归一。这里给自己排字的块（直接有字，或者字在行内元素里；`<p>` 也算，KINDLE_RULES 26）接 `line-height:倍数`：倍数照
//! Kindle 算，和这台上本来的（自己写的照写的、继承的照继承来的，`normal` 是 1；`<p>` 只算行内写的，没写是 1）差不到 0.01 的不写。
//! 全书正文行距和写出器一样（[`km::base_line_height`]）。
//!
//! **标题字号**（标题的 `font-size` 只认关键字的阅读器：Move，profile `heading_font_keywords` + `[heading_sizes]`）。xochitl 的
//! h1–h6 不认 em、px、pt、%、`font` 简写（类规则、行内、`!important`、标题里的 `<span>`/`<div>`/`<p>` 写的都不认），固定按标签排；
//! 只认几个关键字，而且是绝对大小（不随外层字号变；2026-10-13 测试书「测试-标题字号」1–4，量出来的大小在 profile 里）。Kindle 照
//! CSS 排（字号按正文归一）：《平凡的世界》章名写 2em 的 h2，Kindle 上 2em、Move 上 1.5em。这里给每个标题取它在 Kindle 上的字号
//! （标题里的字按所在元素的计算字号计字数，取最多的；`kindle_rules` 已经照正文归一过，根是阅读器的正文字号），挑比例最接近的
//! 关键字，和这台上本来的（写了认的关键字照写的，没写照标签的）不同时接 `font-size:关键字`。
//!
//! **行距（只认长度的阅读器）**：掌阅（profile `line_height_numbers = false`）不认不带单位的 `line-height`（`normal` 也算），整条当没写，
//! 写成长度的（em、%）照 CSS 排（2026-10-13 怪癖测试书 Q80–Q84：`line-height:2` 在 p、div、标题、行内都没变化，`3em` 一行 114px；
//! 缺省一行 58px，profile `line_px`）。所以正文行距写 1.84 的书，Kindle 上标题、小字的块按 1.2/1.84 收紧，掌阅上全是缺省行距；正文写
//! `1.8em`、`130%` 的书，Kindle 上正文是阅读器的行距设置，掌阅上照写的排。照 Kindle 改成 em（D＝缺省行距是 em 的几倍）：
//!
//! - 样式表、`<style>`、行内 `style` 里的 `line-height`：em、%，和倍数（÷ 全书正文行距，不低于 0.6）不是 1 的数、`normal`（1.2）改成
//!   「Kindle 的倍数 × D」em，写了这条的元素就和 Kindle 一样了。`<body>` 上（选择器、只用在 `<body>` 上的类、行内）倍数是 1 的数
//!   不改：掌阅当没写，就是缺省行距，而且和数一样随字号缩放（改成长度的话按绝对值往下继承，字号不一样的块全要补）；别的元素上的
//!   照改（祖先改成了长度时，掌阅上当没写就继承了祖先的）；px 这类不动。
//! - 剩下的是没自己写、从祖先继承的：长度按绝对值继承，字号和祖先不一样的块（套在写了行距的 div 里的大标题）和 Kindle 不一样；
//!   没写、祖先也没写的块是阅读器缺省行距，Kindle 按 1.2 ÷ 正文行距。逐个块（`<p>` 也算）算出掌阅上的一行和 Kindle 上的一行，
//!   差 3% 以上的接 `line-height:Nem`（同 `tools/kfx/unify.py` 的容差；以前 5%，《人生海海》小节标题 Kindle 1.05 行、掌阅 1 行没改）。
//!
//! 写成长度以后，阅读器里改行距设置这些块不跟着变（写了长度的书本来就不跟着变）。
//!
//! 同一个标签两样都要改时先接行距、再接字号。
use super::*;
use crate::cascade::MediaEnv;
use crate::kindle_model::{self as km, Chapter, LH_EM};
use crate::reader_model::{HeadingSizes, ReaderModel};
use std::collections::{BTreeMap, HashSet};

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>, reader: &ReaderModel) {
    let heads = reader.heading_sizes.as_ref();
    if !reader.line_height_except_p && reader.line_em.is_none() && heads.is_none() && !reader.heading_center {
        return;
    }
    let guard = DivGuard::of(entries);
    let (chapters, docs) = super::scan_chapters(entries, media, |e, css| scan(e, media, css, heads, &guard, reader.heading_center));
    let base = {
        let mut count: HashMap<i64, usize> = HashMap::new();
        for it in docs.iter().flat_map(|d| &d.items) {
            if it.chars > 0 {
                *count.entry(km::key(it.lh.unwrap_or(LH_EM))).or_default() += it.chars;
            }
        }
        km::base_line_height(&count)
    };
    if let Some(d) = reader.line_em {
        rep.kindle_rule_edits += scale_line_heights(entries, base, d, &body_only_classes(entries));
    }
    // 先按章定好要改什么（快，串行），再各章并行改写：各章互不相干，只有改动计数要加总（和顺序无关），产物和逐章串行做一样。
    // 以前整段串行，《古龙作品全集》1506 章光写回就占这一步的六成（2026-10-15）。
    let mut jobs: HashMap<usize, Job> = HashMap::new();
    for (&i, doc) in chapters.iter().zip(&docs) {
        // 第几个开始标签 → 要接的声明（先行距、后字号）
        let mut decls: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        if let Some(d) = reader.line_em {
            for (n, v) in length_line_heights(&doc.items, &doc.body, base, d) {
                decls.entry(n).or_default().push(format!("line-height:{}em", fmt(v)));
            }
        } else if reader.line_height_except_p {
            for (n, v) in line_heights(&doc.items, base) {
                decls.entry(n).or_default().push(format!("line-height:{}", fmt(v)));
            }
        }
        // 关键字是按这台的字号挑的：标题在 Kindle 上的字号记在 `data-eink-fs`，后面照 Kindle 算的 pass（垫图、内边距和间距）按它算
        // （[`crate::kindle_model::Chapter::parse`]；以前按关键字的 CSS 字号算，large 从 1.2 改成 Send to Kindle 的 1.125 后标题前后的间距跟着偏）
        let kindle_fs: HashMap<usize, f64> = doc.heading_sizes.iter().filter(|h| h.3.is_none()).map(|(n, _, fs, _)| (*n, *fs)).collect();
        // 改成 `div` 的标题：层叠出来的声明全写进行内（改了标签以后选 `hN` 的规则就选不到了，行内的和原来一样），打上记号；
        // 目录处理完以后 `heading_div::finish` 才改标签。行距接在后面（同一个 `style` 里后写的胜）
        let divs: HashSet<usize> = doc.heading_sizes.iter().filter(|h| h.3.is_some()).map(|h| h.0).collect();
        for (n, kw, _, div) in &doc.heading_sizes {
            let d = decls.entry(*n).or_default();
            match div {
                Some(own) => d.splice(0..0, own.iter().cloned()).for_each(drop),
                None => d.push(format!("font-size:{kw}")),
            }
        }
        for (n, d) in &doc.aligns {
            decls.entry(*n).or_default().push(d.clone());
        }
        if !decls.is_empty() {
            jobs.insert(i, Job { decls, kindle_fs, divs });
        }
    }
    let mut work: Vec<(&mut Entry, Job)> = entries.iter_mut().enumerate().filter_map(|(i, e)| jobs.remove(&i).map(|j| (e, j))).collect();
    rep.kindle_rule_edits += crate::util::par_map_mut(&mut work, |(e, job)| rewrite_tags(e, job)).into_iter().sum::<usize>();
}

/// 一章要接的声明（[`apply`]）：第几个开始标签 → 声明；标题在 Kindle 上的字号；要改成 `div` 的标题。
struct Job {
    decls: BTreeMap<usize, Vec<String>>,
    kindle_fs: HashMap<usize, f64>,
    divs: HashSet<usize>,
}

/// 照 `job` 改写一章的开始标签，返回改动数。
fn rewrite_tags(e: &mut Entry, job: &Job) -> usize {
    let Job { decls, kindle_fs, divs } = job;
    let mut edited = 0;
    let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
    let Some((bs, be)) = html::body_range(text) else { return 0 };
    let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
    let mut edits = Vec::new();
    for (&n, ds) in decls {
        let Some(t) = starts.get(n) else { continue };
        let mut tag = text[t.start..t.end].to_string();
        for d in ds {
            let style = match html::attr_value(&tag, "style") {
                Some(s) if !s.trim().is_empty() => format!("{};{d};", html::style_base(s)),
                _ => format!("{d};"),
            };
            tag = html::set_attr(&tag, "style", &style);
        }
        if let Some(fs) = kindle_fs.get(&n) {
            tag = html::set_attr(&tag, km::KINDLE_FS_ATTR, &fmt(*fs));
        }
        if divs.contains(&n) {
            tag = html::set_attr(&tag, km::DIV_ATTR, "1");
            edited += 1;
        }
        edited += ds.len();
        edits.push((t.start, t.end, tag));
    }
    e.data = html::apply_edits(text, edits).into_bytes();
    edited
}

fn fmt(v: f64) -> String {
    let s = format!("{:.4}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 一章扫出来的：元素表（行距用）、要改字号的标题。
#[derive(Default)]
struct Doc {
    items: Vec<Item>,
    /// `<body>` 的 (写的 `line-height`, 计算出的行高, 字号)：只认长度的阅读器上往下继承。
    body: (Option<String>, Option<f64>, f64),
    /// (第几个开始标签, 关键字, 标题在 Kindle 上的字号, 改成 `div` 时写进行内的声明)。
    heading_sizes: Vec<(usize, String, f64, Option<Vec<String>>)>,
    /// 标题自己没写对齐、这台缺省居中的：(第几个开始标签, Kindle 上继承来的对齐)。
    aligns: Vec<(usize, String)>,
}

/// `<body>` 里的一个元素（文档序，和标签扫描的开始标签一一对应）。
struct Item {
    /// 父元素在列表里的下标（父元素是 `<body>` 的 `None`）。
    parent: Option<usize>,
    /// 计算出的行高（元素字号的倍数，`None`＝没写/normal）。
    lh: Option<f64>,
    /// 自己（含行内 `style`）写了 `line-height`。
    own: bool,
    /// 自己写的 `line-height`（层叠出来的值）。
    own_decl: Option<String>,
    /// 计算出的字号（根 em）。
    fs: f64,
    /// 行距要管的块：块级、自己排字（`<p>` 也算）。
    target: bool,
    /// `<p>`：Move 上只认行内 `style` 写的行距，外链、`<style>` 里的不认（总是阅读器的行距）。
    p: bool,
    /// 生效的 `line-height` 写在行内 `style` 里。
    inline_lh: bool,
    /// 块级（`<p>` 也算）。
    is_block: bool,
    /// 自己排的字数（不算空白；行内元素里的算给所在的块），统计正文行距用。
    chars: usize,
}

/// 扫一章；元素数和标签扫描对不上（解析器补了标签）的整章不管，也不计字数。
fn scan(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>, heads: Option<&HeadingSizes>, guard: &DivGuard, heading_center: bool) -> Doc {
    let Ok(text) = std::str::from_utf8(&e.data) else { return Doc::default() };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else { return Doc::default() };
    let mut items: Vec<Item> = (0..ch.els.len())
        .map(|i| {
            let el = &ch.els[i];
            let decl = el.decls.get("line-height");
            Item {
                parent: el.parent,
                lh: el.comp.line_height,
                own: decl.is_some(),
                own_decl: decl.cloned(),
                fs: el.comp.font_size,
                target: el.is_block,
                p: ch.el(i).value().name() == "p",
                inline_lh: el.origin.get("line-height").is_some_and(|(o, _)| *o == crate::cascade::Origin::Inline),
                is_block: el.is_block,
                chars: 0,
            }
        })
        .collect();
    // 字算给所在的块；直接在 `<body>` 里的、隐藏的不算
    for (i, el) in ch.els.iter().enumerate() {
        if let (Some(b), false) = (el.block, el.hidden) {
            items[b].chars += ch.own_chars(i);
        }
    }
    let heading_sizes = heads.map(|h| heading_sizes(&ch, h, guard)).unwrap_or_default();
    // 标题缺省居中的阅读器（掌阅）：自己没写对齐的照 Kindle 继承来的写明（`inherit` 掌阅不认）
    let aligns = if heading_center {
        (0..ch.els.len())
            .filter(|&i| crate::html::heading_level_of(ch.el(i).value().name()).is_some() && !ch.els[i].hidden && !ch.els[i].decls.contains_key("text-align"))
            .filter_map(|i| {
                let v = match ch.els[i].comp.text_align.as_deref().unwrap_or("left") {
                    "start" => "left",
                    "end" => "right",
                    v => v,
                };
                (v != "center").then(|| (i, format!("text-align:{v}")))
            })
            .collect()
    } else {
        Vec::new()
    };
    let bc = &ch.body_comp;
    Doc { items, body: (ch.body_decls.get("line-height").cloned(), bc.line_height, bc.font_size), heading_sizes, aligns }
}

/// 要改行距的：(第几个开始标签, 行距倍数)。文档序走一遍，记下每个元素在这台上的行距（改过的往下继承改后的值）。
fn line_heights(items: &[Item], base: f64) -> Vec<(usize, f64)> {
    let mut on_reader: Vec<f64> = Vec::with_capacity(items.len());
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        // 没自己写的跟父元素：父元素改过就是改后的值，没改过就是原来算出的（数字倍数照继承、长度按绝对值继承，同 CSS）
        let inherited = it.parent.map(|p| (on_reader[p], items[p].lh));
        let mut cur = match inherited {
            // `<p>` 只认行内写的
            _ if it.p => if it.inline_lh { it.lh.unwrap_or(1.0) } else { 1.0 },
            Some((pm, plh)) if !it.own && (pm - plh.unwrap_or(1.0)).abs() > 1e-9 => pm,
            _ => it.lh.unwrap_or(1.0),
        };
        if it.target && it.chars > 0 {
            let want = km::shown_line_height_ratio(it.lh, base);
            let want = (want * 10000.0).round() / 10000.0;
            if (want - cur).abs() > 0.01 {
                out.push((i, want));
                cur = want;
            }
        }
        on_reader.push(cur);
    }
    out
}

/// 只认长度的阅读器上一条 `line-height` 怎么算。
#[derive(Clone, Copy, Debug, PartialEq)]
enum LhKind {
    /// 没写、`inherit` 这类，或者阅读器不认、改了也一样的（倍数和正文差不到 5% 的数、`normal`）：跟父元素。
    Inherit,
    /// [`scale_line_heights`] 改成照 Kindle 的 em：em、%，倍数不是正文的数、`normal`。
    Scaled,
    /// 别的长度（px、pt……）：照写的排。
    Fixed,
}

/// `body` 是不是写在 `<body>` 上（选择器、行内）：`<body>` 上倍数是 1 的数不改。
fn lh_kind(v: Option<&str>, base: f64, body: bool) -> LhKind {
    let Some(v) = v else { return LhKind::Inherit };
    let v = crate::cssunlock::split_important(v).0.to_ascii_lowercase();
    match scaled_factor(&v) {
        // `<body>` 上不带单位的：掌阅当没写，排成缺省行距（随字号缩放，同数的继承）；Kindle 的倍数也是 1 就不用改，改成长度反而按
        // 绝对值往下继承给每个块。别的元素上的照改：祖先改成了长度时，掌阅上当没写就继承了祖先的
        Some((f, false)) if body && (km::shown_line_height_ratio(Some(f), base) - 1.0).abs() <= 0.05 => LhKind::Inherit,
        Some(_) => LhKind::Scaled,
        None if v.starts_with(|c: char| c.is_ascii_digit() || c == '.') => LhKind::Fixed,
        None => LhKind::Inherit,
    }
}

/// 数、`normal`、em、% 写的行高折成本元素字号的倍数，和是不是长度（em、%）；别的 `None`。
fn scaled_factor(v: &str) -> Option<(f64, bool)> {
    if v == "normal" {
        return Some((LH_EM, false));
    }
    let (num, k, len) = if let Some(n) = v.strip_suffix('%') {
        (n, 0.01, true)
    } else if let Some(n) = v.strip_suffix("em").filter(|n| !n.ends_with('r')) {
        (n, 1.0, true)
    } else {
        (v, 1.0, false)
    };
    num.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0).map(|n| (n * k, len))
}

/// 照 Kindle 的一行（本元素字号的倍数 × D，D＝缺省行距是 em 的几倍）。
fn kindle_line(lh: Option<f64>, base: f64, d: f64) -> f64 {
    km::shown_line_height_ratio(lh, base) * d
}

/// 全书样式表、`<style>`、行内 `style` 里 [`LhKind::Scaled`] 的 `line-height` 改成 [`kindle_line`] 的 em。返回改了几条。
fn scale_line_heights(entries: &mut [Entry], base: f64, d: f64, body_classes: &HashSet<String>) -> usize {
    fn decls(text: &str, base: f64, d: f64, body: bool, n: &mut usize) -> Option<String> {
        let mut edits = Vec::new();
        for c in html::css_decls(text) {
            if !c.prop.eq_ignore_ascii_case("line-height") {
                continue;
            }
            let (v, imp) = crate::cssunlock::split_important(c.value);
            if lh_kind(Some(v), base, body) != LhKind::Scaled {
                continue;
            }
            let Some((f, _)) = scaled_factor(&v.to_ascii_lowercase()) else { continue };
            let start = c.start + c.raw.find(c.value).unwrap_or(0);
            edits.push((start, start + c.value.len(), format!("{}em{imp}", fmt(kindle_line(Some(f), base, d)))));
        }
        *n += edits.len();
        (!edits.is_empty()).then(|| html::apply_edits(text, edits))
    }
    let css = |text: &str, n: &mut usize| -> String {
        let edits: Vec<(usize, usize, String)> = crate::cascade::rule_spans(text)
            .into_iter()
            .filter(|r| r.closed && !crate::cascade::rule_selector(r.prelude).trim().starts_with('@'))
            .filter_map(|r| {
                let sel = crate::cascade::rule_selector(r.prelude);
                // 选择器列表里有一项选的是 `<body>` 就算
                let body = crate::cascade::split_top(&sel, ',').iter().any(|s| super::kindle_rules::is_body_selector(s.trim(), body_classes));
                decls(r.body, base, d, body, n).map(|t| (r.body_start, r.body_start + r.body.len(), t))
            })
            .collect();
        html::apply_edits(text, edits)
    };
    crate::util::par_map_mut(entries, |e| {
        let mut n = 0;
        if is_css_name(&e.name) {
            let (text, latin1) = match std::str::from_utf8(&e.data) {
                Ok(t) => (std::borrow::Cow::Borrowed(t), false),
                Err(_) => (std::borrow::Cow::Owned(crate::util::latin1_decode(&e.data)), true),
            };
            let out = css(&text, &mut n);
            if n > 0 {
                e.data = if latin1 { crate::util::latin1_encode(&out) } else { out.into_bytes() };
            }
            return n;
        }
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
        let s = html::edit_style_attrs(text, |t, v| decls(v, base, d, t.is("body"), &mut n).map_or(html::Edit::Keep, html::Edit::Set)).into_owned();
        let s = html::edit_style_blocks(&s, |_, c| html::Edit::Set(css(c, &mut n))).into_owned();
        if n > 0 {
            e.data = s.into_bytes();
        }
        n
    })
    .into_iter()
    .sum()
}

/// 只用在 `<body>` 上的类（别的元素也用的不算）。
fn body_only_classes(entries: &[Entry]) -> HashSet<String> {
    // 各章并行扫（结果是集合，合并和顺序无关）
    let per_file = crate::util::par_map(entries, |e| {
        let (mut body, mut other) = (HashSet::new(), HashSet::new());
        let Some(t) = is_chapter_entry(e).then(|| std::str::from_utf8(&e.data).ok()).flatten() else { return (body, other) };
        for g in html::tags(t).filter(|g| g.is_start()) {
            let Some(c) = html::attr_value(&t[g.start..g.end], "class") else { continue };
            let set = if g.is("body") { &mut body } else { &mut other };
            set.extend(c.split_whitespace().map(str::to_string));
        }
        (body, other)
    });
    let (mut body, mut other) = (HashSet::new(), HashSet::new());
    for (b, o) in per_file {
        body.extend(b);
        other.extend(o);
    }
    body.retain(|c| !other.contains(c));
    body
}

/// 只认长度的阅读器上、[`scale_line_heights`] 改过以后还要接行距的：(第几个开始标签, 写多少 em)。`d` 是缺省行距是 em 的几倍。
/// 一行的高按根 em 算：自己写了的照算（改过的是 Kindle 的一行，px 这类照写的），没写的跟父元素（长度照绝对值继承；直接在
/// `<body>` 里的跟 `<body>`），到顶是缺省行距 × 字号。
fn length_line_heights(items: &[Item], body: &(Option<String>, Option<f64>, f64), base: f64, d: f64) -> Vec<(usize, f64)> {
    let own_line = |decl: Option<&String>, lh: Option<f64>, fs: f64, body: bool| match lh_kind(decl.map(String::as_str), base, body) {
        LhKind::Scaled => Some(kindle_line(lh, base, d) * fs),
        LhKind::Fixed => Some(lh.unwrap_or(1.0) * fs),
        LhKind::Inherit => None,
    };
    let body_line = own_line(body.0.as_ref(), body.1, body.2, true);
    // 每个元素在这台上定下的一行（根 em），`None`＝缺省行距（随字号）
    let mut fixed: Vec<Option<f64>> = Vec::with_capacity(items.len());
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let mut cur = own_line(it.own_decl.as_ref(), it.lh, it.fs, false).or_else(|| it.parent.map_or(body_line, |p| fixed[p]));
        if it.chars > 0 && it.is_block {
            let want = kindle_line(it.lh, base, d);
            let have = cur.map_or(d, |l| l / it.fs);
            if (have / want - 1.0).abs() > 0.03 {
                let want = (want * 10000.0).round() / 10000.0;
                out.push((i, want));
                cur = Some(want * it.fs);
            }
        }
        fixed.push(cur);
    }
    out
}

/// 要改字号的标题：(第几个开始标签, 关键字)。
/// 标题改成 `div` 拿不准的书：样式表里有选得到我们改出来的 `div` 的规则——光写 `div` 的（`div`、`body > div`、`div + p`），
/// 或者 `div.类` 的类在标题上也有（标题原来的类照留）；某一级标题出现在组合选择器的前面（`h2 + p`、`h1 small`）或者带伪元素
/// （`h2::before`）的，改了标签选不到别的元素，这一级不改。
#[derive(Default)]
pub(super) struct DivGuard {
    bare: bool,
    classes: HashSet<String>,
    levels: HashSet<String>,
}

impl DivGuard {
    /// 全书的样式表和各章的 `<style>`：逐条目并行算（[`DivGuard::of_entry`]），再合起来（都是集合、一个「有没有」，和顺序无关）。
    pub(super) fn of(entries: &[Entry]) -> DivGuard {
        let mut g = DivGuard::default();
        for one in crate::util::par_map(entries, DivGuard::of_entry) {
            g.bare |= one.bare;
            g.classes.extend(one.classes);
            g.levels.extend(one.levels);
        }
        g
    }

    /// 一个条目（样式表，或章节里的 `<style>`）里的规则；别的条目是空的。先看是不是样式表、章节再解码（图片不白解码一遍）。
    fn of_entry(e: &Entry) -> DivGuard {
        let mut g = DivGuard::default();
        {
            let sheets: Vec<String> = if is_css_name(&e.name) {
                vec![String::from_utf8_lossy(&e.data).into_owned()]
            } else if is_chapter_entry(e) {
                let text = String::from_utf8_lossy(&e.data);
                html::style_blocks(&text).into_iter().map(|b| text[b.content_start..b.content_end].to_string()).collect()
            } else {
                return g;
            };
            for css in &sheets {
                for r in crate::cascade::rule_spans(css) {
                    let sel = crate::cascade::rule_selector(r.prelude);
                    for l in 1..=6 {
                        let name = format!("h{l}");
                        for (_, rest) in type_tokens(&sel, &name) {
                            let compound: String = rest.chars().take_while(|c| !c.is_whitespace() && !matches!(c, ',' | '>' | '+' | '~')).collect();
                            let after = rest[compound.len()..].trim_start();
                            if compound.contains(':') || !(after.is_empty() || after.starts_with(',')) {
                                g.levels.insert(name.clone());
                            }
                        }
                    }
                    for (_, rest) in type_tokens(&sel, "div") {
                        // 后面紧跟的类（`div.a.b`）
                        let compound: String = rest.chars().take_while(|c| !c.is_whitespace() && !matches!(c, ',' | '>' | '+' | '~' | '{')).collect();
                        let classes: Vec<&str> = compound.split('.').skip(1).map(|c| c.split([':', '#', '[']).next().unwrap_or("")).filter(|c| !c.is_empty()).collect();
                        if classes.is_empty() {
                            g.bare = true;
                        }
                        g.classes.extend(classes.into_iter().map(str::to_string));
                    }
                }
            }
        }
        g
    }

    /// 这个标题改成 `div` 以后不会被书里别的规则选到。
    fn allows(&self, el: &scraper::ElementRef) -> bool {
        !self.bare && !self.levels.contains(&el.value().name().to_ascii_lowercase()) && el.value().classes().all(|c| !self.classes.contains(c))
    }
}

/// 选择器文本里作为类型选择器出现的 `name`（前面是开头、空白、`,`、`>`、`+`、`~`、`(`，后面是结尾或不是名字字符）：(位置, 名字后面的文本)。
pub(super) fn type_tokens<'a>(sel: &'a str, name: &str) -> Vec<(usize, &'a str)> {
    let lower = sel.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = lower[from..].find(name) {
        let at = from + k;
        let end = at + name.len();
        let before_ok = lower[..at].chars().next_back().is_none_or(|c| c.is_whitespace() || matches!(c, ',' | '>' | '+' | '~' | '('));
        let after_ok = lower[end..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '-' || c == '_'));
        if before_ok && after_ok {
            out.push((at, &sel[end..]));
        }
        from = end;
    }
    out
}

fn heading_sizes(ch: &Chapter, h: &HeadingSizes, guard: &DivGuard) -> Vec<(usize, String, f64, Option<Vec<String>>)> {
    // 标题：(第几个开始标签, 这台上本来的大小, Kindle 上的字号 → 字数)
    struct Heading {
        n: usize,
        on_reader: f64,
        sizes: HashMap<i64, usize>,
        /// 标题元素在 Kindle 上的字号（根 em）。
        kindle: f64,
    }
    let mut heads: Vec<Heading> = Vec::new();
    // 每个元素所在的标题（在 heads 里的下标）
    let mut head_of: Vec<Option<usize>> = Vec::with_capacity(ch.els.len());
    for (i, el) in ch.els.iter().enumerate() {
        let tag = ch.el(i).value().name();
        // 标题里再套标题（不合法）按外面的算
        let head = match el.parent.and_then(|p| head_of[p]) {
            None if h.tag_size(tag).is_some() => {
                let on_reader = h.on_reader(tag, el.decls.get("font-size").map(String::as_str)).expect("是标题");
                heads.push(Heading { n: i, on_reader, sizes: HashMap::new(), kindle: el.comp.font_size });
                Some(heads.len() - 1)
            }
            other => other,
        };
        head_of.push(head);
        // 标题里的字按所在元素的计算字号计
        if let (Some(hd), false) = (head, el.hidden) {
            let k = ch.own_chars(i);
            if k > 0 {
                *heads[hd].sizes.entry(km::key(el.comp.font_size)).or_default() += k;
            }
        }
    }
    heads
        .into_iter()
        .filter_map(|hd| {
            let want = km::weighted_mode(&hd.sizes)?;
            let (kw, size) = h.nearest(want);
            // 最近的关键字也差 3% 以上（同 `tools/kfx/unify.py` 的容差）：改成 `div`，字号照 CSS（Move 上 div 的 em 字号认）
            if (size / want - 1.0).abs() > 0.03 && guard.allows(&ch.el(hd.n)) {
                // 字号写成 Kindle 上的（相对父元素的 em）：关键字（`large`）在 Move 上是绝对大小、和 Kindle 不一样
                let parent = ch.els[hd.n].parent.map_or(ch.body_comp.font_size, |p| ch.els[p].comp.font_size);
                let mut own: Vec<String> = ch.els[hd.n].decls.iter().filter(|(p, _)| *p != "font-size").map(|(p, v)| format!("{p}:{}", v.trim())).collect();
                own.push(format!("font-size:{}em", fmt(hd.kindle / parent)));
                // 标签缺省的粗体（书里没写的；`eink-ua.css` 一般写了）
                if ch.els[hd.n].comp.bold && !ch.els[hd.n].decls.contains_key("font-weight") {
                    own.push("font-weight:bold".into());
                }
                own.sort();
                return Some((hd.n, String::new(), hd.kindle, Some(own)));
            }
            ((size - hd.on_reader).abs() > 1e-6).then(|| (hd.n, kw.to_string(), hd.kindle, None))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(page: &str, css: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body>{page}</body></html>"#).into_bytes() },
        ]
    }

    fn move_reader() -> ReaderModel {
        ReaderModel::for_profile(profile::get("xochitl").unwrap())
    }

    fn run_with(page: &str, css: &str, reader: &ReaderModel) -> String {
        let mut b = book(page, css);
        apply(&mut b, &mut WashReport::default(), None, reader);
        String::from_utf8(b[2].data.clone()).unwrap()
    }

    /// 只看行距：标题字号不管的阅读器。
    fn run(page: &str, css: &str) -> String {
        run_with(page, css, &ReaderModel { heading_sizes: None, ..move_reader() })
    }

    /// 正文 1.618（body 上写、段落继承）：标题、div 归一成 1（Move 上本来是 1.618），`<p>` 不动；标题自己写了 1em 的 → 1/1.618，
    /// 低于 Kindle 显示的下限，写 0.744；
    /// 行内 `style` 后面接；套着块、自己不排字的 div 不动，里面的块照算；隐藏的不算正文。
    #[test]
    fn blocks_other_than_p_follow_kindle() {
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let out = run(&format!(r#"<h1><span>标题</span></h1>{p}<div>说明</div><h2 class="t" style="color:red;line-height:0.744;">小标题</h2><div class="w"><h3>节</h3><p>正文</p></div>"#), "body{line-height:1.618} h2.t{line-height:1em}");
        assert!(out.contains(r#"<h1 style="line-height:1;"><span>标题</span></h1>"#), "{out}");
        assert!(out.contains(r#"<p>正文正文正文正文正文正文</p><div style="line-height:1;">说明</div><h2 class="t" style="color:red;line-height:0.744;">小标题</h2><div class="w"><h3 style="line-height:1;">节</h3><p>正文</p></div>"#), "{out}");
    }

    /// `<p>`：外链写的 Move 不认（是 1），和 Kindle 不一样的接行内；行内写了的 Move 认，照 Kindle 归一；正文段落不动。
    #[test]
    fn paragraphs_get_inline_line_height() {
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let out = run(&format!(r#"{p}<p class="poem" style="line-height:0.744;">诗</p><p style="line-height:2">行内</p><p style="line-height:1.618">同正文</p>"#), "body{line-height:1.618} p.poem{line-height:1}");
        assert!(out.contains(r#"<p>正文正文正文正文正文正文</p><p class="poem" style="line-height:0.744;">诗</p><p style="line-height:2;line-height:1.2361;">行内</p><p style="line-height:1.618;line-height:1;">同正文</p>"#), "{out}");
    }

    /// 没写行距的书（正文按 1.2）：Move 上本来就是 1，不改；正文 1.5em、div 没写：Kindle 1.2/1.5 = 0.8；写成 1.5em 的 div 是 1。
    /// 改过的块里套着的块按改后的值继承。
    #[test]
    fn unwritten_and_inherited() {
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let page = format!(r#"<div>说明</div>{p}<h1>标题</h1>"#);
        assert_eq!(run(&page, "p{margin:0}"), String::from_utf8(book(&page, "p{margin:0}")[2].data.clone()).unwrap());
        let out = run(&format!(r#"<div>说明</div>{p}<div class="e">一<div>二</div></div>"#), "p{line-height:1.5em} div.e{line-height:1.5em}");
        assert!(out.contains(r#"<div style="line-height:0.8;">说明</div>"#), "{out}");
        // div.e：Kindle 1.5/1.5 = 1，Move 上本来 1.5 → 写 1；里面的 div 继承 1.5em（长度按绝对值继承，Kindle 照样 1），Move 上继承改后的 1，不再写
        assert!(out.contains(r#"<div class="e" style="line-height:1;">一<div>二</div></div>"#), "{out}");
    }

    /// 写 2em 的 h2 → xx-large；缺省的 h2（1.5em）不动；h3 写 1em → medium；字在 span 里按 span 的字号；行内 `style` 后面接；
    /// 最近的关键字也差 3% 以上的改成 div（打记号、层叠出来的声明写进行内，字号写 Kindle 上的 em）：h4 `large`（Kindle 1.125，
    /// Move 的 large 1.216）、h6 0.67em（small 0.784）。行距不管的阅读器只改字号。
    /// 标题缺省居中的阅读器（掌阅）：自己没写对齐的照 Kindle 继承来的写明；写了的、外层居中的、不居中的阅读器不动。
    #[test]
    fn heading_alignment_written_where_reader_centers() {
        let reader = ReaderModel { heading_center: true, ..ReaderModel::default() };
        let out = run_with(r#"<h2>甲</h2><h2 class="c">乙</h2><div class="m"><h3>丙</h3></div><p>正文</p>"#, "body{text-align:justify} .c{text-align:right} div.m{text-align:center}", &reader);
        assert!(out.contains(r#"<h2 style="text-align:justify;">甲</h2><h2 class="c">乙</h2><div class="m"><h3>丙</h3></div>"#), "{out}");
        let plain = run_with("<h2>甲</h2>", "", &ReaderModel::default());
        assert!(plain.contains("<h2>甲</h2>"), "{plain}");
    }

    /// 改成 div 拿不准的不改（照旧按关键字）：书里有光写 `div` 的规则、`div.类` 的类标题上也有、这一级出现在组合选择器前面。
    #[test]
    fn heading_div_guarded() {
        let reader = ReaderModel { line_height_except_p: false, ..move_reader() };
        for css in ["h4{font-size:large} div{margin:0}", "h4{font-size:large} div.kw{color:red}", "h4{font-size:large} h4 + p{text-indent:0}", "h4{font-size:large} h4::before{content:'x'}"] {
            let out = run_with(r#"<h4 class="kw">四</h4><p>正文</p>"#, css, &reader);
            // 本来写的就是 Move 认的 large：原样不动
            assert!(out.contains(r#"<h4 class="kw">四</h4><p>正文</p>"#), "{css}: {out}");
        }
    }

    #[test]
    fn headings_get_nearest_keyword() {
        let reader = ReaderModel { line_height_except_p: false, ..move_reader() };
        let out = run_with(
            r#"<h2 class="big">章</h2><h2>节</h2><h3 class="one" style="color:red">小节</h3><h1><span class="s">标题</span></h1><h4 class="kw">四</h4><h6>六</h6><p class="big">正文</p>"#,
            "h1{font-size:2em} h2{font-size:1.5em} h3{font-size:1.17em} h4{font-size:1em} h6{font-size:0.67em} .big{font-size:2em} h3.one{font-size:1em} span.s{font-size:0.6em} h4.kw{font-size:large}",
            &reader,
        );
        assert!(out.contains(r#"<h2 class="big" style="font-size:xx-large;" data-eink-fs="2">章</h2><h2>节</h2><h3 class="one" style="color:red;font-size:medium;" data-eink-fs="1">小节</h3>"#), "{out}");
        // span 0.6em × 2em = 1.2em → large
        // Kindle 上的字号记在 data-eink-fs（后面照 Kindle 算的 pass 用）
        assert!(out.contains(r#"<h1 style="font-size:large;" data-eink-fs="2"><span class="s">标题</span></h1><h4 class="kw" style="font-size:1.125em;font-weight:bold;" data-eink-div="1">四</h4><h6 style="font-size:0.67em;font-weight:bold;" data-eink-div="1">六</h6><p class="big">正文</p>"#), "{out}");
    }

    /// 只认长度的阅读器（缺省行距 1.5em）：样式表里的行距照 Kindle 改成 em（body 上倍数是 1 的不改，`<p>` 上的 1.84 → 1.5em；标题 1.2 → 1.2/1.84 低于
    /// Kindle 显示的下限 0.744，× 1.5，3em → 3/1.84 × 1.5，
    /// `normal` 同 1.2，px 不动）；没写的 div 继承 body 的 1.84，两边都是缺省行距，不接；套在写了 150% 的 div 里、字号大一倍的 h2
    /// 继承绝对的行高（Kindle 上是自己字号的 0.75 倍，÷ 1.84 低于显示的下限 0.744），掌阅上是改过的 div 的一行，接行内 0.744 × 1.5。
    #[test]
    fn length_only_reader() {
        let reader = ReaderModel { line_em: Some(1.5), ..ReaderModel::default() };
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let css = "body{line-height:1.84} p{line-height:1.84} h1{line-height:1.2 !important} p.w{line-height:3em} .n{line-height:normal} .x{line-height:20px} div.b{line-height:150%} h2{font-size:2em}";
        let mut b = book(&format!(r#"<h1>标题</h1>{p}<div>说明</div><p class="w">宽</p><div class="b"><h2>大</h2></div>"#), css);
        apply(&mut b, &mut WashReport::default(), None, &reader);
        let out_css = String::from_utf8(b[1].data.clone()).unwrap();
        assert_eq!(out_css, "body{line-height:1.84} p{line-height:1.5em} h1{line-height:1.116em !important} p.w{line-height:2.4457em} .n{line-height:1.116em} .x{line-height:20px} div.b{line-height:1.2228em} h2{font-size:2em}");
        let out = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(out.contains(r#"<h1>标题</h1><p>正文"#), "{out}");
        assert!(out.contains(r#"<div>说明</div><p class="w">宽</p><div class="b"><h2 style="line-height:1.116em;">大</h2></div>"#), "{out}");
    }

    /// 两样都要改的标签：先行距、后字号；照 CSS 排的阅读器什么都不改。
    #[test]
    fn both_on_one_tag_and_css_reader_untouched() {
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let page = format!(r#"<h2 style="color:red">章</h2>{p}"#);
        let css = "body{line-height:1.618} h2{font-size:2em}";
        assert!(run_with(&page, css, &move_reader()).contains(r#"<h2 style="color:red;line-height:1;font-size:xx-large;" data-eink-fs="2">章</h2>"#));
        assert_eq!(run_with(&page, css, &ReaderModel::default()), String::from_utf8(book(&page, css)[2].data.clone()).unwrap());
    }
}

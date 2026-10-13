//! 照 Send to Kindle 的规则排（只修复的文字书，profile `kindle_rules`：掌阅、Move；2026-10-08 用户定「三台统一是一套规则」）。
//! Kindle 那边由 KFX 写出器按这些规则写（`kfx::write`，规则表见 docs/kfx.md#send-to-kindle-的样式规则）；这里把其中会让
//! Kindle 和按 CSS 排的阅读器看起来不一样的几条，就地改进书自己的样式表（只改值、删声明，不加选择器——xochitl 的 CSS 解析器很脆）：
//!
//! - 标签的缺省样式（`<p>` 上下 1em 等）：`eink-ua.css`，见 [`crate::uastyle`]（`repair_entries` 里挂）；
//! - **正文字体用阅读器的字体**：`font-family` 第一个是正文字体（全书层叠按块算字数最多的计算字体，同 KFX 写出器，见 `book_facts`）的整条去掉
//!   （Kindle 写 `default,…`，`default` 排第一、后面的备选不起作用；EPUB 里留着备选，阅读器会去找「宋体」这些系统字体）；
//!   写在 `font` 简写里的拆成分项、不写字体族（字号、行高、粗斜体照原值；拆不清的不动）；
//! - **body 的左右外边距、内边距不要**（Kindle 不写）；用到负的左（右）外边距的文件的字数占多数时，这一侧照留（Kindle 按文件定）；
//! - **文字和背景的对比度至少 4.5:1**（[`crate::color::ensure_contrast`]）：同一条规则里写了背景色的按它；没写的按白页面，
//!   但只调亮度不超过 0.5 的颜色（白字、浅灰多半压在背景图或外层的深色块上，看不到，不动）；规则写了深背景、没写文字颜色、
//!   缺省的黑字不够看清时补一条文字颜色。按规则算（Kindle 按元素算，背景来自祖先时这里看不到）。
//!
//! 文字一个不动；行内 `style` 同样处理（xochitl 不认行内样式，掌阅认）。
use crate::cascade::{parse_font_shorthand, FontShorthand, MediaEnv};
use crate::cascade::{box_sides, compound_tag_classes, last_compound, rule_selector, rule_spans, BoxSides};
use super::*;
use crate::color::{contrast, ensure_contrast, luminance, over_white, parse_color};

/// 照 Send to Kindle 的规则改书的样式表（独立的 `.css`、`<style>`）和行内 `style`；改了的声明数记进 `rep.kindle_rule_edits`。
pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, opts: &WashOpts) -> Option<f64> {
    let media = opts.media.as_ref();
    let facts = book_facts(entries, media);
    let font_scale = font_scale(entries, facts.base_font_size);
    let mut ctx = Ctx {
        body_fonts: facts.body_font.iter().cloned().collect(),
        body_classes: facts.body_classes,
        neg_left: facts.neg[0],
        neg_right: facts.neg[1],
        font_scale,
        opaque_rgba: !opts.css_rgba,
        drop_single_backgrounds: !opts.background_no_repeat,
        tiled: HashSet::new(),
        bg_fixes: HashMap::new(),
    };
    // 平铺背景的阅读器：整页背景图做成整页一块（`bgfit::plan_tiles`），这些规则照留，图由优化器改（`rep.page_tiles`）
    if let Some(g) = opts.page_tiles.filter(|_| ctx.drop_single_backgrounds) {
        rep.page_tiles = crate::bgfit::plan_tiles(entries, &ctx.body_classes, g);
        ctx.tiled = rep.page_tiles.keys().cloned().collect();
        // 背景图的位置相对 body 盒子（Kindle：body 只到最后一块的下外边距为止，`bottom` 的图贴着它，2026-10-12《飘》截图）。
        // 用这张图的页都只有一个块、body 底差不多时，图块里的定位范围就到那里（按内容区高折过来）；`cover` 写出器按整页画，不动
        if let Some((_, k)) = opts.image_sizing {
            body_box_areas(entries, media, k, &mut rep.page_tiles);
        }
    }
    // 背景尺寸的高写 `auto` 会把图拉长到整页高的阅读器（掌阅，profile `background_auto_height = false`）：整页背景图照 Kindle 上
    // 的大小、位置（同 Move 的图块，`bgfit::plan_tiles`）写成屏幕像素的 `background-size`、`background-position`，接在规则末尾
    if let (Some(g), Some((_, k))) = (opts.bg_place, opts.image_sizing) {
        let mut plans = crate::bgfit::plan_tiles(entries, &ctx.body_classes, g);
        body_box_areas(entries, media, k, &mut plans);
        let dims = super::image_dims_of(entries, rep, opts.image_dims.as_deref());
        for (path, t) in plans {
            let Some(&(w, h)) = dims.get(path.as_str()) else { continue };
            let (x, y, dw, dh) = t.placement(w, h);
            let n = |v: f64| fmt_num(v.round());
            ctx.bg_fixes.insert(path, format!("background-size:{}px {}px;background-position:{}px {}px;", n(dw), n(dh), n(x), n(y)));
        }
    }
    ctx.body_fonts.extend(exposed_fonts(entries, media, &ctx, &facts.fonts));
    rep.kindle_rule_edits += rewrite_entries(entries, &ctx);
    ctx.font_scale.filter(|&k| k != 1.0)
}

/// 改完自查（正文字体）：Kindle 把正文字体写成 `default`，盖过书里所有的字体声明；这里只能删声明，删掉胜出的那条，层叠里被它
/// 盖住的另一条就露出来了（《嘯風山莊》每章先链 `idstyles.css` 的 `p.p{AR MingU30 Light}`、后链 `CSS.css` 的
/// `p.p{AR MingU30 DemiBold}`：删了 DemiBold，21.9 万字的正文显示成 Light，2026-10-12）。所以在样式表、章节的副本上照改一遍，
/// 用同一套层叠（[`book_facts`]）重数各字体的字数：哪个字体接管的字数比它本来就生效的还多，就是露出来的，一并当正文字体去掉，
/// 再查一轮（最多 3 轮）。代价：露出来的字体本来就生效的那些字（《嘯風山莊》2575 字）也换成阅读器字体，Kindle 上它们仍是原字体。
fn exposed_fonts(entries: &[Entry], media: Option<&MediaEnv>, ctx: &Ctx, before: &HashMap<Option<String>, usize>) -> HashSet<String> {
    let mut found = HashSet::new();
    if ctx.body_fonts.is_empty() {
        return found;
    }
    let total: usize = before.values().sum();
    let mut trial = Ctx { body_fonts: ctx.body_fonts.clone(), body_classes: ctx.body_classes.clone(), tiled: ctx.tiled.clone(), bg_fixes: ctx.bg_fixes.clone(), font_scale: None, ..*ctx };
    for _ in 0..3 {
        let mut copy: Vec<Entry> = entries.iter().filter(|e| is_css_name(&e.name) || is_chapter_entry(e)).cloned().collect();
        rewrite_entries(&mut copy, &trial);
        let after = book_facts(&copy, media).fonts;
        let new: Vec<String> = after
            .iter()
            .filter_map(|(f, &n)| {
                let f = f.as_ref()?;
                let had = before.get(&Some(f.clone())).copied().unwrap_or(0);
                // 接管的字数多过本来就有的、也不是零星几个字（全书的千分之一以上）
                let gained = n.saturating_sub(had);
                (!trial.body_fonts.contains(f) && gained > had && gained * 1000 > total).then(|| f.clone())
            })
            .collect();
        if new.is_empty() {
            break;
        }
        found.extend(new.iter().cloned());
        trial.body_fonts.extend(new);
    }
    found
}

/// 照 `ctx` 改全书的样式表（独立的 `.css`、`<style>`）和行内 `style`，返回改了的声明数。
fn rewrite_entries(entries: &mut [Entry], ctx: &Ctx) -> usize {
    crate::util::par_map_mut(entries, |e| {
        let mut n = 0;
        if is_css_name(&e.name) {
            let out = match std::str::from_utf8(&e.data) {
                Ok(text) => rewrite_css_in(text, &e.name, ctx, &mut n).into_bytes(),
                // 不是 UTF-8 的样式表（《啸风山庄》的 CSS.css 是 Big5）：按单字节原样读写。CSS 的语法都是 ASCII，要改的值（颜色、边距）
                // 也是，别的字节（Big5 写的字体名）原样留着
                Err(_) => crate::util::latin1_encode(&rewrite_css_in(&crate::util::latin1_decode(&e.data), &e.name, ctx, &mut n)),
            };
            if n > 0 {
                e.data = out;
            }
            return n;
        }
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
        let s = html::edit_style_attrs(text, |t, value| {
            let body = t.name.eq_ignore_ascii_case("body");
            let (v, k) = rewrite_decls(value, body, &e.name, ctx);
            n += k;
            if k == 0 {
                html::Edit::Keep
            } else if v.trim().is_empty() {
                html::Edit::Remove
            } else {
                html::Edit::Set(v)
            }
        })
        .into_owned();
        let s = html::edit_style_blocks(&s, |_, css| html::Edit::Set(rewrite_css_in(css, &e.name, ctx, &mut n))).into_owned();
        if n > 0 {
            e.data = s.into_bytes();
        }
        n
    })
    .into_iter()
    .sum()
}

struct Ctx {
    /// 正文字体（规范化：去引号、小写）：全书正文的计算字体，加上去掉它以后露出来的（[`exposed_fonts`]）。空＝正文没写字体。
    body_fonts: HashSet<String>,
    /// 只用在 `<body>` 上的类（别的元素也用的不算：改它的边距会连带改别处）。
    body_classes: HashSet<String>,
    neg_left: bool,
    neg_right: bool,
    /// 字号要乘的系数（1 / 全书正文字号，[`font_scale`]），不动字号的 `None`。
    font_scale: Option<f64>,
    /// 阅读器不认 `rgba()`（profile `css_rgba = false`，掌阅）：规则、行内样式里的 `rgba()` 按白底合成不透明色
    /// （[`crate::cssunlock::rgba_to_opaque`]）。《飘》「第一卷」标题底下的 `rgba(255,255,255,0.8)` 白条在掌阅上整条作废，
    /// 深色小字压在插图最密处看不见（2026-10-12 测试书：只有改成 `#fff` 的看得见）。
    opaque_rgba: bool,
    /// 阅读器不认背景图的 `no-repeat`（profile `background_no_repeat = false`，xochitl）：写了不平铺的背景图去掉，背景色照留。
    drop_single_backgrounds: bool,
    /// 做成整页图块的背景图（zip 路径，`bgfit::plan_tiles`）：用它们的规则不去掉。
    tiled: HashSet<String>,
    /// 背景图（zip 路径）→ 接在用它的规则末尾的尺寸、位置（掌阅，`background_auto_height = false`）。
    bg_fixes: HashMap<String, String>,
}

/// 竖直间距照 Kindle 换算（2026-10-12 用户定，「测试-竖直间距」三台量的）：Kindle 的上下外边距、内边距按行距换像素（一 rem ≈
/// 正文一行 / 1.2 ≈ 52.5px），掌阅、Move 按自己的字号（≈ 37.5px），同样写 1em，Kindle 上占页高多一半。这里把上下外边距、内边距
/// 改写成在这个阅读器上占内容区高的比例和 Kindle 一样：
///
/// - `em`、`rem` 乘 `em`（跟着字号走，阅读器里调字号照样变）；
/// - `%`、`px`、`pt` 先照 KFX 写出器折成 Kindle 上的像素（`%` 按 32em 页宽、px 按 0.45pt、12pt 一 em），再按两台内容区的高折成
///   这个阅读器的屏幕像素写 `px`（掌阅、Move 的 px 是屏幕像素；顺带修了掌阅把 `pt` 当 px、Move 的 `%` 只有十来像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VScale {
    /// `em` 要乘的系数（Kindle 一 rem 的像素 × 内容区高之比 / 这个阅读器一 em 的像素）。
    pub em: f64,
    /// Kindle 竖直一 rem 的像素。
    pub kindle_rem_px: f64,
    /// 这个阅读器内容区高 / Kindle 内容区高。
    pub ratio: f64,
}

impl VScale {
    /// 一个长度换算后的写法；不用换的（0、`auto`、`calc()`、认不出的单位）`None`。
    fn length(&self, v: &str) -> Option<String> {
        let v = v.trim();
        let end = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))?;
        let n: f64 = v[..end].parse().ok()?;
        if n == 0.0 {
            return None;
        }
        let px = |kindle_rem: f64| format!("{}px", fmt_num((kindle_rem * self.kindle_rem_px * self.ratio * 10.0).round() / 10.0));
        Some(match v[end..].to_ascii_lowercase().as_str() {
            "em" | "rem" => format!("{}{}", fmt_num(n * self.em), &v[end..]),
            "%" => px(n / 100.0 * 32.0),
            "px" => px(n * 0.45 / crate::cascade::PT_PER_EM),
            "pt" => px(n / crate::cascade::PT_PER_EM),
            _ => return None,
        })
    }
}

/// 一段声明里的上下外边距、内边距照 [`VScale`] 换算（分项、简写一样；简写只换上下两个值）。返回 (新文本, 改了几条)。
pub(super) fn scale_vertical(text: &str, vs: &VScale) -> (String, usize) {
    let decls = html::css_decls(text);
    let mut edits = Vec::new();
    for d in &decls {
        let prop = d.prop.to_ascii_lowercase();
        let (value, imp) = strip_important(d.value);
        let new = match prop.as_str() {
            "margin-top" | "margin-bottom" | "padding-top" | "padding-bottom" => vs.length(value).map(|v| format!("{v}{imp}")),
            "margin" | "padding" => match box_sides(d.value) {
                BoxSides::Sides([t, r, b, l], imp) => {
                    let (nt, nb) = (vs.length(t), vs.length(b));
                    (nt.is_some() || nb.is_some()).then(|| format!("{} {r} {} {l}{imp}", nt.unwrap_or_else(|| t.to_string()), nb.unwrap_or_else(|| b.to_string())))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(v) = new {
            let start = d.start + d.raw.find(d.value).unwrap_or(0);
            edits.push((start, start + d.value.len(), v));
        }
    }
    let n = edits.len();
    (html::apply_edits(text, edits), n)
}

/// 全书的样式表、`<style>`、行内 `style` 照 [`VScale`] 换算，返回改了的声明数。**最后做**：页首垫图、背景图位置、图片大小都按
/// 原来的 CSS 算 Kindle 上的样子（先换算的话它们读到的是放大过的边距，《飘》「第一卷」垫图垫高了、标题挤到下一页）。
pub(super) fn scale_vertical_entries(entries: &mut [Entry], vs: &VScale) -> usize {
    crate::util::par_map_mut(entries, |e| {
        let mut n = 0;
        if is_css_name(&e.name) {
            // 不是 UTF-8 的样式表（《嘯風山莊》的 CSS.css 是 Big5）按单字节原样读写，同 `rewrite_entries`
            let (text, latin1) = match std::str::from_utf8(&e.data) {
                Ok(t) => (Cow::Borrowed(t), false),
                Err(_) => (Cow::Owned(crate::util::latin1_decode(&e.data)), true),
            };
            let out = scale_vertical_css(&text, vs);
            if out != text {
                n = 1;
                e.data = if latin1 { crate::util::latin1_encode(&out) } else { out.into_bytes() };
            }
            return n;
        }
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
        let s = html::edit_style_attrs(text, |_, value| {
            let (v, k) = scale_vertical(value, vs);
            n += k;
            if k == 0 { html::Edit::Keep } else { html::Edit::Set(v) }
        })
        .into_owned();
        let s = html::edit_style_blocks(&s, |_, css| html::Edit::Set(scale_vertical_css(css, vs))).into_owned();
        if s != text {
            n = n.max(1);
            e.data = s.into_bytes();
        }
        n
    })
    .into_iter()
    .sum()
}

/// 整份样式表的规则体照 [`scale_vertical`] 换算。
pub(super) fn scale_vertical_css(css: &str, vs: &VScale) -> String {
    let edits: Vec<(usize, usize, String)> = rule_spans(css)
        .into_iter()
        .filter(|r| r.closed && !rule_selector(r.prelude).trim().starts_with('@'))
        .filter_map(|r| {
            let (t, k) = scale_vertical(r.body, vs);
            (k > 0).then(|| (r.body_start, r.body_start + r.body.len(), t))
        })
        .collect();
    html::apply_edits(css, edits)
}

/// 背景图的定位范围相对 body 盒子（Kindle：body 只到最后一块的下外边距为止，`bottom` 的图贴着它，2026-10-12《飘》截图）。
/// 用这张图的页都只有一个块、body 底差不多时，定位范围的高就到那里（按内容区高折到这个阅读器）；`cover` 写出器按整页画，不动。
fn body_box_areas(entries: &[Entry], media: Option<&MediaEnv>, k: KindleImageRef, plans: &mut HashMap<String, crate::bgfit::PageTile>) {
    if plans.is_empty() {
        return;
    }
    let area = plans.values().next().map(|t| t.area).unwrap_or_default();
    let boxes = super::top_spacer::page_boxes(entries, media, super::SpacerGeometry { kindle: k, area });
    for (path, tile) in plans.iter_mut().filter(|(_, t)| t.fit != crate::bgfit::TileFit::Cover) {
        let bottoms: Vec<Option<f64>> = boxes.values().filter(|b| b.bg.as_deref() == Some(path.as_str()) && !b.bg_cover).map(|b| b.body_bottom).collect();
        let Some(lo) = bottoms.iter().copied().collect::<Option<Vec<f64>>>().filter(|v| !v.is_empty()) else { continue };
        let (min, max) = lo.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        if max - min > 0.03 * k.content_h {
            continue;
        }
        let h = ((min + max) / 2.0 * f64::from(tile.area.1) / k.content_h).round() as u32;
        tile.area.1 = h.clamp(1, tile.area.1);
    }
}

/// 字号归一要乘的系数：1 / 全书正文字号 `base`（[`BookFacts::base_font_size`]；正文已经是 1em 的是 1）。
/// `html`、`:root` 上写了字号的书不动（`None`：掌阅不认 `html` 上的字号，2026-10-10 真机；Kindle 认，正文实际多大两边就对不上，拿不准就不处理）。
fn font_scale(entries: &[Entry], base: f64) -> Option<f64> {
    if root_font_size_declared(entries) {
        return None;
    }
    Some(if (base - 1.0).abs() < 0.005 { 1.0 } else { 1.0 / base })
}

/// 样式表、`<style>` 里有没有选 `html`/`:root` 并写了 `font-size`（或 `font` 简写）的规则；`<html style>` 也算。
fn root_font_size_declared(entries: &[Entry]) -> bool {
    let sizes = |decls: &str| html::css_decls(decls).iter().any(|d| d.prop.eq_ignore_ascii_case("font-size") || d.prop.eq_ignore_ascii_case("font"));
    let in_css = |css: &str| {
        rule_spans(css).into_iter().any(|c| {
            sizes(c.body)
                && rule_selector(c.prelude).split(',').any(|part| {
                    let last = last_compound(part).to_ascii_lowercase();
                    last == ":root" || compound_tag_classes(&last).0 == "html"
                })
        })
    };
    entries.iter().any(|e| {
        if is_css_name(&e.name) {
            return in_css(&String::from_utf8_lossy(&e.data));
        }
        if !is_chapter_entry(e) {
            return false;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { return false };
        html::style_blocks(t).into_iter().any(|b| in_css(&t[b.content_start..b.content_end]))
            || html::tags(t).find(|g| g.is_start() && g.is("html")).and_then(|g| html::attr_value(&t[g.start..g.end], "style").map(sizes)).unwrap_or(false)
    })
}

/// 按全书层叠算出来的、`kindle_rules` 要的事实（[`book_facts`]）。
struct BookFacts {
    /// 正文字号（根 em）：每段文字按所在块（[`crate::cascade::is_block`]）的计算字号、按字数（不算空白）计，取最多的
    /// （[`crate::cascade::body_font_size`]，同 KFX 写出器；算出来不在 0.5–3 之间的用 1）。
    base_font_size: f64,
    /// 正文字体（小写）：每段文字按所在块的计算字体计字数，取最多的，字数一样多的取名字小的、没写字体的排最前（同 KFX 写出器的
    /// `TextCounts::body_font`）；`None`＝正文没写字体。
    body_font: Option<String>,
    /// 所在块的字体（小写，没写的 `None`）→ 字数（不算空白），全书合计：自查时比去掉正文字体前后的变化（[`exposed_fonts`]）。
    fonts: HashMap<Option<String>, usize>,
    /// 只用在 `<body>` 上的类（别的元素也用的不算：改它的边距会连带改别处）。
    body_classes: HashSet<String>,
    /// body 的左、右边距要不要照留：用到负的左（右）外边距的文件的字数多过没用到的（Kindle 按文件定：文件里有块的这一侧外边距
    /// 是负的，body 这一侧的边距照加，KFX 写出器 `has_negative`；样式表全书共用，这里按字数取多数——《平凡的世界》183 个文件
    /// 里 5 个用了负的左边距，丢掉；《罗杰疑案》每章都用，留）。
    neg: [bool; 2],
}

/// 一个文件的统计（[`book_facts`] 按文件并行算，再合起来）。
#[derive(Default)]
struct FileFacts {
    /// 所在块的字号（千分之一取整）→ 字数（不算空白）。
    sizes: HashMap<i64, usize>,
    /// 所在块的字体（小写，没写的 `None`）→ 字数（不算空白）。
    fonts: HashMap<Option<String>, usize>,
    on_body: HashSet<String>,
    elsewhere: HashSet<String>,
    /// 有没有块的左、右外边距是负的。
    neg: [bool; 2],
    /// 可见文字的字数（不算空白）。
    chars: usize,
}

/// 全书只走一趟 DOM，层叠用 KFX 写出器同一套（[`crate::cascade`]：每篇文档的样式表 [`crate::cascade::Sheet::for_doc`]，
/// `@media`、`<link media>` 按阅读模式的 `media` 求，写出器按 KFX 阅读范围求），几样统计挂在同一趟遍历上：正文字号、正文字体、
/// body 上的类、负外边距。2026-10-10 审计以前正文字体、负外边距由另一套正则加粗糙层叠（只认类和裸标签）按原始 HTML 扫，
/// 和 KFX 写出器认出的正文字体可能不是同一个；每个章节还要多扫好几遍。
fn book_facts(entries: &[Entry], media: Option<&MediaEnv>) -> BookFacts {
    use crate::cascade::{Computed, Len, Rules, Sheet};
    use scraper::{ElementRef, Html, Node};
    use std::rc::Rc;
    let css: HashMap<&str, std::sync::Arc<Rules>> = crate::kindle_model::book_css(entries, media);
    let negative = |l: &Option<Len>| match l {
        Some(Len::Em(v) | Len::Percent(v) | Len::Pt(v)) => *v < -1e-9,
        None => false,
    };
    let files: Vec<&Entry> = entries.iter().filter(|e| is_chapter_entry(e)).collect();
    let per_file = crate::util::par_map(&files, |e| {
        let mut f = FileFacts::default();
        let Ok(text) = std::str::from_utf8(&e.data) else { return f };
        let html = Html::parse_document(text);
        let sheet = Sheet::for_doc(&html, &e.name, media, |path| css.get(path).cloned());
        let comp = |el: &ElementRef, parent: &Computed| {
            let mut decls = sheet.cascade(el, &e.name);
            crate::cascade::presentational_hints(el, &mut decls);
            Computed::derive(parent, &decls, el.value().name())
        };
        // 不递归（没关的标签能套上千层）：(节点, 父元素的计算值, 所在块的计算值)。行内元素改字号、字体的字算给所在的块（同写出器）
        let root = html.root_element();
        let rc = Rc::new(comp(&root, &Computed::root()));
        let mut stack: Vec<(ego_tree::NodeRef<Node>, Rc<Computed>, Rc<Computed>)> = root.children().rev().map(|ch| (ch, rc.clone(), rc.clone())).collect();
        while let Some((node, parent, block)) = stack.pop() {
            match node.value() {
                Node::Text(t) => {
                    let n = t.chars().filter(|c| !c.is_whitespace()).count();
                    if n > 0 {
                        f.chars += n;
                        *f.sizes.entry((block.font_size * 1000.0).round() as i64).or_default() += n;
                        *f.fonts.entry(block.font_family.as_ref().map(|s| s.to_ascii_lowercase())).or_default() += n;
                    }
                }
                Node::Element(v) => {
                    if matches!(v.name(), "head" | "script" | "style" | "title") {
                        continue;
                    }
                    let Some(el) = ElementRef::wrap(node) else { continue };
                    // body 上的类收在显示不显示之前：`display:none` 的元素上的类照样算"别处用到"
                    let set = if v.name() == "body" { &mut f.on_body } else { &mut f.elsewhere };
                    set.extend(v.classes().map(str::to_string));
                    let c = comp(&el, &parent);
                    if c.display.as_deref() == Some("none") {
                        continue;
                    }
                    let c = Rc::new(c);
                    let block = if crate::cascade::is_block(&el, &c) {
                        f.neg[0] |= negative(&c.margin[3]);
                        f.neg[1] |= negative(&c.margin[1]);
                        c.clone()
                    } else {
                        block
                    };
                    for ch in node.children().rev() {
                        stack.push((ch, c.clone(), block.clone()));
                    }
                }
                _ => {}
            }
        }
        f
    });
    let (mut sizes, mut fonts) = (HashMap::new(), HashMap::<Option<String>, usize>::new());
    let (mut on_body, mut elsewhere) = (HashSet::new(), HashSet::new());
    // 每一侧：(用到负外边距的文件的字数, 没用到的)
    let mut chars = [(0usize, 0usize); 2];
    for f in per_file {
        for (k, n) in f.sizes {
            *sizes.entry(k).or_default() += n;
        }
        for (k, n) in f.fonts {
            *fonts.entry(k).or_default() += n;
        }
        on_body.extend(f.on_body);
        elsewhere.extend(f.elsewhere);
        for (c, used) in chars.iter_mut().zip(f.neg) {
            if used {
                c.0 += f.chars;
            } else {
                c.1 += f.chars;
            }
        }
    }
    BookFacts {
        base_font_size: crate::cascade::body_font_size(&sizes),
        body_font: crate::kindle_model::body_font(&fonts),
        fonts,
        body_classes: on_body.difference(&elsewhere).cloned().collect(),
        neg: [chars[0].0 > chars[0].1, chars[1].0 > chars[1].1],
    }
}

/// 改一份样式表：按 [`rule_spans`] 逐条规则（`@media` 里的也改，不看条件——阅读器求不求、怎么求说不准），只替换规则体，
/// 选择器连同前面的注释、`@charset …;` 原样留。`@font-face`、`@page` 这类 at 规则不动；到文末没闭合的规则不动（拿不准）。
#[cfg(test)]
fn rewrite_css(css: &str, ctx: &Ctx, n: &mut usize) -> String {
    rewrite_css_in(css, "", ctx, n)
}

/// 同 [`rewrite_css`]，`base` 是样式表所在的条目（解析 `url()` 用）。
fn rewrite_css_in(css: &str, base: &str, ctx: &Ctx, n: &mut usize) -> String {
    let mut edits = Vec::new();
    for r in rule_spans(css) {
        let clean = rule_selector(r.prelude);
        let trimmed = clean.trim();
        if !r.closed || trimmed.starts_with('@') {
            continue;
        }
        let body = trimmed.split(',').all(|s| is_body_selector(s, &ctx.body_classes));
        let (decls, k) = rewrite_decls(r.body, body, base, ctx);
        *n += k;
        if k > 0 {
            edits.push((r.body_start, r.body_start + r.body.len(), decls));
        }
    }
    html::apply_edits(css, edits)
}

/// 选择器是不是只选 `<body>`：`body`、`body.x`、`html body`、只用在 body 上的 `.x`。
fn is_body_selector(sel: &str, body_classes: &HashSet<String>) -> bool {
    let last = last_compound(sel);
    if last.contains([':', '[', '#']) {
        return false;
    }
    let (tag, classes) = compound_tag_classes(last);
    match tag.as_str() {
        "body" => true,
        "" => !classes.is_empty() && classes.iter().all(|c| body_classes.contains(*c)),
        _ => false,
    }
}

/// 改一段声明（规则体或行内 `style`）。返回 (新文本, 改了几条)。
fn rewrite_decls(text: &str, body: bool, base: &str, ctx: &Ctx) -> (String, usize) {
    let decls = html::css_decls(text);
    let mut out = String::new();
    let mut changed = 0;
    let mut last = 0;
    // 同一条规则里的背景色
    let bg = decls.iter().rev().find_map(|d| match d.prop.to_ascii_lowercase().as_str() {
        "background-color" => parse_color(strip_important(d.value).0),
        "background" => crate::cascade::tokens(strip_important(d.value).0).0.into_iter().find_map(parse_color),
        _ => None,
    });
    let bg = bg.filter(|c| c >> 24 != 0).map(over_white).filter(|&c| c != 0xFFFF_FFFF);
    let has_bg = bg.is_some();
    let bg = bg.unwrap_or(0xFFFF_FFFF);
    let mut has_color = false;
    // 这条规则写了不平铺（`no-repeat`、`repeat-x`、`repeat-y`、`space`、`round`）：不认 `no-repeat` 的阅读器会平铺它，去掉背景图
    // 背景图做成了整页图块的规则照留（xochitl 本来就平铺、不管尺寸位置，图块做成整页一份）
    let tiled = !ctx.tiled.is_empty()
        && decls.iter().any(|d| html::css_urls(d.value).iter().any(|u| ctx.tiled.contains(&crate::epubzip::resolve_link(base, u.value).0)));
    // 照 Kindle 写成像素的背景图（掌阅）：规则末尾接上尺寸、位置
    let fix = decls.iter().find_map(|d| html::css_urls(d.value).iter().find_map(|u| ctx.bg_fixes.get(&crate::epubzip::resolve_link(base, u.value).0)));
    let single_bg = ctx.drop_single_backgrounds
        && !tiled
        && decls.iter().any(|d| {
            let v = strip_important(d.value).0.to_ascii_lowercase();
            match d.prop.to_ascii_lowercase().as_str() {
                "background-repeat" => v.split(|c: char| c.is_whitespace() || c == ',').any(|t| !t.is_empty() && t != "repeat"),
                "background" => crate::cascade::tokens(&v).0.iter().any(|t| matches!(*t, "no-repeat" | "repeat-x" | "repeat-y" | "space" | "round")),
                _ => false,
            }
        });
    for d in &decls {
        out.push_str(&text[last..d.start]);
        last = d.start + d.raw.len();
        let prop = d.prop.to_ascii_lowercase();
        let (value, imp) = strip_important(d.value);
        let lead = &d.raw[..d.raw.find(|c: char| !c.is_whitespace()).unwrap_or(0)];
        let replaced: Option<String> = match prop.as_str() {
            // 整条去掉：留下备选（「st」「宋体」）的话阅读器会去找这些系统字体，就不是阅读器自己的字体了（Kindle 写 `default` 排第一）
            "font-family" => body_font_removed(value, &ctx.body_fonts).map(|_| String::new()),
            // `font` 简写里的字体族是正文字体：拆成分项、不写字体族（字号、行高、粗斜体照原值）
            "font" => rewrite_font_shorthand(value, imp, lead, body, ctx),
            "font-size" => ctx.font_scale.and_then(|k| scaled_font_size(value, body, k)).map(|v| format!("{lead}font-size:{v}{imp};")),
            "color" => {
                has_color = true;
                parse_color(value).and_then(|col| {
                    // 规则里没写背景色时看不到祖先的背景（背景图、外层的背景色）：只调中等亮度以下的颜色，浅色（白字、浅灰）
                    // 多半压在图或深色块上，不动（Kindle 按元素算，背景图上的白字照写）
                    if !has_bg && luminance(over_white(col)) > 0.5 {
                        return None;
                    }
                    let adj = ensure_contrast(col, bg);
                    (adj != col).then(|| format!("{lead}color:{}{imp};", css_color(adj)))
                })
            }
            // body 的左右外边距、内边距（分项、简写一样）：这一侧有负外边距时照留（Kindle 把 body 这一侧的外边距加内边距照加）
            "margin-left" | "padding-left" if body && !ctx.neg_left => Some(String::new()),
            "margin-right" | "padding-right" if body && !ctx.neg_right => Some(String::new()),
            "margin" | "padding" if body => split_box(&prop, d.value, lead, ctx.neg_left, ctx.neg_right),
            // 不平铺的背景图在不认 `no-repeat` 的阅读器上去掉；简写里的背景色照留（换成分项）
            "background-image" if single_bg => Some(String::new()),
            "background" if single_bg && value.to_ascii_lowercase().contains("url(") => {
                Some(crate::cssunlock::background_color(value).map(|c| format!("{lead}background-color:{c}{imp};")).unwrap_or_default())
            }
            _ => None,
        };
        match replaced {
            Some(r) => {
                changed += 1;
                out.push_str(&r);
            }
            None => out.push_str(d.raw),
        }
    }
    out.push_str(&text[last..]);
    // 阅读器不认 `rgba()`：整条声明会作废，换成按白底合成的不透明色（掌阅）
    if ctx.opaque_rgba && out.to_ascii_lowercase().contains("rgba(") {
        let before = out.clone();
        out = crate::cssunlock::rgba_to_opaque(&before).into_owned();
        changed += before.to_ascii_lowercase().matches("rgba(").count() - out.to_ascii_lowercase().matches("rgba(").count();
    }
    // 写了深背景、没写文字颜色：缺省的黑字看不清时补上
    if !has_color && bg != 0xFFFF_FFFF && contrast(0xFF00_0000, bg) < 4.5 {
        let adj = ensure_contrast(0xFF00_0000, bg);
        if !out.trim_end().is_empty() && !out.trim_end().ends_with(';') {
            out.push(';');
        }
        out.push_str(&format!("color:{};", css_color(adj)));
        changed += 1;
    }
    if let Some(f) = fix {
        if !out.trim_end().is_empty() && !out.trim_end().ends_with(';') {
            out.push(';');
        }
        out.push_str(f);
        changed += 1;
    }
    (out, changed)
}

fn strip_important(v: &str) -> (&str, &'static str) {
    crate::cssunlock::split_important(v)
}

/// `font-family` 的第一个是正文字体时去掉它，返回剩下的备选（可能为空）；不是正文字体返回 `None`。
fn body_font_removed(value: &str, body: &HashSet<String>) -> Option<String> {
    let names: Vec<&str> = value.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let first = super::fonts::norm_family(names.first()?)?;
    body.contains(&first).then(|| names[1..].join(","))
}

/// `font` 简写里的字体族第一个是正文字体：换成分项，不写字体族（交给阅读器的字体），别的照简写的意思写全——简写会把没写的项
/// 重置成缺省值，所以没写的粗细、斜体、小型大写、行高写 `normal`（`font-stretch` 只在写了时写：认它的阅读器少）。
/// 不是正文字体、拆不清的返回 `None`（不动）。
fn font_without_body_family(f: &FontShorthand, size: &str, imp: &str, lead: &str, body: &HashSet<String>) -> Option<String> {
    if !body.contains(&super::fonts::norm_family(f.family)?) {
        return None;
    }
    let mut s = String::from(lead);
    let mut put = |prop: &str, v: &str| s.push_str(&format!("{prop}:{v}{imp};"));
    put("font-style", f.style.unwrap_or("normal"));
    put("font-variant", f.variant.unwrap_or("normal"));
    put("font-weight", f.weight.unwrap_or("normal"));
    if let Some(st) = f.stretch {
        put("font-stretch", st);
    }
    put("font-size", size);
    put("line-height", f.line_height.unwrap_or("normal"));
    Some(s)
}

/// `font` 简写：字体族是正文字体的拆成分项（[`font_without_body_family`]）；字号要归一的（[`scaled_font_size`]）换掉字号。
/// 都不用改、拆不清的返回 `None`（不动）。
fn rewrite_font_shorthand(value: &str, imp: &str, lead: &str, body: bool, ctx: &Ctx) -> Option<String> {
    let f = parse_font_shorthand(value)?;
    let scaled = ctx.font_scale.and_then(|k| scaled_font_size(f.size, body, k));
    let size = scaled.as_deref().unwrap_or(f.size);
    if let Some(s) = font_without_body_family(&f, size, imp, lead, &ctx.body_fonts) {
        return Some(s);
    }
    let new = scaled?;
    // 字号是 `value` 里的一段：原地换掉，别的照原样
    let at = f.size.as_ptr() as usize - value.as_ptr() as usize;
    Some(format!("{lead}font:{}{new}{}{imp};", &value[..at], &value[at + f.size.len()..]))
}

/// 字号归一（Send to Kindle 把全书正文字号当阅读器字号设置的 1.0，别的按比例）：每个元素的字号都乘 `k`（1 / 正文字号）。
/// 相对字号（em、%）跟着父元素走，乘在最外层就够——`body` 上的乘（没写的由 `eink-ua.css` 补一条 `body` 字号，
/// [`crate::uastyle::ua_css`]），别处不动。绝对字号（pt、rem 这些）按 CSS 的换算折成 px 再乘（`k` 是 1 也折）：掌阅的 px 是
/// 16px＝阅读器字号、不受 body 和父元素的字号影响，pt 却按 px 的数算（12pt 只有 0.75 倍），rem 跟着 body 缩（把 body 当根），
/// 只有 px 靠得住（2026-10-10 真机）。关键字（`small`、`larger`）、`calc()` 这类不动（真书很少：24 本只有 1 处 `smaller`）。
/// 不用改返回 `None`。
fn scaled_font_size(value: &str, body: bool, k: f64) -> Option<String> {
    let v = value.trim();
    let split = v.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: f64 = num.parse().ok()?;
    let px = match unit.to_ascii_lowercase().as_str() {
        "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "rem" => 16.0,
        "em" | "%" | "ex" | "ch" if body && k != 1.0 => return Some(format!("{}{unit}", fmt_num(n * k))),
        _ => return None,
    };
    let out = format!("{}px", fmt_num(n * px * k));
    (out != v).then_some(out)
}

/// 写进 CSS 的数：最多 4 位小数，去掉末尾的 0。
pub(crate) fn fmt_num(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

/// body 的 `margin`/`padding` 简写拆开，只留上下（和要留的左右）。值按 [`box_sides`] 拆（括号里的空格不算分隔：`calc(1em + 2px)`），
/// 拆不清的不动。
fn split_box(prop: &str, value: &str, lead: &str, keep_left: bool, keep_right: bool) -> Option<String> {
    let ([top, right, bottom, left], imp) = match box_sides(value) {
        BoxSides::Sides(s, imp) => (s, imp),
        BoxSides::Keyword(k, imp) => ([k; 4], imp),
        BoxSides::Unknown => return None,
    };
    let zero = |v: &str| v.trim_start_matches(['0', '.']).chars().all(|c| c.is_ascii_alphabetic() || c == '%') && v.starts_with('0');
    if zero(right) && zero(left) {
        return None;
    }
    let mut s = format!("{lead}{prop}-top:{top}{imp};{prop}-bottom:{bottom}{imp};");
    if keep_left {
        s.push_str(&format!("{prop}-left:{left}{imp};"));
    }
    if keep_right {
        s.push_str(&format!("{prop}-right:{right}{imp};"));
    }
    Some(s)
}

/// ARGB → CSS `#rrggbb`。写的都是 [`ensure_contrast`] 调出来的不透明颜色（半透明的按合成后的颜色调，结果不透明），
/// 不写 `rgba()`（掌阅不认，整条声明作废）。
fn css_color(c: u32) -> String {
    debug_assert_eq!(c >> 24, 0xFF, "只写不透明颜色");
    format!("#{:06x}", c & 0xFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx { body_fonts: ["宋体".to_string()].into_iter().collect(), body_classes: ["calibre".to_string()].into_iter().collect(), neg_left: false, neg_right: false, font_scale: None, opaque_rgba: false, drop_single_backgrounds: false, tiled: HashSet::new(), bg_fixes: HashMap::new() }
    }

    #[test]
    fn rules_like_send_to_kindle() {
        let c = ctx();
        let mut n = 0;
        let css = r#"body{margin:0 5pt;font-family:"宋体",serif} .calibre{padding-left:1em;margin-top:1em} p{font-family:"宋体"} h1{color:#fff;background-color:#f0a200} .pale{color:#f5ac00} .dark{background-color:#0168b7} .ok{color:#333}"#;
        let out = rewrite_css(css, &c, &mut n);
        assert!(out.contains("body{margin-top:0;margin-bottom:0;}"), "{out}");
        assert!(out.contains(".calibre{margin-top:1em}"), "只用在 body 上的类去掉左右：{out}");
        assert!(out.contains("p{}"), "正文字体整条去掉：{out}");
        let white = rewrite_css(".t{color:#fff}", &c, &mut n);
        assert_eq!(white, ".t{color:#fff}", "没写背景色的白字不动（多半压在背景图上）");
        assert!(out.contains("h1{color:#454545;background-color:#f0a200}") && out.contains(".pale{color:#9d6e00;}"), "{out}");
        assert!(out.contains(".dark{background-color:#0168b7;color:#e4e4e4;}") && out.contains(".ok{color:#333}"), "{out}");
        // 有负的左外边距：body 的左边距照留
        let c2 = Ctx { neg_left: true, ..ctx() };
        let out2 = rewrite_css("body{margin:0 5pt}", &c2, &mut n);
        assert_eq!(out2, "body{margin-top:0;margin-bottom:0;margin-left:5pt;}");
    }

    /// 负外边距那一侧：分项、简写的 margin 和 padding 都照留（以前简写的 padding 左右照删，和分项的 `padding-left` 不一致）。
    #[test]
    fn negative_side_kept_for_padding_shorthand_too() {
        let c = Ctx { neg_left: true, ..ctx() };
        let mut n = 0;
        assert_eq!(rewrite_css("body{padding-left:3em}", &c, &mut n), "body{padding-left:3em}");
        assert_eq!(rewrite_css("body{padding:0 3em}", &c, &mut n), "body{padding-top:0;padding-bottom:0;padding-left:3em;}");
        assert_eq!(rewrite_css("body{margin:1em 0 1em 2em}", &c, &mut n), "body{margin-top:1em;margin-bottom:1em;margin-left:2em;}");
    }

    /// 带空格的 `calc()` 不再被空格拆坏（以前 `calc(1em` `+` `2px)` 当成三个值，写出坏 CSS）。
    #[test]
    fn calc_in_box_shorthand_not_broken() {
        let c = ctx();
        let mut n = 0;
        assert_eq!(rewrite_css("body{margin:0 calc(1em + 2px)}", &c, &mut n), "body{margin-top:0;margin-bottom:0;}");
        let c2 = Ctx { neg_left: true, ..ctx() };
        assert_eq!(rewrite_css("body{margin:0 calc(1em + 2px) !important}", &c2, &mut n), "body{margin-top:0 !important;margin-bottom:0 !important;margin-left:calc(1em + 2px) !important;}");
    }

    /// `font` 简写里的正文字体：拆成分项、不写字体族；别的字体、拆不清的不动。
    #[test]
    fn body_font_in_font_shorthand() {
        let c = ctx();
        let mut n = 0;
        assert_eq!(
            rewrite_css(r#"p{font:italic bold 1.2em/1.5 "宋体", serif}"#, &c, &mut n),
            "p{font-style:italic;font-variant:normal;font-weight:bold;font-size:1.2em;line-height:1.5;}"
        );
        assert_eq!(rewrite_css(r#"p{font: 12pt 宋体 !important}"#, &c, &mut n), "p{font-style:normal !important;font-variant:normal !important;font-weight:normal !important;font-size:12pt !important;line-height:normal !important;}");
        for keep in [r#"p{font:1em "楷体"}"#, "p{font:caption}", "p{font:calc(1em + 1px) 宋体}", "p{font:inherit}", "p{font:bold 宋体}"] {
            assert_eq!(rewrite_css(keep, &c, &mut n), keep);
        }
    }

    /// 字号归一：body 上的相对、绝对字号和别处的绝对字号乘系数，别处的相对字号、关键字不动；`font` 简写里的字号同样。
    #[test]
    fn font_sizes_scaled_to_body_text() {
        let c = Ctx { font_scale: Some(0.8), ..ctx() };
        let mut n = 0;
        assert_eq!(rewrite_css("body{font-size:125%} p{font-size:15pt} h1{font-size:1.5em} .s{font-size:smaller} .r{font-size:2rem}", &c, &mut n), "body{font-size:100%;} p{font-size:16px;} h1{font-size:1.5em} .s{font-size:smaller} .r{font-size:25.6px;}");
        assert_eq!(rewrite_css(".calibre{font-size:1.25em !important}", &c, &mut n), ".calibre{font-size:1em !important;}", "只用在 body 上的类");
        assert_eq!(rewrite_css(r#"h2{font:bold 20px/1.2 "楷体"}"#, &c, &mut n), r#"h2{font:bold 16px/1.2 "楷体";}"#);
        assert_eq!(rewrite_css(r#"p{font:italic 15pt "宋体", serif}"#, &c, &mut n), "p{font-style:italic;font-variant:normal;font-weight:normal;font-size:16px;line-height:normal;}");
        assert_eq!(rewrite_css(r#"p{font:1.2em "楷体"}"#, &c, &mut n), r#"p{font:1.2em "楷体"}"#);
        // 正文已经是 1em：相对字号不动，绝对字号照样折成 px（掌阅的 pt 按 px 的数算）
        let one = Ctx { font_scale: Some(1.0), ..ctx() };
        assert_eq!(rewrite_css("body{font-size:90%} p{font-size:12pt} .a{font-size:16px} .b{font-size:0.75rem}", &one, &mut n), "body{font-size:90%} p{font-size:16px;} .a{font-size:16px} .b{font-size:12px;}");
        assert_eq!(fmt_num(100.0 / 1.15), "86.9565");
        assert_eq!(fmt_num(12.0), "12");
    }

    /// 全书正文字号按块、按字数算（行内元素改字号的字算给块），`html` 上写了字号的不归一。
    #[test]
    fn base_font_size_and_scale() {
        let e = |name: &str, data: &str| Entry { name: name.into(), data: data.as_bytes().to_vec() };
        let page = |body: &str| format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#);
        let long = "正文".repeat(50);
        let book = |css: &str| vec![e("OEBPS/s.css", css), e("OEBPS/c1.xhtml", &page(&format!(r#"<h1>标题</h1><p>{long}<span class="big">大字大字</span></p><p class="note">注释</p>"#)))];
        let base = |b: &[Entry], m: Option<&MediaEnv>| book_facts(b, m).base_font_size;
        let scale = |b: &[Entry]| font_scale(b, base(b, None));
        assert_eq!(base(&book("p{font-size:15pt} .big{font-size:3em} .note{font-size:0.5em}"), None), 1.25);
        assert_eq!(scale(&book("p{font-size:15pt}")), Some(0.8));
        assert_eq!(scale(&book("p{font-size:1em}")), Some(1.0));
        assert_eq!(scale(&book("html{font-size:80%} p{font-size:15pt}")), None, "html 上写了字号");
        // `@media` 按阅读模式的阅读范围求（同 KFX 写出器）：掌阅 1264 宽，`min-width:1200px` 成立、`min-width:2000px` 不成立；
        // 没给环境时特性条件一律不成立
        let ireader = MediaEnv { width: 1264.0, height: 1680.0, device_width: 1264.0, device_height: 1680.0, color: false };
        let css = "p{font-size:1em} @media (min-width:1200px){p{font-size:15pt}} @media (min-width:2000px){p{font-size:2em}}";
        assert_eq!(base(&book(css), Some(&ireader)), 1.25);
        assert_eq!(base(&book(css), None), 1.0);
    }

    /// 正文字体按全书层叠算（写在 `<body class>` 上、段落继承的也认；行内元素的字体算给所在的块），负外边距按块的计算值、
    /// 按文件字数取多数，body 上的类别处也用到的不算。
    #[test]
    fn book_facts_font_negative_margin_and_body_classes() {
        let e = |name: &str, data: &str| Entry { name: name.into(), data: data.as_bytes().to_vec() };
        let page = |body: &str| format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head>{body}</html>"#);
        let css = r#".song{font-family:"FZLanTingSong", serif} .kai{font-family:楷体} .hang{margin:0 1em 0 -2em} .wrap{margin-right:-5%}"#;
        let long = "正文".repeat(60);
        let book = vec![
            e("OEBPS/s.css", css),
            // 字体写在 body 的类上、段落继承；行内的楷体算给段落；用了负的左外边距
            e("OEBPS/c1.xhtml", &page(&format!(r#"<body class="song main"><p>{long}<span class="kai">楷体楷体楷体楷体楷体楷体楷体楷体楷体楷体楷体楷体</span></p><p class="hang">悬挂</p></body>"#))),
            // 字少的文件用了负的右外边距：字数不占多数，不留；`main` 在这里不在 body 上
            e("OEBPS/c2.xhtml", &page(r#"<body class="song"><div class="wrap main"><p class="kai">短短的楷体</p></div></body>"#)),
        ];
        let f = book_facts(&book, None);
        assert_eq!(f.body_font.as_deref(), Some("fzlantingsong"));
        assert_eq!(f.neg, [true, false]);
        assert_eq!(f.body_classes, HashSet::from(["song".to_string()]));
    }

    /// 改完自查：两份样式表都给正文写了字体，后链的胜出；删掉胜出的那条，先链的那条不能露出来（《嘯風山莊》）。
    /// 露出来的字体本来就生效的少数字一起换成阅读器字体；别的字体（标题的黑体）不动。
    #[test]
    fn exposed_body_font_removed_too() {
        let e = |name: &str, data: &str| Entry { name: name.into(), data: data.as_bytes().to_vec() };
        let page = |body: &str| format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="a.css"/><link rel="stylesheet" href="b.css"/></head><body>{body}</body></html>"#);
        let long = "正文".repeat(200);
        let mut book = vec![
            e("OEBPS/a.css", r#"p.p{font-family:"Light", serif} p.t{font-family:"Light"} h1{font-family:"Hei"}"#),
            e("OEBPS/b.css", r#"p.p{font-family:"DemiBold", serif}"#),
            e("OEBPS/c1.xhtml", &page(&format!(r#"<h1>标题标题</h1><p class="p">{long}</p><p class="t">年表年表</p>"#))),
        ];
        let mut rep = WashReport::default();
        apply(&mut book, &mut rep, &WashOpts::default());
        let css = |i: usize| String::from_utf8(book[i].data.clone()).unwrap();
        assert_eq!(css(1), "p.p{}", "胜出的正文字体去掉");
        assert_eq!(css(0), r#"p.p{} p.t{} h1{font-family:"Hei"}"#, "露出来的 Light 一并去掉，黑体不动");
        let after = book_facts(&book, None);
        assert_eq!(after.body_font, None, "正文按层叠算没有指定字体");
    }

    /// 不认 `rgba()` 的阅读器（掌阅）：规则、行内样式里的 `rgba()` 按白底合成不透明色（《飘》「第一卷」标题的半透明白条）；
    /// 认的（Kindle 那边不过 `kindle_rules`，Move 认）不动。
    #[test]
    fn rgba_made_opaque_only_when_reader_lacks_it() {
        let mut n = 0;
        let c = Ctx { opaque_rgba: true, ..ctx() };
        assert_eq!(rewrite_css("h1{color:#2e5b60;background-color:rgba(255,255,255,0.8)}", &c, &mut n), "h1{color:#2e5b60;background-color:#ffffff}");
        assert_eq!(rewrite_css(".x{border-color:rgba(0,0,0,0.5)}", &c, &mut n), ".x{border-color:#808080}");
        let keep = "h1{background-color:rgba(255,255,255,0.8)}";
        assert_eq!(rewrite_css(keep, &ctx(), &mut n), keep);
    }

    /// 不认 `no-repeat` 的阅读器（Move）：写了不平铺的背景图去掉（简写里的背景色照留）；本来就平铺的、认 `no-repeat` 的不动。
    #[test]
    fn single_backgrounds_dropped_when_reader_tiles() {
        let mut n = 0;
        let c = Ctx { drop_single_backgrounds: true, ..ctx() };
        assert_eq!(rewrite_css(r#"body.fen{background:url('a.jpg') no-repeat bottom center;background-size:100% auto;background-color:#fff}"#, &c, &mut n), "body.fen{background-size:100% auto;background-color:#fff}");
        assert_eq!(rewrite_css(r#".a{background:#eee url(a.png) no-repeat}"#, &c, &mut n), ".a{background-color:#eee;}");
        assert_eq!(rewrite_css(r#".b{background-image:url(b.png);background-repeat:repeat-x}"#, &c, &mut n), ".b{background-repeat:repeat-x}");
        for keep in [r#".t{background-image:url(paper.jpg)}"#, r#".t{background:url(paper.jpg) repeat}"#] {
            assert_eq!(rewrite_css(keep, &c, &mut n), keep, "本来就平铺的照留");
        }
        let keep = r#".a{background:url(a.png) no-repeat}"#;
        assert_eq!(rewrite_css(keep, &ctx(), &mut n), keep, "认 no-repeat 的阅读器不动");
        // 做成整页图块的背景图（`bgfit::plan_tiles`）：规则照留，按样式表所在位置解析 url
        let t = Ctx { tiled: ["OEBPS/Images/a.jpg".to_string()].into_iter().collect(), ..c };
        let fen = r#"body.fen{background:url('../Images/a.jpg') no-repeat bottom center;background-size:100% auto}"#;
        assert_eq!(rewrite_css_in(fen, "OEBPS/Styles/s.css", &t, &mut n), fen);
        assert_eq!(rewrite_css_in(fen, "s.css", &t, &mut n), "body.fen{background-size:100% auto}", "别的图照旧去掉");
        // 掌阅：照 Kindle 算好的像素尺寸、位置接在规则末尾（规则本身不动）
        let f = Ctx { bg_fixes: [("OEBPS/Images/a.jpg".to_string(), "background-size:1264px 932px;background-position:0px 263px;".to_string())].into_iter().collect(), ..ctx() };
        assert_eq!(rewrite_css_in(fen, "OEBPS/Styles/s.css", &f, &mut n), "body.fen{background:url('../Images/a.jpg') no-repeat bottom center;background-size:100% auto;background-size:1264px 932px;background-position:0px 263px;}");
    }

    #[test]
    fn written_colors_are_opaque_hex() {
        assert_eq!(css_color(0xFF12_AB34), "#12ab34");
        // 半透明的浅色字：按合成到背景上的颜色调，写出来是不透明的 #hex（不写 rgba：掌阅不认）
        let c = ctx();
        let mut n = 0;
        let out = rewrite_css(".x{color:rgba(0,0,0,0.4)}", &c, &mut n);
        assert!(out.starts_with(".x{color:#") && !out.contains("rgba"), "{out}");
    }

    /// 竖直间距照 Kindle 换算（掌阅的系数，「测试-竖直间距」量的）：em 乘系数，%、px、pt 折成像素，简写只换上下，别的不动。
    #[test]
    fn vertical_lengths_scaled_like_kindle() {
        let vs = VScale { em: 52.5 * (1680.0 / 1546.0) / 37.5, kindle_rem_px: 52.5, ratio: 1680.0 / 1546.0 };
        let (t, n) = scale_vertical("margin-top:1em;margin-bottom:10%;padding-top:30px;padding-bottom:24pt !important;margin-left:2em;", &vs);
        assert_eq!(t, "margin-top:1.5213em;margin-bottom:182.6px;padding-top:64.2px;padding-bottom:114.1px !important;margin-left:2em;");
        assert_eq!(n, 4);
        assert_eq!(scale_vertical("margin:1em 2em", &vs).0, "margin:1.5213em 2em 1.5213em 2em");
        for keep in ["margin:0", "margin-top:auto", "margin-top:calc(1em + 2px)", "padding:0 1em", "line-height:2em"] {
            assert_eq!(scale_vertical(keep, &vs), (keep.to_string(), 0), "{keep}");
        }
        assert!(scale_vertical_css("p{margin-top:1em;margin-bottom:1em;}\n", &vs).starts_with("p{margin-top:1.5213em;margin-bottom:1.5213em;}"));
    }
}
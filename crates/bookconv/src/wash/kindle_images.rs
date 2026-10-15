//! 图片照 Kindle 上的大小显示（只修复、照 Send to Kindle 统一的文字书，profile `image_sizing` 不是 `css` 的阅读器：掌阅、Move）。
//!
//! 2026-10-12 三台测试书（`测试-插图`）量出来，同一张图三台显示的大小按各自的规矩来：Kindle（KFX 写出器写的）不写宽度的图按原图
//! 像素显示（1 图像素＝1 屏幕像素，宽过内容区的缩到内容区），em、pt、px 写的宽度折成 em、跟着字号走，只写高的不管高；掌阅照 CSS，
//! 但 px 是屏幕像素（300px 的图 Kindle 占三分之二页宽，掌阅四分之一）；Move 只认百分比宽度，别的一律拉满一行。
//! 这里按 Kindle 写出器的规则算出每张图在 Kindle 上显示多宽，再按阅读器的规矩写成显示出来一样大的写法：
//!
//! - `percent`（Move）：除了本来就写百分比宽度的独立图片，一律写成 Kindle 上占内容区宽度的百分比；
//! - `device-px`（掌阅）：Kindle 上跟着字号走的（行内图片、em/pt/px 写的宽度）写成 em，按原图像素显示的（不写宽度的独立图片）
//!   写成百分比；本来就照 CSS 一样大的（em 写的、百分比写的）不动。
//!
//! 写法：给 `<img>` 加类 `eink-iw-…`，规则写在单独的 `eink-img.css`（挂在每章最后，选择器 `html body img.类`；不用 `!important`：
//! 掌阅不按它提优先级，带了反而输），`<img>` 行内 style 里的宽高去掉（两台都认行内样式、压过类规则，见 [`strip_inline_size`]）；
//! 它的 CSS 解析器遇到类选择器可能整张表作废（2026-09-04 诊断书），所以不往别的样式表里加。高写成 `auto`（Kindle 独立图片
//! 不管高，宽度定了按比例）。行内还是独立成块照写出器的判断（`kfx::write::parse` 的 `Doc::flush`）：段落里有字的嵌在字里；
//! 整段没有字的、所在那行（按 `<br/>` 分）没有字而且后面跟着换行的、`display:block` 的独立成块。
use super::*;
use crate::cascade::{Computed, Len, MediaEnv, Rules, Sheet};
use profile::ImageSizing;
use scraper::{ElementRef, Html, Node};
use std::collections::BTreeMap;
use std::rc::Rc;

/// 写图片尺寸规则的样式表（OPF 同目录）。
const IMG_CSS_NAME: &str = "eink-img.css";

/// 百分比折页宽按 32em（同 KFX 写出器：行内图片的百分比宽度写成这么多 em）。
use crate::kindle_model::PAGE_WIDTH_EM;

/// 一张 `<img>` 在 Kindle 上怎么排（DOM 顺序，每张都有一项）。
#[derive(Clone, Debug)]
enum Kind {
    /// 不显示的、取不到尺寸的：不动。
    Skip,
    /// 嵌在字里。
    Inline,
    /// 独立成块。
    Block,
    /// 整页单图（只有这一张图、没有字的文档：封面页、整页插图）：Kindle 缩放到满页。
    Page,
}

#[derive(Clone, Debug)]
struct Img {
    kind: Kind,
    /// 图自己的字号（根 em）。
    fs: f64,
    /// 层叠出来的宽度（独立图片用，同写出器：px 按 0.75pt）。
    css_width: Option<Len>,
    /// 宽度是 HTML 属性 `width="80%"` 写的（CSS 里没写）：Move 只认 CSS 的百分比宽度、不认这个属性，要写成类（《梁羽生》作者介绍页的图
    /// 在 Kindle 上 80%，Move 上拉满一行，文字被挤到下一页，2026-10-15）。
    attr_percent: bool,
    /// 行内图片的宽、高（同写出器：CSS 的 px 按 0.45pt，没写的用 HTML 属性）。
    box_w: Option<Len>,
    box_h: Option<Len>,
    /// 原图像素。
    dims: Option<(u32, u32)>,
}

#[derive(Default)]
struct FileImgs {
    imgs: Vec<Img>,
    /// 各块字号（千分之一根 em）的字数：全书正文字号用（同 `kindle_rules::book_facts`）。
    sizes: HashMap<i64, usize>,
}

/// 在 Kindle 上显示的宽度。
enum Shown {
    /// 跟着字号走：正文 em。
    Em(f64),
    /// 固定：占内容区宽度的比例（0–1）。
    Frac(f64),
}

pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, known: Option<&HashMap<String, (u32, u32)>>, sizing: ImageSizing, kref: KindleImageRef) {
    if sizing == ImageSizing::Css {
        return;
    }
    let css: HashMap<&str, std::sync::Arc<Rules>> = crate::kindle_model::book_css(entries, media);
    // 原始宽高：调用方读好的（流式优化时图片条目是空占位），没有的从条目字节读
    // 调用方按原书的名字读的；清洗层改过名的（《飘》`*::**.png` 这种名字规范成下划线）按改名记录换过来
    let mut dims: HashMap<&str, (u32, u32)> = HashMap::new();
    if let Some(k) = known {
        let back: HashMap<&str, &str> = rep.renamed.iter().map(|(old, new)| (new.as_str(), old.as_str())).collect();
        for e in entries.iter().filter(|e| crate::util::is_image_ext(&e.name)) {
            if let Some(d) = k.get(back.get(e.name.as_str()).copied().unwrap_or(&e.name)) {
                dims.insert(e.name.as_str(), *d);
            }
        }
    }
    for e in entries.iter().filter(|e| crate::util::is_image_ext(&e.name) && !e.data.is_empty()) {
        if let Some(d) = crate::imgopt::guard(|| image::ImageReader::new(std::io::Cursor::new(&e.data)).with_guessed_format().ok()?.into_dimensions().ok()) {
            dims.entry(e.name.as_str()).or_insert(d);
        }
    }
    let chapters = super::chapter_indices(entries);
    let per_file: Vec<FileImgs> = crate::util::par_map(&chapters, |&i| scan(&entries[i], media, &css, &dims));
    let mut sizes = HashMap::new();
    for f in &per_file {
        for (k, n) in &f.sizes {
            *sizes.entry(*k).or_default() += n;
        }
    }
    let base = crate::cascade::body_font_size(&sizes).max(1e-3);
    // 每章每张图的类（没有＝不动），用到的类和规则
    let mut rules: BTreeMap<String, String> = BTreeMap::new();
    let classes: Vec<Vec<Option<String>>> = per_file
        .iter()
        .map(|f| {
            f.imgs
                .iter()
                .map(|im| {
                    let (class, rule) = restyle(im, sizing, kref, base)?;
                    rules.entry(class.clone()).or_insert(rule);
                    Some(class)
                })
                .collect()
        })
        .collect();
    if rules.is_empty() {
        return;
    }
    let css_path = super::beside_opf(entries, IMG_CSS_NAME);
    let mut edited = 0;
    for (&i, cls) in chapters.iter().zip(&classes) {
        if cls.iter().all(Option::is_none) {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let tags: Vec<(usize, usize)> = html::tags(text).filter(|t| t.is_start() && t.is("img")).map(|t| (t.start, t.end)).collect();
        // 解析器和标签扫描数出来的 `<img>` 对不上（写坏的 HTML）：拿不准，这章不动
        if tags.len() != cls.len() {
            continue;
        }
        let edits: Vec<(usize, usize, String)> = tags.iter().zip(cls).filter_map(|(&(s, t), c)| Some((s, t, html::add_class(&strip_inline_size(&text[s..t]), c.as_deref()?)))).collect();
        edited += edits.len();
        let out = html::apply_edits(text, edits);
        let href = crate::epubzip::href_to(dir_of(&e.name), &css_path, "");
        e.data = typeset::inject_css_link(&out, &href).into_bytes();
    }
    if edited == 0 {
        return;
    }
    rep.kindle_rule_edits += edited;
    let mut sheet = String::from("/* 图片照 Kindle 上的大小显示（sheng-ren kindle_rules，见 wash/kindle_images.rs） */\n");
    for r in rules.values() {
        sheet.push_str(r);
        sheet.push('\n');
    }
    add_css_entry(entries, find_opf(entries), &css_path, &sheet, "eink-img-css");
}

/// 待走的节点：(节点, 父元素的计算值, 所在块的计算值, 在不显示的元素里)。
type Pending<'a> = (ego_tree::NodeRef<'a, Node>, Rc<Computed>, Rc<Computed>, bool);

/// 一章：每张 `<img>`（DOM 顺序）在 Kindle 上怎么排、尺寸写法，各块字号的字数。
fn scan(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<Rules>>, dims: &HashMap<&str, (u32, u32)>) -> FileImgs {
    let mut f = FileImgs::default();
    let Ok(text) = std::str::from_utf8(&e.data) else { return f };
    let html = Html::parse_document(text);
    let sheet = Sheet::for_doc(&html, &e.name, media, |path| css.get(path).cloned());
    let decls_of = |el: &ElementRef| {
        let mut d = sheet.cascade(el, &e.name);
        crate::cascade::presentational_hints(el, &mut d);
        d
    };
    // 先算全部元素的计算值（不递归：没关的标签能套上千层），记下每个元素是不是块、显示不显示
    let root = html.root_element();
    let mut comp: HashMap<ego_tree::NodeId, Rc<Computed>> = HashMap::new();
    let mut hidden: HashSet<ego_tree::NodeId> = HashSet::new();
    let mut block: HashSet<ego_tree::NodeId> = HashSet::new();
    let rc = Rc::new(Computed::derive(&Computed::root(), &decls_of(&root), "html"));
    comp.insert(root.id(), rc.clone());
    block.insert(root.id());
    let mut stack: Vec<Pending> = root.children().rev().map(|ch| (ch, rc.clone(), rc.clone(), false)).collect();
    while let Some((node, parent, blk, gone)) = stack.pop() {
        match node.value() {
            Node::Text(t) if !gone => {
                let n = t.chars().filter(|c| !c.is_whitespace()).count();
                if n > 0 {
                    *f.sizes.entry((blk.font_size * 1000.0).round() as i64).or_default() += n;
                }
            }
            Node::Element(v) => {
                let Some(el) = ElementRef::wrap(node) else { continue };
                let c = Rc::new(Computed::derive(&parent, &decls_of(&el), v.name()));
                let gone = gone || matches!(v.name(), "head" | "script" | "style" | "title") || c.display.as_deref() == Some("none");
                if gone {
                    hidden.insert(node.id());
                }
                let is_block = crate::cascade::is_block(&el, &c) || (v.name() == "img" && c.display.as_deref() == Some("block"));
                if is_block {
                    block.insert(node.id());
                }
                let blk = if is_block { c.clone() } else { blk.clone() };
                comp.insert(node.id(), c.clone());
                for ch in node.children().rev() {
                    stack.push((ch, c.clone(), blk.clone(), gone));
                }
            }
            _ => {}
        }
    }
    // 写出器把带背景、边框、内边距、阴影或宽度的块写成容器（`kfx::write::parse` 的 `container_block`）：图外面有这种块就不是整页单图
    let boxed = |id: ego_tree::NodeId| {
        comp.get(&id).is_some_and(|c| {
            c.background.is_some() || c.bg_image.is_some() || c.has_border() || c.box_shadow.is_some() || c.width.is_some() || c.padding.iter().flatten().any(|l| !matches!(l, Len::Em(n) | Len::Pt(n) | Len::Percent(n) if *n == 0.0))
        })
    };
    let mut in_box = Vec::new();
    for node in root.descendants() {
        let Some(el) = ElementRef::wrap(node).filter(|el| el.value().name() == "img") else { continue };
        in_box.push(node.ancestors().any(|a| boxed(a.id())));
        let c = comp.get(&node.id()).cloned().unwrap_or_else(|| rc.clone());
        let src = el.value().attr("src").map(|s| crate::epubzip::resolve_link(&e.name, s).0);
        let d = src.as_deref().and_then(|s| dims.get(s).copied());
        let kind = if hidden.contains(&node.id()) || d.is_none() {
            Kind::Skip
        } else if block.contains(&node.id()) {
            Kind::Block
        } else {
            classify(node, &block, &hidden)
        };
        let decls = decls_of(&el);
        let len = |k: &str| {
            let css = decls.get(k).filter(|v| v.trim() != "auto").and_then(|v| crate::cascade::parse_box_len(v));
            css.or_else(|| el.value().attr(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|n| *n > 0.0).map(|n| Len::Pt(n * 0.45)))
        };
        let attr_percent = !sheet.cascade(&el, &e.name).contains_key("width") && el.value().attr("width").is_some_and(|v| v.trim().ends_with('%'));
        f.imgs.push(Img { kind, fs: c.font_size, css_width: c.width, attr_percent, box_w: len("width"), box_h: len("height"), dims: d });
    }
    // 只有一张图、没有字的文档（封面页、整页插图）：Kindle 写成整页图片版面、缩放到满页（Move 会把小封面拉满一行宽、
    // 掌阅按原图像素显示）。写出器要图外面没有带框的容器才这样排（《疯探》封面套在 `width:100%` 的 div 里，Kindle 按原图像素显示，
    // 544×776 占 49%；《阿加莎》套在带边框的 div 里的 80% 宽的图照百分比）
    if f.sizes.is_empty() && f.imgs.iter().filter(|i| !matches!(i.kind, Kind::Skip)).count() == 1 {
        for (i, b) in f.imgs.iter_mut().zip(&in_box) {
            if matches!(i.kind, Kind::Block) && !b {
                i.kind = Kind::Page;
            }
        }
    }
    f
}

/// 不是块的 `<img>`：嵌在字里还是独立成块（同写出器 `Doc::flush`）。在它所属的那段行内内容里看——从最近的块祖先起，
/// 遇到里面套的块（写出器在那里切开）就另起一段。
fn classify(img: ego_tree::NodeRef<Node>, block: &HashSet<ego_tree::NodeId>, hidden: &HashSet<ego_tree::NodeId>) -> Kind {
    let Some(anc) = img.ancestors().find(|a| block.contains(&a.id())) else { return Kind::Inline };
    // 记号：t＝字、n＝换行、i＝本图、o＝别的图；遇到套的块清空（到本图以后遇到就停）
    let mut seq: Vec<char> = Vec::new();
    let mut found = false;
    let mut it = anc.traverse();
    while let Some(edge) = it.next() {
        let ego_tree::iter::Edge::Open(n) = edge else { continue };
        if n.id() == anc.id() {
            continue;
        }
        if hidden.contains(&n.id()) || block.contains(&n.id()) {
            if found {
                break;
            }
            seq.clear();
            // 跳过整个子树
            for e in it.by_ref() {
                if matches!(e, ego_tree::iter::Edge::Close(c) if c.id() == n.id()) {
                    break;
                }
            }
            continue;
        }
        match n.value() {
            Node::Text(t) if t.chars().any(|c| !c.is_whitespace()) => seq.push('t'),
            Node::Element(v) if v.name() == "br" => seq.push('n'),
            Node::Element(v) if v.name() == "img" => {
                if n.id() == img.id() {
                    found = true;
                    seq.push('i');
                } else {
                    seq.push('o');
                }
            }
            _ => {}
        }
    }
    let at = seq.iter().position(|c| *c == 'i').unwrap_or(0);
    let line_start = seq[..at].iter().rposition(|c| *c == 'n').map_or(0, |p| p + 1);
    let line_end = seq[at..].iter().position(|c| *c == 'n').map(|p| at + p);
    let line_has_text = seq[line_start..line_end.unwrap_or(seq.len())].contains(&'t');
    if line_has_text {
        Kind::Inline
    } else if !seq.contains(&'t') || line_end.is_some() {
        Kind::Block
    } else {
        Kind::Inline
    }
}

/// 一张图要加的类和它的规则；不用改的 `None`。
fn restyle(im: &Img, sizing: ImageSizing, k: KindleImageRef, base: f64) -> Option<(String, String)> {
    let (iw, ih) = im.dims?;
    let (iw, ih) = (f64::from(iw.max(1)), f64::from(ih.max(1)));
    let em_of = |l: Len| match l {
        Len::Percent(p) => p / 100.0 * PAGE_WIDTH_EM,
        Len::Em(n) => n * im.fs,
        Len::Pt(n) => n / crate::cascade::PT_PER_EM,
    };
    // Kindle 上显示的宽度，以及阅读器照原写法会不会显示得一样大（同写出器：行内图片 `inline_image_props`，独立图片 `width_value`）
    let (shown, same) = match im.kind {
        Kind::Skip => return None,
        Kind::Page => (Shown::Frac(1.0), false),
        Kind::Inline => {
            let w = match (im.box_w.map(em_of), im.box_h.map(em_of)) {
                (Some(w), _) => w,
                (None, Some(h)) => h * iw / ih,
                (None, None) => iw / 16.0,
            };
            // 掌阅照 CSS：em 写的（含只写高）和 Kindle 一样；px、pt、没写尺寸的不一样
            let em_only = (im.box_w.is_some() || im.box_h.is_some()) && [im.box_w, im.box_h].iter().flatten().all(|l| matches!(l, Len::Em(_) | Len::Percent(_)));
            (Shown::Em(w / base), em_only)
        }
        Kind::Block => match im.css_width {
            // 百分比照写；只有比一页还高、Kindle 按一页高缩小了的（《金庸》竖长插图 100% 宽在 Kindle 上只占 84%–89%）写成缩小后的比例
            Some(Len::Percent(p)) => {
                let fit = k.content_h * iw / ih / k.content_px;
                if p / 100.0 <= fit + 0.005 && !(im.attr_percent && sizing == ImageSizing::Percent) {
                    return None;
                }
                (Shown::Frac(p / 100.0), false)
            }
            Some(l @ (Len::Em(_) | Len::Pt(_))) => (Shown::Em(em_of(l) / base), matches!(l, Len::Em(_))),
            None => (Shown::Frac((iw / k.content_px).min(1.0)), false),
        },
    };
    // 比一页还高的图 Kindle 保持比例缩到一页高（测试书 600×2000：42%）
    let fit = k.content_h * iw / ih / k.content_px;
    let pct = |s: &Shown| match *s {
        Shown::Em(e) => (e * k.em_px / k.content_px).min(1.0).min(fit) * 100.0,
        Shown::Frac(f) => f.min(fit) * 100.0,
    };
    let (name, value) = match (sizing, &shown) {
        (ImageSizing::Css, _) => return None,
        (ImageSizing::DevicePx, _) if same => return None,
        (ImageSizing::DevicePx, Shown::Em(e)) => {
            // 写在 `<img>` 上的 em 相对它自己的字号
            let v = e * base / im.fs.max(1e-3);
            (format!("{}em", num(v, 2)), format!("{}em", num(v, 2)))
        }
        (ImageSizing::Percent, _) | (ImageSizing::DevicePx, Shown::Frac(_)) => {
            let p = pct(&shown);
            (format!("{}p", num(p, 1)), format!("{}%", num(p, 1)))
        }
    };
    let class = format!("eink-iw-{}", name.replace('.', "-"));
    // em 写的宽度同 Kindle 限在一行宽以内（写出器给 em 宽度加最大宽度 100%）。不写 `!important`：掌阅不按它提优先级，带了反而输给
    // 前面样式表里的普通声明（2026-10-12 掌阅诊断书）；靠挂在最后、选择器 `html body img.类`（0,1,3）压过书里 `div.logo img` 这类
    let cap = if value.ends_with("em") { ";max-width:100%" } else { "" };
    let rule = format!("html body img.{class}{{width:{value};height:auto{cap}}}");
    Some((class, rule))
}

/// `<img>` 行内 `style` 里的宽高（`width`、`height`、`max-*`、`min-*`）去掉：两台都认行内样式、压过类规则（Move 2026-10-13 探针，
/// 以前以为 Move 不认），留着的话类规则写的 Kindle 尺寸不起作用（《克莱因壶》`style="width:12em"` 的图 Move 拉满一行、Kindle 42%；
/// 《狼厅》`style="width:769px;max-width:428px…"` 掌阅按屏幕像素显示 66%、Kindle 满宽，2026-10-14 `unify.py`）。尺寸照 Kindle 算时已经
/// 按层叠算进了行内的值。
fn strip_inline_size(tag: &str) -> String {
    let Some(style) = html::attr_value(tag, "style").map(|s| crate::util::xml_unescape(s).into_owned()) else { return tag.to_string() };
    let sized = |p: &str| matches!(p, "width" | "height" | "max-width" | "max-height" | "min-width" | "min-height");
    let decls = html::css_decls(&style);
    if !decls.iter().any(|d| sized(&d.prop.to_ascii_lowercase())) {
        return tag.to_string();
    }
    let kept: String = decls.iter().filter(|d| !sized(&d.prop.to_ascii_lowercase())).map(|d| d.raw).collect();
    if kept.trim().is_empty() {
        html::remove_attr(tag, "style")
    } else {
        html::set_attr(tag, "style", &crate::util::xml_escape(kept.trim()))
    }
}

/// 数字写法：保留 `d` 位小数，去掉末尾的 0。
fn num(v: f64, d: usize) -> String {
    let s = format!("{v:.d$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KREF: KindleImageRef = KindleImageRef { content_px: 1104.0, content_h: 1546.0, em_px: 39.0, v_rem_px: 52.5 };

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        image::GrayImage::new(w, h).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Jpeg).unwrap();
        v
    }

    /// 测试书 `测试-插图` 的几种写法：返回 (每张图加的类, eink-img.css)。
    fn run(sizing: ImageSizing, body: &str, css: &str) -> (Vec<String>, String) {
        let page = format!(r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#);
        let mut entries = vec![
            Entry { name: "O/c.opf".into(), data: br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="t" href="t.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="t"/></spine></package>"#.to_vec() },
            Entry { name: "O/t.xhtml".into(), data: page.into_bytes() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/s.jpg".into(), data: jpeg(300, 150) },
            Entry { name: "O/m.jpg".into(), data: jpeg(800, 400) },
        ];
        let mut rep = WashReport::default();
        apply(&mut entries, &mut rep, None, None, sizing, KREF);
        let t = String::from_utf8(entries[1].data.clone()).unwrap();
        let classes = html::tags(&t).filter(|g| g.is_start() && g.is("img")).map(|g| html::attr_value(&t[g.start..g.end], "class").unwrap_or("").to_string()).collect();
        let sheet = entries.iter().find(|e| e.name == "O/eink-img.css").map(|e| String::from_utf8(e.data.clone()).unwrap()).unwrap_or_default();
        assert!(sheet.is_empty() || t.contains("eink-img.css"), "改了图的章挂上样式表");
        (classes, sheet)
    }

    const BODY: &str = r#"<div><img src="s.jpg"/></div><div><img class="w50" src="m.jpg"/></div><div><img class="px" src="m.jpg"/></div><div><img class="em" src="m.jpg"/></div><p>字<img class="ic" src="s.jpg"/>字</p><p>字<img src="s.jpg"/>字</p>"#;
    const CSS: &str = "img.w50{width:50%} img.px{width:300px} img.em{width:10em} img.ic{width:1em}";

    /// 加了类的图，行内 style 里的宽高去掉（两台都认行内样式、压过类规则），别的行内声明留着；没加类的不动。
    #[test]
    fn inline_size_dropped_when_classed() {
        let page = format!(r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head><body>{}</body></html>"#,
            r#"<div><img style="width:12em;border:0" src="m.jpg"/></div><div><img style="max-width:428px;width:769px;height:1106px" src="m.jpg"/></div>"#);
        let mut entries = vec![
            Entry { name: "O/c.opf".into(), data: br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="t" href="t.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="t"/></spine></package>"#.to_vec() },
            Entry { name: "O/t.xhtml".into(), data: page.into_bytes() },
            Entry { name: "O/s.css".into(), data: Vec::new() },
            Entry { name: "O/m.jpg".into(), data: jpeg(800, 400) },
        ];
        apply(&mut entries, &mut WashReport::default(), None, None, ImageSizing::Percent, KREF);
        let t = String::from_utf8(entries[1].data.clone()).unwrap();
        assert!(t.contains(r#"<img style="border:0" src="m.jpg" class="eink-iw-"#), "{t}");
        assert!(t.contains(r#"<div><img src="m.jpg" class="eink-iw-"#), "{t}");
    }

    /// Move 只认百分比：独立图片按 Kindle 的大小（原图 300px＝27.2%；300px＝18.75em＝66.2%；10em＝35.3%）、行内的同样折成百分比；
    /// 本来就写百分比的不动。
    #[test]
    fn percent_reader_gets_kindle_fractions() {
        let (c, sheet) = run(ImageSizing::Percent, BODY, CSS);
        assert_eq!(c, ["eink-iw-27-2p", "w50", "px eink-iw-66-2p", "em eink-iw-35-3p", "ic eink-iw-3-5p", "eink-iw-66-2p"]);
        assert!(sheet.contains("html body img.eink-iw-27-2p{width:27.2%;height:auto}"), "{sheet}");
    }

    /// HTML 属性 `width="50%"`（CSS 里没写）：Move 不认属性，写成类；CSS 写的百分比不动。
    #[test]
    fn attribute_percent_becomes_class_for_percent_reader() {
        let (c, sheet) = run(ImageSizing::Percent, r#"<div><img width="50%" src="m.jpg"/></div><div><img class="w50" width="30%" src="m.jpg"/></div>"#, CSS);
        assert_eq!(c, ["eink-iw-50p", "w50"]);
        assert!(sheet.contains("html body img.eink-iw-50p{width:50%;height:auto}"), "{sheet}");
        let (c, _) = run(ImageSizing::DevicePx, r#"<p>字</p><div><img width="50%" src="m.jpg"/></div>"#, CSS);
        assert_eq!(c, [""], "掌阅的属性认不认没量过，先不动");
    }

    /// 掌阅照 CSS、px 是屏幕像素：em 写的不动；px 写的折成 Kindle 的 em；不写宽度的独立图片按原图像素折成百分比；
    /// 没写尺寸的行内小图照 Kindle 的 1/16em 一像素。
    #[test]
    fn device_px_reader_only_fixes_px_and_unsized() {
        let (c, _) = run(ImageSizing::DevicePx, BODY, CSS);
        assert_eq!(c, ["eink-iw-27-2p", "w50", "px eink-iw-18-75em", "em", "ic", "eink-iw-18-75em"]);
    }

    /// 行内还是独立成块同写出器：图后换行接图注的独立成块（按原图像素），段末单独一行的小图嵌在字里。
    #[test]
    fn line_rule_matches_writer() {
        let (c, _) = run(ImageSizing::Percent, r#"<p><img src="s.jpg"/><br/>图注</p><p>第一行<br/><img src="s.jpg"/></p>"#, "");
        assert_eq!(c, ["eink-iw-27-2p", "eink-iw-66-2p"]);
    }
}

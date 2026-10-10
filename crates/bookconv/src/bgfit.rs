//! 整页背景图按原书的尺寸意图预先缩好（去掉 `background-size` 的阅读模式用，profile `background_sizing = false`，掌阅）。
//!
//! 掌阅写了 `background-size` 会把图挤变形，所以清洗层把尺寸去掉了（见 `cssunlock::background_longhands`）；去掉后图按
//! 自身像素大小显示，2160×3124 的图在 1264×1680 的屏上只露出左上一块。这里按原书写的尺寸先把图缩成阅读范围里该有的大小：
//!
//! | 原书写法 | 缩成 |
//! |---|---|
//! | `cover` | 等比缩到刚好盖满阅读范围（宽高两个比例取大者） |
//! | `contain` | 等比缩进阅读范围（取小者） |
//! | `100%`、`100% auto` | 宽撑满阅读范围的宽 |
//! | 没写尺寸、`no-repeat` | 等比缩进阅读范围（取小者） |
//!
//! **最后都不超出阅读范围**（再和"缩进阅读范围"取小者）：图有一截显示不出来，等于原书内容看不见（用户 2026-10-06：不删原书
//! 任何内容）。所以 `cover` 实际效果和 `contain` 一样——整张图都看得见，最多一边留白；《雪国》封底 1080×2400 缩成 756×1680。
//!
//! **只缩不放**：比目标小的图不动——PNG 放大只会糊、文件变大，而且掌阅按图自身像素显示，放大后的像素也不会比原图多出细节；
//! 原书意图（宽 100%）在掌阅上本来就做不到，宁可小一点也不改原画。
//!
//! 只认**整页背景**：规则的选择器是 `body`/`html`（可带类、id），值里只有一张图。同一张图还被 `<img>`、SVG、别的规则
//! （尺寸意图不同的、不是整页的、`border-image` 之类）引用，或是封面图时不处理（拿不准就不处理；复制一份要改 manifest 和
//! 样式表，为少见情况不值得）。内联 `style=""` 里的背景图也不处理。

use std::collections::HashMap;

/// 原书的尺寸意图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgFit {
    /// `cover`：盖满阅读范围。
    Cover,
    /// `contain`、没写尺寸：缩进阅读范围。
    Contain,
    /// 宽 `100%`：宽撑满阅读范围。
    Width,
}

impl BgFit {
    /// `w × h` 的图在 `area` 里该缩成的尺寸；不用缩（目标不比原图小）→ `None`。
    pub fn target(self, w: u32, h: u32, area: crate::imgopt::Screen) -> Option<(u32, u32)> {
        let (sw, sh) = (area.width as f64 / w as f64, area.height as f64 / h as f64);
        let s = match self {
            BgFit::Cover => sw.max(sh),
            BgFit::Contain => sw.min(sh),
            BgFit::Width => sw,
        }
        .min(sw.min(sh)); // 不超出阅读范围：超出的那截显示不出来

        if s >= 1.0 {
            return None;
        }
        let (nw, nh) = (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1));
        ((nw, nh) != (w, h)).then_some((nw, nh))
    }
}

/// 一张图被引用的情况：`Some(意图)`＝目前只当这种整页背景用；`None`＝有拿不准的用法，不处理。
type Uses = HashMap<String, Option<BgFit>>;

fn note(uses: &mut Uses, path: String, fit: Option<BgFit>) {
    uses.entry(path).and_modify(|f| if *f != fit { *f = None }).or_insert(fit);
}

/// 扫全书（样式表、`<style>`、各种引用），返回能按意图缩放的整页背景图（zip 路径 → 意图）。
/// 要在清洗之前调用（清洗会去掉 `background-size`）；清洗改了名的图由调用方换成新名。
pub fn plan(entries: &[crate::epubzip::Entry]) -> HashMap<String, BgFit> {
    // 各条目独立扫出 (图, 意图) 的记录（多线程，`util::par_map`），再按条目顺序记进去（结果其实与顺序无关：同一张图的记录全一样才留意图）
    let per_entry = crate::util::par_map(entries, |e| {
        let mut notes: Vec<(String, Option<BgFit>)> = Vec::new();
        let Ok(text) = std::str::from_utf8(&e.data) else { return notes };
        if e.name.to_ascii_lowercase().ends_with(".css") {
            scan_css(text, &e.name, &mut notes);
        } else if crate::epubzip::is_html_entry(&e.name, &e.data) {
            for c in crate::html::style_block_re().captures_iter(text) {
                scan_css(&c[2], &e.name, &mut notes);
            }
            // `<img src>`、SVG `<image xlink:href>`、链接：都算别的用法
            for v in crate::html::link_values(text) {
                if !crate::html::is_external(v) {
                    notes.push((crate::epubzip::resolve_link(&e.name, v).0, None));
                }
            }
            // 内联样式里的 `url()`：不处理
            for t in crate::html::tags(text).filter(|t| t.is_start()) {
                if let Some(s) = crate::html::attr_value(&text[t.start..t.end], "style") {
                    for u in urls(s) {
                        notes.push((crate::epubzip::resolve_link(&e.name, &u).0, None));
                    }
                }
            }
        } else if e.name.to_ascii_lowercase().ends_with(".opf") {
            if let Some(c) = crate::wash::opf::declared_cover(text) {
                notes.push((crate::epubzip::resolve_link(&e.name, c.href).0, None));
            }
        }
        notes
    });
    let mut uses: Uses = HashMap::new();
    for (path, fit) in per_entry.into_iter().flatten() {
        note(&mut uses, path, fit);
    }
    uses.into_iter().filter_map(|(p, f)| f.map(|f| (p, f))).collect()
}

/// 样式表里每条规则的 `url()`：整页背景的记下意图，别的（`@font-face` 里的字体也在内，反正不是图）记成拿不准。
fn scan_css(css: &str, base: &str, notes: &mut Vec<(String, Option<BgFit>)>) {
    for c in crate::wash::css_rule_re().captures_iter(css) {
        let sel = crate::wash::rule_selector(&c[1]);
        let decls = crate::html::css_decls(&c[2]);
        let fit = rule_fit(&sel, &decls);
        for d in &decls {
            let p = d.prop.to_ascii_lowercase();
            let is_bg = p == "background" || p == "background-image";
            for u in urls(d.value) {
                if crate::html::is_external(&u) {
                    continue;
                }
                notes.push((crate::epubzip::resolve_link(base, &u).0, if is_bg { fit } else { None }));
            }
        }
    }
}

/// 一条规则的整页背景意图：选择器都是 `body`/`html`、只有一张背景图、尺寸认得出（没写尺寸的还要 `no-repeat`）。否则 `None`。
fn rule_fit(selector: &str, decls: &[crate::html::CssDecl]) -> Option<BgFit> {
    if !selector.split(',').all(is_page_selector) {
        return None;
    }
    let (mut images, mut repeat, mut size) = (0usize, None::<String>, None::<String>);
    for d in decls {
        let p = d.prop.to_ascii_lowercase();
        let longhands = match p.as_str() {
            "background" => crate::cssunlock::background_longhands(d.value, true),
            "background-image" | "background-repeat" | "background-size" => vec![format!("{p}:{}", d.value)],
            _ => continue,
        };
        for l in longhands {
            let Some((prop, val)) = l.split_once(':') else { continue };
            let val = crate::cssunlock::split_important(val).0.trim().to_ascii_lowercase();
            match prop {
                // 多层背景（逗号隔开）、渐变：拿不准
                "background-image" if val.contains(',') || !val.starts_with("url(") => return None,
                "background-image" => images += 1,
                "background-repeat" => repeat = Some(val),
                "background-size" => size = Some(val),
                _ => {}
            }
        }
    }
    if images != 1 {
        return None;
    }
    let size = size.unwrap_or_default();
    let toks: Vec<&str> = size.split_whitespace().collect();
    match toks.as_slice() {
        ["cover"] => Some(BgFit::Cover),
        ["contain"] => Some(BgFit::Contain),
        ["100%"] | ["100%", "auto"] => Some(BgFit::Width),
        [] | ["auto"] | ["auto", "auto"] if repeat.as_deref() == Some("no-repeat") => Some(BgFit::Contain),
        _ => None,
    }
}

/// 选择器（逗号分开的一支）最后一段是 `body`/`html` 本身（可带 `.类`、`#id`、属性、伪类）。
fn is_page_selector(sel: &str) -> bool {
    let last = sel.trim().rsplit(|c: char| c.is_whitespace() || c == '>' || c == '+' || c == '~').next().unwrap_or("").to_ascii_lowercase();
    ["body", "html"].iter().any(|t| last.strip_prefix(t).is_some_and(|r| r.is_empty() || r.starts_with(['.', '#', '[', ':'])))
}

/// 值里每个 `url(…)` 的地址（去掉引号）。
pub(crate) fn urls(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = value.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("url(") {
        let s = from + i + 4;
        let Some(j) = value[s..].find(')') else { break };
        let u = value[s..s + j].trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if !u.is_empty() {
            out.push(u.to_string());
        }
        from = s + j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epubzip::Entry;

    fn e(name: &str, s: &str) -> Entry {
        Entry { name: name.into(), data: s.as_bytes().to_vec() }
    }

    fn area() -> crate::imgopt::Screen {
        crate::imgopt::Screen { width: 1264, height: 1680 }
    }

    #[test]
    fn plan_reads_intent_from_page_rules() {
        let css = r#"body.bq{background:url("../Images/bq.png") bottom / 100% no-repeat fixed rgba(117, 0, 0, 1)}
            body.juan{background:url("../Images/juan.png") no-repeat fixed rgba(17, 17, 17, 1)}
            /* 注释 */ body.jsy{background:url("../Images/jsy.png") bottom / cover no-repeat fixed #700}
            html{background-image:url(../Images/c.png);background-size:contain}
            body.tile{background:url(../Images/tile.png)}
            div.box{background:url(../Images/box.png) / cover no-repeat}
            body.two{background:url(../Images/a.png), url(../Images/b.png)}
            blockquote{border-image:url("../Images/letter.png") 49% stretch}"#;
        let p = plan(&[e("OEBPS/Styles/s.css", css)]);
        let g = |n: &str| p.get(&format!("OEBPS/Images/{n}")).copied();
        assert_eq!(g("bq.png"), Some(BgFit::Width));
        assert_eq!(g("juan.png"), Some(BgFit::Contain), "没写尺寸、no-repeat、body：缩进阅读范围");
        assert_eq!(g("jsy.png"), Some(BgFit::Cover));
        assert_eq!(g("c.png"), Some(BgFit::Contain));
        assert_eq!(g("tile.png"), None, "没写尺寸又平铺的不动");
        assert_eq!(g("box.png"), None, "不是整页背景不动");
        assert_eq!((g("a.png"), g("b.png")), (None, None), "多层背景不动");
        assert_eq!(g("letter.png"), None);
    }

    #[test]
    fn shared_or_conflicting_images_are_left_alone() {
        let css = "body.a{background:url(i/x.png) / cover no-repeat} body.b{background:url(i/x.png) / 100% no-repeat}
            body.c{background:url(i/y.png) / cover} body.d{background:url(i/z.png) / cover} body.e{background:url(i/w.png) / cover}
            body.f{background:url(i/v.png) / cover} body.g{background:url(i/v.png) / cover}";
        let html = r#"<html><head><style>body.h{background:url(i/u.png) / cover}</style></head><body><img src="i/y.png"/><svg><image xlink:href="i/z.png"/></svg><p style="background:url(i/w.png)">字</p></body></html>"#;
        let opf = r#"<package><manifest><item id="u" href="i/u.png" media-type="image/png" properties="cover-image"/></manifest></package>"#;
        let p = plan(&[e("s.css", css), e("c.xhtml", html), e("content.opf", opf)]);
        assert_eq!(p.get("i/x.png"), None, "两条规则意图不同");
        assert_eq!(p.get("i/y.png"), None, "也被 <img> 用");
        assert_eq!(p.get("i/z.png"), None, "也被 SVG 用");
        assert_eq!(p.get("i/w.png"), None, "也被内联样式用");
        assert_eq!(p.get("i/u.png"), None, "是封面图");
        assert_eq!(p.get("i/v.png"), Some(&BgFit::Cover), "两条规则意图相同照样处理");
    }

    #[test]
    fn targets_follow_intent_and_never_upscale() {
        // 《绍宋》的三张图在掌阅阅读范围里
        assert_eq!(BgFit::Cover.target(2400, 3200, area()), Some((1260, 1680)), "cover 也不超出阅读范围：整张图看得见");
        assert_eq!(BgFit::Cover.target(1080, 2400, area()), Some((756, 1680)), "《雪国》封底：高出阅读范围的要缩，不能裁掉上半截");
        assert_eq!(BgFit::Contain.target(2160, 3124, area()), Some((1162, 1680)));
        assert_eq!(BgFit::Width.target(1080, 1562, area()), None, "比阅读范围窄：不放大");
        assert_eq!(BgFit::Width.target(2000, 1000, area()), Some((1264, 632)));
        assert_eq!(BgFit::Contain.target(1264, 1680, area()), None);
        assert_eq!(BgFit::Cover.target(1000, 1500, area()), None, "盖满要放大：不动");
        assert_eq!(BgFit::Width.target(2000, 4000, area()), Some((840, 1680)), "宽撑满后高出阅读范围：再缩进去");
    }
}

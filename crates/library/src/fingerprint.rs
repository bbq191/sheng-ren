//! 产物指纹里「设备（阅读模式）」那几段：profile 里哪些字段影响产物，就按什么写法进指纹。
//!
//! 指纹没变就不重新生成、不重传，所以这里是两个方向的同一个风险：影响产物的字段漏进指纹＝改了规则书不重建；
//! 不影响产物的字段进了指纹＝白白重传全部书。**写法（字母、顺序）一个字都不能改**——改了所有书库的书都会过期；
//! `tests/fp_golden.rs` 把内置三个模式逐字段改一遍的指纹和 `fp_golden.txt` 对拍。
//!
//! 给 profile 加了字段，先看 `profile` crate 的 `every_field_has_a_fingerprint_decision`（逼着决定它进不进指纹），
//! 进的话在这里加一段，再重跑黄金对拍确认旧模式的指纹没变（新字段取缺省值时不能出新字母）。

use profile::{Format, ImageSizing, NoteIcons, Notes, Profile, Screen};

/// 设备相关的几段指纹（[`device_sig`]）。
pub(crate) struct DeviceSig {
    /// 这一路（文字书/漫画）的处理规则版本，放在指纹的版本位置。
    pub version: String,
    /// 阅读范围、漫画画布、各种只管这一路的 profile 字段。
    pub area_seg: String,
    /// 注释呈现方式（两路都带）。
    pub notes: String,
}

pub(crate) fn device_sig(device: &Profile, area: Screen, comic: bool) -> DeviceSig {
    let notes = notes_seg(device);
    // 文字书、漫画分开算（2026-10-08）：只影响另一路的版本号、profile 字段变了，这一路不过期
    let (version, area_seg) = if comic {
        (format!("c{}", bookconv::optimize::COMIC_VERSION), comic_seg(device, area))
    } else {
        (bookconv::optimize::OPTIMIZE_VERSION.to_string(), text_seg(device, area))
    };
    DeviceSig { version, area_seg, notes }
}

/// 注释呈现方式（profile 的 notes：弹窗/跳转）；图标注释号换数字（`note_icons = "number"`）带 `#`，保留回链（`note_backlinks`，缺省保留）带 `<`。
fn notes_seg(device: &Profile) -> String {
    let mut notes = match device.notes.mode {
        Notes::Popup => "popup",
        Notes::Jump => "jump",
    }
    .to_string();
    if device.notes.icons == NoteIcons::Number {
        notes.push('#');
    }
    if device.notes.backlinks {
        notes.push('<');
    }
    notes
}

/// 两路都过清洗层：背景图（`b` 保留且保留尺寸、`bn` 保留但去尺寸）、阅读器不认 `rgba()`（`r`）。
fn shared_seg(device: &Profile) -> String {
    let mut s = match (device.background_images, device.background_sizing) {
        (true, true) => "b".to_string(),
        (true, false) => "bn".to_string(),
        _ => String::new(),
    };
    if !device.css.css_rgba {
        s.push('r');
    }
    // 书架缩略图（`t` + 宽高比，2026-10-30）：不做的模式不出新字母
    if let Some(a) = device.cover_thumb_aspect {
        s.push_str(&format!("t{a}"));
    }
    s
}

/// 漫画：EPUB 阅读范围（漫画书里别的图按它缩）+ 漫画画布 + 白边 + 页边距（`m`）+ 翻页方向改写（`d`）+ 固定版式（`f`），
/// 两路共用的那段之后是中间调加深（`G` + 伽马，[`gamma_seg`]），再后是拉满对比度（`A`，profile `comic_autocontrast`）、裁白边不让
/// 杂点挡住（`S`，`comic_trim_specks`）、裁页码（`P`，`comic_page_numbers`）、小页不放大（`N`，`comic_upscale = false`）；不开的模式不出新字母，指纹和以前一样。
fn comic_seg(device: &Profile, area: Screen) -> String {
    let c = device.comic_readable();
    let mut seg = format!("{}x{}c{}x{}+{}{}", area.width, area.height, c.width, c.height, device.comic.margin, device.comic.reader_margins.map(|m| format!("m{m}")).unwrap_or_default());
    if let Some(d) = &device.comic.page_direction {
        seg.push_str(&format!("d{d}"));
    }
    if device.comic.fixed_layout {
        seg.push('f');
    }
    seg.push_str(&shared_seg(device));
    seg.push_str(&gamma_seg(device.comic.gamma));
    for (on, c) in [(device.comic.autocontrast, 'A'), (device.comic.trim_specks, 'S'), (device.comic.page_numbers, 'P'), (!device.comic.upscale, 'N')] {
        if on {
            seg.push(c);
        }
    }
    seg
}

/// 中间调加深（profile `comic_gamma`、`image_gamma`，2026-10-16）：开着、又不是 1 时写 `G` + 伽马，放在这一路的最后（不加深的模式
/// 不出新字母，指纹和以前一样）。
fn gamma_seg(gamma: Option<f64>) -> String {
    gamma.filter(|&g| g != 1.0).map_or_else(String::new, |g| format!("G{g}"))
}

/// 文字书：EPUB 阅读范围 + 只管文字书的字段：带图注的竖长图写宽度（`k`）、正文图片透明处合成白底（`a`，`image_alpha = false`）、
/// 只修复（`t`）、注释可跳（`n`）、照 Send to Kindle 的规则统一（`u`）；`u` 之后还有只在 `kindle_rules` 时才有的几段（[`kindle_rules_seg`]），
/// 然后是插图中间调加深（[`gamma_seg`]），最后是 WebP 转 PNG（`w`，`webp = false`）。
fn text_seg(device: &Profile, area: Screen) -> String {
    let repair = device.text_repair_only;
    let mut seg = format!("{}x{}{}", area.width, area.height, shared_seg(device));
    for (on, c) in [(device.caption_fit, 'k'), (!device.image_alpha, 'a'), (repair, 't'), (repair && device.repair_note_links, 'n'), (repair && device.kindle_rules, 'u')] {
        if on {
            seg.push(c);
        }
    }
    if repair && device.kindle_rules {
        kindle_rules_seg(device, &mut seg);
    }
    seg.push_str(&gamma_seg(device.image_gamma));
    // 不认 WebP、WebP 转成 PNG（profile `webp = false`，Move）：放在最后，认 WebP 的模式指纹和以前一样
    if !device.webp {
        seg.push('w');
    }
    seg
}

/// `kindle_rules` 的文字书才有的几段（按顺序）：规则版本（第 1 版只写 `u`）、`g`（阅读器不认背景图的 `no-repeat`）、
/// `h`（背景尺寸的 auto 高会拉长图）、`p`（不认页首的 margin-top）、
/// `i`（不照 CSS 定图片尺寸：带方式和参照设备的内容区宽、1em 像素，改了参照设备这两个量，掌阅、Move 的书也要重建）、
/// `m`（阅读器模型 [`bookconv::reader_model::ReaderModel`] 的签名加 `em_px` 的哈希，2026-10-13 定进指纹）、
/// `@`（统计正文字号求 `@media` 用的屏幕，自定义模式改了屏幕也要重建）。
fn kindle_rules_seg(device: &Profile, seg: &mut String) {
    if bookconv::wash::KINDLE_RULES_VERSION != "1" {
        seg.push_str(bookconv::wash::KINDLE_RULES_VERSION);
    }
    if !device.css.background_no_repeat {
        seg.push('g');
    }
    // 2026-10-15 补的缺口（以前改了产物会变、指纹不变）：背景尺寸的 auto 高会拉长图的阅读器（`background_auto_height = false`，
    // 整页背景图照 Kindle 写成像素，`bg_place`）带 `h`；不认页首 margin-top 的阅读器（`margin_top = false`，页首垫图，`top_spacer`）带 `p`
    if !device.css.background_auto_height {
        seg.push('h');
    }
    if !device.css.margin_top {
        seg.push('p');
    }
    if device.image_sizing != ImageSizing::Css {
        let k = profile::reference();
        seg.push_str(&format!("i{:?}{}x{}", device.image_sizing, k.map_or(0, |k| k.readable(Format::Kfx).width), k.and_then(|k| k.font_px).unwrap_or(0)));
    }
    let sig = bookconv::reader_model::ReaderModel::for_profile(device).signature();
    if !sig.is_empty() || device.em_px.is_some() {
        let em = device.em_px.map_or_else(String::new, |e| format!("e{e}"));
        seg.push_str(&format!("m{:016x}", bookconv::util::fnv64(format!("{sig}{em}").as_bytes())));
    }
    seg.push_str(&format!("@{}x{}", device.screen.width, device.screen.height));
}

/// 2026-10-08 以前的写法（文字书、漫画不分，一个版本号、profile 的字段全带上）里设备那一段。生成记录里存的是它的，说明规则没变、
/// 只是指纹写法改了，记录改成新写法、不重新生成。版本号用这一路现在的：这一路的版本变过，就对不上，照常重建。
pub(crate) fn legacy_device_seg(device: &Profile, area: Screen) -> String {
    let c = device.comic_readable();
    let mut seg = match (c != area, device.comic.reader_margins) {
        (false, None) => String::new(),
        (_, m) => format!("c{}x{}{}", c.width, c.height, m.map(|m| format!("m{m}")).unwrap_or_default()),
    };
    if let Some(d) = &device.comic.page_direction {
        seg.push_str(&format!("d{d}"));
    }
    for (on, x) in [
        (device.comic.fixed_layout, "f"),
        (device.background_images && device.background_sizing, "b"),
        (device.background_images && !device.background_sizing, "bn"),
        (!device.css.css_rgba, "r"),
        (device.caption_fit, "k"),
        (!device.image_alpha, "a"),
        (device.text_repair_only, "t"),
    ] {
        if on {
            seg.push_str(x);
        }
    }
    seg
}

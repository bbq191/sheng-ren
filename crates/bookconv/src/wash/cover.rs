//! 封面声明：保证 OPF 声明了有效封面图。
use super::opf::{
    cover_meta_tags, declared_cover, first_spine_image, insert_manifest_items, insert_metadata,
    insert_spine_first, is_image_item, NewItem,
};
use super::*;

// ───────────────────────── 封面声明 ─────────────────────────

/// 保证 OPF 声明了一个**有效的封面图**（xochitl 靠它生成书库里的封面缩略图）。2026-09-20 要求核查日志时发现：
/// 设备日志 `rm.epub.container Asked for unknown item ""` + `rm.docworker failed extracting cover: got null cover
/// image`——9 本已投的书里 7 本没有封面。原因有两类：①OPF 根本没有 `<meta name="cover">`（如火影）；②声明了但**指向的不是
/// 图片条目**（如 Calibre 产物 `<meta name="cover" content="cover.txt"/>`，指向一个 txt）。
///
/// 已有有效声明（`<meta name="cover">` 指向图片条目，或某图片条目带 `properties="cover-image"`）→ 不动，返回 `false`；
/// 否则取第一个 spine 页里的第一张图（漫画/画册的第一页就是封面），在 manifest 里找到对应条目：删掉旧的（无效）`cover`
/// meta，写入 `<meta name="cover" content="该条目 id"/>`，并给该条目补 `properties="cover-image"`（EPUB3），返回 `true`。
/// **必须在清洗（`wash_entries`）之前调用**：清洗会把只含 SVG 封面的 titlepage 当空页删掉（2026-09-20《镖人(卷四)》真机核对）。
/// 找不到候选也不动。只读 html/OPF 文本，不碰图片字节（流式优化阶段一时图片条目是空占位）。
/// 判定与 `booklib meta --edit` 读封面（`epubzip::cover_image_of`）、改封面（`opfmeta`）共用 `wash::opf` 的一套。
pub fn ensure_cover_declared(entries: &mut [Entry]) -> bool {
    let Some(opf) = parse_opf(entries) else {
        return false;
    };
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let items = manifest_items(&text);
    let has_prop = |i: &ManifestItem| i.properties.split_whitespace().any(|p| p == "cover-image");
    let metas = cover_meta_tags(&text);
    // meta 声明指向的图片条目（若有效）
    // 指向书架缩略图（[`THUMB_ID`]）的 meta 不算封面声明（同 `declared_cover`）
    let meta_target = metas
        .iter()
        .filter(|(_, _, id)| *id != THUMB_ID)
        .find_map(|(_, _, id)| items.iter().find(|i| i.id == *id && is_image_item(i)));
    // 目标封面条目：meta 指向的有效图片 → 已带 cover-image 属性的图片 → 前几页的第一张真实图片（下面找）。
    // **meta 和 `properties="cover-image"` 必须同时有**（2026-09-20 真机对照实验：xochitl 对封面条目 id 带点的仅 meta 声明
    // ——如 Calibre/Sigil 产物的 `x00000001.jpg`——取不到封面，日志 `null cover image`；加上 `cover-image` 属性就取得到；
    // id 简单如 `cover` 时仅 meta 也行）。所以已有 meta 但条目缺属性也要补。
    let existing = declared_cover(&text);
    if let Some(t) = &existing {
        if meta_target.is_some_and(|m| m.id == t.id) && has_prop(t) {
            return false;
        }
        // 已优化过的掌阅产物：meta 全指着缩略图、真封面带 `cover-image`——这是 [`add_cover_thumbnail`] 写的样子，不动（幂等）
        let thumb_ok = items.iter().any(|i| i.id == THUMB_ID && is_image_item(i));
        if thumb_ok
            && !metas.is_empty()
            && metas.iter().all(|(_, _, id)| *id == THUMB_ID)
            && has_prop(t)
        {
            return false;
        }
    }
    // 候选：前 12 个 spine 页（跳过导航页）里第一张能对上 manifest 图片条目的图。多看几页是因为封面页常常是只含一张
    // SVG `<image>` 的 titlepage；有的书（《镖人(卷四)》）连封面图本身都坏了——titlepage 和 cover.xhtml 引用的都是一个 239 字节的
    // `cover.txt` 文本残片，书里没有真封面——此时兜底用书里第一张真实图片（漫画的第一页）。
    let cover = match existing {
        Some(t) => t,
        None => {
            let by_name = name_index(entries);
            let Some(path) = first_spine_image(&text, &opf.dir, 12, true, |p| {
                by_name
                    .get(p)
                    .and_then(|&i| std::str::from_utf8(&entries[i].data).ok())
                    .map(str::to_string)
            }) else {
                return false;
            };
            let Some(it) = items
                .into_iter()
                .find(|i| is_image_item(i) && i.path(&opf.dir) == path)
            else {
                return false;
            };
            it
        }
    };
    // 条目补 `properties="cover-image"`：没有 properties 时紧跟在 id 后面写（与此前产物逐字节一致），有就追加一个值。
    let new_tag = if has_prop(&cover) {
        cover.tag.to_string()
    } else if cover.properties.is_empty() {
        match html::attr(cover.tag, "id") {
            Some(a) => format!(
                r#"{} properties="cover-image"{}"#,
                &cover.tag[..a.end],
                &cover.tag[a.end..]
            ),
            None => return false,
        }
    } else {
        html::set_attr(
            cover.tag,
            "properties",
            &format!("{} cover-image", cover.properties),
        )
    };
    let mut edits: Vec<(usize, usize, String)> = metas
        .iter()
        .map(|&(s, e, _)| (s, e, String::new()))
        .collect();
    edits.push((cover.pos, cover.pos + cover.tag.len(), new_tag));
    edits.sort_by_key(|e| e.0);
    let out = html::apply_edits(&text, edits);
    let Some(out) = insert_metadata(
        &out,
        &format!(r#"<meta name="cover" content="{}"/>"#, cover.id),
    ) else {
        return false;
    };
    entries[opf.index].data = out.into_bytes();
    true
}

/// 补的封面页的文件名（OPF 同目录）和 manifest id。
const COVER_PAGE: &str = "eink-cover.xhtml";
/// 书架缩略图（[`add_cover_thumbnail`]）的文件名、manifest id；OPF 里 `<meta name="eink-thumbnail" content="eink-thumb"/>` 指着它。
const THUMB_FILE: &str = "eink-thumb.jpg";
pub const THUMB_ID: &str = "eink-thumb";
/// 指明书架缩略图的 `<meta>` 名字（KFX 写出器认它：有就拿它当 `cover_image`，书里的封面页照旧用 `cover-image` 那张）。
pub const THUMB_META: &str = "eink-thumbnail";
const COVER_PAGE_ID: &str = "eink-cover-page";

/// 原书没有封面页（OPF 声明了封面图，可哪一页都没用到它）时，在 spine 最前面补一页只放封面图的页面——照 Send to Kindle
/// （Amazon 转的《绍宋》《狼厅》第一页是补出来的整页封面；2026-10-09 ：掌阅、Move 也补）。写法照真书最常见的封面页
/// （《人生海海》《罗杰疑案》）：SVG 按图的宽高定 `viewBox`、`preserveAspectRatio` 等比放进一页。不加文字。
/// `size(图的 zip 路径)` 取图的宽高（流式优化时图片条目是空占位，调用方从原书读文件头）。补了 → `true`。
pub fn prepend_cover_page(
    entries: &mut Vec<Entry>,
    size: impl FnOnce(&str) -> Option<(u32, u32)>,
) -> bool {
    let Some(opf) = parse_opf(entries) else {
        return false;
    };
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let Some(cover) = declared_cover(&text) else {
        return false;
    };
    let cover_path = cover.path(&opf.dir);
    let by_name = name_index(entries);
    let items = manifest_items(&text);
    let used = items
        .iter()
        .filter(|i| i.media_type.contains("html"))
        .any(|it| {
            let page = it.path(&opf.dir);
            by_name
                .get(page.as_str())
                .and_then(|&i| std::str::from_utf8(&entries[i].data).ok())
                .is_some_and(|t| super::opf::page_images(&page, t).contains(&cover_path))
        });
    let page = if opf.dir.is_empty() {
        COVER_PAGE.to_string()
    } else {
        format!("{}/{COVER_PAGE}", opf.dir)
    };
    if used || by_name.contains_key(page.as_str()) || items.iter().any(|i| i.id == COVER_PAGE_ID) {
        return false;
    }
    let Some((w, h)) = size(&cover_path).filter(|&(w, h)| w > 0 && h > 0) else {
        return false;
    };
    // 页面和 OPF 同目录，图的相对路径就是 manifest 里 href 的原文（已转义、已百分号编码）
    let xhtml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
<title>Cover</title>
<style type="text/css">
body {{ margin: 0; padding: 0; text-align: center; }}
svg {{ margin: 0; padding: 0; }}
</style>
</head>
<body epub:type="cover">
<div>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.1" width="100%" height="100%" viewBox="0 0 {w} {h}" preserveAspectRatio="xMidYMid meet">
<image width="{w}" height="{h}" xlink:href="{}"/>
</svg>
</div>
</body>
</html>
"#,
        cover.href.replace('"', "&quot;")
    );
    let Some(out) = insert_manifest_items(
        &text,
        &[NewItem {
            id: COVER_PAGE_ID,
            href: COVER_PAGE,
            media_type: "application/xhtml+xml",
            properties: "svg",
        }],
    ) else {
        return false;
    };
    let Some(out) = insert_spine_first(&out, COVER_PAGE_ID) else {
        return false;
    };
    entries[opf.index].data = out.into_bytes();
    entries.push(Entry {
        name: page,
        data: xhtml.into_bytes(),
    });
    true
}

/// 加一张书架缩略图（profile `cover_thumb_aspect`）：`make(封面在 zip 里的路径)` 给出缩略图字节（[`crate::imgopt::cover_thumbnail`]），
/// 放在 OPF 同目录叫 [`THUMB_FILE`]，manifest 加一项（不进 spine，正文不引用），`<metadata>` 里加 `<meta name="eink-thumbnail">` 指着它。
/// 书里的封面页、`cover-image` 都不动：只给书架用（2026-10-30 维护者定「只裁书架缩略图」）。没有声明封面、
/// `make` 不给（比例本来就对、解不开）→ 不加。
///
/// 已经有缩略图（再优化一遍）：`keep(缩略图在 zip 里的路径)` 说它还合用（比例和这次的一样）就留着，只把 meta 指向对齐这次的模式；
/// 不合用（换了比例的设备）就整张去掉（[`strip_cover_thumbnail`]）照这次的重做。同一模式再跑一遍什么也不改（幂等）。
/// zip 里有 [`THUMB_FILE`] 但 manifest 没有这一项（不是我们加的）→ 不碰。
///
/// `meta_cover`（profile `cover_meta_thumb`，掌阅）：书架认 EPUB 2 的 `<meta name="cover">`，所以再把它指向缩略图——改第一个
/// `cover` meta 的 `content`、多的去掉，没有就加一个；`cover-image` 属性、封面页照旧，[`declared_cover`] 跳过指向缩略图的 meta，
/// 真封面还是 `cover-image` 那张。不开时指着缩略图的 meta（掌阅产物拿来给别的设备再优化）改回真封面。改了什么 → `true`。
pub fn add_cover_thumbnail(
    entries: &mut Vec<Entry>,
    meta_cover: bool,
    keep: impl FnOnce(&str) -> bool,
    make: impl FnOnce(&str) -> Option<Vec<u8>>,
) -> bool {
    let Some(opf) = parse_opf(entries) else {
        return false;
    };
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let file = if opf.dir.is_empty() {
        THUMB_FILE.to_string()
    } else {
        format!("{}/{THUMB_FILE}", opf.dir)
    };
    let mut out = text.clone();
    let mut dropped = None;
    if let Some(old) = manifest_items(&text).iter().find(|i| i.id == THUMB_ID) {
        if keep(&old.path(&opf.dir)) {
            let Some(cover) = declared_cover(&text) else {
                return false;
            };
            let new = if meta_cover {
                point_meta_cover(&text, THUMB_ID)
            } else if cover_meta_tags(&text).iter().any(|m| m.2 == THUMB_ID) {
                point_meta_cover(&text, cover.id)
            } else {
                None
            };
            return match new {
                Some(new) if new != text => {
                    entries[opf.index].data = new.into_bytes();
                    true
                }
                _ => false,
            };
        }
        if let Some((s, path)) = strip_cover_thumbnail(&text, &opf.dir) {
            out = s;
            dropped = Some(path);
        }
    } else if name_index(entries).contains_key(file.as_str()) {
        return false;
    }
    let finish = |entries: &mut Vec<Entry>, opf_text: String, add: Option<Vec<u8>>| {
        let changed = opf_text != text || add.is_some() || dropped.is_some();
        entries[opf.index].data = opf_text.into_bytes();
        if let Some(d) = &dropped {
            entries.retain(|e| &e.name != d);
        }
        if let Some(data) = add {
            entries.push(Entry {
                name: file.clone(),
                data,
            });
        }
        changed
    };
    let made = declared_cover(&out).and_then(|c| make(&c.path(&opf.dir)));
    let Some(bytes) = made else {
        return finish(entries, out, None);
    };
    let with = insert_manifest_items(
        &out,
        &[NewItem {
            id: THUMB_ID,
            href: THUMB_FILE,
            media_type: "image/jpeg",
            properties: "",
        }],
    )
    .and_then(|o| {
        insert_metadata(
            &o,
            &format!(r#"<meta name="{THUMB_META}" content="{THUMB_ID}"/>"#),
        )
    })
    .and_then(|o| {
        if meta_cover {
            point_meta_cover(&o, THUMB_ID)
        } else {
            Some(o)
        }
    });
    match with {
        Some(o) => finish(entries, o, Some(bytes)),
        None => finish(entries, out, None),
    }
}

/// 去掉 [`add_cover_thumbnail`] 加的书架缩略图（OPF 文字层）：manifest 的 [`THUMB_ID`] 项、`<meta name="eink-thumbnail">`；
/// 指着缩略图的 `<meta name="cover">` 改回真封面（[`declared_cover`]，没有真封面就去掉）。返回 (新 OPF, 缩略图在 zip 里的路径，
/// 调用方从 zip 删掉)。没有这一项 → `None`。换封面、删封面（`opfmeta`）、给不做缩略图的设备再优化时用：旧缩略图留着书架就还是旧封面。
pub fn strip_cover_thumbnail(opf: &str, opf_dir: &str) -> Option<(String, String)> {
    let path = manifest_items(opf)
        .iter()
        .find(|i| i.id == THUMB_ID)?
        .path(opf_dir);
    let cover = declared_cover(opf).map(|c| c.id.to_string());
    let out =
        super::opf::remove_items(opf, |i| i.id == THUMB_ID).unwrap_or_else(|| opf.to_string());
    let edits: Vec<(usize, usize, String)> = html::tags(&out)
        .filter(|t| t.is_start() && super::opf::is_local(t.name, "meta"))
        .filter(|t| html::attr_value(&out[t.start..t.end], "name") == Some(THUMB_META))
        .map(|t| (t.start, super::opf::element_end(&out, &t), String::new()))
        .collect();
    let out = html::apply_edits(&out, edits);
    let metas = cover_meta_tags(&out);
    let out = if !metas.iter().any(|m| m.2 == THUMB_ID) {
        out
    } else if let Some(c) = &cover {
        point_meta_cover(&out, c).unwrap_or(out)
    } else {
        let edits = metas
            .iter()
            .filter(|m| m.2 == THUMB_ID)
            .map(|&(s, e, _)| (s, e, String::new()))
            .collect();
        html::apply_edits(&out, edits)
    };
    Some((out, path))
}

/// [`strip_cover_thumbnail`] 的条目版：OPF 改好、缩略图文件从 `entries` 去掉。没有缩略图 → `false`。
pub fn drop_cover_thumbnail(entries: &mut Vec<Entry>) -> bool {
    let Some(opf) = parse_opf(entries) else {
        return false;
    };
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let Some((out, path)) = strip_cover_thumbnail(&text, &opf.dir) else {
        return false;
    };
    entries[opf.index].data = out.into_bytes();
    entries.retain(|e| e.name != path);
    true
}

/// 让 `<meta name="cover">` 指向 `id`：第一个原位改成新的（保住 metadata 里的顺序，原来写成 `<opf:meta>` 的照旧带前缀），多的去掉；
/// 已经只有一个指着 `id` 就原样返回。一个都没有就加在 `</metadata>` 前，这时 OPF 没有 `</metadata>` → `None`
/// （原位改不需要 `</metadata>`，照常返回）。
pub(crate) fn point_meta_cover(opf: &str, id: &str) -> Option<String> {
    let metas = cover_meta_tags(opf);
    if metas.is_empty() {
        return insert_metadata(opf, &format!(r#"<meta name="cover" content="{id}"/>"#));
    }
    if metas.len() == 1 && metas[0].2 == id {
        return Some(opf.to_string());
    }
    let s0 = metas[0].0 + 1;
    let qname_len = opf[s0..]
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(0);
    let prefix = opf[s0..s0 + qname_len].strip_suffix("meta").unwrap_or("");
    let tag = format!(r#"<{prefix}meta name="cover" content="{id}"/>"#);
    let edits = metas
        .iter()
        .enumerate()
        .map(|(n, &(s, e, _))| (s, e, if n == 0 { tag.clone() } else { String::new() }))
        .collect();
    Some(html::apply_edits(opf, edits))
}

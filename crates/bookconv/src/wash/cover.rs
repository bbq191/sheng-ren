//! 封面声明：保证 OPF 声明了有效封面图。
use super::opf::{cover_meta_tags, declared_cover, first_spine_image, insert_metadata, is_image_item};
use super::*;

// ───────────────────────── 封面声明 ─────────────────────────

/// 保证 OPF 声明了一个**有效的封面图**（xochitl 靠它生成书库里的封面缩略图）。2026-09-20 用户要求核查日志时发现：
/// 设备日志 `rm.epub.container Asked for unknown item ""` + `rm.docworker failed extracting cover: got null cover
/// image`——9 本已投的书里 7 本没有封面。原因有两类：①OPF 根本没有 `<meta name="cover">`（如火影）；②声明了但**指向的不是
/// 图片条目**（如 Calibre 产物 `<meta name="cover" content="cover.txt"/>`，指向一个 txt）。
///
/// 已有有效声明（`<meta name="cover">` 指向图片条目，或某图片条目带 `properties="cover-image"`）→ 不动，返回 `false`；
/// 否则取第一个 spine 页里的第一张图（漫画/画册的第一页就是封面），在 manifest 里找到对应条目：删掉旧的（无效）`cover`
/// meta，写入 `<meta name="cover" content="该条目 id"/>`，并给该条目补 `properties="cover-image"`（EPUB3），返回 `true`。
/// **必须在清洗（`wash_entries`）之前调用**：清洗会把只含 SVG 封面的 titlepage 当空页删掉（2026-09-20《镖人(卷四)》真机核对）。
/// 找不到候选也不动。只读 html/OPF 文本，不碰图片字节（流式优化阶段一时图片条目是空占位）。
/// 判定与 `ebook-meta` 读封面（`epubzip::cover_image_of`）、改封面（`opfmeta`）共用 `wash::opf` 的一套。
pub fn ensure_cover_declared(entries: &mut [Entry]) -> bool {
    let Some(opf) = parse_opf(entries) else { return false };
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let items = manifest_items(&text);
    let has_prop = |i: &ManifestItem| i.properties.split_whitespace().any(|p| p == "cover-image");
    let metas = cover_meta_tags(&text);
    // meta 声明指向的图片条目（若有效）
    let meta_target = metas.iter().find_map(|(_, _, id)| items.iter().find(|i| i.id == *id && is_image_item(i)));
    // 目标封面条目：meta 指向的有效图片 → 已带 cover-image 属性的图片 → 前几页的第一张真实图片（下面找）。
    // **meta 和 `properties="cover-image"` 必须同时有**（2026-09-20 真机对照实验：xochitl 对封面条目 id 带点的仅 meta 声明
    // ——如 Calibre/Sigil 产物的 `x00000001.jpg`——取不到封面，日志 `null cover image`；加上 `cover-image` 属性就取得到；
    // id 简单如 `cover` 时仅 meta 也行）。所以已有 meta 但条目缺属性也要补。
    let existing = declared_cover(&text);
    if let Some(t) = &existing {
        if meta_target.is_some_and(|m| m.id == t.id) && has_prop(t) {
            return false;
        }
    }
    // 候选：前 12 个 spine 页（跳过导航页）里第一张能对上 manifest 图片条目的图。多看几页是因为封面页常常是只含一张
    // SVG `<image>` 的 titlepage；有的书（《镖人(卷四)》）连封面图本身都坏了——titlepage 和 cover.xhtml 引用的都是一个 239 字节的
    // `cover.txt` 文本残片，书里没有真封面——此时兜底用书里第一张真实图片（漫画的第一页）。
    let cover = match existing {
        Some(t) => t,
        None => {
            let by_name: HashMap<&str, &Entry> = entries.iter().rev().map(|e| (e.name.as_str(), e)).collect();
            let Some(path) = first_spine_image(&text, &opf.dir, 12, true, |p| by_name.get(p).and_then(|e| std::str::from_utf8(&e.data).ok()).map(str::to_string)) else { return false };
            let Some(it) = items.into_iter().find(|i| is_image_item(i) && resolve(&opf.dir, &percent_decode(i.href)) == path) else { return false };
            it
        }
    };
    // 条目补 `properties="cover-image"`：没有 properties 时紧跟在 id 后面写（与此前产物逐字节一致），有就追加一个值。
    let new_tag = if has_prop(&cover) {
        cover.tag.to_string()
    } else if cover.properties.is_empty() {
        match html::attr(cover.tag, "id") {
            Some(a) => format!(r#"{} properties="cover-image"{}"#, &cover.tag[..a.end], &cover.tag[a.end..]),
            None => return false,
        }
    } else {
        html::set_attr(cover.tag, "properties", &format!("{} cover-image", cover.properties))
    };
    let mut edits: Vec<(usize, usize, String)> = metas.iter().map(|&(s, e, _)| (s, e, String::new())).collect();
    edits.push((cover.pos, cover.pos + cover.tag.len(), new_tag));
    edits.sort_by_key(|e| e.0);
    let out = html::apply_edits(&text, edits);
    let Some(out) = insert_metadata(&out, &format!(r#"<meta name="cover" content="{}"/>"#, cover.id)) else { return false };
    entries[opf.index].data = out.into_bytes();
    true
}

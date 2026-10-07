//! EPUB 元数据（OPF 里的 Dublin Core 与封面）的读取和改写：`booklib meta --edit` 命令、书库生成时补简介/标签/封面共用。
//!
//! 改写动 OPF（改了书名时连 NCX 的书名）和封面（图、只放封面的页面、目录里指向它的条目）；`booklib meta --edit` 另做一遍
//! EPUB 3 规范整理（[`Edits::normalize`]，会改 XHTML/OPF/NCX 的标记，可见文字不动）。没改到的条目按原样**原始拷贝**
//! （不解压不重压），图片只读要换的封面那一张以外一张都不解压。
//! 设字段 = 先删掉这个字段的全部元素（连同 EPUB3 用 `refines="#id"` 挂在它们身上的 `<meta>`，如作者的角色、排序名），
//! 再在第一个被删元素原来的位置写新值；书里原来没有这个字段就插在 `</metadata>` 前面。

use crate::html::{self, TagKind};
use crate::util::xml_escape;
use std::path::Path;

/// 补元数据（[`edit_epub`]）的版本：改了会影响书库产物的行为就加一。书库补过东西的书的指纹带着它（`i{VERSION}`），没补过的不受影响。
/// - 3（2026-09-30）：补完做 EPUB 3 规范整理。
/// - 4（2026-09-30）：书库补元数据不再做规范整理（[`Edits::normalize`]，优化器的清洗层反正要做），和没补过东西的书走同一条路。
/// - 5（2026-10-07）：`<opf:package>`、`<opf:metadata>` 这类带前缀的 OPF 也认（以前插入点找得到，EPUB2 判定和挂在旧元素上的
///   `<opf:meta refines>` 认不出：EPUB2 作者少了 `opf:role`、旧的 refines 留着成了悬空引用）。
pub const VERSION: &str = "5";

/// 支持读写的 Dublin Core 字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DcField {
    Title,
    Creator,
    Language,
    Publisher,
    Description,
    Subject,
    Identifier,
    Date,
}

impl DcField {
    pub const ALL: [DcField; 8] =
        [DcField::Title, DcField::Creator, DcField::Language, DcField::Publisher, DcField::Description, DcField::Subject, DcField::Identifier, DcField::Date];

    /// 元素名（不带 `dc:` 前缀）。
    pub fn name(self) -> &'static str {
        match self {
            DcField::Title => "title",
            DcField::Creator => "creator",
            DcField::Language => "language",
            DcField::Publisher => "publisher",
            DcField::Description => "description",
            DcField::Subject => "subject",
            DcField::Identifier => "identifier",
            DcField::Date => "date",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DcField::Title => "标题",
            DcField::Creator => "作者",
            DcField::Language => "语言",
            DcField::Publisher => "出版社",
            DcField::Description => "简介",
            DcField::Subject => "标签",
            DcField::Identifier => "标识符",
            DcField::Date => "日期",
        }
    }
}

/// 一个 `<dc:…>` 元素在 OPF 文本里的范围、`id` 和纯文本值。
struct DcElem {
    field: DcField,
    start: usize,
    end: usize,
    id: Option<String>,
    value: String,
}

/// OPF 里 `<metadata>` 范围内的全部 Dublin Core 元素（文档序）。
fn dc_elements(opf: &str) -> Vec<DcElem> {
    let mut out = Vec::new();
    for t in html::tags(opf).filter(|t| t.is_start()) {
        let Some(local) = t.name.strip_prefix("dc:").or_else(|| t.name.strip_prefix("DC:")) else { continue };
        let Some(field) = DcField::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(local)) else { continue };
        let tag = &opf[t.start..t.end];
        let id = html::attr_value(tag, "id").map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
        let (end, value) = if t.kind == TagKind::SelfClosing {
            (t.end, String::new())
        } else {
            match html::find_close(opf, t.end, t.name) {
                Some(c) => (c.end, html::plain_text(&opf[t.end..c.start])),
                None => continue,
            }
        };
        out.push(DcElem { field, start: t.start, end, id, value });
    }
    out
}

/// 读出 OPF 里各字段的全部非空值（纯文本；多值字段按文档序）。
pub fn read(opf: &str) -> Vec<(DcField, Vec<String>)> {
    let elems = dc_elements(opf);
    DcField::ALL.into_iter().map(|f| (f, elems.iter().filter(|e| e.field == f && !e.value.is_empty()).map(|e| e.value.clone()).collect())).collect()
}

/// 要做的改动。
#[derive(Clone, Debug, Default)]
pub struct Edits {
    /// 整体替换这些字段（给几个值就是最终的几个；空列表 = 删掉这个字段）。
    pub set: Vec<(DcField, Vec<String>)>,
    /// 封面：`None` 不动。
    pub cover: Option<CoverEdit>,
    /// 写进 `dcterms:modified` 的时间（`booklib meta --edit` 命令给现在的时间）；`None` 不改（booklib 生成产物前补元数据时用：
    /// 产物要逐字节可重现）。
    pub modified: Option<String>,
    /// 改完再做 EPUB 3 规范整理（[`crate::wash::normalize_epub3`]）。`booklib meta --edit` 要（写出的书和 booklib 的产物一样符合 EPUB 3）；
    /// booklib 生成产物前补元数据不要——补完马上要过优化器，清洗层会做同一套整理。
    pub normalize: bool,
}

/// 封面怎么改。
#[derive(Clone, Debug)]
pub enum CoverEdit {
    /// 换成这张图（JPEG/PNG 字节）：书里声明了封面图就原地换掉它的内容（格式不同时转成原图的格式），没有就新加一个。
    Set(Vec<u8>),
    /// 去掉（见 [`remove_cover`]）。
    Remove,
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.cover.is_none()
    }
}

/// 改写 OPF 文本里的字段（封面另算，见 [`edit_epub`]）。`<metadata>` 找不到时报错。
pub fn apply_fields(opf: &str, set: &[(DcField, Vec<String>)]) -> Result<String, String> {
    // `<package>`、`<metadata>`、`<meta>` 都认带命名空间前缀的写法（`<opf:package>` 等），同 `wash::opf::package_unique_identifier`
    use crate::wash::opf::is_local;
    let meta_close = html::tags(opf).find(|t| t.kind == TagKind::Close && is_local(t.name, "metadata")).ok_or("OPF 里没有 </metadata>")?.start;
    let elems = dc_elements(opf);
    let epub2 = html::tags(opf)
        .find(|t| t.is_start() && is_local(t.name, "package"))
        .and_then(|t| html::attr_value(&opf[t.start..t.end], "version"))
        .is_some_and(|v| v.starts_with('2'))
        && opf.contains("xmlns:opf");
    // 要删的范围 + 插入点（按位置排序后从后往前改）
    let mut removals: Vec<(usize, usize)> = Vec::new();
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let mut removed_ids: Vec<String> = Vec::new();
    // `<package unique-identifier="X">` 指向的那个标识符不能删（删了 OPF 不合法，NCX 的 dtb:uid 也对不上，reMarkable 会不显示目录）
    let uid = crate::wash::opf::package_unique_identifier(opf).map(str::to_string);
    for (field, values) in set {
        let old: Vec<&DcElem> = elems.iter().filter(|e| e.field == *field && !(e.field == DcField::Identifier && uid.is_some() && e.id == uid)).collect();
        let at = old.first().map_or(meta_close, |e| e.start);
        for e in &old {
            removals.push((e.start, e.end));
            removed_ids.extend(e.id.clone());
        }
        let name = field.name();
        // EPUB2 的作者带上角色（opf:role="aut"，阅读器据此区分作者和译者等）；EPUB3 用 refines 写角色，不写也是作者
        let attrs = if *field == DcField::Creator && epub2 { r#" opf:role="aut""# } else { "" };
        let text: String = values.iter().filter(|v| !v.trim().is_empty()).map(|v| format!("<dc:{name}{attrs}>{}</dc:{name}>", xml_escape(v.trim()))).collect();
        if !text.is_empty() {
            inserts.push((at, text));
        }
    }
    // 挂在被删元素上的 `<meta refines="#id">`（EPUB3：作者角色、排序名、书名类型等）一起删
    if !removed_ids.is_empty() {
        for t in html::tags(opf).filter(|t| t.is_start() && is_local(t.name, "meta")) {
            let Some(r) = html::attr_value(&opf[t.start..t.end], "refines") else { continue };
            if !removed_ids.iter().any(|id| r.strip_prefix('#') == Some(id.as_str())) {
                continue;
            }
            let end = if t.kind == TagKind::SelfClosing { t.end } else { html::find_close(opf, t.end, t.name).map_or(t.end, |c| c.end) };
            removals.push((t.start, end));
        }
    }
    // 坏 OPF 里元素可能套着元素，要删的范围会重叠：插入点落在别的要删的范围里面时挪到那个范围的开头（不然新值跟着被删掉）；
    // 重叠的删除由 `apply_edits` 跳过（外面那个已经连里面一起删了）。
    removals.sort();
    for (at, _) in inserts.iter_mut() {
        if let Some(&(s, _)) = removals.iter().find(|&&(s, e)| s < *at && *at < e) {
            *at = s;
        }
    }
    // 同一位置：先插入再删除（插入点就是第一个被删元素的开头）；几个字段插在同一处时后给的在前（以前从后往前逐个插入就是这个顺序）
    inserts.reverse();
    let mut ops: Vec<(usize, usize, String)> = inserts.into_iter().map(|(at, text)| (at, at, text)).collect();
    ops.extend(removals.into_iter().map(|(s, e)| (s, e, String::new())));
    ops.sort_by_key(|&(s, e, _)| (s, e > s));
    Ok(html::apply_edits(opf, ops))
}

/// NCX 的 `<docTitle><text>` 换成新书名（阅读器目录顶上显示的书名）。没有 docTitle 原样返回。
fn set_ncx_title(ncx: &str, title: &str) -> String {
    let Some(dt) = html::tags(ncx).find(|t| t.kind == TagKind::Open && t.is("docTitle")) else { return ncx.to_string() };
    let Some(open) = html::tags(&ncx[dt.end..]).find(|t| t.kind == TagKind::Open && t.is("text")) else { return ncx.to_string() };
    let (s, e) = (dt.end + open.end, dt.end + open.end);
    let Some(close) = html::find_close(ncx, e, "text") else { return ncx.to_string() };
    format!("{}{}{}", &ncx[..s], xml_escape(title), &ncx[close.start..])
}

/// 书里声明的封面图：(zip 路径, 扩展名)。判定见 `wash::opf::declared_cover`（与优化器、`cover_image_of` 同一套）。
fn declared_cover(opf: &str, opf_dir: &str) -> Option<(String, String)> {
    let item = crate::wash::opf::declared_cover(opf)?;
    let path = item.path(opf_dir);
    let ext = crate::util::image_ext_of(&path);
    Some((path, ext))
}

/// 换封面图的内容：`ext` 是原封面图的扩展名。返回 (新字节, 要改成的 manifest `media-type`)。
/// - 原图是 jpg/png：转成原图的格式（已经是就原样），条目名、media-type 都不用动；
/// - 原图是别的格式（gif、webp）：新图原样写进这个条目，manifest 的 `media-type` 改成新图的格式。条目不改名——改名要把
///   全书指向它的 `src`、`href`、`url()` 都改掉，漏一处图就丢；EPUB 按 manifest 的 media-type 认图片格式
///   （优化器把漫画里的 GIF/WebP 页转成 PNG/JPEG 也是这么做的）。此前一律转成 JPEG 写进 `.gif` 条目、media-type 还是 gif。
fn to_format(image: &[u8], ext: &str) -> Result<(Vec<u8>, Option<&'static str>), String> {
    let (have, mime) = crate::convert::common::image_ext_mime(image).ok_or("封面图不是 JPEG/PNG")?;
    let want = if ext == "jpeg" { "jpg" } else { ext };
    if want != "jpg" && want != "png" {
        return Ok((image.to_vec(), Some(mime)));
    }
    if have == want {
        return Ok((image.to_vec(), None));
    }
    let img = crate::imgopt::guard(|| Some(image::load_from_memory(image))).ok_or("封面图解不开")?.map_err(|e| format!("封面图解不开：{e}"))?;
    let mut out = Vec::new();
    let fmt = if want == "png" { image::ImageFormat::Png } else { image::ImageFormat::Jpeg };
    let img = if fmt == image::ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
    img.write_to(&mut std::io::Cursor::new(&mut out), fmt).map_err(|e| format!("封面图转格式失败：{e}"))?;
    Ok((out, None))
}

/// OPF 里 zip 路径为 `path` 的 manifest 项的 `media-type` 改成 `mt`（没有这一项或它没有 `media-type` 属性就原样）。
fn set_media_type(opf: &str, opf_dir: &str, path: &str, mt: &str) -> String {
    for it in crate::wash::manifest_items(opf) {
        if it.path(opf_dir) != path {
            continue;
        }
        if let Some(a) = html::attr(it.tag, "media-type") {
            return html::apply_edits(opf, vec![(it.pos + a.value_start, it.pos + a.value_end, mt.to_string())]);
        }
    }
    opf.to_string()
}

/// 要改写的 zip 条目：(路径, 新内容)。
pub type Rewritten = (String, Vec<u8>);

/// 去掉书里的封面（2026-09-30 用户要）：封面声明（`<meta name="cover">`、`cover-image`）、**只放封面图的页面**（没有可见文字、
/// 只有这一张图；连同 spine、guide、NCX、nav 里指向它的条目），以及封面图本身——正文别的页也用着这张图时图留着，只去掉声明。
/// `read(zip 路径)` 取文本。返回 (新 OPF, 要从 zip 删掉的条目, 要改写的条目 (路径, 新内容))。书里没有声明封面图 → 原样。
pub fn remove_cover(opf: &str, opf_dir: &str, mut read: impl FnMut(&str) -> Option<String>) -> (String, Vec<String>, Vec<Rewritten>) {
    use crate::epubzip::{resolve_link, resolve_rel};
    use crate::wash::opf as o;
    let Some(cover) = o::declared_cover(opf) else { return (opf.to_string(), Vec::new(), Vec::new()) };
    let (cover_id, cover_path) = (cover.id.to_string(), cover.path(opf_dir));
    let items = o::manifest_items(opf);
    // 页面里引用的图（img src、SVG image href）
    let images_in = |page: &str, text: &str| -> Vec<String> {
        html::tags(text)
            .filter(|t| t.is_start() && (o::is_local(t.name, "img") || o::is_local(t.name, "image")))
            .filter_map(|t| ["src", "xlink:href", "href"].iter().find_map(|a| html::attr_value(&text[t.start..t.end], a)))
            .filter(|v| !html::is_external(v))
            .map(|v| resolve_link(page, v).0)
            .collect()
    };
    let (mut cover_pages, mut used_elsewhere) = (Vec::<(String, String)>::new(), false);
    for it in items.iter().filter(|i| i.media_type.contains("html")) {
        let page = it.path(opf_dir);
        let Some(text) = read(&page) else { continue };
        let imgs = images_in(&page, &text);
        if !imgs.contains(&cover_path) {
            continue;
        }
        let body = html::tags(&text).find(|t| t.is_start() && t.is("body")).map_or(0, |t| t.end);
        if imgs.len() == 1 && html::plain_text(&text[body..]).is_empty() {
            cover_pages.push((it.id.to_string(), page));
        } else {
            used_elsewhere = true;
        }
    }
    let page_ids: Vec<&str> = cover_pages.iter().map(|(id, _)| id.as_str()).collect();
    let page_paths: Vec<&str> = cover_pages.iter().map(|(_, p)| p.as_str()).collect();
    let mut opf = o::remove_items(opf, |it| page_ids.contains(&it.id) || (!used_elsewhere && it.id == cover_id)).unwrap_or_else(|| opf.to_string());
    // 封面声明、guide 里指向封面页的条目、图还留着时它身上的 cover-image
    let mut edits: Vec<(usize, usize, String)> = o::cover_meta_tags(&opf).into_iter().map(|(s, e, _)| (s, e, String::new())).collect();
    for t in html::tags(&opf).filter(|t| t.is_start()) {
        let tag = &opf[t.start..t.end];
        if o::is_local(t.name, "reference") && html::attr_value(tag, "href").is_some_and(|h| page_paths.contains(&resolve_rel(opf_dir, &crate::util::xml_unescape(html::split_href(h).0)).as_str())) {
            let end = o::element_end(&opf, &t);
            let ws = opf[end..].len() - opf[end..].trim_start().len();
            edits.push((t.start, end + ws, String::new()));
        } else if o::is_local(t.name, "item") && html::attr_value(tag, "id") == Some(cover_id.as_str()) {
            if let Some(props) = html::attr_value(tag, "properties") {
                let rest: Vec<&str> = props.split_whitespace().filter(|p| *p != "cover-image").collect();
                let new = if rest.is_empty() { html::remove_attr(tag, "properties") } else { html::set_attr(tag, "properties", &rest.join(" ")) };
                edits.push((t.start, t.end, new));
            }
        }
    }
    edits.sort_by_key(|e| e.0);
    edits.dedup_by_key(|e| e.0);
    opf = html::apply_edits(&opf, edits);
    // guide 空了整个去掉（EPUB 2 规定 guide 里至少有一条 reference）
    if let Some(g) = html::tags(&opf).find(|t| t.kind == TagKind::Open && o::is_local(t.name, "guide")) {
        if let Some(close) = html::tags_in(&opf, g.end, opf.len()).find(|t| t.kind == TagKind::Close && o::is_local(t.name, "guide")) {
            if !html::tags_in(&opf, g.end, close.start).any(|t| t.is_start() && o::is_local(t.name, "reference")) {
                let ws = opf[close.end..].len() - opf[close.end..].trim_start().len();
                let start = opf[..g.start].trim_end().len();
                opf = format!("{}{}{}", &opf[..start], if ws > 0 { &opf[close.end..close.end + ws] } else { "" }, &opf[close.end + ws..]);
            }
        }
    }
    // NCX 的 navPoint、nav 的 <li>：指向封面页的整条去掉
    let mut rewritten = Vec::new();
    if !page_paths.is_empty() {
        for it in o::manifest_items(&opf).iter().filter(|i| i.media_type.contains("dtbncx") || i.properties.split_whitespace().any(|p| p == "nav")) {
            let file = it.path(opf_dir);
            let Some(text) = read(&file) else { continue };
            let elem = if it.media_type.contains("dtbncx") { "navPoint" } else { "li" };
            let new = drop_entries_pointing_to(&text, &file, elem, &page_paths);
            if new != text {
                rewritten.push((file, new.into_bytes()));
            }
        }
    }
    let mut gone: Vec<String> = cover_pages.into_iter().map(|(_, p)| p).collect();
    if !used_elsewhere {
        gone.push(cover_path);
    }
    (opf, gone, rewritten)
}

/// 去掉 `elem` 元素（NCX 的 `navPoint`、nav 的 `li`）里第一个链接指向 `pages` 之一的那些（整个元素连同里面的子项）。
fn drop_entries_pointing_to(text: &str, file: &str, elem: &str, pages: &[&str]) -> String {
    let tags: Vec<html::Tag> = html::tags(text).collect();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut skip_until = 0;
    for (k, t) in tags.iter().enumerate() {
        if t.start < skip_until || !(t.kind == TagKind::Open && crate::wash::opf::is_local(t.name, elem)) {
            continue;
        }
        // 这个元素自己的第一个链接（NCX 是 <content src>，nav 是 <a href>）
        let link = tags[k + 1..].iter().find(|x| x.is_start() && (crate::wash::opf::is_local(x.name, "content") || x.is("a"))).and_then(|x| {
            let tag = &text[x.start..x.end];
            html::attr_value(tag, "src").or_else(|| html::attr_value(tag, "href"))
        });
        let Some(link) = link else { continue };
        if !pages.contains(&crate::epubzip::resolve_link(file, link).0.as_str()) {
            continue;
        }
        let Some(close) = find_matching_close(text, &tags, k) else { continue };
        let ws = text[close..].len() - text[close..].trim_start().len();
        edits.push((t.start, close + ws, String::new()));
        skip_until = close;
    }
    if edits.is_empty() { text.to_string() } else { html::apply_edits(text, edits) }
}

/// `tags[k]`（开标签）对应的闭合标签终点：按同名元素的嵌套层数配对。
fn find_matching_close(text: &str, tags: &[html::Tag], k: usize) -> Option<usize> {
    let name = tags[k].name;
    let mut depth = 0usize;
    for t in &tags[k..] {
        if !t.name.eq_ignore_ascii_case(name) {
            continue;
        }
        match t.kind {
            TagKind::Open => depth += 1,
            TagKind::Close => {
                depth -= 1;
                if depth == 0 {
                    return Some(t.end.min(text.len()));
                }
            }
            _ => {}
        }
    }
    None
}

/// 改完之后的摘要（给命令行打印）。
#[derive(Debug, Default, PartialEq)]
pub struct EditReport {
    pub fields: Vec<DcField>,
    /// 封面：`Some(true)` 换掉了书里原有的封面图，`Some(false)` 新加了封面。
    pub cover_replaced: Option<bool>,
    /// 去封面时去掉的 zip 条目（封面图、只放封面的页面）；`Some(空)` = 书里本来就没有封面。
    pub cover_removed: Option<Vec<String>>,
}

/// 读一本 EPUB 的元数据：(OPF 路径, 各字段)。
pub fn read_epub(path: &Path) -> Result<Vec<(DcField, Vec<String>)>, String> {
    let (_, _, opf) = crate::epubzip::open_opf(path)?;
    Ok(read(&opf))
}

/// 把 `src` 改好写到 `dst`（`dst` 不能是 `src`；原地改由调用方先写临时文件再改名）。
///
/// 只把文字条目读进内存（`epubzip::read_skeleton`，图片不解压）；写出时内容变了的条目重写，其余条目原样拷贝压缩数据。
/// [`Edits::normalize`] 时再过一遍清洗层的规范整理（[`crate::wash::normalize_epub3`]：XHTML 修成合法 XML、OPF 升到 3.0、
/// 补导航文档和 landmarks、NCX 标识对齐；2026-09-30 用户定：`booklib meta --edit` 写出的书和 booklib 的产物一样符合 EPUB 3），
/// 给了 [`Edits::modified`] 就把 `dcterms:modified` 写成它（`booklib meta --edit` 给现在的时间：书确实改了）。
/// 可见文字一个不动；XHTML 的变化只限规范整理那几条（DOCTYPE、命名实体、命名空间等）。
pub fn edit_epub(src: &Path, dst: &Path, edits: &Edits) -> Result<EditReport, String> {
    let (mut zip, opf_path, opf_text) = crate::epubzip::open_opf(src)?;
    let mut entries = crate::epubzip::read_skeleton(&mut zip)?.entries;
    // 读进来时的样子（图片是空占位）：写出时内容没变的条目原样拷贝
    let before: std::collections::HashMap<String, Vec<u8>> = entries.iter().map(|e| (e.name.clone(), e.data.clone())).collect();
    let opf_dir = crate::epubzip::dir_of(&opf_path).to_string();
    let mut opf = apply_fields(&opf_text, &edits.set)?;
    let mut report = EditReport { fields: edits.set.iter().map(|(f, _)| *f).collect(), cover_replaced: None, cover_removed: None };
    let text_of = |entries: &[crate::epubzip::Entry], n: &str| entries.iter().find(|e| e.name == n).map(|e| String::from_utf8_lossy(&e.data).into_owned());
    let set_entry = |entries: &mut Vec<crate::epubzip::Entry>, n: String, data: Vec<u8>| match entries.iter_mut().find(|e| e.name == n) {
        Some(e) => e.data = data,
        None => entries.push(crate::epubzip::Entry { name: n, data }),
    };

    // 换了书名：NCX 的 docTitle 一起换
    if let Some(t) = edits.set.iter().find(|(f, _)| *f == DcField::Title).and_then(|(_, v)| v.first()) {
        let ncx = crate::wash::manifest_items(&opf).iter().find(|i| i.media_type.contains("dtbncx")).map(|i| i.path(&opf_dir));
        if let Some(ncx) = ncx {
            if let Some(text) = text_of(&entries, &ncx) {
                set_entry(&mut entries, ncx, set_ncx_title(&text, t).into_bytes());
            }
        }
    }
    // 封面
    match &edits.cover {
        None => {}
        Some(CoverEdit::Remove) => {
            let (new_opf, gone, rewritten) = remove_cover(&opf, &opf_dir, |n| text_of(&entries, n));
            opf = new_opf;
            for (n, data) in rewritten {
                set_entry(&mut entries, n, data);
            }
            entries.retain(|e| !gone.contains(&e.name));
            report.cover_removed = Some(gone);
        }
        Some(CoverEdit::Set(image)) => match declared_cover(&opf, &opf_dir) {
            Some((path, ext)) => {
                let (bytes, retype) = to_format(image, &ext)?;
                if let Some(mt) = retype {
                    opf = set_media_type(&opf, &opf_dir, &path, mt);
                }
                set_entry(&mut entries, path, bytes);
                report.cover_replaced = Some(true);
            }
            None => {
                use crate::wash::opf as o;
                let (ext, mime) = crate::convert::common::image_ext_mime(image).ok_or("封面图不是 JPEG/PNG")?;
                let name = format!("eink-cover.{ext}");
                opf = o::insert_manifest_items(&opf, &[o::NewItem { id: "eink-cover", href: &name, media_type: mime, properties: "cover-image" }]).ok_or("OPF 没有 </manifest>")?;
                // 没有指向图片的封面声明：旧的（常见指向 txt 的坏声明）全去掉，换成新的
                let old: Vec<(usize, usize, String)> = o::cover_meta_tags(&opf).into_iter().map(|(s, e, _)| (s, e, String::new())).collect();
                opf = html::apply_edits(&opf, old);
                opf = o::insert_metadata(&opf, r#"<meta name="cover" content="eink-cover"/>"#).ok_or("OPF 里没有 </metadata>")?;
                set_entry(&mut entries, if opf_dir.is_empty() { name } else { format!("{opf_dir}/{name}") }, image.clone());
                report.cover_replaced = Some(false);
            }
        },
    }
    set_entry(&mut entries, opf_path.clone(), opf.into_bytes());

    if edits.normalize {
        crate::wash::normalize_epub3(&mut entries);
    }
    if let (Some(when), Some(e)) = (&edits.modified, entries.iter_mut().find(|e| e.name == opf_path)) {
        e.data = set_modified(&String::from_utf8_lossy(&e.data), when).into_bytes();
    }

    // mimetype 第一个、不压缩（`EpubWriter` 管）；变了的条目重写（图片不压缩，其余压缩），没变的原样拷贝
    let mut w = crate::epubzip::EpubWriter::create(dst)?;
    for e in entries.iter().filter(|e| e.name != "mimetype") {
        if before.get(&e.name) == Some(&e.data) {
            w.raw_copy(zip.by_name(&e.name).map_err(|err| format!("{}: {err}", e.name))?)?;
        } else {
            w.put(&e.name, &e.data)?;
        }
    }
    w.finish()?;
    Ok(report)
}

/// 把 `<meta property="dcterms:modified">` 的值换成 `when`；没有就在 `</metadata>` 前补一条（EPUB 3 必需）。
fn set_modified(opf: &str, when: &str) -> String {
    for t in html::tags(opf).filter(|t| t.kind == TagKind::Open && crate::wash::opf::is_local(t.name, "meta")) {
        if html::attr_value(&opf[t.start..t.end], "property") != Some("dcterms:modified") {
            continue;
        }
        if let Some(close) = html::tags_in(opf, t.end, opf.len()).find(|c| c.kind == TagKind::Close && crate::wash::opf::is_local(c.name, "meta")) {
            return format!("{}{when}{}", &opf[..t.end], &opf[close.start..]);
        }
    }
    crate::wash::opf::insert_metadata(opf, &format!(r#"<meta property="dcterms:modified">{when}</meta>"#)).unwrap_or_else(|| opf.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_cover_drops_declaration_cover_page_and_image() {
        let opf = r#"<package version="2.0"><metadata><meta name="cover" content="cover"/></metadata><manifest>
    <item id="cover" href="Images/c.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="titlepage" href="Text/title.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest><spine toc="ncx">
    <itemref idref="titlepage"/>
    <itemref idref="ch1"/>
  </spine><guide>
    <reference type="cover" href="Text/title.xhtml" title="封面"/>
  </guide></package>"#;
        let files = |n: &str| -> Option<String> {
            Some(match n {
                "OEBPS/Text/title.xhtml" => r#"<html><body><div><img src="../Images/c.jpg" alt="封面"/></div></body></html>"#,
                "OEBPS/Text/ch1.xhtml" => "<html><body><p>正文</p></body></html>",
                "OEBPS/toc.ncx" => r#"<ncx><navMap><navPoint id="a"><navLabel><text>封面</text></navLabel><content src="Text/title.xhtml"/></navPoint><navPoint id="b"><navLabel><text>一</text></navLabel><content src="Text/ch1.xhtml"/></navPoint></navMap></ncx>"#,
                _ => return None,
            }.to_string())
        };
        let (out, gone, rewritten) = remove_cover(opf, "OEBPS", files);
        assert_eq!(gone, ["OEBPS/Text/title.xhtml", "OEBPS/Images/c.jpg"]);
        for s in ["cover", "title.xhtml", "guide"] {
            assert!(!out.contains(s), "{s} 应去掉: {out}");
        }
        assert!(out.contains(r#"<itemref idref="ch1"/>"#) && out.contains(r#"id="ch1""#), "正文不动: {out}");
        assert_eq!(rewritten.len(), 1);
        let ncx = String::from_utf8(rewritten[0].1.clone()).unwrap();
        assert!(!ncx.contains("title.xhtml") && ncx.contains("ch1.xhtml"), "{ncx}");

        // 正文别的页也用这张图：图留着，只去掉声明和封面页
        let used = |n: &str| if n == "OEBPS/Text/ch1.xhtml" { Some(r#"<html><body><p>字</p><img src="../Images/c.jpg"/></body></html>"#.to_string()) } else { files(n) };
        let (out, gone, _) = remove_cover(opf, "OEBPS", used);
        assert_eq!(gone, ["OEBPS/Text/title.xhtml"]);
        assert!(out.contains(r#"<item id="cover" href="Images/c.jpg" media-type="image/jpeg"/>"#) && !out.contains("cover-image") && !out.contains(r#"name="cover""#), "{out}");
        // 没有封面：原样
        let none = r#"<package><metadata/><manifest/></package>"#;
        assert_eq!(remove_cover(none, "", |_| None).0, none);
    }

    const OPF: &str = r##"<package version="3.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:x</dc:identifier>
    <dc:title id="t1">旧书名</dc:title>
    <meta refines="#t1" property="title-type">main</meta>
    <dc:creator id="c1">作者甲</dc:creator>
    <meta refines="#c1" property="role" scheme="marc:relators">aut</meta>
    <dc:creator id="c2">作者乙</dc:creator>
    <dc:language>en</dc:language>
    <dc:subject>旧标签</dc:subject>
  </metadata><manifest/></package>"##;

    #[test]
    fn read_lists_all_values() {
        let m = read(OPF);
        let get = |f: DcField| m.iter().find(|(x, _)| *x == f).unwrap().1.clone();
        assert_eq!(get(DcField::Creator), ["作者甲", "作者乙"]);
        assert_eq!(get(DcField::Title), ["旧书名"]);
        assert!(get(DcField::Publisher).is_empty());
    }

    #[test]
    fn set_replaces_in_place_and_drops_refines() {
        let out = apply_fields(
            OPF,
            &[
                (DcField::Title, vec!["新书名 & 副题".into()]),
                (DcField::Creator, vec!["作者丙".into()]),
                (DcField::Publisher, vec!["出版社".into()]),
                (DcField::Subject, vec![]),
            ],
        )
        .unwrap();
        let m = read(&out);
        let get = |f: DcField| m.iter().find(|(x, _)| *x == f).unwrap().1.clone();
        assert_eq!(get(DcField::Title), ["新书名 & 副题"]);
        assert_eq!(get(DcField::Creator), ["作者丙"]);
        assert_eq!(get(DcField::Publisher), ["出版社"]);
        assert!(get(DcField::Subject).is_empty(), "空列表 = 删掉");
        assert_eq!(get(DcField::Identifier), ["urn:x"], "没给的字段不动");
        assert!(!out.contains("refines=\"#t1\"") && !out.contains("refines=\"#c1\""), "挂在旧元素上的 refines 一起删：{out}");
        assert!(out.find("<dc:title>").unwrap() < out.find("<dc:creator>").unwrap(), "新值写在原来的位置");
        assert!(out.contains("新书名 &amp; 副题"));
        // 再改一次结果一样（幂等）
        let again = apply_fields(&out, &[(DcField::Title, vec!["新书名 & 副题".into()])]).unwrap();
        assert_eq!(read(&again), read(&out));
    }

    /// 坏 OPF 里元素套着元素（`<dc:title>` 没闭合就又开一个、作者写在书名里面）：要删的范围互相重叠。以前按原文偏移从后往前
    /// 删，删完里面那个再删外面那个时偏移已经越界，panic（`booklib meta --edit`、生成时补元数据都会遇到外来的 OPF）。
    #[test]
    fn nested_elements_do_not_panic() {
        let opf = r##"<package version="3.0"><metadata><dc:title>甲<dc:title>乙</dc:title></dc:title><dc:subject>A<dc:creator id="c">作者</dc:creator></dc:subject><meta refines="#c" property="role">aut</meta></metadata></package>"##;
        let out = apply_fields(opf, &[(DcField::Title, vec!["新".into()]), (DcField::Creator, vec!["丙".into()]), (DcField::Subject, vec!["标签".into()])]).unwrap();
        let m = read(&out);
        let get = |f: DcField| m.iter().find(|(x, _)| *x == f).unwrap().1.clone();
        assert_eq!(get(DcField::Title), ["新"], "{out}");
        assert_eq!(get(DcField::Creator), ["丙"], "套在别的元素里的作者也换掉，新值不丢：{out}");
        assert_eq!(get(DcField::Subject), ["标签"], "{out}");
        assert!(!out.contains("refines"), "{out}");
    }

    /// 几个字段都插在同一处（OPF 里原来都没有）时的先后顺序：和以前从后往前插入的结果一样（后给的字段在前）。
    #[test]
    fn inserts_at_same_place_keep_previous_order() {
        let opf = "<package><metadata><dc:identifier>x</dc:identifier></metadata></package>";
        let out = apply_fields(opf, &[(DcField::Publisher, vec!["社".into()]), (DcField::Date, vec!["2020".into()])]).unwrap();
        assert_eq!(out, "<package><metadata><dc:identifier>x</dc:identifier><dc:date>2020</dc:date><dc:publisher>社</dc:publisher></metadata></package>");
    }

    /// 带 `opf:` 前缀的写法（`<opf:package>`、`<opf:metadata>`、`<opf:meta refines>`）：EPUB2 判定、插入点、挂在旧元素上的 refines 都认。
    #[test]
    fn opf_prefixed_package_and_metadata() {
        let opf = r##"<opf:package xmlns:opf="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid"><opf:metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">x</dc:identifier><dc:creator id="c">甲</dc:creator><opf:meta refines="#c" property="role">aut</opf:meta></opf:metadata><opf:manifest/></opf:package>"##;
        let out = apply_fields(opf, &[(DcField::Title, vec!["新".into()]), (DcField::Creator, vec!["乙".into()])]).unwrap();
        assert!(out.contains(r#"<dc:title>新</dc:title></opf:metadata>"#), "没有的字段插在 </opf:metadata> 前：{out}");
        assert!(out.contains(r#"<dc:creator opf:role="aut">乙</dc:creator>"#), "认出是 EPUB2：{out}");
        assert!(!out.contains("refines"), "挂在旧作者上的 <opf:meta refines> 一起删：{out}");
    }

    #[test]
    fn unique_identifier_kept_when_identifiers_replaced() {
        let out = apply_fields(OPF, &[(DcField::Identifier, vec!["isbn:9787".into()])]).unwrap();
        let ids = read(&out).into_iter().find(|(f, _)| *f == DcField::Identifier).unwrap().1;
        assert_eq!(ids, ["urn:x", "isbn:9787"]);
        assert!(out.contains(r#"<dc:identifier id="uid">urn:x</dc:identifier>"#));
    }

    /// 写一本最小 EPUB（图片 deflate 压缩，好看出是不是原样拷贝的）。
    fn epub_with(path: &Path, cover_href: &str, cover_mt: &str) {
        use std::io::Write;
        let opf = format!(
            r#"<package version="2.0" unique-identifier="uid"><metadata><dc:identifier id="uid">x</dc:identifier><dc:title>t</dc:title><meta name="cover" content="cv"/></metadata><manifest><item id="cv" href="{cover_href}" media-type="{cover_mt}"/><item id="p" href="p.png" media-type="image/png"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#
        );
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let d = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (n, b) in [
            ("mimetype", b"application/epub+zip".as_slice()),
            ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#.as_slice()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/c1.xhtml", "<html><body><p>&nbsp;正文</p></body></html>".as_bytes()),
            ("OEBPS/p.png", &[7u8; 64]),
            (&format!("OEBPS/{cover_href}"), b"GIF89a-old"),
        ] {
            z.start_file(n, d).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
    }

    /// 原封面是 GIF：新图原样写进这个条目、manifest 的 media-type 跟着改（此前转成 JPEG 塞进 .gif、media-type 还是 gif）；
    /// 没改的条目原样拷贝（压缩数据不动）；不要规范整理时 XHTML 一个字节不变，要时 OPF 升到 3.0。
    #[test]
    fn edit_epub_retypes_gif_cover_and_raw_copies_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("a.epub"), d.path().join("b.epub"));
        epub_with(&src, "cv.gif", "image/gif");
        let mut png = Vec::new();
        image::RgbImage::new(4, 6).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let edits = Edits { cover: Some(CoverEdit::Set(png.clone())), ..Default::default() };
        assert_eq!(edit_epub(&src, &dst, &edits).unwrap().cover_replaced, Some(true));
        let mut z = zip::ZipArchive::new(std::fs::File::open(&dst).unwrap()).unwrap();
        let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        assert_eq!(names[0], "mimetype");
        assert_eq!(z.by_index(0).unwrap().compression(), zip::CompressionMethod::Stored);
        let read = |z: &mut zip::ZipArchive<std::fs::File>, n: &str| crate::epubzip::read_by_name(z, n).unwrap();
        assert_eq!(read(&mut z, "OEBPS/cv.gif"), png);
        let opf = String::from_utf8(read(&mut z, "OEBPS/content.opf")).unwrap();
        assert!(opf.contains(r#"<item id="cv" href="cv.gif" media-type="image/png"/>"#), "{opf}");
        assert_eq!(read(&mut z, "OEBPS/c1.xhtml"), "<html><body><p>&nbsp;正文</p></body></html>".as_bytes(), "不整理：正文条目原样");
        assert_eq!(z.by_name("OEBPS/p.png").unwrap().compression(), zip::CompressionMethod::Deflated, "没改的图片原样拷贝，压缩方式不变");

        let edits = Edits { set: vec![(DcField::Title, vec!["新".into()])], normalize: true, ..Default::default() };
        edit_epub(&src, &dst, &edits).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&dst).unwrap()).unwrap();
        let opf = String::from_utf8(read(&mut z, "OEBPS/content.opf")).unwrap();
        assert!(opf.contains(r#"version="3.0""#) && opf.contains("<dc:title>新</dc:title>"), "{opf}");
        assert_eq!(read(&mut z, "OEBPS/cv.gif"), b"GIF89a-old");
    }

    #[test]
    fn ncx_title_replaced() {
        let ncx = "<ncx><docTitle><text>旧</text></docTitle><navMap/></ncx>";
        assert_eq!(set_ncx_title(ncx, "新 <书>"), "<ncx><docTitle><text>新 &lt;书&gt;</text></docTitle><navMap/></ncx>");
    }
}

//! EPUB 元数据（OPF 里的 Dublin Core 与封面）的读取和改写：`ebook-meta` 命令、书库生成时补简介/标签/封面共用。
//!
//! 改写只动 OPF（改了书名时连 NCX 的书名）和封面图这几个条目，其余条目按原样**原始拷贝**（不解压不重压），
//! 正文一个字节都不变。
//! 设字段 = 先删掉这个字段的全部元素（连同 EPUB3 用 `refines="#id"` 挂在它们身上的 `<meta>`，如作者的角色、排序名），
//! 再在第一个被删元素原来的位置写新值；书里原来没有这个字段就插在 `</metadata>` 前面。

use crate::html::{self, TagKind};
use crate::util::xml_escape;
use std::io::{Read, Seek, Write};
use std::path::Path;

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
        let id = html::attr_value(tag, "id").filter(|v| !v.is_empty()).map(str::to_string);
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
    /// 封面图（JPEG/PNG 字节）。书里声明了封面图就原地换掉它的内容（格式不同时转成原图的格式），没有就新加一个。
    pub cover: Option<Vec<u8>>,
    /// 去掉书里的封面（见 [`remove_cover`]）。和 `cover` 同时给时以 `cover` 为准（换封面）。
    pub remove_cover: bool,
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.cover.is_none() && !self.remove_cover
    }
}

/// 改写 OPF 文本里的字段（封面另算，见 [`edit_epub`]）。`<metadata>` 找不到时报错。
pub fn apply_fields(opf: &str, set: &[(DcField, Vec<String>)]) -> Result<String, String> {
    let meta_close = html::tags(opf).find(|t| t.kind == TagKind::Close && (t.is("metadata") || t.is("opf:metadata"))).ok_or("OPF 里没有 </metadata>")?.start;
    let elems = dc_elements(opf);
    let epub2 = html::tags(opf)
        .find(|t| t.is_start() && t.is("package"))
        .and_then(|t| html::attr_value(&opf[t.start..t.end], "version"))
        .is_some_and(|v| v.starts_with('2'))
        && opf.contains("xmlns:opf");
    // 要删的范围 + 插入点（按位置排序后从后往前改）
    let mut removals: Vec<(usize, usize)> = Vec::new();
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let mut removed_ids: Vec<String> = Vec::new();
    // `<package unique-identifier="X">` 指向的那个标识符不能删（删了 OPF 不合法，NCX 的 dtb:uid 也对不上，reMarkable 会不显示目录）
    let uid = html::tags(opf).find(|t| t.is_start() && t.is("package")).and_then(|t| html::attr_value(&opf[t.start..t.end], "unique-identifier")).map(str::to_string);
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
        for t in html::tags(opf).filter(|t| t.is_start() && t.is("meta")) {
            let Some(r) = html::attr_value(&opf[t.start..t.end], "refines") else { continue };
            if !removed_ids.iter().any(|id| r.strip_prefix('#') == Some(id.as_str())) {
                continue;
            }
            let end = if t.kind == TagKind::SelfClosing { t.end } else { html::find_close(opf, t.end, t.name).map_or(t.end, |c| c.end) };
            removals.push((t.start, end));
        }
    }
    // 从后往前：同一位置先插入再删除（插入点就是第一个被删元素的开头）
    let mut ops: Vec<(usize, Option<usize>, String)> = removals.into_iter().map(|(s, e)| (s, Some(e), String::new())).collect();
    ops.extend(inserts.into_iter().map(|(at, text)| (at, None, text)));
    ops.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.is_none().cmp(&b.1.is_none())));
    let mut out = opf.to_string();
    for (at, end, text) in ops {
        match end {
            Some(e) => out.replace_range(at..e, ""),
            None => out.insert_str(at, &text),
        }
    }
    Ok(out)
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
    let path = crate::epubzip::resolve(opf_dir, &crate::epubzip::percent_decode(item.href));
    let ext = crate::util::image_ext_of(&path);
    Some((path, ext))
}

/// 把图片转成 `ext`（jpg/png）格式；已经是就原样返回。
fn to_format(image: &[u8], ext: &str) -> Result<Vec<u8>, String> {
    let (have, _) = crate::convert::common::image_ext_mime(image).ok_or("封面图不是 JPEG/PNG")?;
    let want = if ext == "jpeg" { "jpg" } else { ext };
    if have == want {
        return Ok(image.to_vec());
    }
    let img = image::load_from_memory(image).map_err(|e| format!("封面图解不开：{e}"))?;
    let mut out = Vec::new();
    let fmt = if want == "png" { image::ImageFormat::Png } else { image::ImageFormat::Jpeg };
    let img = if fmt == image::ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
    img.write_to(&mut std::io::Cursor::new(&mut out), fmt).map_err(|e| format!("封面图转格式失败：{e}"))?;
    Ok(out)
}

/// 要改写的 zip 条目：(路径, 新内容)。
pub type Rewritten = (String, Vec<u8>);

/// 去掉书里的封面（2026-09-30 用户要）：封面声明（`<meta name="cover">`、`cover-image`）、**只放封面图的页面**（没有可见文字、
/// 只有这一张图；连同 spine、guide、NCX、nav 里指向它的条目），以及封面图本身——正文别的页也用着这张图时图留着，只去掉声明。
/// `read(zip 路径)` 取文本。返回 (新 OPF, 要从 zip 删掉的条目, 要改写的条目 (路径, 新内容))。书里没有声明封面图 → 原样。
pub fn remove_cover(opf: &str, opf_dir: &str, mut read: impl FnMut(&str) -> Option<String>) -> (String, Vec<String>, Vec<Rewritten>) {
    use crate::epubzip::{percent_decode, resolve, resolve_href};
    use crate::wash::opf as o;
    let Some(cover) = o::declared_cover(opf) else { return (opf.to_string(), Vec::new(), Vec::new()) };
    let (cover_id, cover_path) = (cover.id.to_string(), resolve(opf_dir, &percent_decode(cover.href)));
    let items = o::manifest_items(opf);
    let path_of = |href: &str| resolve(opf_dir, &percent_decode(href));
    // 页面里引用的图（img src、SVG image href）
    let images_in = |page: &str, text: &str| -> Vec<String> {
        html::tags(text)
            .filter(|t| t.is_start() && (o::is_local(t.name, "img") || o::is_local(t.name, "image")))
            .filter_map(|t| ["src", "xlink:href", "href"].iter().find_map(|a| html::attr_value(&text[t.start..t.end], a)))
            .filter(|v| !html::is_external(v))
            .map(|v| resolve_href(page, &crate::util::xml_unescape(v)).0)
            .collect()
    };
    let (mut cover_pages, mut used_elsewhere) = (Vec::<(String, String)>::new(), false);
    for it in items.iter().filter(|i| i.media_type.contains("html")) {
        let page = path_of(it.href);
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
        if o::is_local(t.name, "reference") && html::attr_value(tag, "href").is_some_and(|h| page_paths.contains(&path_of(html::split_href(h).0).as_str())) {
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
            let file = path_of(it.href);
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
        if !pages.contains(&crate::epubzip::resolve_href(file, link).0.as_str()) {
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

fn read_text<R: Read + Seek>(z: &mut zip::ZipArchive<R>, name: &str) -> Result<String, String> {
    let b = crate::epubzip::read_by_name(z, name)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// 读一本 EPUB 的元数据：(OPF 路径, 各字段)。
pub fn read_epub(path: &Path) -> Result<Vec<(DcField, Vec<String>)>, String> {
    let (_, _, opf) = crate::epubzip::open_opf(path)?;
    Ok(read(&opf))
}

/// 把 `src` 改好写到 `dst`（`dst` 不能是 `src`；原地改由调用方先写临时文件再改名）。
pub fn edit_epub(src: &Path, dst: &Path, edits: &Edits) -> Result<EditReport, String> {
    let (mut zin, opf_path, opf_text) = crate::epubzip::open_opf(src)?;
    let opf_dir = crate::epubzip::dir_of(&opf_path).to_string();
    let mut opf = apply_fields(&opf_text, &edits.set)?;
    let mut report = EditReport { fields: edits.set.iter().map(|(f, _)| *f).collect(), cover_replaced: None, cover_removed: None };

    // 换了书名：NCX 的 docTitle 一起换
    let mut replaced: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some((_, titles)) = edits.set.iter().find(|(f, _)| *f == DcField::Title) {
        if let Some(t) = titles.first() {
            let ncx = crate::wash::manifest_items(&opf)
                .iter()
                .find(|i| i.media_type.contains("dtbncx"))
                .map(|i| crate::epubzip::resolve(&opf_dir, &crate::epubzip::percent_decode(i.href)));
            if let Some(ncx) = ncx {
                if let Ok(text) = read_text(&mut zin, &ncx) {
                    replaced.push((ncx, set_ncx_title(&text, t).into_bytes()));
                }
            }
        }
    }
    // 去封面
    let mut dropped: Vec<String> = Vec::new();
    if edits.remove_cover && edits.cover.is_none() {
        let (new_opf, gone, rewritten) = remove_cover(&opf, &opf_dir, |n| read_text(&mut zin, n).ok());
        opf = new_opf;
        replaced.extend(rewritten);
        dropped = gone;
        report.cover_removed = Some(dropped.clone());
    }
    // 封面
    let mut added: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(image) = &edits.cover {
        match declared_cover(&opf, &opf_dir) {
            Some((path, ext)) => {
                replaced.push((path, to_format(image, &ext)?));
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
                added.push((if opf_dir.is_empty() { name } else { format!("{opf_dir}/{name}") }, image.clone()));
                report.cover_replaced = Some(false);
            }
        }
    }
    replaced.push((opf_path, opf.into_bytes()));

    let out = std::fs::File::create(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let mut zw = zip::ZipWriter::new(out);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for i in 0..zin.len() {
        let e = zin.by_index_raw(i).map_err(|e| e.to_string())?;
        let name = e.name().to_string();
        if dropped.contains(&name) {
            continue;
        }
        match replaced.iter().find(|(n, _)| *n == name) {
            Some((_, data)) => {
                drop(e);
                let opts = if crate::util::is_image_ext(&name) { stored } else { deflated };
                zw.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
                zw.write_all(data).map_err(|e| e.to_string())?;
            }
            None => zw.raw_copy_file(e).map_err(|e| e.to_string())?,
        }
    }
    for (name, data) in &added {
        zw.start_file(name.as_str(), stored).map_err(|e| e.to_string())?;
        zw.write_all(data).map_err(|e| e.to_string())?;
    }
    zw.finish().map_err(|e| e.to_string())?;
    Ok(report)
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

    #[test]
    fn unique_identifier_kept_when_identifiers_replaced() {
        let out = apply_fields(OPF, &[(DcField::Identifier, vec!["isbn:9787".into()])]).unwrap();
        let ids = read(&out).into_iter().find(|(f, _)| *f == DcField::Identifier).unwrap().1;
        assert_eq!(ids, ["urn:x", "isbn:9787"]);
        assert!(out.contains(r#"<dc:identifier id="uid">urn:x</dc:identifier>"#));
    }

    #[test]
    fn ncx_title_replaced() {
        let ncx = "<ncx><docTitle><text>旧</text></docTitle><navMap/></ncx>";
        assert_eq!(set_ncx_title(ncx, "新 <书>"), "<ncx><docTitle><text>新 &lt;书&gt;</text></docTitle><navMap/></ncx>");
    }
}

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
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.cover.is_none()
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

/// 书里声明的封面图：(manifest id, zip 路径, 扩展名)。`<meta name="cover">` 优先，其次 `properties="cover-image"`；必须指向图片。
fn declared_cover(opf: &str, opf_dir: &str) -> Option<(String, String, String)> {
    let items = crate::wash::manifest_items(opf);
    let by_meta = crate::wash::cover_meta_re().find(opf).and_then(|m| crate::wash::tag_attr(m.as_str(), "content")).and_then(|id| items.iter().find(|i| i.id == id));
    let item = by_meta.or_else(|| items.iter().find(|i| i.properties.split_whitespace().any(|p| p == "cover-image")))?;
    if !crate::util::is_image_ext(item.href) {
        return None;
    }
    let path = crate::epubzip::resolve(opf_dir, &crate::epubzip::percent_decode(item.href));
    Some((item.id.to_string(), path.clone(), crate::util::image_ext_of(&path)))
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

/// 改完之后的摘要（给命令行打印）。
#[derive(Debug, Default, PartialEq)]
pub struct EditReport {
    pub fields: Vec<DcField>,
    /// 封面：`Some(true)` 换掉了书里原有的封面图，`Some(false)` 新加了封面。
    pub cover_replaced: Option<bool>,
}

fn read_text<R: Read + Seek>(z: &mut zip::ZipArchive<R>, name: &str) -> Result<String, String> {
    let b = crate::epubzip::read_by_name(z, name)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// 读一本 EPUB 的元数据：(OPF 路径, 各字段)。
pub fn read_epub(path: &Path) -> Result<Vec<(DcField, Vec<String>)>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: 不是 EPUB（{e}）", path.display()))?;
    let container = read_text(&mut z, "META-INF/container.xml")?;
    let opf_path = crate::wash::tag_attr(&container, "full-path").ok_or("container.xml 里没有 full-path")?.to_string();
    Ok(read(&read_text(&mut z, &opf_path)?))
}

/// 把 `src` 改好写到 `dst`（`dst` 不能是 `src`；原地改由调用方先写临时文件再改名）。
pub fn edit_epub(src: &Path, dst: &Path, edits: &Edits) -> Result<EditReport, String> {
    let f = std::fs::File::open(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut zin = zip::ZipArchive::new(f).map_err(|e| format!("{}: 不是 EPUB（{e}）", src.display()))?;
    let container = read_text(&mut zin, "META-INF/container.xml")?;
    let opf_path = crate::wash::tag_attr(&container, "full-path").ok_or("container.xml 里没有 full-path")?.to_string();
    let opf_dir = crate::epubzip::dir_of(&opf_path).to_string();
    let mut opf = apply_fields(&read_text(&mut zin, &opf_path)?, &edits.set)?;
    let mut report = EditReport { fields: edits.set.iter().map(|(f, _)| *f).collect(), cover_replaced: None };

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
    // 封面
    let mut added: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(image) = &edits.cover {
        match declared_cover(&opf, &opf_dir) {
            Some((_, path, ext)) => {
                replaced.push((path, to_format(image, &ext)?));
                report.cover_replaced = Some(true);
            }
            None => {
                let (ext, mime) = crate::convert::common::image_ext_mime(image).ok_or("封面图不是 JPEG/PNG")?;
                let name = format!("eink-cover.{ext}");
                let item = format!(r#"<item id="eink-cover" href="{name}" media-type="{mime}" properties="cover-image"/>"#);
                let m = opf.find("</manifest>").ok_or("OPF 没有 </manifest>")?;
                opf.insert_str(m, &item);
                // 没有指向图片的封面声明：旧的（常见指向 txt 的坏声明）去掉，换成新的
                if let Some(old) = crate::wash::cover_meta_re().find(&opf).map(|m| m.range()) {
                    opf.replace_range(old, "");
                }
                let md = html::tags(&opf).find(|t| t.kind == TagKind::Close && (t.is("metadata") || t.is("opf:metadata"))).ok_or("OPF 里没有 </metadata>")?.start;
                opf.insert_str(md, r#"<meta name="cover" content="eink-cover"/>"#);
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

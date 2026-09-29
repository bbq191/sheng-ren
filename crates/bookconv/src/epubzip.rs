//! EPUB 的 zip 层与 zip 内 posix 路径工具（清洗/优化/质量门/占位共用的最底层）。
//!
//! - [`Entry`]：zip 条目（目录项已剔除）。
//! - [`read_entries`]：整本读入；[`read_skeleton`]：只读"骨架"——图片条目留空占位、其余整份读（流式优化、
//!   质量门的阶段一）。
//! - [`cover_image_of`]：只读 container.xml、OPF 与少数几个条目取出封面图。
//! - `posix_norm/dir_of/resolve/relative_to/percent_decode/is_html`：EPUB 内路径与文件名判断。
use std::io::{Read, Seek, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// zip 条目（目录项已剔除）。
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn is_html(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".xhtml") || l.ends_with(".html") || l.ends_with(".htm")
}

/// `is_html` 纯按扩展名判断，漏掉一种真实存在的 EPUB 形态：章节文件**没有任何扩展名**，但 OPF
/// manifest 正确声明了 `media-type="application/xhtml+xml"`（2026-09-23 真机《甲午：摇摆的战争》
/// 坐实：`Chapter_2`/`Chapter_7_1`/`_2`/`_3` 四个真实章节文件是这个形态，跟同一本书里其它带
/// `.xhtml` 后缀的章节混在一起）。这类文件被 `is_html` 判定"不是 html"后，清洗/优化管线从未碰过
/// 它们——脚注图标没转换、内联 `font-size` 死值没清零，真机复现"这几章字体锁死改不了、脚注图标
/// 巨大"。只在**完全没有扩展名**时才嗅探内容开头（有扩展名但不是 html 家族的——css/opf/ncx/图片/
/// 字体等——一律信扩展名，不误判），避免把真正的非 html 资源当章节处理。
pub fn is_html_entry(name: &str, data: &[u8]) -> bool {
    if is_html(name) {
        return true;
    }
    let base = name.rsplit('/').next().unwrap_or(name);
    if base.contains('.') {
        return false;
    }
    let head = String::from_utf8_lossy(&data[..data.len().min(200)]);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    let head_lower = head.to_ascii_lowercase();
    head.starts_with("<?xml") || head_lower.starts_with("<!doctype html") || head_lower.starts_with("<html")
}

/// 按 zip 目录声明的解压大小预分配时的上限。声明大小来自文件本身：损坏或恶意的条目可以声称几 GB，照单
/// `Vec::with_capacity` 在内存紧的设备上会直接分配失败、整个进程 abort（不是 `catch_unwind` 兜得住的 panic）。
/// 真实条目超过这个值时 `read_to_end` 照常按需扩容，结果不变。
const PREALLOC_CAP: u64 = 32 * 1024 * 1024;

/// 读完一个 zip 条目的全部字节（`declared` = 目录里声明的解压大小，只用来预分配，封顶 [`PREALLOC_CAP`]）。
/// 本模块各读取入口共用。
pub(crate) fn read_all(mut r: impl Read, declared: u64) -> Result<Vec<u8>, String> {
    let mut v = Vec::with_capacity(declared.min(PREALLOC_CAP) as usize);
    r.read_to_end(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}

/// 不压缩的条目选项（EPUB 的 `mimetype` 必须 STORED 且排第一；`epub::assemble` 全部条目也用它）。
pub(crate) fn stored() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

/// deflate 压缩的条目选项（缺省级别）。
pub(crate) fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

/// 往 zip 写一个完整条目（`start_file` + `write_all`，错误转成字符串）。优化器两条路径、`epub::assemble`、占位文档共用
/// （此前每处都是一对 `.map_err(|e| e.to_string())?` 样板）。
pub(crate) fn put_entry<W: Write + Seek>(zw: &mut ZipWriter<W>, name: &str, opts: SimpleFileOptions, data: &[u8]) -> Result<(), String> {
    zw.start_file(name, opts).map_err(|e| e.to_string())?;
    zw.write_all(data).map_err(|e| e.to_string())
}

/// [`read_skeleton`] 的结果。
pub struct Skeleton {
    /// 条目表：图片条目（`imgopt::is_page_image`）的 `data` 为空占位，其余是真实字节。
    pub entries: Vec<Entry>,
}

/// 读"骨架"：非图片条目整份读，图片条目只记名字和大小、`data` 留空（真实字节留到阶段二按需读回）。
/// 流式路径的峰值内存因此是"全书文字 + 一张图"而不是"全书图片"。目录项剔除；任一条目读失败整体报错
/// （绝不能静默跳过条目产出残缺 EPUB）。
pub fn read_skeleton<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Skeleton, String> {
    let mut entries = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("读 EPUB 条目 {i}: {e}"))?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        let data = if crate::imgopt::is_page_image(&name) {
            Vec::new()
        } else {
            let size = f.size();
            read_all(&mut f, size)?
        };
        entries.push(Entry { name, data });
    }
    Ok(Skeleton { entries })
}

/// 按名字读一个 zip 条目的全部字节；条目不存在 → `Ok(None)`，其它（损坏/IO）错误 → `Err`。
/// 流式路径"图片按需从源 zip 读回"的统一入口。
pub fn read_by_name_opt<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Option<Vec<u8>>, String> {
    let mut f = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let size = f.size();
    read_all(&mut f, size).map(Some)
}

/// 同 [`read_by_name_opt`]，条目不存在也算错误。
pub fn read_by_name<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    read_by_name_opt(zip, name)?.ok_or_else(|| format!("zip 里没有条目 {name}"))
}

/// 从 zip 字节读条目表（目录项剔除，图片也整份读）。优化器与质量门共用。
pub fn read_entries(epub: &[u8]) -> Result<Vec<Entry>, String> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(epub)).map_err(|e| format!("解 EPUB(非 zip?): {e}"))?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| format!("读 EPUB 条目 {i}: {e}"))?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        let size = f.size();
        out.push(Entry { name, data: read_all(&mut f, size)? });
    }
    Ok(out)
}

// ───────────────────────── 按路径读 OPF 与封面 ─────────────────────────

type FileZip = ZipArchive<std::io::BufReader<std::fs::File>>;

/// 条目按文本读（非 UTF-8 字节按 lossy 替换）；不存在或读失败都是 `None`。
fn read_text_opt(zip: &mut FileZip, name: &str) -> Option<String> {
    read_by_name_opt(zip, name).ok().flatten().map(|b| String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// 打开 EPUB 并读出 OPF：`(zip, OPF 在 zip 里的路径, OPF 文本)`。只读 container.xml 和 OPF 两个条目，不解压整本。
pub(crate) fn open_opf(epub: &std::path::Path) -> Result<(FileZip, String, String), String> {
    let file = std::fs::File::open(epub).map_err(|e| format!("打开 {} 失败: {e}", epub.display()))?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| format!("解 EPUB 失败: {e}"))?;
    let container = read_text_opt(&mut zip, "META-INF/container.xml").ok_or("缺 META-INF/container.xml")?;
    let opf_path = crate::wash::tag_attr(&container, "full-path").ok_or("container.xml 里没有 full-path")?.to_string();
    let opf = read_text_opt(&mut zip, &opf_path).ok_or("读不到 OPF")?;
    Ok((zip, opf_path, opf))
}

/// 读出一本 EPUB 的封面图（扩展名, 字节）：OPF 声明的封面（`wash::opf::declared_cover`：`<meta name="cover">` 指向的图片、
/// 其次 `properties="cover-image"`；指向 txt 之类的坏声明不算）；没有就取前几个 spine 页里第一张对得上 manifest 的图
/// （`wash::opf::first_spine_image`，与优化器补封面声明同一套）。只读需要的几个条目，不解压整本；找不到返回 `None`。
pub fn cover_image_of(epub: &std::path::Path) -> Option<(String, Vec<u8>)> {
    use crate::wash::opf;
    let (mut zip, opf_path, text) = open_opf(epub).ok()?;
    let dir = dir_of(&opf_path);
    let path = match opf::declared_cover(&text) {
        Some(it) => resolve(dir, &percent_decode(it.href)),
        None => opf::first_spine_image(&text, dir, 12, false, |p| read_text_opt(&mut zip, p))?,
    };
    Some((crate::util::image_ext_of(&path), read_by_name_opt(&mut zip, &path).ok()??))
}

// ───────────────────────── 路径工具（zip 内 posix 路径） ─────────────────────────

pub fn posix_norm(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

pub fn dir_of(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

pub fn resolve(base_dir: &str, rel: &str) -> String {
    if base_dir.is_empty() {
        posix_norm(rel)
    } else {
        posix_norm(&format!("{base_dir}/{rel}"))
    }
}

/// `target` 相对 `base_dir` 的路径（都是 zip 内绝对路径）。
pub fn relative_to(base_dir: &str, target: &str) -> String {
    let b: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    let t: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = b.iter().zip(t.iter()).take_while(|(x, y)| x == y).count();
    let mut out: Vec<String> = vec!["..".into(); b.len() - common];
    out.extend(t[common..].iter().map(|s| s.to_string()));
    out.join("/")
}

/// 链接值 → (目标文件的 zip 路径, 锚点原文)。`base_file` 是链接所在文件的 zip 路径；路径部分为空（`#x`）时目标就是
/// `base_file` 自己。路径先百分号解码再按 `base_file` 所在目录解析、规整（全书各处"链接指向哪个文件"一律走这里）。
/// 书外链接（`http:`、`mailto:`…）不该传进来，调用方先用 [`crate::html::is_external`] 筛掉。
pub fn resolve_href<'a>(base_file: &str, href: &'a str) -> (String, Option<&'a str>) {
    let (p, frag) = crate::html::split_href(href);
    let path = if p.is_empty() { base_file.to_string() } else { resolve(dir_of(base_file), &percent_decode(p)) };
    (path, frag)
}

/// zip 内路径写进 `href`/`src` 用的百分号编码：字母数字与 `-._~/` 原样，其余（空格、`#`、`%`、非 ASCII）按 UTF-8 字节编码。
pub fn encode_href_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 从 `from_dir`（zip 内目录）指向 `target`（zip 内路径）的链接值：相对路径百分号编码，`frag`（锚点原文）非空时带 `#frag`。
/// 结果是属性值原文，写进 XML 前调用方仍要 `xml_escape`（锚点里可能有 `&`）。
pub fn href_to(from_dir: &str, target: &str, frag: &str) -> String {
    let rel = encode_href_path(&relative_to(from_dir, target));
    if frag.is_empty() { rel } else { format!("{rel}#{frag}") }
}

/// `%XX` 解码（XX 必须是两位十六进制，否则原样保留）。此前每个 `%` 现拼一个 `String` 再 `from_str_radix`，
/// 后者还接受 `+` 号前缀，`%+1` 会被误解成字节 0x01。
pub fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let (Some(h), Some(l)) = (b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c))) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    /// 写一本最小 EPUB 到 `path`：OPF 在 `opf_path`，`files` 是其余条目。
    fn write_epub(path: &std::path::Path, opf_path: &str, opf: &str, files: &[(&str, &[u8])]) {
        let mut all: Vec<(&str, &[u8])> = vec![("mimetype", b"application/epub+zip")];
        let container = format!(r#"<container><rootfiles><rootfile full-path="{opf_path}"/></rootfiles></container>"#);
        all.push(("META-INF/container.xml", container.as_bytes()));
        all.push((opf_path, opf.as_bytes()));
        all.extend_from_slice(files);
        std::fs::write(path, zip_of(&all)).unwrap();
    }

    #[test]
    fn cover_image_from_meta_or_first_spine_page() {
        let d = tempfile::tempdir().unwrap();
        let files: &[(&str, &[u8])] = &[("OEBPS/Text/p1.xhtml", br#"<html><body><img src="../images/cv.jpg"/></body></html>"#), ("OEBPS/images/cv.jpg", b"\xFF\xD8COVERBYTES\xFF\xD9")];
        for meta in [r#"<meta name="cover" content="cv"/>"#, ""] {
            let p = d.path().join("real.epub");
            let opf = format!(r#"<package><metadata><dc:title>t</dc:title>{meta}</metadata><manifest><item id="cv" href="images/cv.jpg" media-type="image/jpeg"/><item id="c1" href="Text/p1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#);
            write_epub(&p, "OEBPS/content.opf", &opf, files);
            assert_eq!(cover_image_of(&p), Some(("jpg".to_string(), b"\xFF\xD8COVERBYTES\xFF\xD9".to_vec())), "meta={meta:?}");
        }
        assert_eq!(cover_image_of(&d.path().join("missing.epub")), None);
    }

    /// Calibre 产物 `<meta name="cover">` 指向 txt：必须回退到第一页的真图片。
    #[test]
    fn cover_declared_as_non_image_falls_back_to_first_page_image() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("calibre.epub");
        let opf = r#"<package><metadata><dc:title>t</dc:title><meta name="cover" content="cover.txt"/></metadata><manifest><item id="cover.txt" href="cover.txt" media-type="text/plain"/><item id="c1" href="p1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
        write_epub(&p, "content.opf", opf, &[("cover.txt", b"not an image"), ("p1.xhtml", br#"<html><body><img src="real.jpg"/></body></html>"#), ("real.jpg", b"\xFF\xD8REALCOVER\xFF\xD9")]);
        assert_eq!(cover_image_of(&p).map(|(_, b)| b), Some(b"\xFF\xD8REALCOVER\xFF\xD9".to_vec()));
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let o = zip::write::SimpleFileOptions::default();
            for (n, d) in files {
                if n.ends_with('/') {
                    z.add_directory(*n, o).unwrap();
                } else {
                    z.start_file(*n, o).unwrap();
                    z.write_all(d).unwrap();
                }
            }
            z.finish().unwrap();
        }
        buf
    }

    #[test]
    fn paths() {
        assert_eq!(posix_norm("OEBPS/../a/./b"), "a/b");
        assert_eq!(resolve("OEBPS/text", "../style.css"), "OEBPS/style.css");
        assert_eq!(relative_to("OEBPS", "OEBPS/text/c1.xhtml"), "text/c1.xhtml");
        assert_eq!(relative_to("OEBPS/text", "OEBPS/style.css"), "../style.css");
        assert_eq!(relative_to("", "a.xhtml"), "a.xhtml");
        assert_eq!(percent_decode("%E5%AD%97.xhtml"), "字.xhtml");
        assert_eq!(percent_decode("a%20b%2"), "a b%2", "末尾不完整的 % 原样保留");
        assert_eq!(percent_decode("%+1x%zz"), "%+1x%zz", "非十六进制（含 + 号）不解码");
        assert_eq!(percent_decode("%41"), "A");
    }

    /// 真机《甲午：摇摆的战争》坐实的真实形态：`Chapter_2`/`Chapter_7_1` 这类没有扩展名的章节文件，
    /// manifest 里正确声明 `application/xhtml+xml`，但纯扩展名判断的 `is_html` 会漏判、整个跳过清洗。
    #[test]
    fn is_html_entry_sniffs_extensionless_chapter_by_content() {
        assert!(is_html_entry("Text/Chapter_2", b"<?xml version=\"1.0\"?><html><body><p>x</p></body></html>"));
        assert!(is_html_entry("Text/Chapter_7_1", b"<!DOCTYPE html><html><body>x</body></html>"));
        assert!(is_html_entry("Text/Chapter_7_1", b"<html><body>x</body></html>"), "没有 XML 声明、直接 <html> 开头也该认");
        assert!(is_html_entry("a.xhtml", b"whatever"), "有正牌扩展名走老路径，不用嗅探内容");
    }

    #[test]
    fn is_html_entry_does_not_misclassify_non_html_extensionless_or_extensioned_files() {
        assert!(!is_html_entry("images/cover", b"\x89PNG\r\n\x1a\n"), "扩展名缺失但内容明显不是 html 的图片，不该被嗅探误判");
        assert!(!is_html_entry("style.css", b"<?xml version=\"1.0\"?>"), "有 .css 扩展名，即便内容巧了像 xml 开头也不该被当 html（信扩展名）");
        assert!(!is_html_entry("book.opf", b"<?xml version=\"1.0\"?><package></package>"), "opf 也是 xml 开头，但有扩展名就不该走嗅探");
    }

    #[test]
    fn skeleton_leaves_images_empty_keeps_text() {
        let bytes = zip_of(&[("dir/", b""), ("a.xhtml", b"<p>hi</p>"), ("images/p1.JPG", &[7u8; 300]), ("images/p2.gif", &[9u8; 10]), ("a.svg", b"<svg/>")]);
        let mut z = ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let sk = read_skeleton(&mut z).unwrap();
        let by: HashMap<&str, &Entry> = sk.entries.iter().map(|e| (e.name.as_str(), e)).collect();
        assert_eq!(sk.entries.len(), 4, "目录项剔除");
        assert_eq!(by["a.xhtml"].data, b"<p>hi</p>");
        assert!(by["images/p1.JPG"].data.is_empty(), "位图留空占位");
        assert!(by["images/p2.gif"].data.is_empty(), "GIF/WebP 也留空（漫画页会处理它们，阶段二按需读）");
        assert_eq!(by["a.svg"].data, b"<svg/>", "SVG 是文字，照常整份读");
    }

    /// 条目在 zip 目录里谎报解压大小（损坏/恶意文件）：不能照单预分配几 GB（设备上分配失败＝进程 abort）。
    #[test]
    fn lying_declared_size_does_not_drive_huge_preallocation() {
        let mut bytes = zip_of(&[("a.xhtml", b"<p>hi</p>")]);
        // 中央目录项 `PK\x01\x02` 偏移 24 是 4 字节 uncompressed size，改成 ~4GB。
        let cd = bytes.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
        bytes[cd + 24..cd + 28].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        let mut z = ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        // zip crate 不校验声明大小与实际解压量是否一致，照常读出真实内容——所以只能靠预分配封顶兜住。
        let sk = read_skeleton(&mut z).unwrap();
        assert_eq!(sk.entries[0].data, b"<p>hi</p>");
        assert!(sk.entries[0].data.capacity() as u64 <= PREALLOC_CAP, "预分配必须封顶");
        assert!(read_by_name_opt(&mut z, "a.xhtml").unwrap().unwrap().capacity() as u64 <= PREALLOC_CAP);
    }

    #[test]
    fn read_by_name_distinguishes_missing_from_present() {
        let bytes = zip_of(&[("a.txt", b"hello")]);
        let mut z = ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(read_by_name_opt(&mut z, "a.txt").unwrap(), Some(b"hello".to_vec()));
        assert_eq!(read_by_name_opt(&mut z, "nope").unwrap(), None, "条目不存在不是错误");
        assert!(read_by_name(&mut z, "nope").unwrap_err().contains("nope"));
        assert_eq!(read_by_name(&mut z, "a.txt").unwrap(), b"hello");
    }

    #[test]
    fn read_entries_reads_everything_including_images_and_rejects_non_zip() {
        let bytes = zip_of(&[("a.xhtml", b"x"), ("i.png", &[1u8; 5])]);
        let e = read_entries(&bytes).unwrap();
        assert_eq!(e, vec![Entry { name: "a.xhtml".into(), data: b"x".to_vec() }, Entry { name: "i.png".into(), data: vec![1u8; 5] }]);
        assert!(read_entries(b"definitely not a zip").unwrap_err().contains("非 zip"));
    }
}

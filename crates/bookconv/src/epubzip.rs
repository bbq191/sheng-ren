//! EPUB 的 zip 层与 zip 内 posix 路径工具（清洗/优化/质量门/占位共用的最底层）。
//!
//! - [`Entry`]：zip 条目（目录项已剔除）。
//! - [`read_entries`]：整本读入；[`read_skeleton`]：只读"骨架"——图片条目留空占位、其余整份读（流式优化、
//!   质量门的阶段一）；两者都走 [`read_entries_from`]。
//! - [`EpubWriter`]：写 EPUB（`mimetype` 置首 STORED，图片 STORED、其余 deflate，可原样拷贝源条目）。
//! - [`cover_image_of`]：只读 container.xml、OPF 与少数几个条目取出封面图。
//! - `posix_norm/dir_of/resolve/resolve_rel/resolve_href/relative_to/percent_decode/is_html`：EPUB 内路径与文件名判断；
//!   [`resolve_link`]：链接属性原文 → (zip 路径, 解码后的锚点)，全书解析链接的统一入口。
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

/// 单个条目解压后的上限。真实书里最大的条目（600dpi 扫描的漫画页、整本一个文件的网文、嵌入字体）远小于它；
/// 几 KB 的压缩数据能解出几 GB（zip 炸弹），目录里声明的大小也可以造假，所以读的时候按实际解出的字节数截。
/// EPUB 和 CBZ 的各读取入口共用（此前只有 CBZ 收页设了上限，读 EPUB 条目没有）。
pub const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// 读完一个 zip 条目的全部字节（`declared` = 目录里声明的解压大小，只用来预分配，封顶 [`PREALLOC_CAP`]）；
/// 解出来超过 [`MAX_ENTRY_BYTES`] 报错。本模块各读取入口和 CBZ 收页共用。
pub(crate) fn read_all(r: impl Read, declared: u64, name: &str) -> Result<Vec<u8>, String> {
    read_all_capped(r, declared, name, MAX_ENTRY_BYTES)
}

fn read_all_capped(r: impl Read, declared: u64, name: &str, cap: u64) -> Result<Vec<u8>, String> {
    let mut v = Vec::with_capacity(declared.min(PREALLOC_CAP).min(cap) as usize);
    r.take(cap + 1).read_to_end(&mut v).map_err(|e| format!("{name}: {e}"))?;
    if v.len() as u64 > cap {
        return Err(format!("{name}: 解压后超过单个条目上限 {} MB（损坏或恶意的压缩包？）", cap >> 20));
    }
    Ok(v)
}

/// 不压缩的条目选项。
fn stored() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

/// deflate 压缩的条目选项（缺省级别）。
fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

/// 写 EPUB 的 zip：建的时候先写 `mimetype`（第一个条目、STORED、内容就是 `application/epub+zip`，EPUB 规范 OCF 的要求），
/// 之后 [`put`](EpubWriter::put) 按条目名选压缩方式——本身已压缩的图片（jpg/png/gif/webp，再 deflate 几乎没收益、白花 CPU）
/// STORED，其余 deflate（缺省级别）。优化器、`opfmeta` 改元数据、`epub::assemble` 组装母版共用（此前各写一份）。
/// 条目时间戳是 zip 的缺省值（没开 zip 的 `time` 特性），同样的输入写出逐字节相同。
pub struct EpubWriter<W: Write + Seek> {
    zw: ZipWriter<W>,
}

impl EpubWriter<std::io::BufWriter<std::fs::File>> {
    /// 建输出文件（带缓冲）并写好 `mimetype`。
    pub fn create(path: &std::path::Path) -> Result<Self, String> {
        let f = std::fs::File::create(path).map_err(|e| format!("建输出文件 {} 失败: {e}", path.display()))?;
        EpubWriter::new(std::io::BufWriter::new(f))
    }
}

impl<W: Write + Seek> EpubWriter<W> {
    /// 在 `w` 上开一个 EPUB 并写好 `mimetype`。
    pub fn new(w: W) -> Result<Self, String> {
        let mut me = EpubWriter { zw: ZipWriter::new(w) };
        me.put_with("mimetype", stored(), MIMETYPE)?;
        Ok(me)
    }

    /// 写一个条目：图片 STORED，其余 deflate（见类型文档）。不要再写 `mimetype`（建的时候写过了）。
    pub fn put(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let opts = if crate::util::is_image_ext(name) { stored() } else { deflated() };
        self.put_with(name, opts, data)
    }

    /// 写一个 STORED 条目（`epub::assemble` 的母版全部不压缩）。
    pub fn put_stored(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        self.put_with(name, stored(), data)
    }

    fn put_with(&mut self, name: &str, opts: SimpleFileOptions, data: &[u8]) -> Result<(), String> {
        self.zw.start_file(name, opts).map_err(|e| e.to_string())?;
        self.zw.write_all(data).map_err(|e| e.to_string())
    }

    /// 原样拷贝源 zip 的一个条目（压缩数据一个字节不动，不解压不重压）。
    pub fn raw_copy(&mut self, f: zip::read::ZipFile) -> Result<(), String> {
        self.zw.raw_copy_file(f).map_err(|e| e.to_string())
    }

    /// 写中央目录并 flush（`finish()` 只保证写完中央目录，底下 `BufWriter` 的缓冲不一定落盘——显式 flush，
    /// 不指望 Drop 的静默兜底，出错会被吞掉）。
    pub fn finish(self) -> Result<W, String> {
        let mut w = self.zw.finish().map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())?;
        Ok(w)
    }
}

/// EPUB 规范：`mimetype` 的内容（不带换行）。
pub const MIMETYPE: &[u8] = b"application/epub+zip";

/// [`read_skeleton`] 的结果。
pub struct Skeleton {
    /// 条目表：图片条目（`imgopt::is_page_image`）的 `data` 为空占位，其余是真实字节。
    pub entries: Vec<Entry>,
}

/// 逐个读 zip 条目（目录项剔除，zip 里的顺序）：`keep_bytes(条目名)` 为假的只记名字、`data` 留空占位。
/// 任一条目读失败整体报错（绝不能静默跳过条目产出残缺 EPUB）。[`read_skeleton`]、[`read_entries`]、质量门共用。
pub fn read_entries_from<R: Read + Seek>(zip: &mut ZipArchive<R>, keep_bytes: impl Fn(&str) -> bool) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("读 EPUB 条目 {i}: {e}"))?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        let data = if keep_bytes(&name) {
            let size = f.size();
            read_all(&mut f, size, &name)?
        } else {
            Vec::new()
        };
        entries.push(Entry { name, data });
    }
    Ok(entries)
}

/// 读"骨架"：非图片条目整份读，图片条目只记名字、`data` 留空（真实字节留到阶段二按需读回）。
/// 流式路径的峰值内存因此是"全书文字 + 一张图"而不是"全书图片"。
pub fn read_skeleton<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Skeleton, String> {
    Ok(Skeleton { entries: read_entries_from(zip, |n| !crate::imgopt::is_page_image(n))? })
}

/// 按名字读一个 zip 条目的全部字节；条目不存在 → `Ok(None)`，其它（损坏/IO）错误 → `Err`。
/// 流式路径"图片按需从源 zip 读回"的统一入口。
pub fn read_by_name_opt<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Option<Vec<u8>>, String> {
    let mut f = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(format!("{name}: {e}")),
    };
    let size = f.size();
    read_all(&mut f, size, name).map(Some)
}

/// 同 [`read_by_name_opt`]，条目不存在也算错误。
pub fn read_by_name<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    read_by_name_opt(zip, name)?.ok_or_else(|| format!("zip 里没有条目 {name}"))
}

/// 从 zip 字节读条目表（目录项剔除，图片也整份读）。
pub fn read_entries(epub: &[u8]) -> Result<Vec<Entry>, String> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(epub)).map_err(|e| format!("解 EPUB(非 zip?): {e}"))?;
    read_entries_from(&mut archive, |_| true)
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
        Some(it) => it.path(dir),
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

/// 已还原字符引用的相对路径（还可能百分号编码，不带 `#锚点`）→ zip 路径：先百分号解码，再按 `base_dir` 解析。
/// OPF manifest 项用 [`crate::wash::ManifestItem::path`]（属性原文还要先还原 `&amp;` 等）；带锚点、以所在文件为基准的链接用 [`resolve_href`]。
pub fn resolve_rel(base_dir: &str, href: &str) -> String {
    resolve(base_dir, &percent_decode(href))
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
    let path = if p.is_empty() { base_file.to_string() } else { resolve_rel(dir_of(base_file), p) };
    (path, frag)
}

/// 链接**属性原文** → (目标文件的 zip 路径, 解码后的锚点)。全书"属性原文里的链接指向哪里"一律走这里：依次还原字符引用
/// （`a&amp;b.xhtml` 是文件 `a&b.xhtml`）、拆出锚点、路径与锚点各自百分号解码、按 `base_file` 所在目录解析规整；路径部分为空
/// （`#x`）时目标就是 `base_file`。锚点拿去对 id 时，id 也要先还原字符引用（`util::xml_unescape`）。没有 `#` 时锚点是 `None`。
/// 要把锚点**原样写回**链接的调用方（目录、改链）别用这里的锚点，用 [`crate::html::split_href`] 拆出的原文。
/// 书外链接不该传进来，调用方先用 [`crate::html::is_external`] 筛掉。
pub fn resolve_link(base_file: &str, raw: &str) -> (String, Option<String>) {
    let v = crate::util::xml_unescape(raw);
    let (p, frag) = crate::html::split_href(&v);
    let path = if p.is_empty() { base_file.to_string() } else { resolve_rel(dir_of(base_file), p) };
    (path, frag.map(percent_decode))
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

    #[test]
    fn resolve_link_unescapes_then_splits_and_decodes() {
        assert_eq!(resolve_link("OEBPS/t/c.xhtml", "../n&amp;b%20x.xhtml#%E6%B3%A8&amp;1"), ("OEBPS/n&b x.xhtml".to_string(), Some("注&1".to_string())));
        assert_eq!(resolve_link("OEBPS/c.xhtml", "#a"), ("OEBPS/c.xhtml".to_string(), Some("a".to_string())));
        assert_eq!(resolve_link("c.xhtml", "./x.xhtml"), ("x.xhtml".to_string(), None));
    }

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

    /// zip 炸弹：压缩数据很小、解出来超过上限的条目报错，而不是一直解到内存耗尽（以前读 EPUB 条目没有上限）。
    /// 生产上限 256MB，测试用同一实现、小上限。
    #[test]
    fn entry_bigger_than_cap_is_rejected_not_read_to_the_end() {
        let bytes = zip_of(&[("a.xhtml", &[b'x'; 5000]), ("b.xhtml", &[b'y'; 100])]);
        let mut z = ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let read = |z: &mut ZipArchive<std::io::Cursor<&Vec<u8>>>, n: &str, cap| {
            let mut f = z.by_name(n).unwrap();
            let size = f.size();
            read_all_capped(&mut f, size, n, cap)
        };
        let err = read(&mut z, "a.xhtml", 4096).unwrap_err();
        assert!(err.contains("a.xhtml") && err.contains("上限"), "{err}");
        assert_eq!(read(&mut z, "a.xhtml", 5000).unwrap().len(), 5000, "正好等于上限的照常读");
        assert_eq!(read(&mut z, "b.xhtml", 4096).unwrap(), vec![b'y'; 100]);
        // 谎报很小的声明大小也拦得住（上限按实际解出的字节数算）
        let mut lying = zip_of(&[("a.xhtml", &[b'x'; 5000])]);
        let cd = lying.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
        lying[cd + 24..cd + 28].copy_from_slice(&10u32.to_le_bytes());
        let mut z = ZipArchive::new(std::io::Cursor::new(&lying)).unwrap();
        let mut f = z.by_name("a.xhtml").unwrap();
        assert!(read_all_capped(&mut f, 10, "a.xhtml", 4096).is_err());
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

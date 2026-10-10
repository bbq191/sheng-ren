//! 最小合规 EPUB3 组装（格式转换器产出母版用）。mimetype 首个 STORED。
//! 所有条目都走 STORED（不压缩）：母版只是中间产物，按设备优化时会重新打包压缩。

use crate::htmlproc::fix_internal_links;

pub struct Chapter {
    pub title: String,
    pub html_body: String,
    /// 目录层级（1=部/篇顶层，>=2 为其下的章/节）。用于生成嵌套 nav。
    pub level: i64,
}

pub struct BookMeta {
    pub book_id: String,
    pub title: String,
    pub author: String,
    pub language: String,
    pub publisher: String,
    pub cover: Option<Vec<u8>>,
    pub cover_ext: String,
    pub cover_media_type: String,
    /// `<dc:subject>` 标签（CBZ 转出来的写"漫画"，见 [`crate::comic_detect::COMIC_SUBJECT`]）。
    pub subjects: Vec<String>,
}

/// 章节 HTML 引用的图片等资源（如漫画页、网页插图）。
/// `path` 为 OEBPS 内相对路径（如 `images/img1.jpg`），章节 html 以此相对引用。
pub struct Resource {
    pub path: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

pub struct Book {
    pub meta: BookMeta,
    pub chapters: Vec<Chapter>,
    /// 章节引用的嵌入资源（插图、漫画页等），可以为空。
    pub resources: Vec<Resource>,
    /// 显式目录。空＝沿用"每个有标题的章节文件一条目录"；非空＝按这里生成 nav，条目可以指向
    /// 文件内锚点、也可以多条指向同一个文件。
    pub nav: Vec<NavEntry>,
}

/// 一条目录：`href` 是相对 `OEBPS/` 的章节文件名，可带 `#锚点`。
#[derive(Clone, Debug, PartialEq)]
pub struct NavEntry {
    pub title: String,
    pub level: i64,
    pub href: String,
}

/// 文字与属性值共用的 XML 转义（`& < > "`，见 `util::xml_escape`）。
use crate::util::xml_escape as xesc;

/// `assemble` 写出的 OPF 在 zip 里的路径（`container.xml` 指向它）。
pub const OPF_PATH: &str = "OEBPS/content.opf";
/// 转换器组装的 EPUB 在 OPF `dc:identifier` 里写的前缀：`urn:bookconv:{book_id}`。
pub const ID_SCHEME: &str = "urn:bookconv:";

/// 转换器组装的 EPUB 里第 `i` 章（0 起）的 XHTML 文件名：`chap_0001.xhtml`……（在 `OEBPS/` 下）。
pub fn chapter_filename(i: usize) -> String {
    format!("chap_{:04}.xhtml", i + 1)
}

/// `<html>` 上的语言属性：书的语言（`BookMeta::language`）同时写 `lang` 与 `xml:lang`；语言未知或不是合法的语言标签
/// （只允许字母、数字、`-`，`und` 视同未知）时不写，交给清洗层按正文判断后补。
fn lang_attrs(lang: &str) -> String {
    let lang = lang.trim();
    let valid = !lang.is_empty() && !lang.eq_ignore_ascii_case("und") && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid { format!(" lang=\"{lang}\" xml:lang=\"{lang}\"") } else { String::new() }
}

/// 一章的完整 XHTML 文档。`head_extra` 原样插在 `</head>` 前（共用样式表的 `<link>`），没有时传 `""`。
fn chapter_doc(ch: &Chapter, lang: &str, head_extra: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"{}>\n<head><title>{}</title>{head_extra}</head>\n<body>{}</body>\n</html>\n",
        lang_attrs(lang),
        xesc(&ch.title),
        ch.html_body
    )
}

fn container_xml() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n  <rootfiles>\n    <rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\n  </rootfiles>\n</container>\n"
}

fn cover_xhtml(m: &BookMeta) -> String {
    // 封面**水平+垂直双居中**、限高一屏：早先只 text-align:center（仅水平居中、height:auto 垂直贴顶），
    // 宽高比偏方/偏宽的封面宽度撑满后高度不足就贴页顶 → 书库缩略图「偏上」。table/table-cell +
    // vertical-align:middle 是老渲染器（xochitl epub 引擎）也吃的垂直居中法；max-height:100vh 防超高。
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"{}>\n<head><title>封面</title>\n<style>html,body{{margin:0;padding:0;height:100%;}} .cv{{display:table;width:100%;height:100vh;}} .cv-c{{display:table-cell;vertical-align:middle;text-align:center;}} .cv-c img{{max-width:100%;max-height:100vh;}}</style></head>\n<body>\n  <div class=\"cv\"><div class=\"cv-c\"><img src=\"cover.{}\" alt=\"{}\"/></div></div>\n</body>\n</html>\n",
        lang_attrs(&m.language),
        m.cover_ext,
        xesc(&m.title)
    )
}

fn content_opf(book: &Book, opts: &AssembleOpts) -> String {
    let m = &book.meta;
    let has_cover = m.cover.is_some();
    let mut manifest: Vec<String> = vec![
        "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>".to_string(),
    ];
    let mut spine: Vec<String> = Vec::new();
    if has_cover {
        manifest.push(format!(
            "    <item id=\"cover-image\" href=\"cover.{}\" media-type=\"{}\" properties=\"cover-image\"/>",
            m.cover_ext, m.cover_media_type
        ));
        manifest.push("    <item id=\"cover\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>".to_string());
        spine.push("    <itemref idref=\"cover\"/>".to_string());
    }
    for i in 0..book.chapters.len() {
        let cid = format!("c{}", i + 1);
        let href = chapter_filename(i);
        manifest.push(format!(
            "    <item id=\"{cid}\" href=\"{href}\" media-type=\"application/xhtml+xml\"/>"
        ));
        spine.push(format!("    <itemref idref=\"{cid}\"/>"));
    }
    // 嵌入资源（插图等），只进 manifest、不进 spine
    for (i, r) in book.resources.iter().enumerate() {
        manifest.push(format!(
            "    <item id=\"res{}\" href=\"{}\" media-type=\"{}\"/>",
            i + 1,
            xesc(&r.path),
            xesc(&r.media_type)
        ));
    }
    if let Some(css) = &opts.shared_css {
        manifest.push(format!("    <item id=\"{}\" href=\"{}\" media-type=\"text/css\"/>", xesc(&css.id), xesc(&css.file)));
    }
    let id_scheme = opts.id_scheme.as_deref().unwrap_or(ID_SCHEME);
    let author = if m.author.is_empty() {
        String::new()
    } else {
        format!("\n    <dc:creator>{}</dc:creator>", xesc(&m.author))
    };
    let publisher = if m.publisher.is_empty() {
        String::new()
    } else {
        format!("\n    <dc:publisher>{}</dc:publisher>", xesc(&m.publisher))
    };
    let subjects: String = m.subjects.iter().map(|s| format!("\n    <dc:subject>{}</dc:subject>", xesc(s))).collect();
    let cover_meta = if has_cover {
        "\n    <meta name=\"cover\" content=\"cover-image\"/>"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"pub-id\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"pub-id\">{}{}</dc:identifier>\n    <dc:title>{}</dc:title>\n    <dc:language>{}</dc:language>{}{}{}{}\n  </metadata>\n  <manifest>\n{}\n  </manifest>\n  <spine>\n{}\n  </spine>\n</package>\n",
        xesc(id_scheme),
        xesc(&m.book_id),
        xesc(&m.title),
        xesc(&m.language),
        author,
        publisher,
        subjects,
        cover_meta,
        manifest.join("\n"),
        spine.join("\n")
    )
}

/// 生成嵌套 nav，层级不限：`level<=1` 为顶层，更深的收进前一条的子 `<ol>`。层级不能跳级——比前一条深不止一级时
/// 按"前一条的下一级"算（章下面直接是 3 级标题也只缩进一层）；书首就是子级的条目按顶层算。只收非空标题。
fn nav_body(book: &Book) -> String {
    let visible: Vec<NavEntry> = if book.nav.is_empty() {
        book.chapters
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.title.is_empty())
            .map(|(i, c)| NavEntry { title: c.title.clone(), level: c.level, href: chapter_filename(i) })
            .collect()
    } else {
        book.nav.iter().filter(|n| !n.title.is_empty()).cloned().collect()
    };
    // 每条的实际深度（1 起）。
    let mut depths: Vec<usize> = Vec::with_capacity(visible.len());
    for n in &visible {
        let prev = depths.last().copied().unwrap_or(0);
        depths.push((n.level.max(1) as usize).min(prev + 1));
    }
    // 深度 d 的 `<li>` 缩进 6+4(d-1) 格，它的子 `<ol>` 再缩进 2 格。
    let li_indent = |d: usize| " ".repeat(6 + 4 * (d - 1));
    let ol_indent = |d: usize| " ".repeat(8 + 4 * (d - 1));
    let mut out = String::new();
    let mut open: Vec<usize> = Vec::new(); // 子 `<ol>` 还没闭合的父条目深度
    for (i, (n, &d)) in visible.iter().zip(&depths).enumerate() {
        let link = format!("<a href=\"{}\">{}</a>", xesc(&n.href), xesc(&n.title));
        let next = depths.get(i + 1).copied().unwrap_or(0);
        if next > d {
            out.push_str(&format!("{}<li>{link}\n{}<ol>\n", li_indent(d), ol_indent(d)));
            open.push(d);
            continue;
        }
        out.push_str(&format!("{}<li>{link}</li>\n", li_indent(d)));
        // 下一条比这些父条目浅（或没有下一条）：闭合它们的子 `<ol>` 和 `<li>`。
        while let Some(&p) = open.last() {
            if p < next {
                break;
            }
            out.push_str(&format!("{}</ol>\n{}</li>\n", ol_indent(p), li_indent(p)));
            open.pop();
        }
    }
    out
}

fn nav_xhtml(book: &Book) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"{}>\n<head><title>目录</title></head>\n<body>\n  <nav epub:type=\"toc\" id=\"toc\">\n    <ol>\n{}    </ol>\n  </nav>\n</body>\n</html>\n",
        lang_attrs(&book.meta.language),
        nav_body(book)
    )
}


/// 章节共用的一份外链样式表（[`AssembleOpts::shared_css`]）：写成 `OEBPS/{file}`（zip 里排在资源之后），进 manifest，
/// 按 `link_if` 挂到章节的 `<head>` 上。
pub struct SharedCss {
    /// `OEBPS/` 下的文件名，也是章节 `<link href>` 的值（章节都在 `OEBPS/` 下，相对路径即文件名），如 `extra.css`。
    pub file: String,
    /// manifest 里的 `id`，不能和组装器自己的 id（`nav`、`cover`、`cover-image`、`c1`…、`res1`…）重名。
    pub id: String,
    /// 样式表内容。
    pub content: String,
    /// 哪些章节挂 `<link>`：参数是章节正文（`Chapter::html_body`，已做过链接规整）。`None` = 每章都挂。
    /// 只给用得着的章节挂，免得纯文字章也被一条外链样式表改动排版。
    pub link_if: Option<fn(&str) -> bool>,
}

/// [`assemble_with`] 的选项。`Default` = 与 [`assemble`] 完全相同（产物逐字节一致）。
#[derive(Default)]
pub struct AssembleOpts {
    /// 写完一份资源就释放它（组装后 `book.resources` 为空）：调用方不再用这批资源时，省掉"资源 + zip 缓冲"同时驻留的一整份体积。
    pub consume_resources: bool,
    /// OPF `dc:identifier` 的前缀（写成 `{前缀}{book_id}`）。`None` = [`ID_SCHEME`]。调用方靠自己的前缀认出自己转出来的书时用。
    pub id_scheme: Option<String>,
    /// 章节共用的外链样式表（见 [`SharedCss`]）。`None` = 不写。
    pub shared_css: Option<SharedCss>,
}

/// 把 Book 打包成 EPUB 字节。组装前对每章：先 fix_internal_links（脚注同文件锚点规整），
/// 再 break_footnote_cycles（拆双向脚注互指对——reMarkable 索引器遇互指对会整对丢弃致点不动）。
pub fn assemble(book: &mut Book) -> Result<Vec<u8>, String> {
    assemble_with(book, AssembleOpts::default())
}

/// 转换器（CBZ/网页文章）组装**与设备无关的母版 EPUB** 的统一收尾：同 [`assemble`]，但写完一份资源
/// 就释放（组装后 `book.resources` 为空），不让"资源表 + zip 缓冲"同时各占一整份体积。产物字节与 [`assemble`] 相同。
/// 按设备的优化（图片缩放、字体解锁等）在入库之后按 profile 另做，不在转换时写死某台设备。
pub fn assemble_master(book: &mut Book) -> Result<Vec<u8>, String> {
    assemble_with(book, AssembleOpts { consume_resources: true, ..Default::default() })
}

/// 带选项的组装（[`AssembleOpts`]）：[`assemble`]、[`assemble_master`] 都是它的特例。组装前同样对每章做
/// `fix_internal_links` 与 `break_footnote_cycles`；mimetype 首个，全部条目 STORED。
pub fn assemble_with(book: &mut Book, opts: AssembleOpts) -> Result<Vec<u8>, String> {
    if book.chapters.is_empty() {
        return Err("EPUB 至少要有一章".into());
    }
    // 预留足够容量：全部 STORED，产物 ≈ 资源 + 章节文本 + 少量固定条目。Vec 倍增扩容会在峰值瞬间同时持有新旧两块。
    let cap = book.resources.iter().map(|r| r.bytes.len()).sum::<usize>()
        + book.chapters.iter().map(|c| c.html_body.len() + 512).sum::<usize>()
        + opts.shared_css.as_ref().map_or(0, |c| c.content.len())
        + 32 * 1024;
    let mut buf: Vec<u8> = Vec::with_capacity(cap);
    let consume = opts.consume_resources;
    write_book(book, &opts, std::io::Cursor::new(&mut buf), |z, book| {
        if consume {
            for r in std::mem::take(&mut book.resources) {
                z.put_stored(&format!("OEBPS/{}", r.path), &r.bytes)?; // r 在本次迭代结束即释放
            }
        } else {
            for r in &book.resources {
                z.put_stored(&format!("OEBPS/{}", r.path), &r.bytes)?;
            }
        }
        Ok(())
    })?;
    Ok(buf)
}

/// 同 [`assemble_master`]，但资源的字节不在 `book.resources` 里（`bytes` 留空，只用 `path`、`media_type` 写 manifest）：写到资源那一步时
/// 按顺序调 `load(第几个资源)` 取一份、写完就丢，写进 `w`（如打开的文件）。产物与把字节放进 `resources` 再 [`assemble_master`] 逐字节相同，
/// 峰值内存只有一份资源（CBZ 整本转母版用，见 `convert::cbz`）。
pub(crate) fn assemble_master_streaming<W: std::io::Write + std::io::Seek>(book: &mut Book, w: W, mut load: impl FnMut(usize) -> Result<Vec<u8>, String>) -> Result<W, String> {
    if book.chapters.is_empty() {
        return Err("EPUB 至少要有一章".into());
    }
    let opts = AssembleOpts { consume_resources: true, ..Default::default() };
    write_book(book, &opts, w, |z, book| {
        for (i, r) in book.resources.iter().enumerate() {
            z.put_stored(&format!("OEBPS/{}", r.path), &load(i)?)?;
        }
        Ok(())
    })
}

/// 组装的共同部分：每章规整链接、拆脚注互指环 → OPF → 按固定顺序写条目（container、OPF、nav、封面、各章、资源、共用样式表）。
/// 资源由 `put_resources` 写（字节在内存里还是逐个读进来由调用方定）。章节不能为空（调用方查）。
fn write_book<W: std::io::Write + std::io::Seek>(book: &mut Book, opts: &AssembleOpts, w: W, put_resources: impl FnOnce(&mut crate::epubzip::EpubWriter<W>, &mut Book) -> Result<(), String>) -> Result<W, String> {
    for ch in book.chapters.iter_mut() {
        ch.html_body = fix_internal_links(&ch.html_body, None);
        ch.html_body = crate::htmlproc::break_footnote_cycles(&ch.html_body);
    }
    let opf = content_opf(book, opts);
    // mimetype 首个、STORED（`EpubWriter::new` 写），其余也全部 STORED
    let mut z = crate::epubzip::EpubWriter::new(w)?;
    z.put_stored("META-INF/container.xml", container_xml().as_bytes())?;
    z.put_stored(OPF_PATH, opf.as_bytes())?;
    drop(opf);
    z.put_stored("OEBPS/nav.xhtml", nav_xhtml(book).as_bytes())?;
    if let Some(cover) = &book.meta.cover {
        z.put_stored(&format!("OEBPS/cover.{}", book.meta.cover_ext), cover)?;
        z.put_stored("OEBPS/cover.xhtml", cover_xhtml(&book.meta).as_bytes())?;
    }
    let css_link = opts.shared_css.as_ref().map(|c| (format!("<link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>", xesc(&c.file)), c.link_if));
    for (i, ch) in book.chapters.iter().enumerate() {
        let head_extra = match &css_link {
            Some((link, cond)) if cond.is_none_or(|f| f(&ch.html_body)) => link.as_str(),
            _ => "",
        };
        z.put_stored(&format!("OEBPS/{}", chapter_filename(i)), chapter_doc(ch, &book.meta.language, head_extra).as_bytes())?;
    }
    put_resources(&mut z, book)?;
    if let Some(css) = &opts.shared_css {
        z.put_stored(&format!("OEBPS/{}", css.file), css.content.as_bytes())?;
    }
    Ok(z.finish()?)
}

#[cfg(test)]
mod nav_tests {
    use super::*;
    fn ch(title: &str, level: i64) -> Chapter {
        Chapter { title: title.into(), html_body: "<p>x</p>".into(), level }
    }
    #[test]
    fn nests_level2_under_level1() {
        let book = Book {
            meta: BookMeta {
                book_id: "b".into(), title: "t".into(), author: "".into(), language: "zh".into(),
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
            chapters: vec![ch("第一部", 1), ch("第一章", 2), ch("第二章", 2), ch("第二部", 1)],
            resources: vec![],
            nav: Vec::new(),
        };
        let nav = nav_body(&book);
        // 第一部带子 ol 包住两章，第二部无子节平铺
        assert!(nav.contains("第一部"), "{nav}");
        assert!(nav.matches("<ol>").count() == 1, "应恰有一个子 ol: {nav}");
        assert!(nav.matches("</ol>").count() == 1, "子 ol 未闭合: {nav}");
        let pos_part = nav.find("第一章").unwrap();
        let pos_ol = nav.find("<ol>").unwrap();
        assert!(pos_ol < pos_part, "第一章应在子 ol 内: {nav}");
    }
    #[test]
    fn embeds_resources_into_zip_and_manifest() {
        let mut book = Book {
            meta: BookMeta {
                book_id: "b".into(), title: "t".into(), author: "".into(), language: "zh".into(),
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
            chapters: vec![Chapter { title: "章".into(), html_body: "<p><img src=\"images/a.png\"/></p>".into(), level: 1 }],
            resources: vec![Resource { path: "images/a.png".into(), media_type: "image/png".into(), bytes: vec![1, 2, 3, 4] }],
            nav: Vec::new(),
        };
        let opf = content_opf(&book, &AssembleOpts::default());
        assert!(opf.contains("id=\"res1\" href=\"images/a.png\" media-type=\"image/png\""), "manifest 缺资源项: {opf}");
        let bytes = assemble(&mut book).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut f = zip.by_name("OEBPS/images/a.png").expect("zip 内应有资源文件");
        use std::io::Read;
        let mut got = Vec::new();
        f.read_to_end(&mut got).unwrap();
        assert_eq!(got, vec![1, 2, 3, 4]);
    }

    #[test]
    fn nests_arbitrary_depth_and_never_skips_levels() {
        let book = Book {
            meta: BookMeta {
                book_id: "b".into(), title: "t".into(), author: "".into(), language: "en".into(),
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
            chapters: vec![ch("部", 1), ch("章", 2), ch("节", 3), ch("小节", 4), ch("章二", 2), ch("跳级", 4), ch("部二", 1)],
            resources: vec![],
            nav: Vec::new(),
        };
        let nav = nav_body(&book);
        assert_eq!(nav.matches("<ol>").count(), nav.matches("</ol>").count(), "ol 要配对: {nav}");
        assert_eq!(nav.matches("<li>").count(), nav.matches("</li>").count(), "li 要配对: {nav}");
        // 整份包进 <ol> 后必须是合法 XML，且层级正确：小节在第 4 层，跳级的 4 只比章二深一级。
        let xml = format!("<ol>{nav}</ol>");
        let mut r = quick_xml::Reader::from_str(&xml);
        let (mut depth, mut max_depth) = (0usize, 0usize);
        let mut depth_of: std::collections::HashMap<String, usize> = Default::default();
        loop {
            match r.read_event().unwrap() {
                quick_xml::events::Event::Start(e) if e.name().as_ref() == b"li" => {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                }
                quick_xml::events::Event::End(e) if e.name().as_ref() == b"li" => depth -= 1,
                quick_xml::events::Event::Text(t) => {
                    let t = t.unescape().unwrap().trim().to_string();
                    if !t.is_empty() {
                        depth_of.insert(t, depth);
                    }
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
        assert_eq!(max_depth, 4, "{nav}");
        assert_eq!((depth_of["部"], depth_of["章"], depth_of["节"], depth_of["小节"]), (1, 2, 3, 4), "{nav}");
        assert_eq!((depth_of["章二"], depth_of["跳级"], depth_of["部二"]), (2, 3, 1), "{nav}");
        assert!(nav_xhtml(&book).contains(r#" lang="en" xml:lang="en">"#), "nav 用书的语言");
    }

    #[test]
    fn lang_attrs_follow_book_language_or_are_omitted() {
        assert_eq!(lang_attrs("zh-CN"), r#" lang="zh-CN" xml:lang="zh-CN""#);
        assert_eq!(lang_attrs(""), "");
        assert_eq!(lang_attrs("und"), "");
        assert_eq!(lang_attrs("zh\" onload=\"x"), "", "不合法的语言标签不写");
        let mut b = two_chapter_book(false);
        b.meta.language = String::new();
        let entries = zip_names_and_text(assemble(&mut b).unwrap());
        let chap = String::from_utf8(entries.iter().find(|(k, _)| k == "OEBPS/chap_0001.xhtml").unwrap().1.clone()).unwrap();
        assert!(chap.contains("<html xmlns=\"http://www.w3.org/1999/xhtml\">"), "语言未知不写: {chap}");
        let mut b = two_chapter_book(false);
        b.meta.language = "ja".into();
        let entries = zip_names_and_text(assemble(&mut b).unwrap());
        let chap = String::from_utf8(entries.iter().find(|(k, _)| k == "OEBPS/chap_0001.xhtml").unwrap().1.clone()).unwrap();
        assert!(chap.contains(r#"<html xmlns="http://www.w3.org/1999/xhtml" lang="ja" xml:lang="ja">"#), "{chap}");
    }

    #[test]
    fn flat_when_all_level1() {
        let book = Book {
            meta: BookMeta {
                book_id: "b".into(), title: "t".into(), author: "".into(), language: "zh".into(),
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
            chapters: vec![ch("一", 1), ch("二", 1)],
            resources: vec![],
            nav: Vec::new(),
        };
        let nav = nav_body(&book);
        assert!(!nav.contains("<ol>"), "全顶层不应有子 ol: {nav}");
    }
    fn two_chapter_book(with_img: bool) -> Book {
        Book {
            meta: BookMeta {
                book_id: "b".into(), title: "t".into(), author: "".into(), language: "zh".into(),
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
            chapters: vec![
                Chapter { title: "图页".into(), html_body: if with_img { "<div><IMG src=\"images/a.png\"/></div>".into() } else { "<p>字</p>".into() }, level: 1 },
                Chapter { title: "字页".into(), html_body: "<p>纯文字</p>".into(), level: 1 },
            ],
            resources: vec![Resource { path: "images/a.png".into(), media_type: "image/png".into(), bytes: vec![9; 64] }],
            nav: Vec::new(),
        }
    }

    fn zip_names_and_text(bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
        use std::io::Read;
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        (0..z.len()).map(|i| { let mut f = z.by_index(i).unwrap(); let mut v = Vec::new(); f.read_to_end(&mut v).unwrap(); (f.name().to_string(), v) }).collect()
    }

    #[test]
    fn assemble_with_consume_resources_is_byte_identical_and_empties_resources() {
        let plain = assemble(&mut two_chapter_book(true)).unwrap();
        let mut b = two_chapter_book(true);
        let consumed = assemble_with(&mut b, AssembleOpts { consume_resources: true, ..Default::default() }).unwrap();
        assert_eq!(plain, consumed, "consume_resources 只影响内存，不影响产物");
        assert!(b.resources.is_empty(), "资源写完即释放");
    }

    #[test]
    fn shared_css_links_only_matching_chapters_and_id_scheme_is_configurable() {
        fn has_img(body: &str) -> bool {
            body.contains("<IMG")
        }
        let mut b = two_chapter_book(true);
        let opts = AssembleOpts {
            consume_resources: true,
            id_scheme: Some("urn:x:".into()),
            shared_css: Some(SharedCss { file: "extra.css".into(), id: "extra-css".into(), content: "img{max-width:100%;}\n".into(), link_if: Some(has_img) }),
        };
        let entries = zip_names_and_text(assemble_with(&mut b, opts).unwrap());
        assert_eq!(entries.last().unwrap().0, "OEBPS/extra.css", "样式表排在资源之后");
        let get = |n: &str| String::from_utf8(entries.iter().find(|(k, _)| k == n).unwrap().1.clone()).unwrap();
        assert_eq!(get("OEBPS/extra.css"), "img{max-width:100%;}\n");
        assert!(get("OEBPS/chap_0001.xhtml").contains("<title>图页</title><link rel=\"stylesheet\" type=\"text/css\" href=\"extra.css\"/></head>"));
        assert!(!get("OEBPS/chap_0002.xhtml").contains("<link"), "不满足 link_if 的章不挂");
        let opf = get(OPF_PATH);
        assert!(opf.contains(">urn:x:b</dc:identifier>"), "{opf}");
        assert!(opf.contains("    <item id=\"extra-css\" href=\"extra.css\" media-type=\"text/css\"/>\n  </manifest>"), "{opf}");
        // link_if 为 None：每章都挂
        let mut b = two_chapter_book(false);
        let opts = AssembleOpts { shared_css: Some(SharedCss { file: "a.css".into(), id: "a".into(), content: String::new(), link_if: None }), ..Default::default() };
        let entries = zip_names_and_text(assemble_with(&mut b, opts).unwrap());
        assert_eq!(entries.iter().filter(|(k, v)| k.contains("chap_") && String::from_utf8_lossy(v).contains("href=\"a.css\"")).count(), 2);
    }

    #[test]
    fn explicit_nav_entries_can_share_a_file_and_carry_fragments() {
        let mut b = two_chapter_book(false);
        b.nav = vec![
            NavEntry { title: "栏目".into(), level: 1, href: "chap_0001.xhtml".into() },
            NavEntry { title: "文章甲".into(), level: 2, href: "chap_0001.xhtml#a".into() },
            NavEntry { title: "文章乙".into(), level: 2, href: "chap_0002.xhtml#b".into() },
        ];
        let nav = nav_body(&b);
        assert!(nav.contains("<li><a href=\"chap_0001.xhtml\">栏目</a>\n        <ol>"), "{nav}");
        assert!(nav.contains("<li><a href=\"chap_0001.xhtml#a\">文章甲</a></li>") && nav.contains("<li><a href=\"chap_0002.xhtml#b\">文章乙</a></li>"), "{nav}");
        assert!(!nav.contains("图页") && !nav.contains("字页"), "有显式目录时不再按章节生成: {nav}");
    }

}

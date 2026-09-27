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
}

/// 章节 HTML 引用的图片等资源（如 FB2/MOBI 内联插图）。
/// `path` 为 OEBPS 内相对路径（如 `images/img1.jpg`），章节 html 以此相对引用。
pub struct Resource {
    pub path: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

pub struct Book {
    pub meta: BookMeta,
    pub chapters: Vec<Chapter>,
    /// 章节引用的嵌入资源；默认空（墨香下书路径不用）。FB2/MOBI 转换填插图。
    pub resources: Vec<Resource>,
    /// 显式目录。空＝沿用"每个有标题的章节文件一条目录"；非空＝按这里生成 nav，条目可以指向
    /// 文件内锚点、也可以多条指向同一个文件（PDF→EPUB 单文件模式/分组标题，见
    /// `pdf_ingest::to_epub`）。
    pub nav: Vec<NavEntry>,
}

/// 一条目录：`href` 是相对 `OEBPS/` 的章节文件名，可带 `#锚点`。
#[derive(Clone, Debug, PartialEq)]
pub struct NavEntry {
    pub title: String,
    pub level: i64,
    pub href: String,
}

/// 对齐 xml.sax.saxutils.escape：只转 & < >（不动引号）。
use crate::util::xml_escape as xesc;

/// `assemble` 写出的 OPF 在 zip 里的路径（`container.xml` 指向它；PDF 来源识别等也按这个路径读）。
pub(crate) const OPF_PATH: &str = "OEBPS/content.opf";
/// 转换器组装的 EPUB 在 OPF `dc:identifier` 里写的前缀：`urn:bookconv:{book_id}`。
pub(crate) const ID_SCHEME: &str = "urn:bookconv:";

pub(crate) fn chapter_filename(i: usize) -> String {
    format!("chap_{:04}.xhtml", i + 1)
}

/// `<html>` 上的语言属性：书的语言（`BookMeta::language`）同时写 `lang` 与 `xml:lang`；语言未知或不是合法的语言标签
/// （只允许字母、数字、`-`，`und` 视同未知）时不写，交给清洗层按正文判断后补。
fn lang_attrs(lang: &str) -> String {
    let lang = lang.trim();
    let valid = !lang.is_empty() && !lang.eq_ignore_ascii_case("und") && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid { format!(" lang=\"{lang}\" xml:lang=\"{lang}\"") } else { String::new() }
}

/// `head_extra` 原样插在 `</head>` 前（外链样式表 `<link>` 等），普通章节传 `""`。
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

fn content_opf(book: &Book) -> String {
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
    let cover_meta = if has_cover {
        "\n    <meta name=\"cover\" content=\"cover-image\"/>"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"pub-id\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"pub-id\">{ID_SCHEME}{}</dc:identifier>\n    <dc:title>{}</dc:title>\n    <dc:language>{}</dc:language>{}{}{}\n  </metadata>\n  <manifest>\n{}\n  </manifest>\n  <spine>\n{}\n  </spine>\n</package>\n",
        xesc(&m.book_id),
        xesc(&m.title),
        xesc(&m.language),
        author,
        publisher,
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


/// 组装时可选的外链共用样式表：写进 `OEBPS/<file>`、OPF manifest 补一项、**只给正文含 `<img` 的章节**挂 `<link>`
/// （纯文字页不该吃到 `body{margin:0}` 之类的清零规则）。组装时一次写成，不必组出整本 zip 再读回改条目。
pub(crate) struct SharedCss<'a> {
    /// `OEBPS/` 下的文件名（也是章节 `<link href>` 的值）。
    pub file: &'a str,
    /// manifest 项 id。
    pub id: &'a str,
    pub css: &'a str,
}

/// [`assemble`] 的内部变体选项。`Default` = 与 [`assemble`] 完全相同。
#[derive(Default)]
pub(crate) struct AssembleOpts<'a> {
    pub shared_css: Option<SharedCss<'a>>,
    /// 写完一份资源就释放它（组装后 `book.resources` 为空）：调用方不再用这批资源时，省掉"资源 + zip 缓冲"同时驻留的一整份体积。
    pub consume_resources: bool,
    /// 书是"从右往左"翻页（日漫）：OPF `<spine>` 写上 `page-progression-direction="rtl"`（xochitl 的日漫翻页
    /// 只看这个属性，2026-09-24）。
    pub rtl: bool,
}

/// 不区分大小写的 `<img` 探测（不为此分配整章小写副本）。
fn has_img_tag(html: &str) -> bool {
    html.as_bytes().windows(4).any(|w| w.eq_ignore_ascii_case(b"<img"))
}

/// PDF→EPUB 颜色 span 探测（`optimize_pdf_to_epub` 生成的 `class="eink-cN"`）——共享 CSS 现在除了
/// `pdf-img.css` 的图片尺寸规则，还可能带颜色规则（见 `assemble_pdf_derived`），纯文字页也可能
/// 用到颜色，不能再只按"有没有 `<img`"决定要不要挂 `<link>`（2026-09-23）。
fn has_color_span(html: &str) -> bool {
    html.contains("class=\"eink-c")
}

/// 把 Book 打包成 EPUB 字节。组装前对每章：先 fix_internal_links（脚注同文件锚点规整），
/// 再 break_footnote_cycles（拆双向脚注互指对——reMarkable 索引器遇互指对会整对丢弃致点不动）。
pub fn assemble(book: &mut Book) -> Result<Vec<u8>, String> {
    assemble_with(book, AssembleOpts::default())
}

/// PDF→EPUB 转出的书专用：`<img>` 没有 `width`/`height`（源自 PDF 页内嵌图，原始像素尺寸），也没有任何
/// 外链 CSS 撑住布局：xochitl
/// 原生阅读器走标准文档流，无 CSS 兜底的 `<img>` 不撑满、甚至整个不出现在渲染结果里（2026-09-23 真机
/// 投一本真实 PDF 手册核实：内部生成的预览 PDF 里 `pdfimages -list` 空，24 张图一张没有）。跟漫画
/// `COMIC_CSS`（`width:100%` 强撑满整页）不是一回事——PDF 里的图是跟正文混排的小插图/二维码，不该被
/// 拉伸到整页宽；用 `max-width:100%` 只封顶超宽图，不撑大本来就小的图，也不清零正文页边距。
/// `extra_css`：`optimize_pdf_to_epub` 返回的颜色 CSS（`.eink-cN{color:#rrggbb;}` 逐条，可能是空
/// 串——全书没解析出任何非黑颜色时就是空）。拼进同一份共享样式表而不是另起一个文件：颜色规则
/// 也得挂在"含 `<img` 或颜色 span 才 `<link>`"这同一条判定里，两份文件反而要维护两条判定逻辑。
/// **组装后 `book.resources` 被清空**（写一张释放一张）：PDF 转出的书图片可达上百 MB，不让"资源表 + zip 缓冲"
/// 同时各占一整份（2026-09-24 审计；调用方组装后不再用 `book`）。
pub fn assemble_pdf_derived(book: &mut Book, extra_css: &str) -> Result<Vec<u8>, String> {
    let css = format!("{PDF_IMG_CSS}{extra_css}");
    assemble_with(book, AssembleOpts { shared_css: Some(SharedCss { file: "pdf-img.css", id: "pdf-img-css", css: &css }), consume_resources: true, rtl: false })
}

const PDF_IMG_CSS: &str = "img{max-width:100%;height:auto;}\n";

pub(crate) fn assemble_with(book: &mut Book, opts: AssembleOpts) -> Result<Vec<u8>, String> {
    if book.chapters.is_empty() {
        return Err("EPUB 至少要有一章".into());
    }
    for ch in book.chapters.iter_mut() {
        ch.html_body = fix_internal_links(&ch.html_body);
        ch.html_body = crate::htmlproc::break_footnote_cycles(&ch.html_body);
    }
    let mut opf = content_opf(book);
    if opts.rtl {
        opf = opf.replacen("<spine>", "<spine page-progression-direction=\"rtl\">", 1);
    }
    if let Some(c) = &opts.shared_css {
        opf = opf.replacen("</manifest>", &format!("<item id=\"{}\" href=\"{}\" media-type=\"text/css\"/></manifest>", c.id, c.file), 1);
    }
    // 预留足够容量：全部 STORED，产物 ≈ 资源 + 章节文本 + 少量固定条目。Vec 倍增扩容会在峰值瞬间同时持有新旧两块。
    let cap = book.resources.iter().map(|r| r.bytes.len()).sum::<usize>()
        + book.chapters.iter().map(|c| c.html_body.len() + 512).sum::<usize>()
        + opf.len()
        + 16 * 1024;
    let mut buf: Vec<u8> = Vec::with_capacity(cap);
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut z = zip::ZipWriter::new(cursor);
        let stored = crate::epubzip::stored();
        let put = crate::epubzip::put_entry;
        // mimetype 必须首个、STORED
        put(&mut z, "mimetype", stored, b"application/epub+zip")?;
        put(&mut z, "META-INF/container.xml", stored, container_xml().as_bytes())?;
        put(&mut z, OPF_PATH, stored, opf.as_bytes())?;
        put(&mut z, "OEBPS/nav.xhtml", stored, nav_xhtml(book).as_bytes())?;
        if let Some(cover) = &book.meta.cover {
            put(&mut z, &format!("OEBPS/cover.{}", book.meta.cover_ext), stored, cover)?;
            put(&mut z, "OEBPS/cover.xhtml", stored, cover_xhtml(&book.meta).as_bytes())?;
        }
        for (i, ch) in book.chapters.iter().enumerate() {
            let link = match &opts.shared_css {
                Some(c) if has_img_tag(&ch.html_body) || has_color_span(&ch.html_body) => format!("<link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>", c.file),
                _ => String::new(),
            };
            put(&mut z, &format!("OEBPS/{}", chapter_filename(i)), stored, chapter_doc(ch, &book.meta.language, &link).as_bytes())?;
        }
        if opts.consume_resources {
            for r in std::mem::take(&mut book.resources) {
                put(&mut z, &format!("OEBPS/{}", r.path), stored, &r.bytes)?; // r 在本次迭代结束即释放
            }
        } else {
            for r in &book.resources {
                put(&mut z, &format!("OEBPS/{}", r.path), stored, &r.bytes)?;
            }
        }
        if let Some(c) = &opts.shared_css {
            put(&mut z, &format!("OEBPS/{}", c.file), stored, c.css.as_bytes())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf)
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
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(),
            },
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
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(),
            },
            chapters: vec![Chapter { title: "章".into(), html_body: "<p><img src=\"images/a.png\"/></p>".into(), level: 1 }],
            resources: vec![Resource { path: "images/a.png".into(), media_type: "image/png".into(), bytes: vec![1, 2, 3, 4] }],
            nav: Vec::new(),
        };
        let opf = content_opf(&book);
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
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(),
            },
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
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(),
            },
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
                publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(),
            },
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
        let consumed = assemble_with(&mut b, AssembleOpts { shared_css: None, consume_resources: true, rtl: false }).unwrap();
        assert_eq!(plain, consumed, "consume_resources 只影响内存，不影响产物");
        assert!(b.resources.is_empty(), "资源写完即释放");
    }

    #[test]
    fn shared_css_goes_last_and_links_only_chapters_with_img_case_insensitively() {
        let out = assemble_with(&mut two_chapter_book(true), AssembleOpts { shared_css: Some(SharedCss { file: "x.css", id: "xcss", css: "img{}" }), consume_resources: false, rtl: true }).unwrap();
        let entries = zip_names_and_text(out);
        assert_eq!(entries.last().unwrap().0, "OEBPS/x.css", "样式表条目排在资源之后");
        assert_eq!(entries.last().unwrap().1, b"img{}");
        let get = |n: &str| String::from_utf8(entries.iter().find(|(k, _)| k == n).unwrap().1.clone()).unwrap();
        assert!(get("OEBPS/chap_0001.xhtml").contains("<link rel=\"stylesheet\" type=\"text/css\" href=\"x.css\"/></head>"), "含 <IMG（大小写不敏感）的章要挂 link");
        assert!(!get("OEBPS/chap_0002.xhtml").contains("<link"), "纯文字章不挂");
        assert!(get("OEBPS/content.opf").contains("<item id=\"xcss\" href=\"x.css\" media-type=\"text/css\"/></manifest>"));
        assert!(get("OEBPS/content.opf").contains("<spine page-progression-direction=\"rtl\">"), "rtl 选项写进 spine");
    }

    /// `assemble_pdf_derived` 是 PDF→EPUB 路径实际调用的入口（2026-09-23 真机投一本真实 PDF
    /// 手册发现：没有这条 CSS 时图片在 xochitl 原生阅读器里完全不出现，见函数头注）。这里只确认它正确
    /// 接上了 `pdf-img.css`/`max-width`，不重复 `shared_css_goes_last...` 已经测过的通用机制。
    #[test]
    fn assemble_pdf_derived_links_max_width_css_to_image_chapters_only() {
        let out = assemble_pdf_derived(&mut two_chapter_book(true), "").unwrap();
        let entries = zip_names_and_text(out);
        let get = |n: &str| String::from_utf8(entries.iter().find(|(k, _)| k == n).unwrap().1.clone()).unwrap();
        assert_eq!(get("OEBPS/pdf-img.css"), "img{max-width:100%;height:auto;}\n");
        assert!(get("OEBPS/chap_0001.xhtml").contains("href=\"pdf-img.css\""), "含图的章要挂 pdf-img.css");
        assert!(!get("OEBPS/chap_0002.xhtml").contains("<link"), "纯文字章不挂");
        assert!(get("OEBPS/content.opf").contains("<item id=\"pdf-img-css\" href=\"pdf-img.css\" media-type=\"text/css\"/>"));
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

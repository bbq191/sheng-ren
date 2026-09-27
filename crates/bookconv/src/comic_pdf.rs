//! EPUB 漫画 → PDF：真机反复实测坐实 xochitl 的 EPUB 渲染走"文字排版盒模型"，内容区相对物理
//! 页面有一个消不掉的固定内边距（`.content` 元数据的 `margins` 字段，UI 只给 28/56/112 三档
//! 离散预设；直接改文件也没用——xochitl 渲染文档时会用自己的逻辑把这个字段覆盖回去），文字页和
//! 图片页共享同一份限制，CSS 层面（`width` 超 100%、负 `margin`）也测过绕不开——这是 EPUB 渲染
//! 路径本身的硬限制，不是我们代码的 bug。同一批真机实测：PDF 直传（页面物理尺寸精确等于设备
//! 屏幕 954×1696px）左右留白量得 **0.00%**，且 `.content` 里 PDF 文档根本没有 `margins` 这个
//! 字段——PDF 走的是完全独立于 EPUB 文字排版盒模型的直接光栅化路径，从根上不受这个限制。
//!
//! 这个模块是"漫画类 EPUB 优化时改产出 PDF（带书签）"的实现：书签与页面图片来自 [`crate::ncx`]
//! （`ncx_titles_in_range`、`imgs_referenced`），图片处理用 `imgopt::prepare_comic_page_for_pdf`（裁边+缩放合成单趟，PDF 不需要靠补白像素控制留白分布，
//! 直接在页面里摆位置即可，摆位算法见 `convert::pdfwrite::place_image`）。也不碰 `convert::
//! pdfwrite::images_to_pdf`/`convert::cbz`（CBZ→PDF 现状路径），只用新增的 `images_to_pdf_
//! with_toc`/`extract_pages`/`page_count`。

use crate::convert::pdfwrite;
use crate::epubzip::{dir_of, is_html, Entry};
use crate::wash::parse_opf;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PdfReport {
    pub pages: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
}

/// **当前生产路径不调用**（2026-09-20 用户拍板"优化不改格式"，漫画 EPUB 优化保持 EPUB，见
/// `book-serve::Staging::optimize`）。保留是因为"漫画 EPUB→整本 PDF"仍是有测试覆盖的可用能力（含 `PdfPieceWriter`
/// 流式写出与目录书签），日后若要重开这条路（如超限漫画改投 PDF）不必重写；本仓库内只有单测引用它。
/// 不标 `#[cfg(test)]`：book-serve 的测试也要跨 crate 调用它。
///
/// 读入方式：阶段一：图片条目留空占位、只读 html/opf/ncx
/// 真实字节判断是不是漫画+抽标题；真正的图片字节按需读、处理完立刻编进 `PdfImage` 就丢原始字节，
/// 峰值内存是"处理到哪张图"而不是"全书图片"，与流式优化同一套纪律。
///
/// **一图一页**（不像 EPUB 那样允许一个 spine 页塞多张图）——漫画天然就是一页一图，PDF 场景更
/// 贴近这个语义；一个 spine 页如果引用了 N 张图，会展开成 N 个 PDF 页，NCX 标题落在这个 spine
/// 页对应的第一张图上。零图的纯文字页（如后记）在 PDF 场景没有对应物，直接跳过不产出页面——
/// PDF 不是文字排版容器，硬塞文字进去不是这次任务范围。
pub fn optimize_comic_epub_to_pdf_streaming(
    input_path: &Path,
    output_path: &Path,
    screen: crate::imgopt::Screen,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<PdfReport, String> {
    let bytes_before = std::fs::metadata(input_path).map(|m| m.len() as usize).unwrap_or(0);
    let file = std::fs::File::open(input_path).map_err(|e| format!("打开母版库文件失败: {e}"))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("解 EPUB(非 zip?): {e}"))?;

    let entries: Vec<Entry> = crate::epubzip::read_skeleton(&mut zip)?.entries;
    drop(zip);

    if !crate::comic_detect::is_comic(&entries) {
        return Err("不是漫画书，漫画→PDF 这条路径不适用".into());
    }
    let (_, text_chars) = crate::comic_detect::epub_image_stats(&entries);
    if text_chars > 0 {
        // PDF 一图一页，文字页/图片页里夹的文字没有对应物、会被丢掉——不允许变动书籍内容，拒绝而不是静默丢字。
        return Err(format!("这本书含 {text_chars} 字正文文字（版权页/章节标题/台词等），转 PDF 会丢掉文字，保持 EPUB"));
    }
    let opf = parse_opf(&entries).ok_or("解不出 OPF/spine")?;
    let titles_by_spine_idx =
        crate::ncx::ncx_titles_in_range(&entries, &opf, 0, opf.spine.len());

    // 先摸一遍每个 spine 项引用了几张图，凑出总图数给进度条用（不解码，只读 html 文本里的 <img> 引用）。
    let mut per_page_imgs: Vec<Vec<String>> = Vec::with_capacity(opf.spine.len());
    for p in &opf.spine {
        if !is_html(p) {
            per_page_imgs.push(Vec::new());
            continue;
        }
        let imgs = entries
            .iter()
            .find(|e| &e.name == p)
            .and_then(|e| std::str::from_utf8(&e.data).ok())
            .map(|html| crate::ncx::imgs_referenced(html, dir_of(p)))
            .unwrap_or_default();
        per_page_imgs.push(imgs);
    }
    let total_imgs: usize = per_page_imgs.iter().map(|v| v.len()).sum();
    if total_imgs == 0 {
        return Err("没有找到任何图片，没法生成漫画 PDF".into());
    }

    let file2 = std::fs::File::open(input_path).map_err(|e| format!("重开母版库文件失败: {e}"))?;
    let mut zip2 = zip::ZipArchive::new(std::io::BufReader::new(file2)).map_err(|e| e.to_string())?;

    // 用 PdfPieceWriter 逐页读逐页写——`total_imgs`（总页数）已经在上面数出来了，不用先攒出
    // 整本书的 `Vec<PdfImage>` 才知道有几页。真机 245MB/600页 样本坐实过：先攒整本 `Vec<PdfImage>`
    // 再一次性序列化，`VmHWM` 峰值到过 595MB（images 副本 + 序列化中间态叠加）；这里改成一张图
    // 处理完立刻写进 writer 内部缓冲区、这张图的 `PdfImage`/原始字节就地释放，峰值只剩 writer
    // 自己那份累积输出（约等于最终 PDF 体积本身，不再叠加一份"全书图片"的额外副本）。
    // `has_toc` 恒为 true——下面兜底逻辑保证 `titles` 最终不可能是空的（至少有整本书名这一条）。
    let mut writer = pdfwrite::PdfPieceWriter::begin(total_imgs, true, screen);
    let mut titles: Vec<(usize, String)> = Vec::new();
    let mut done = 0usize;
    let mut written = 0usize;
    for (spine_idx, imgs) in per_page_imgs.iter().enumerate() {
        if imgs.is_empty() {
            continue;
        }
        if let Some(title) = titles_by_spine_idx.get(&spine_idx) {
            titles.push((written, title.clone()));
        }
        for img_path in imgs {
            let raw = crate::epubzip::read_by_name(&mut zip2, img_path).map_err(|e| format!("读图片 {img_path} 失败: {e}"))?;
            // 单趟：裁边+一次缩到 PDF 实际绘制的整数像素尺寸+一次编码（见该函数文档：此前两道串联
            // 造成重采样两遍/JPEG 两代/灰度转 RGB）。返回 None = 无需处理，直接嵌原图字节零损失。
            let sized = crate::imgopt::prepare_comic_page_for_pdf(&raw, screen.width, screen.height).unwrap_or(raw);
            let pdf_img = pdfwrite::image_from_bytes(&sized)
                .map_err(|e| format!("图片 {img_path} 编不进 PDF: {e}"))?;
            writer.write_page(&pdf_img)?;
            written += 1;
            done += 1;
            on_progress(done, total_imgs);
        }
    }
    if titles.is_empty() {
        // 源书自己就没有目录（NCX 空/畸形；乱马、火影实测就是空 NCX）——按页分段给书签，至少能按段跳转，
        // 不是只有一条书名。如实标"第 N–M 页"，不假装是章节。
        titles = page_chunk_titles(written);
    }

    let pdf_bytes = writer.finish(&titles)?;
    let bytes_after = pdf_bytes.len();
    let pages = written;
    std::fs::write(output_path, &pdf_bytes).map_err(|e| format!("写出 PDF 失败: {e}"))?;
    Ok(PdfReport { pages, bytes_before, bytes_after })
}

/// 没有源目录时的兜底书签：每 [`FALLBACK_TOC_PAGES`] 页一条，标题"第 N–M 页"（页码 1 起）。
const FALLBACK_TOC_PAGES: usize = 20;
pub(crate) fn page_chunk_titles(total_pages: usize) -> Vec<(usize, String)> {
    (0..total_pages)
        .step_by(FALLBACK_TOC_PAGES)
        .map(|start| (start, format!("第 {}–{} 页", start + 1, (start + FALLBACK_TOC_PAGES).min(total_pages))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn one_px_jpeg() -> Vec<u8> {
        // 复用 pdfwrite 测试里用过的最小 JPEG 骨架构造思路：只需 SOI+SOF0+EOI，宽高任意。
        vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x10, 0x03, 0x01, 0x11,
            0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xFF, 0xD9,
        ]
    }

    fn build_test_epub(chapters: &[(&str, &[&str])]) -> Vec<u8> {
        build_test_epub_opts(chapters, "", true)
    }

    /// `text` 非空时塞进第一章 body（模拟版权页/台词）；`with_ncx=false` 时 NCX 为空 navMap（模拟乱马/火影）。
    fn build_test_epub_opts(chapters: &[(&str, &[&str])], text: &str, with_ncx: bool) -> Vec<u8> {
        // chapters: (spine 文件名, 引用的图片文件名列表)；每个 chapter 一个最小 xhtml。
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut z = zip::ZipWriter::new(cursor);
            let opt = zip::write::SimpleFileOptions::default();
            z.start_file("mimetype", opt).unwrap();
            z.write_all(b"application/epub+zip").unwrap();
            z.start_file("META-INF/container.xml", opt).unwrap();
            z.write_all(br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#).unwrap();

            let mut manifest_items = String::new();
            let mut spine_items = String::new();
            let mut nav_points = String::new();
            let jpeg = one_px_jpeg();
            let mut img_names: Vec<String> = Vec::new();
            for (i, (chap, imgs)) in chapters.iter().enumerate() {
                let mut body: String = imgs
                    .iter()
                    .map(|img| format!(r#"<img src="{img}"/>"#))
                    .collect();
                if i == 0 && !text.is_empty() {
                    body.push_str(&format!("<p>{text}</p>"));
                }
                z.start_file(format!("OEBPS/{chap}"), opt).unwrap();
                z.write_all(format!("<html><body>{body}</body></html>").as_bytes()).unwrap();
                manifest_items.push_str(&format!(r#"<item id="c{i}" href="{chap}" media-type="application/xhtml+xml"/>"#));
                spine_items.push_str(&format!(r#"<itemref idref="c{i}"/>"#));
                if with_ncx { nav_points.push_str(&format!(
                    r#"<navPoint><navLabel><text>第{i}章</text></navLabel><content src="{chap}"/></navPoint>"#
                )); }
                for img in imgs.iter() {
                    if !img_names.contains(&img.to_string()) {
                        img_names.push(img.to_string());
                    }
                }
            }
            for img in &img_names {
                z.start_file(format!("OEBPS/{img}"), opt).unwrap();
                z.write_all(&jpeg).unwrap();
                manifest_items.push_str(&format!(r#"<item id="{img}" href="{img}" media-type="image/jpeg"/>"#));
            }
            manifest_items.push_str(r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#);
            z.start_file("OEBPS/content.opf", opt).unwrap();
            z.write_all(format!(
                r#"<?xml version="1.0"?><package><metadata></metadata><manifest>{manifest_items}</manifest><spine toc="ncx">{spine_items}</spine></package>"#
            ).as_bytes()).unwrap();
            z.start_file("OEBPS/toc.ncx", opt).unwrap();
            z.write_all(format!(r#"<?xml version="1.0"?><ncx><navMap>{nav_points}</navMap></ncx>"#).as_bytes()).unwrap();
            z.finish().unwrap();
        }
        buf
    }

    #[test]
    fn optimize_comic_epub_to_pdf_produces_one_page_per_image_with_toc() {
        // is_comic 要求 >=20 张图——两章分别 10/11 张图，凑够 21 张触发漫画判定。
        let c1_imgs: Vec<String> = (1..=10).map(|i| format!("i{i}.jpg")).collect();
        let c2_imgs: Vec<String> = (11..=21).map(|i| format!("i{i}.jpg")).collect();
        let c1_refs: Vec<&str> = c1_imgs.iter().map(|s| s.as_str()).collect();
        let c2_refs: Vec<&str> = c2_imgs.iter().map(|s| s.as_str()).collect();
        let epub = build_test_epub(&[("c1.xhtml", &c1_refs), ("c2.xhtml", &c2_refs)]);
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("test.epub");
        let output = dir.path().join("test.pdf");
        std::fs::write(&input, &epub).unwrap();

        let mut calls = Vec::new();
        let rep = optimize_comic_epub_to_pdf_streaming(&input, &output, crate::imgopt::test_screen(), |d, t| calls.push((d, t))).unwrap();
        assert_eq!(rep.pages, 21, "21 张图应展开成 21 个 PDF 页");
        assert_eq!(calls.last(), Some(&(21, 21)));

        let mut reader = pdfwrite::PdfFileReader::open(&output).unwrap();
        assert_eq!(reader.page_count().unwrap(), 21);
        let titles = reader.outline_titles().unwrap();
        assert_eq!(titles, vec![(0, "第0章".to_string()), (10, "第1章".to_string())], "标题应落在各章第一张图对应的页码上");
    }

    fn write_comic(dir: &std::path::Path, text: &str, with_ncx: bool) -> (std::path::PathBuf, std::path::PathBuf) {
        let c1_imgs: Vec<String> = (1..=30).map(|i| format!("i{i}.jpg")).collect();
        let c2_imgs: Vec<String> = (31..=45).map(|i| format!("i{i}.jpg")).collect();
        let c1: Vec<&str> = c1_imgs.iter().map(|s| s.as_str()).collect();
        let c2: Vec<&str> = c2_imgs.iter().map(|s| s.as_str()).collect();
        let input = dir.join("t.epub");
        std::fs::write(&input, build_test_epub_opts(&[("c1.xhtml", &c1), ("c2.xhtml", &c2)], text, with_ncx)).unwrap();
        (input, dir.join("t.pdf"))
    }

    #[test]
    fn refuses_to_convert_comic_containing_any_body_text() {
        // 不允许变动书籍内容：PDF 一图一页，夹带的文字（版权/台词）会被丢掉——必须拒绝，不静默丢字。
        let dir = tempfile::tempdir().unwrap();
        let (input, output) = write_comic(dir.path(), "版权信息", true);
        let err = optimize_comic_epub_to_pdf_streaming(&input, &output, crate::imgopt::test_screen(), |_, _| {}).unwrap_err();
        assert!(err.contains("正文文字"), "错误应说明原因: {err}");
        assert!(!output.exists(), "拒绝时不该产出任何文件");
        assert!(!crate::comic_detect::is_text_free_comic_epub_file(&input));
        assert!(crate::comic_detect::is_comic_epub_file(&input), "它仍是漫画，只是不能转 PDF");
    }

    #[test]
    fn text_free_comic_is_convertible() {
        let dir = tempfile::tempdir().unwrap();
        let (input, _) = write_comic(dir.path(), "", true);
        assert!(crate::comic_detect::is_text_free_comic_epub_file(&input));
    }

    #[test]
    fn comic_without_source_toc_gets_page_range_bookmarks_not_a_single_one() {
        let dir = tempfile::tempdir().unwrap();
        let (input, output) = write_comic(dir.path(), "", false);
        optimize_comic_epub_to_pdf_streaming(&input, &output, crate::imgopt::test_screen(), |_, _| {}).unwrap();
        let titles = pdfwrite::PdfFileReader::open(&output).unwrap().outline_titles().unwrap();
        assert_eq!(titles, vec![(0, "第 1–20 页".to_string()), (20, "第 21–40 页".to_string()), (40, "第 41–45 页".to_string())]);
    }

    #[test]
    fn page_chunk_titles_boundaries() {
        assert_eq!(page_chunk_titles(1), vec![(0, "第 1–1 页".to_string())]);
        assert_eq!(page_chunk_titles(20).len(), 1);
        assert_eq!(page_chunk_titles(21).last(), Some(&(20, "第 21–21 页".to_string())));
    }

    #[test]
    fn optimize_comic_epub_to_pdf_rejects_non_comic() {
        // 图太少、判不成漫画（is_comic 需要 >=20 张图）。
        let epub = build_test_epub(&[("c1.xhtml", &["i1.jpg"])]);
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("test.epub");
        let output = dir.path().join("test.pdf");
        std::fs::write(&input, &epub).unwrap();
        assert!(optimize_comic_epub_to_pdf_streaming(&input, &output, crate::imgopt::test_screen(), |_, _| {}).is_err());
    }
}

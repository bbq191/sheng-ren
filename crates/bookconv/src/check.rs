//! EPUB 质量门。只读、不改书。硬失败（`ok=false`）表示产物有结构性问题（书库目前只警告、不拦下）：
//! 1. 真 DRM：`META-INF/encryption.xml` 加密了非字体项（仅字体混淆是合法的，告警不拦）。
//! 2. 目录 href 文件命中率 < 80%（目录指向不存在的文件 = xochitl TOC 面板点不动）。
//! 3. 单标签双 `id=` 属性（非法 XHTML，xochitl 严格 XML 解析整章白屏，《消失的爱人》7 页事故）。
//! 4. 正文资源引用（`<img src>`/`<link href>` 等）命中率 < 80%（2026-09-23 真机坐实：PDF→EPUB
//!    路径拼错多写一层 `../`，图片在包里打包正确、标签也在，但引用指向压缩包里不存在的位置——
//!    xochitl 渲染出来一张图都没有，且此前没有任何检查能发现，只能真机肉眼看出来）。
//!
//! 5. OPF 不是合法 XML（非法控制字符/结构错误；xochitl 严格解析，整本读不出来）。正文章节不合法只告警。
//! 6. `mimetype` 缺失、不是第一个条目、被压缩或内容不对（EPUB 规范 OCF 的硬性要求，只在按 zip 检查的
//!    [`check_epub_file`] 里查，[`check_entries`] 看不到压缩方式）。
//!
//! 告警（不拦）：无 nav/ncx 或零条目（`require_toc` 时升为失败）；目录锚点丢失（xochitl 退化到文件级跳转）。
use crate::epubzip::{dir_of, is_html_entry, percent_decode, resolve_rel, Entry};
use crate::wash::{count_dup_id_tags, encrypted_targets, is_toc_file, real_drm_items};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub toc_files: Vec<String>,
    pub toc_entries: usize,
    pub href_file_hit: usize,
    pub frag_hit: usize,
    pub frag_total: usize,
    pub dup_id_tags: usize,
    pub html_files: usize,
    pub resource_refs_total: usize,
    pub resource_refs_hit: usize,
}

impl CheckReport {
    /// 一行摘要（回执用）。
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("目录 {} 条", self.toc_entries)];
        if self.frag_total > 0 && self.frag_hit < self.frag_total {
            parts.push(format!("锚点丢失 {}/{}", self.frag_total - self.frag_hit, self.frag_total));
        }
        if self.resource_refs_total > 0 && self.resource_refs_hit < self.resource_refs_total {
            parts.push(format!("资源引用丢失 {}/{}", self.resource_refs_total - self.resource_refs_hit, self.resource_refs_total));
        }
        if !self.warnings.is_empty() {
            parts.push(format!("告警 {}", self.warnings.len()));
        }
        parts.join("，")
    }
}

/// 按内存里的 zip 字节检查（测试用；命令行与书库走 [`check_epub_file`]）。
#[cfg(test)]
pub fn check_epub(epub: &[u8], require_toc: bool) -> Result<CheckReport, String> {
    // zip 目录只解析一遍：条目和 mimetype 检查都从这一个 archive 读。
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(epub)).map_err(|e| format!("解 EPUB(非 zip?): {e}"))?;
    let entries = crate::epubzip::read_entries_from(&mut zip, |_| true)?;
    let mut rep = check_entries(&entries, require_toc);
    add_mimetype_problem(&mut rep, &mut zip);
    Ok(rep)
}

/// 按路径检查、省内存：用 [`crate::epubzip::read_skeleton`]（图片条目留空占位，
/// 只有非图片的真实字节整份读入）而不是整本读进内存——大漫画优化产物整本读回内存
/// 就白费了流式优化省下的内存。质量门这几条规则（双 id/href 命中率/正文资源引用）都只看 html/toc
/// 文本内容，不看图片字节，检查结果不受影响。
/// 目录缺失只告警（书库生成用）；要把"无目录"升为失败用 [`check_epub_file_with`]。
pub fn check_epub_file(path: &std::path::Path) -> Result<CheckReport, String> {
    check_epub_file_with(path, false)
}

/// [`check_epub_file`]，`require_toc` 为真时"无目录"算硬失败（`epub-optimize --check --require-toc`）。
pub fn check_epub_file_with(path: &std::path::Path, require_toc: bool) -> Result<CheckReport, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开待校验文件失败: {e}"))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| format!("解 EPUB(非 zip?): {e}"))?;
    let sk = crate::epubzip::read_skeleton(&mut zip)?;
    let mut rep = check_entries(&sk.entries, require_toc);
    add_mimetype_problem(&mut rep, &mut zip);
    Ok(rep)
}

/// 第 6 条：`mimetype` 必须是 zip 的第一个条目、STORED、内容正好是 `application/epub+zip`。有问题记一条硬失败。
fn add_mimetype_problem<R: std::io::Read + std::io::Seek>(rep: &mut CheckReport, zip: &mut zip::ZipArchive<R>) {
    let problem = match zip.by_index(0) {
        Err(_) => Some("是空压缩包".to_string()),
        Ok(f) if f.name() != "mimetype" => Some(format!("第一个条目是 {} 而不是 mimetype", f.name())),
        Ok(f) if f.compression() != zip::CompressionMethod::Stored => Some("mimetype 被压缩了（必须 STORED）".to_string()),
        Ok(mut f) => {
            let mut v = Vec::new();
            match std::io::Read::read_to_end(&mut f, &mut v) {
                Ok(_) if v == b"application/epub+zip" => None,
                _ => Some(format!("mimetype 内容不对（{:?}）", String::from_utf8_lossy(&v))),
            }
        }
    };
    if let Some(p) = problem {
        rep.errors.push(format!("EPUB 容器不合规：{p}，阅读器可能认不出这是 EPUB"));
        rep.ok = false;
    }
}

/// 文档里指向书内文件的链接：(zip 路径, 锚点)。`href`/`src`/`xlink:href`，单双引号都认（`data-src` 不算），
/// 书外链接（带协议、`data:`）和纯同文件锚点跳过。
fn internal_links(base: &str, html: &str) -> Vec<(String, String)> {
    crate::html::link_values(html)
        .into_iter()
        .filter_map(|v| {
            let v = crate::util::xml_unescape(v);
            let (path, frag) = crate::html::split_href(&v);
            if path.is_empty() || crate::html::is_external(path) {
                return None;
            }
            Some((resolve_rel(base, path), frag.map(percent_decode).unwrap_or_default()))
        })
        .collect()
}

pub fn check_entries(entries: &[Entry], require_toc: bool) -> CheckReport {
    let mut rep = CheckReport::default();
    let names: HashMap<&str, &Entry> = entries.iter().map(|e| (e.name.as_str(), e)).collect();

    // 1. DRM
    if let Some(targets) = encrypted_targets(entries) {
        let bad = real_drm_items(&targets);
        if !bad.is_empty() {
            rep.errors.push(format!("加密 EPUB（DRM，加密了 {} 等），阅读器都读不了", bad.iter().take(3).map(String::as_str).collect::<Vec<_>>().join("、")));
        } else {
            rep.warnings.push(format!("仅字体/样式混淆（{} 个文件，非 DRM，可读）", targets.len()));
        }
    }

    // 2. 目录
    rep.toc_files = entries.iter().filter(|e| is_toc_file(&e.name)).map(|e| e.name.clone()).collect();
    let mut targets: Vec<(String, String)> = Vec::new(); // (zip 路径, frag)
    for tf in &rep.toc_files {
        let base = dir_of(tf);
        let t = String::from_utf8_lossy(&names[tf.as_str()].data);
        targets.extend(internal_links(base, &t));
    }
    rep.toc_entries = targets.len();
    if rep.toc_files.is_empty() || targets.is_empty() {
        let msg = "无 nav/ncx 或目录零条目".to_string();
        if require_toc {
            rep.errors.push(msg);
        } else {
            rep.warnings.push(msg);
        }
    }
    rep.href_file_hit = targets.iter().filter(|(t, _)| names.contains_key(t.as_str())).count();
    if !targets.is_empty() && (rep.href_file_hit as f64) / (targets.len() as f64) < 0.8 {
        rep.errors.push(format!("目录 href 文件命中率过低 {}/{}", rep.href_file_hit, targets.len()));
    }
    // 每个目标页只扫一遍收集全部锚点（任何元素的 `id`、`<a name>`；单双引号都认，`data-id` 不算），再按集合判命中。
    let mut cache: HashMap<&str, std::collections::HashSet<String>> = HashMap::new();
    for (t, frag) in targets.iter().filter(|(t, f)| !f.is_empty() && names.contains_key(t.as_str())) {
        rep.frag_total += 1;
        let anchors = cache.entry(t.as_str()).or_insert_with(|| {
            let html = String::from_utf8_lossy(&names[t.as_str()].data);
            crate::html::anchors(&html).into_iter().map(|(v, _)| v.to_string()).collect()
        });
        if anchors.contains(frag) {
            rep.frag_hit += 1;
        }
    }
    if rep.frag_total > 0 && rep.frag_hit < rep.frag_total {
        rep.warnings.push(format!("目录锚点丢失 {}/{}（xochitl 退化到文件级跳转）", rep.frag_total - rep.frag_hit, rep.frag_total));
    }

    // 3. 双 id
    for e in entries.iter().filter(|e| is_html_entry(&e.name, &e.data)) {
        rep.html_files += 1;
        rep.dup_id_tags += count_dup_id_tags(&String::from_utf8_lossy(&e.data));
    }
    if rep.dup_id_tags > 0 {
        rep.errors.push(format!("{} 个标签带双 id 属性（非法 XHTML，xochitl 整章白屏）", rep.dup_id_tags));
    }

    // 4. 正文资源引用（img src / link href 等，和目录那节同一个 [`internal_links`]，区别只是扫的是每章正文）：
    // 书外链接、data: 内联、纯同文件锚点不算。命中率阈值跟目录那节一致，同一份"该拦还是该忍"的标准。
    let mut res_examples: Vec<String> = Vec::new();
    for e in entries.iter().filter(|e| is_html_entry(&e.name, &e.data)) {
        let base = dir_of(&e.name);
        let t = String::from_utf8_lossy(&e.data);
        for (target, _) in internal_links(base, &t) {
            rep.resource_refs_total += 1;
            if names.contains_key(target.as_str()) {
                rep.resource_refs_hit += 1;
            } else if res_examples.len() < 3 {
                res_examples.push(format!("{}→{target}", e.name));
            }
        }
    }
    if rep.resource_refs_total > 0 && (rep.resource_refs_hit as f64) / (rep.resource_refs_total as f64) < 0.8 {
        rep.errors.push(format!(
            "正文资源引用命中率过低 {}/{}（如 {}），图片/样式在包里但引用路径指向不存在的位置，真机会整个不显示",
            rep.resource_refs_hit,
            rep.resource_refs_total,
            res_examples.join("、")
        ));
    }

    // 5. XML 合法性：xochitl 是严格 XML 解析器。OPF 不合法＝整本书读不出来（2026-09-23 真机《T.E.双语》
    // `dc:title` 混进 `\0`，只渲染出 1 页）→ 硬失败；正文章节不合法＝那一章被吞（《消失的爱人》双 id
    // 先例）→ 告警，不拦——原书本来就这样的话，拦了也换不来更好的结果。
    for e in entries.iter().filter(|e| e.name.ends_with(".opf")) {
        if let Some(why) = xml_problem(&e.data) {
            rep.errors.push(format!("{} 不是合法 XML（{why}），xochitl 会整本读不出来", e.name));
        }
    }
    let bad_chapters: Vec<String> = entries.iter().filter(|e| is_html_entry(&e.name, &e.data)).filter_map(|e| xml_problem(&e.data).map(|why| format!("{}（{why}）", e.name))).collect();
    if !bad_chapters.is_empty() {
        rep.warnings.push(format!("{} 个正文文件不是合法 XML，xochitl 可能整章不显示：{}", bad_chapters.len(), bad_chapters.iter().take(3).cloned().collect::<Vec<_>>().join("、")));
    }

    rep.ok = rep.errors.is_empty();
    rep
}

/// 不是合法 XML 的第一个原因；合法返回 `None`。先查 XML 1.0 不允许的字符（quick-xml 不管字符集，
/// `\0` 之类照单全收），再过一遍 quick-xml 查结构（标签配对等）。
fn xml_problem(data: &[u8]) -> Option<String> {
    let text = match std::str::from_utf8(data) {
        Ok(t) => t,
        Err(e) => return Some(format!("第 {} 字节起不是 UTF-8", e.valid_up_to())),
    };
    if let Some((i, c)) = text.char_indices().find(|(_, c)| !crate::util::is_xml_char(*c)) {
        return Some(format!("第 {} 字节有非法字符 U+{:04X}", i, c as u32));
    }
    let mut r = quick_xml::Reader::from_str(text);
    loop {
        match r.read_event() {
            Ok(quick_xml::events::Event::Eof) => return None,
            Ok(_) => {}
            Err(e) => return Some(format!("第 {} 字节附近：{e}", r.buffer_position())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn e(name: &str, data: &str) -> Entry {
        Entry { name: name.into(), data: data.as_bytes().to_vec() }
    }

    #[test]
    fn gate_rules() {
        // 健康书
        let good = vec![
            e("OEBPS/toc.ncx", r#"<ncx><content src="text/c1.xhtml#a"/><content src="text/c2%20x.xhtml"/></ncx>"#),
            e("OEBPS/text/c1.xhtml", r#"<html><body><h1 id="a">x</h1></body></html>"#),
            e("OEBPS/text/c2 x.xhtml", "<html><body/></html>"),
        ];
        let r = check_entries(&good, true);
        assert!(r.ok, "{:?}", r.errors);
        assert_eq!((r.toc_entries, r.href_file_hit, r.frag_hit, r.frag_total), (2, 2, 1, 1));
        assert_eq!(r.summary(), "目录 2 条");
        // 命中率低 + 锚点丢 + 双 id + DRM + 正文资源引用命中率低（nav.xhtml 本身也是合法 html，它那 4 条
        // href 被目录那节（2）与正文资源引用那节（4）各扫一遍——同一份 internal_links 结果，两节各自独立判命中率，
        // 不是重复 bug）。
        let bad = vec![
            e("META-INF/encryption.xml", r#"<CipherReference URI="OEBPS/text/c1.xhtml"/>"#),
            e("OEBPS/nav.xhtml", r#"<html><body><nav><a href="text/c1.xhtml#zz">1</a><a href="text/nope1.xhtml">2</a><a href="text/nope2.xhtml">3</a><a href="text/nope3.xhtml">4</a></nav></body></html>"#),
            e("OEBPS/text/c1.xhtml", r#"<html><body><p id="a" id="b">x</p></body></html>"#),
        ];
        let r = check_entries(&bad, false);
        assert!(!r.ok);
        assert_eq!(r.errors.len(), 4, "{:?}", r.errors);
        assert!(r.errors[0].contains("DRM") && r.errors[1].contains("命中率过低 1/4") && r.errors[2].contains("双 id"));
        assert!(r.errors[3].contains("正文资源引用命中率过低 1/4"), "{:?}", r.errors[3]);
        assert!(r.warnings.iter().any(|w| w.contains("锚点丢失 1/1")));
        // 无目录：告警 / require_toc 失败；字体混淆只告警
        let none = vec![e("META-INF/encryption.xml", r#"<CipherReference URI="f.ttf"/>"#), e("c.xhtml", "<html/>")];
        assert!(check_entries(&none, false).ok);
        let r = check_entries(&none, true);
        assert!(!r.ok && r.warnings.iter().any(|w| w.contains("混淆")));
        // 单引号、带前缀的 CipherReference 也认（以前只认双引号，真 DRM 会被放过）
        let single = vec![e("META-INF/encryption.xml", r#"<enc:CipherReference URI='OEBPS/c.xhtml'/>"#), e("c.xhtml", "<html/>")];
        assert!(check_entries(&single, false).errors.iter().any(|x| x.contains("DRM")));
    }

    #[test]
    fn toc_anchor_hit_accepts_single_quotes_and_ignores_data_id() {
        let toc = e("OEBPS/toc.ncx", r#"<ncx><content src="c.xhtml#a"/><content src="c.xhtml#b"/><content src="c.xhtml#n"/></ncx>"#);
        let chap = e("OEBPS/c.xhtml", r#"<html><body><p id='a'>x</p><p data-id="b">y</p><a name="n"/></body></html>"#);
        let r = check_entries(&[toc, chap], false);
        assert_eq!((r.frag_hit, r.frag_total), (2, 3), "单引号 id、<a name> 算锚点，data-id 不算");
    }

    #[test]
    fn reads_zip() {
        assert!(check_epub(b"notazip", false).is_err());
    }

    #[test]
    fn mimetype_must_be_first_stored_and_exact() {
        let mk = |first: Option<(&str, zip::CompressionMethod, &[u8])>| {
            let mut buf = Vec::new();
            {
                let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
                if let Some((name, method, data)) = first {
                    z.start_file::<_, ()>(name, zip::write::SimpleFileOptions::default().compression_method(method)).unwrap();
                    std::io::Write::write_all(&mut z, data).unwrap();
                }
                z.start_file::<_, ()>("OEBPS/toc.ncx", zip::write::SimpleFileOptions::default()).unwrap();
                std::io::Write::write_all(&mut z, br#"<ncx><content src="c.xhtml"/></ncx>"#).unwrap();
                z.start_file::<_, ()>("OEBPS/c.xhtml", zip::write::SimpleFileOptions::default()).unwrap();
                std::io::Write::write_all(&mut z, b"<html><body/></html>").unwrap();
                z.finish().unwrap();
            }
            check_epub(&buf, false).unwrap()
        };
        use zip::CompressionMethod::{Deflated, Stored};
        assert!(mk(Some(("mimetype", Stored, b"application/epub+zip"))).ok);
        let bad = |r: CheckReport, what: &str| assert!(!r.ok && r.errors.iter().any(|e| e.contains("容器不合规") && e.contains(what)), "{what}: {:?}", r.errors);
        bad(mk(None), "而不是 mimetype");
        bad(mk(Some(("mimetype", Deflated, b"application/epub+zip"))), "被压缩");
        bad(mk(Some(("mimetype", Stored, b"application/epub+zip\n"))), "内容不对");
    }

    /// `check_epub_file` 走 `read_skeleton`（图片留空）而不是整本读入——这条测试确认图片留空不影响
    /// 判断结果：图片条目本身不是 html/toc，判断只看文本条目，空字节不会被误判成"引用了不存在的文件"
    /// （图片条目自己不会被当成正文去扫 href，也不会被当成 img 引用目标之外的东西）。
    #[test]
    fn check_epub_file_skips_image_bytes_but_still_catches_broken_ref() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("book.epub");
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let o = zip::write::SimpleFileOptions::default();
            z.start_file::<_, ()>("OEBPS/chap_0001.xhtml", o).unwrap();
            std::io::Write::write_all(&mut z, br#"<html><body><img src="../images/pic.png"/></body></html>"#).unwrap();
            z.start_file::<_, ()>("OEBPS/images/pic.png", o).unwrap();
            std::io::Write::write_all(&mut z, &[0u8; 5000]).unwrap();
            z.finish().unwrap();
        }
        std::fs::write(&p, &buf).unwrap();
        let rep = check_epub_file(&p).unwrap();
        assert!(!rep.ok, "路径拼错应该被拦下来: {:?}", rep.errors);
        assert_eq!((rep.resource_refs_total, rep.resource_refs_hit), (1, 0));
    }

    /// 复现 2026-09-23 真机 bug 的最小形态：图片确实打包进 EPUB，但 `<img src>` 多写一层 `../`，
    /// 章节文件跟图片其实同级——这条门必须拦下来，不能等真机肉眼才发现。
    #[test]
    fn resource_ref_gate_catches_broken_relative_image_path() {
        let broken = vec![
            e("OEBPS/chap_0001.xhtml", r#"<html><body><p><img src="../images/pic.png"/></p></body></html>"#),
            e("OEBPS/images/pic.png", "fake-png-bytes"),
        ];
        let r = check_entries(&broken, false);
        assert!(!r.ok, "多写一层 ../ 指到包里不存在的位置，这条门应该拦下来");
        assert!(r.errors.iter().any(|w| w.contains("正文资源引用命中率过低")), "{:?}", r.errors);
        assert_eq!((r.resource_refs_total, r.resource_refs_hit), (1, 0));

        let fixed = vec![
            e("OEBPS/chap_0001.xhtml", r#"<html><body><p><img src="images/pic.png"/></p></body></html>"#),
            e("OEBPS/images/pic.png", "fake-png-bytes"),
        ];
        let r2 = check_entries(&fixed, false);
        assert!(r2.ok, "{:?}", r2.errors);
        assert_eq!((r2.resource_refs_total, r2.resource_refs_hit), (1, 1));
    }

    /// 远程图/内联 data: 图不该被当成"引用了包内不存在的文件"误判——这两种压根不指向包内资源。
    #[test]
    fn resource_ref_gate_ignores_remote_and_data_uri_images() {
        let e1 = e("OEBPS/c.xhtml", r#"<html><body><img src="https://example.com/a.png"/><img src="data:image/png;base64,AAAA"/></body></html>"#);
        let r = check_entries(&[e1], false);
        assert!(r.ok, "{:?}", r.errors);
        assert_eq!(r.resource_refs_total, 0, "远程/data: 都不该计入正文资源引用统计");
    }

    #[test]
    fn opf_with_illegal_control_char_is_hard_error_but_bad_chapter_only_warns() {
        let good_opf = "<?xml version=\"1.0\"?><package><metadata><dc:title xmlns:dc=\"x\">T</dc:title></metadata></package>";
        let bad_opf = "<?xml version=\"1.0\"?><package><metadata><dc:title xmlns:dc=\"x\">T\u{0}E</dc:title></metadata></package>";
        let chap = |body: &str| format!("<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><body>{body}</body></html>");
        let mk = |opf: &str, ch: &str| vec![Entry { name: "OEBPS/content.opf".into(), data: opf.as_bytes().to_vec() }, Entry { name: "OEBPS/c.xhtml".into(), data: ch.as_bytes().to_vec() }];

        let rep = check_entries(&mk(bad_opf, &chap("<p>x</p>")), false);
        assert!(!rep.ok && rep.errors.iter().any(|e| e.contains("content.opf") && e.contains("U+0000")), "{:?}", rep.errors);

        let rep = check_entries(&mk(good_opf, &chap("<p>x</div>")), false);
        assert!(rep.errors.iter().all(|e| !e.contains("合法 XML")), "正文不合法不拦: {:?}", rep.errors);
        assert!(rep.warnings.iter().any(|w| w.contains("c.xhtml")), "但要告警: {:?}", rep.warnings);

        let rep = check_entries(&mk(good_opf, &chap("<p>x</p>")), false);
        assert!(rep.errors.iter().chain(rep.warnings.iter()).all(|m| !m.contains("合法 XML")), "{:?} {:?}", rep.errors, rep.warnings);
    }

}

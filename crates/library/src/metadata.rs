//! 联网补元数据：`booklib meta`。
//!
//! 按书名、作者找到这本书，存下元数据（`Meta::info`），书里没有封面的顺带找封面（见 `cover`）：
//! 1. **豆瓣**条目（中文版）：内容简介、标签、原作名，以及条目对应版本的出版社、出版年、ISBN、译者；
//! 2. 豆瓣没有时 **Wikidata** 作品：原作名、首次出版年。
//!
//! **写进书里的只有"作品"层面的信息，而且只补书里没有的**：内容简介（`dc:description`）、标签（`dc:subject`）。
//! 出版社、ISBN、译者这些是豆瓣那个**版本**的，不一定是你手上这本（好读的书多是台湾译本，豆瓣条目多是大陆版），
//! 只记在 `meta.json` 里给人参考，不写进书。书名、作者一律用书自己的，不改。正文不动。

use crate::cover::{epub_has_cover, incomplete, CoverInfo, CoverResult};
use crate::matching::title_candidates;
use crate::{douban, wikidata, Library, Meta};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 联网找来的元数据（`Meta::info`）。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BookInfo {
    /// 从哪来的："豆瓣 <网址>" 或 "Wikidata Q…"。
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub original_title: String,
    /// 首次出版年（原作）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first_published: String,
    /// 内容简介，段落之间用换行。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    /// 匹配到的那个版本的信息（只供参考，不写进书）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
}

/// 豆瓣条目对应的版本。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Edition {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translators: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pubdate: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isbn: String,
}

impl BookInfo {
    /// 会写进书里的部分的指纹（变了产物就过期）。没有要写的返回 `None`。
    pub(crate) fn injected_sig(&self) -> Option<String> {
        if self.description.is_empty() && self.subjects.is_empty() {
            return None;
        }
        let s = format!("{}\u{0}{}", self.description, self.subjects.join("\u{0}"));
        Some(crate::fsutil::sha256_hex(s.as_bytes())[..12].to_string())
    }
}

/// 一次 `meta` 的结果。
pub enum InfoResult {
    Found(BookInfo),
    /// 已经找过（`force` 才重找）。
    Existing(BookInfo),
    /// 没找到（原因）。
    NotFound(String),
}

fn from_douban(hit: &douban::Hit, s: douban::Subject, authors: &[String]) -> BookInfo {
    // 标签里去掉作者名（豆瓣标签常带作者）
    let author_keys: Vec<String> = authors.iter().chain(s.authors.iter()).map(|a| crate::matching::norm_author(a)).collect();
    let subjects: Vec<String> = s.tags.iter().filter(|t| !t.chars().all(|c| c.is_ascii_digit()) && !author_keys.contains(&crate::matching::norm_author(t))).take(8).cloned().collect();
    BookInfo {
        source: format!("豆瓣 {}", hit.url()),
        original_title: s.original_title,
        first_published: String::new(),
        description: s.description,
        subjects,
        edition: Some(Edition {
            title: if s.subtitle.is_empty() { s.title } else { format!("{}：{}", s.title, s.subtitle) },
            authors: s.authors,
            translators: s.translators,
            publisher: s.publisher,
            pubdate: s.pubdate,
            isbn: s.isbn,
        }),
    }
}

fn from_wikidata(w: &wikidata::Work) -> BookInfo {
    BookInfo {
        source: format!("Wikidata {}", w.qid),
        original_title: [&w.original, &w.en].into_iter().find(|n| !n.is_empty()).cloned().unwrap_or_default(),
        first_published: w.published.get(..4).unwrap_or("").to_string(),
        ..Default::default()
    }
}

impl Library {
    /// 给一本书补元数据，书里没有封面的顺带找封面。已经找过的跳过（`force` 重找）。
    /// 网络出错（连不上网、重试完还不行）时返回错误、不把"没查成"当"没有"存下来，下次再查。
    pub fn fetch_metadata(&self, meta: &Meta, force: bool) -> Result<(InfoResult, CoverResult), String> {
        let r = self.fetch_metadata_inner(meta, force);
        match self.net.get() {
            // 连不上网：报第一个连不上的请求，而不是后面那些"跳过"的
            Some(net) if net.offline() && r.is_err() => Err(net.transient_error().unwrap_or_else(|| "连不上网".into())),
            _ => r,
        }
    }

    fn fetch_metadata_inner(&self, meta: &Meta, force: bool) -> Result<(InfoResult, CoverResult), String> {
        let need_info = force || meta.info.is_none();
        let need_cover = match &meta.cover {
            Some(_) if !force => false,
            _ => !self.book_has_own_cover(meta)?,
        };
        let existing_cover = || match &meta.cover {
            Some(c) if !force => CoverResult::Existing(c.clone()),
            _ => CoverResult::HasCover,
        };
        let existing_info = || meta.info.clone().map_or_else(|| InfoResult::NotFound(String::new()), InfoResult::Existing);
        if !need_info && !need_cover {
            return Ok((existing_info(), existing_cover()));
        }
        let net = self.net();
        if net.offline() {
            return Err("连不上网".into());
        }
        net.clear_transient();
        let titles = title_candidates(meta);
        let mut info: Option<BookInfo> = None;
        let mut cover: Option<CoverResult> = None;

        // ① 豆瓣
        let hits = douban::search(net, &titles, &meta.authors);
        let mut chosen = 0; // 元数据取哪个条目：封面用了哪个就取哪个
        if need_cover {
            if let Some((i, bytes, ext)) = douban::best_cover(net, &hits) {
                chosen = i;
                cover = Some(CoverResult::Found(self.store_cover(meta, &bytes, ext, hits[i].pic.clone(), hits[i].label())?));
            }
        }
        if need_info {
            let order = std::iter::once(chosen).chain((0..hits.len()).filter(|&i| i != chosen)).take(2);
            for i in order.filter(|&i| i < hits.len()) {
                if let Ok(s) = douban::subject(net, &hits[i].id) {
                    info = Some(from_douban(&hits[i], s, &meta.authors));
                    break;
                }
            }
        }

        let from_douban = info.is_some();

        // ② Wikidata：原作（元数据没找到、或封面还没有时）
        let mut work = None;
        if (need_info && info.is_none()) || (need_cover && cover.is_none()) {
            for t in &titles {
                if let Some(w) = wikidata::find_work(net, t, &meta.authors)? {
                    work = Some(w);
                    break;
                }
            }
            if need_info && info.is_none() {
                info = work.as_ref().map(from_wikidata);
            }
        }
        // 网络出过错（豆瓣可能只是没查成）：不拿 Wikidata 的元数据凑数——存下了下次就不再查
        if !from_douban && net.transient_error().is_some() {
            info = None;
        }

        let info_result = if !need_info {
            existing_info()
        } else if let Some(i) = info {
            let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
            m.info = Some(i.clone());
            self.save_meta(&m)?;
            InfoResult::Found(i)
        } else if let Some(e) = net.transient_error() {
            return Err(format!("网络出错，没查完（{e}），下次再试"));
        } else {
            InfoResult::NotFound("豆瓣、Wikidata 里都找不到书名、作者对得上的书".into())
        };
        if need_cover && cover.is_none() {
            cover = Some(match self.cover_from_work(net, meta, work.as_ref())? {
                Ok(c) => CoverResult::Found(c),
                // 没找到是因为网络出错：不生成（生成的会存下来，以后就不找了）
                Err(_) if net.transient_error().is_some() => return Err(incomplete(&net.transient_error().unwrap_or_default())),
                // ③ 都没有：生成
                Err(why) => CoverResult::Generated(self.generate_cover(net, meta)?, why),
            });
        }
        Ok((info_result, cover.unwrap_or_else(existing_cover)))
    }

    /// 去掉联网找来的元数据和封面（找错了的时候）。返回有没有东西被去掉。
    pub fn clear_metadata(&self, meta: &Meta) -> Result<bool, String> {
        let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
        let had = m.info.take().is_some();
        let cover: Option<CoverInfo> = m.cover.take();
        if let Some(c) = &cover {
            let _ = std::fs::remove_file(self.entry_dir(&meta.id).join(&c.file));
        }
        if had || cover.is_some() {
            self.save_meta(&m)?;
        }
        Ok(had || cover.is_some())
    }
}

/// 生成产物时往书里补的东西。
pub(crate) struct Additions<'a> {
    /// 封面图和扩展名（书里没封面时才给）。
    pub cover: Option<(Vec<u8>, &'a str)>,
    pub info: Option<&'a BookInfo>,
}

/// 复制 `src` 到 `dst`，补上书里没有的：封面、`dc:description`、`dc:subject`。书里已有的不动。
/// 改写用 `bookconv::opfmeta`（与 `ebook-meta` 命令同一份实现，含 EPUB 3 规范整理；不改 `dcterms:modified`，产物逐字节可重现）。
/// 什么都不用补时不写 `dst`，返回 `false`。
pub(crate) fn inject(src: &Path, dst: &Path, add: &Additions) -> Result<bool, String> {
    use bookconv::opfmeta::{self, DcField, Edits};
    let current = opfmeta::read_epub(src)?;
    let has = |f: DcField| current.iter().any(|(x, v)| *x == f && !v.is_empty());
    let mut edits = Edits { cover: add.cover.as_ref().map(|(b, _)| bookconv::opfmeta::CoverEdit::Set(b.clone())), ..Default::default() };
    if let Some(info) = add.info {
        if !has(DcField::Description) && !info.description.is_empty() {
            edits.set.push((DcField::Description, vec![info.description.clone()]));
        }
        if !has(DcField::Subject) && !info.subjects.is_empty() {
            edits.set.push((DcField::Subject, info.subjects.clone()));
        }
    }
    if edits.is_empty() {
        return Ok(false);
    }
    opfmeta::edit_epub(src, dst, &edits)?;
    Ok(true)
}

/// 生成产物前：书里缺的封面、简介、标签补进去。补了返回新文件，没补返回原文件。
pub(crate) fn with_additions(lib: &Library, meta: &Meta, epub: &Path, tmp: &Path) -> Result<std::path::PathBuf, String> {
    let cover = match &meta.cover {
        Some(c) if !epub_has_cover(epub) => {
            let img = std::fs::read(lib.entry_dir(&meta.id).join(&c.file)).map_err(|e| format!("读封面: {e}"))?;
            Some((img, if c.file.ends_with(".png") { "png" } else { "jpg" }))
        }
        _ => None,
    };
    let add = Additions { cover, info: meta.info.as_ref() };
    if add.cover.is_none() && add.info.and_then(BookInfo::injected_sig).is_none() {
        return Ok(epub.to_path_buf());
    }
    let with = tmp.join("with-metadata.epub");
    Ok(if inject(epub, &with, &add)? { with } else { epub.to_path_buf() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample(dir: &Path, description: &str) -> std::path::PathBuf {
        use bookconv::epub::{assemble, Book, BookMeta, Chapter};
        let mut book = Book {
            meta: BookMeta { book_id: "t".into(), title: "书".into(), author: "作者".into(), language: "zh".into(), publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into() },
            chapters: vec![Chapter { title: "一".into(), html_body: "<h1>一</h1><p>正文</p>".into(), level: 1 }],
            resources: vec![],
            nav: vec![],
        };
        let mut bytes = assemble(&mut book).unwrap();
        if !description.is_empty() {
            let mut es = bookconv::epubzip::read_entries(&bytes).unwrap();
            let opf = es.iter_mut().find(|e| e.name.ends_with(".opf")).unwrap();
            let t = String::from_utf8(opf.data.clone()).unwrap().replace("</metadata>", &format!("<dc:description>{description}</dc:description></metadata>"));
            opf.data = t.into_bytes();
            let mut zw = bookconv::zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for e in &es {
                zw.start_file(e.name.as_str(), bookconv::zip::write::SimpleFileOptions::default()).unwrap();
                zw.write_all(&e.data).unwrap();
            }
            bytes = zw.finish().unwrap().into_inner();
        }
        let p = dir.join(format!("a{}.epub", description.len()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn inject_adds_cover_and_missing_metadata_only() {
        let d = tempfile::tempdir().unwrap();
        let src = sample(d.path(), "");
        let dst = d.path().join("b.epub");
        assert!(!epub_has_cover(&src));
        let mut jpg = Vec::new();
        image::RgbImage::new(400, 600).write_to(&mut std::io::Cursor::new(&mut jpg), image::ImageFormat::Jpeg).unwrap();
        assert_eq!(crate::cover::plausible_cover(&jpg), Some("jpg"));
        let info = BookInfo { source: "t".into(), description: "简介 & <引号>\n第二段".into(), subjects: vec!["推理".into(), "日本".into()], ..Default::default() };
        assert!(inject(&src, &dst, &Additions { cover: Some((jpg, "jpg")), info: Some(&info) }).unwrap());
        assert!(epub_has_cover(&dst));
        let (a, b) = (bookconv::epubzip::read_entries(&std::fs::read(&src).unwrap()).unwrap(), bookconv::epubzip::read_entries(&std::fs::read(&dst).unwrap()).unwrap());
        // 新加了封面图；过了 EPUB 3 规范整理，没有导航文档的书补一份 nav
        assert!(b.len() > a.len());
        for e in a.iter().filter(|e| !e.name.ends_with(".opf")) {
            let out = &b.iter().find(|x| x.name == e.name).unwrap().data;
            if bookconv::epubzip::is_html(&e.name) || e.name.ends_with(".ncx") {
                let text = |d: &[u8]| bookconv::html::plain_text(&String::from_utf8_lossy(d));
                assert_eq!(text(out), text(&e.data), "{} 可见文字不变", e.name);
            } else {
                assert_eq!(*out, e.data, "{} 原样", e.name);
            }
        }
        let opf = String::from_utf8(b.iter().find(|e| e.name.ends_with(".opf")).unwrap().data.clone()).unwrap();
        let dc = bookconv::wash::opf_dc(&opf);
        assert_eq!(dc.description, "简介 & <引号> 第二段", "{opf}");
        assert_eq!(dc.title, "书");
        assert_eq!(opf.matches("<dc:subject>").count(), 2);
        assert!(opf.contains(r#"version="3.0""#) && opf.contains("dcterms:modified"), "EPUB 3: {opf}");

        // 书里已有简介：不覆盖；什么都不用补时不写
        let src2 = sample(d.path(), "原书简介");
        let dst2 = d.path().join("c.epub");
        let only_desc = BookInfo { source: "t".into(), description: "新简介".into(), ..Default::default() };
        assert!(!inject(&src2, &dst2, &Additions { cover: None, info: Some(&only_desc) }).unwrap());
        assert!(!dst2.exists());
    }
}

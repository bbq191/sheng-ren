//! 联网补元数据：`booklib meta`。
//!
//! 按书名、作者找到这本书，存下元数据（`Meta::info`），书里没有封面的顺带找封面（见 `cover`）：
//! 1. **豆瓣**条目（中文版）：内容简介、标签、原作名，以及条目对应版本的出版社、出版年、ISBN、译者；
//! 2. 豆瓣没有时 **Wikidata** 作品：原作名、首次出版年。
//!
//! **写进书里的只有"作品"层面的信息，而且只补书里没有的**：内容简介（`dc:description`）、标签（`dc:subject`）。
//! 出版社、ISBN、译者这些是豆瓣那个**版本**的，不一定是你手上这本（好读的书多是台湾译本，豆瓣条目多是大陆版），
//! 只记在 `meta.json` 里给人参考，不写进书。书名、作者一律用书自己的，不改。正文不动。

use crate::cover::{epub_has_cover, CoverInfo, CoverResult};
use crate::matching::title_candidates;
use crate::net::Net;
use crate::{douban, wikidata, Library, Meta};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, Write};
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
    pub fn fetch_metadata(&self, meta: &Meta, force: bool) -> Result<(InfoResult, CoverResult), String> {
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
        let net = Net::new();
        let titles = title_candidates(meta);
        let mut info: Option<BookInfo> = None;
        let mut cover: Option<CoverResult> = None;

        // ① 豆瓣
        let hits = douban::search(&net, &titles, &meta.authors);
        let mut chosen = 0; // 元数据取哪个条目：封面用了哪个就取哪个
        if need_cover {
            if let Some((i, bytes, ext)) = douban::best_cover(&net, &hits) {
                chosen = i;
                cover = Some(CoverResult::Found(self.store_cover(meta, &bytes, ext, hits[i].pic.clone(), hits[i].label())?));
            }
        }
        if need_info {
            let order = std::iter::once(chosen).chain((0..hits.len()).filter(|&i| i != chosen)).take(2);
            for i in order.filter(|&i| i < hits.len()) {
                if let Ok(s) = douban::subject(&net, &hits[i].id) {
                    info = Some(from_douban(&hits[i], s, &meta.authors));
                    break;
                }
            }
        }

        // ② Wikidata：原作（元数据没找到、或封面还没有时）
        let mut work = None;
        if (need_info && info.is_none()) || (need_cover && cover.is_none()) {
            for t in &titles {
                if let Some(w) = wikidata::find_work(&net, t, &meta.authors)? {
                    work = Some(w);
                    break;
                }
            }
            if need_info && info.is_none() {
                info = work.as_ref().map(from_wikidata);
            }
        }
        if need_cover && cover.is_none() {
            cover = Some(match self.cover_from_work(&net, meta, work.as_ref())? {
                Ok(c) => CoverResult::Found(c),
                // ③ 都没有：生成
                Err(why) => CoverResult::Generated(self.generate_cover(&net, meta)?, why),
            });
        }

        let info_result = if !need_info {
            existing_info()
        } else if let Some(i) = info {
            let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
            m.info = Some(i.clone());
            self.save_meta(&m)?;
            InfoResult::Found(i)
        } else {
            InfoResult::NotFound("豆瓣、Wikidata 里都找不到书名、作者对得上的书".into())
        };
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

/// 复制 `src` 到 `dst`，补上书里没有的：封面（图片放进 OPF 所在目录，manifest 加 `properties="cover-image"` 一项，
/// 加 `<meta name="cover">`）、`dc:description`、`dc:subject`。书里已有的不动；其余条目原样拷（不解压不重压）。
/// 什么都不用补时不写 `dst`，返回 `false`。
pub(crate) fn inject(src: &Path, dst: &Path, add: &Additions) -> Result<bool, String> {
    let f = std::fs::File::open(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut zin = bookconv::zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let container = read_text(&mut zin, "META-INF/container.xml")?;
    let opf_path = bookconv::wash::tag_attr(&container, "full-path").ok_or("container.xml 里没有 full-path")?.to_string();
    let opf = read_text(&mut zin, &opf_path)?;
    let dc = bookconv::wash::opf_dc(&opf);
    let esc = bookconv::util::xml_escape;

    let mut meta_add = String::new();
    let mut manifest_add = String::new();
    let mut image: Option<(String, &[u8])> = None;
    if let Some((bytes, ext)) = &add.cover {
        let dir = bookconv::epubzip::dir_of(&opf_path);
        let img_name = format!("eink-cover.{ext}");
        let img_path = if dir.is_empty() { img_name.clone() } else { format!("{dir}/{img_name}") };
        let mime = if *ext == "png" { "image/png" } else { "image/jpeg" };
        manifest_add.push_str(&format!(r#"<item id="eink-cover" href="{img_name}" media-type="{mime}" properties="cover-image"/>"#));
        meta_add.push_str(r#"<meta name="cover" content="eink-cover"/>"#);
        image = Some((img_path, bytes.as_slice()));
    }
    if let Some(info) = add.info {
        if dc.description.is_empty() && !info.description.is_empty() {
            meta_add.push_str(&format!("<dc:description>{}</dc:description>", esc(&info.description)));
        }
        if !opf.contains("<dc:subject") {
            for s in &info.subjects {
                meta_add.push_str(&format!("<dc:subject>{}</dc:subject>", esc(s)));
            }
        }
    }
    if meta_add.is_empty() && manifest_add.is_empty() {
        return Ok(false);
    }
    let mut new_opf = opf.clone();
    if !manifest_add.is_empty() {
        let m = new_opf.find("</manifest>").ok_or("OPF 没有 </manifest>")?;
        new_opf.insert_str(m, &manifest_add);
    }
    let md = new_opf.find("</metadata>").or_else(|| new_opf.find("</opf:metadata>")).ok_or("OPF 没有 </metadata>")?;
    new_opf.insert_str(md, &meta_add);

    let out = std::fs::File::create(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let mut zw = bookconv::zip::ZipWriter::new(out);
    for i in 0..zin.len() {
        let e = zin.by_index_raw(i).map_err(|e| e.to_string())?;
        if e.name() == opf_path {
            drop(e);
            let opts = bookconv::zip::write::SimpleFileOptions::default().compression_method(bookconv::zip::CompressionMethod::Deflated);
            zw.start_file(opf_path.as_str(), opts).map_err(|e| e.to_string())?;
            zw.write_all(new_opf.as_bytes()).map_err(|e| e.to_string())?;
        } else {
            zw.raw_copy_file(e).map_err(|e| e.to_string())?;
        }
    }
    if let Some((path, bytes)) = image {
        let opts = bookconv::zip::write::SimpleFileOptions::default().compression_method(bookconv::zip::CompressionMethod::Stored);
        zw.start_file(path.as_str(), opts).map_err(|e| e.to_string())?;
        zw.write_all(bytes).map_err(|e| e.to_string())?;
    }
    zw.finish().map_err(|e| e.to_string())?;
    Ok(true)
}

fn read_text<R: Read + Seek>(z: &mut bookconv::zip::ZipArchive<R>, name: &str) -> Result<String, String> {
    let b = bookconv::epubzip::read_by_name(z, name)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
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
        assert_eq!(b.len(), a.len() + 1);
        for e in a.iter().filter(|e| !e.name.ends_with(".opf")) {
            assert_eq!(b.iter().find(|x| x.name == e.name).unwrap().data, e.data, "{} 原样", e.name);
        }
        let opf = String::from_utf8(b.iter().find(|e| e.name.ends_with(".opf")).unwrap().data.clone()).unwrap();
        let dc = bookconv::wash::opf_dc(&opf);
        assert_eq!(dc.description, "简介 & <引号> 第二段", "{opf}");
        assert_eq!(dc.title, "书");
        assert_eq!(opf.matches("<dc:subject>").count(), 2);

        // 书里已有简介：不覆盖；什么都不用补时不写
        let src2 = sample(d.path(), "原书简介");
        let dst2 = d.path().join("c.epub");
        let only_desc = BookInfo { source: "t".into(), description: "新简介".into(), ..Default::default() };
        assert!(!inject(&src2, &dst2, &Additions { cover: None, info: Some(&only_desc) }).unwrap());
        assert!(!dst2.exists());
    }
}

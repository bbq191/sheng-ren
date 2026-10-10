//! 联网补元数据：`booklib meta --fetch`。
//!
//! 按书名、作者找到这本书，存下元数据（`Meta::info`），书里没有封面的顺带找封面（见 `cover`）：
//! 1. **豆瓣**条目（中文版）：内容简介、标签、原作名，以及条目对应版本的出版社、出版年、ISBN、译者；
//! 2. 豆瓣没有时 **QQ 阅读**（`qqread`，网络文学多在这里）：内容简介、分类当标签、封面；
//! 3. 都没有时 **Wikidata** 作品：原作名、首次出版年。
//!
//! **写进书里的只有"作品"层面的信息，而且只补书里没有的**：内容简介（`dc:description`）、标签（`dc:subject`）。
//! 出版社、ISBN、译者这些是豆瓣那个**版本**的，不一定是你手上这本（好读的书多是台湾译本，豆瓣条目多是大陆版），
//! 只记在 `meta.json` 里给人参考，不写进书。书名、作者一律用书自己的，不改。正文不动。

use crate::cover::{epub_has_cover, incomplete, CoverInfo, CoverResult};
use crate::net::Net;
use crate::wikidata::Work;
use crate::{Library, Meta};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 联网找来的元数据（`Meta::info`）。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BookInfo {
    /// 从哪来的："豆瓣 <网址>"、"QQ阅读 <网址>" 或 "Wikidata Q…"。
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
    /// 可能写进书里的部分（简介、标签）的指纹。没有要写的返回 `None`。
    pub(crate) fn injected_sig(&self) -> Option<String> {
        self.injected_sig_for(false, false)
    }

    /// 真正会写进书里的部分的指纹（变了产物就过期）：书里已有简介（`has_description`）、标签（`has_subjects`）的，
    /// 那一项不补（与 [`inject`] 的判定一致），也不进指纹。没有要写的返回 `None`。两项都缺时和 [`BookInfo::injected_sig`] 相同。
    pub(crate) fn injected_sig_for(&self, has_description: bool, has_subjects: bool) -> Option<String> {
        let description = if has_description { "" } else { self.description.as_str() };
        let subjects: &[String] = if has_subjects { &[] } else { &self.subjects };
        if description.is_empty() && subjects.is_empty() {
            return None;
        }
        let s = format!("{description}\u{0}{}", subjects.join("\u{0}"));
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
    /// 网络出错没查成（原因），什么都没存、下次再查；封面这一步有结果（已存下）时才这样报，不然整本报错。
    Failed(String),
}

/// 一个书目网站（豆瓣、QQ 阅读）：按书名作者搜条目、从条目里挑封面、取元数据。搜索的骨架（书名规整、作者规整、
/// 依次试搜索词、出错就停、第一批对得上的就用）是 [`BookSource::search`] 的缺省实现，各网站只给搜索词、网址和怎么认条目。
/// 排进 [`CHAIN`] 的先后就是查找的先后。
pub(crate) trait BookSource {
    /// 搜索结果里书名、作者对得上的一条。
    type Hit;

    /// 搜索词，按先后试（`title` 已去掉首尾空白）。
    fn queries(&self, title: &str, authors: &[String]) -> Vec<String>;

    /// 一次搜索的网址（回 JSON）。
    fn search_url(&self, q: &str) -> String;

    /// 回执里书名（规整后 `nt`）、作者（规整后 `want`）对得上的条目，保持网站给的顺序；回执本身说出错了返回 `Err`
    /// （记成临时错误，别的搜索词也不再试）。
    fn matching_hits(&self, v: &serde_json::Value, nt: &str, want: &[String]) -> Result<Vec<Self::Hit>, String>;

    /// 按书里元数据的书名、作者找（2026-10-08 用户定）：依次试 [`BookSource::queries`]，第一批对得上的就用。
    fn search(&self, net: &Net, title: &str, authors: &[String]) -> Vec<Self::Hit> {
        let nt = crate::matching::norm_s(title);
        if nt.is_empty() {
            return Vec::new();
        }
        let want: Vec<String> = authors.iter().map(|a| crate::matching::norm_author(a)).filter(|a| !a.is_empty()).collect();
        for q in self.queries(title.trim(), authors) {
            let url = self.search_url(&q);
            // 出错（被拦、回来的不是 JSON、网络问题）已记成临时错误（`Net::transient_error`），调用方不会当成"没这本书"；
            // 别的搜索词也不用再试了
            let Ok(v) = net.json_strict(&url) else { break };
            match self.matching_hits(&v, &nt, &want) {
                Ok(hits) if !hits.is_empty() => return hits,
                Ok(_) => {}
                Err(e) => {
                    net.note_transient(&format!("{url}: {e}"));
                    break;
                }
            }
        }
        Vec::new()
    }

    /// 从对得上的条目里挑一张像封面的图：(条目下标, 图片, 扩展名)。
    fn cover(&self, net: &Net, hits: &[Self::Hit]) -> Option<(usize, Vec<u8>, &'static str)>;

    /// 封面图从哪下载的、给人核对的条目说明（记进 `meta.json`）。
    fn cover_origin(&self, hit: &Self::Hit) -> (String, String);

    /// 元数据：先看 `chosen`（封面用了哪个条目就取哪个），网站自己决定还看不看别的条目。`authors` 用来从标签里去掉作者名。
    fn info(&self, net: &Net, hits: &[Self::Hit], chosen: usize, authors: &[String]) -> Option<BookInfo>;
}

/// 查找过程中的进度：责任链上各环依次往里填。
pub(crate) struct Lookup<'a> {
    pub(crate) lib: &'a Library,
    pub(crate) net: &'a Net,
    pub(crate) meta: &'a Meta,
    /// 书里元数据的书名（去掉首尾空白）。
    pub(crate) title: &'a str,
    pub(crate) need_info: bool,
    pub(crate) need_cover: bool,
    pub(crate) info: Option<BookInfo>,
    /// 网站上找到、已经存下的封面。
    pub(crate) cover: Option<CoverResult>,
    /// Wikidata 找到的原作（找封面的后备：Open Library / Commons），和查它时出的错。
    pub(crate) work: Option<Work>,
    pub(crate) work_err: Option<String>,
}

impl Lookup<'_> {
    pub(crate) fn wants_info(&self) -> bool {
        self.need_info && self.info.is_none()
    }

    pub(crate) fn wants_cover(&self) -> bool {
        self.need_cover && self.cover.is_none()
    }
}

/// 责任链上的一环：还缺东西时才轮到它（[`Library::fetch_metadata_inner`]）。
pub(crate) trait Step {
    fn run(&self, l: &mut Lookup<'_>) -> Result<(), String>;
}

/// 书目网站这一环：搜、封面（先存下）、元数据。
impl<S: BookSource> Step for S {
    fn run(&self, l: &mut Lookup<'_>) -> Result<(), String> {
        // 前面的网站出错没查成时不查：这个网站的结果存下了，以后就不会再去前面的网站找
        if l.net.transient_error().is_some() {
            return Ok(());
        }
        let hits = self.search(l.net, l.title, &l.meta.authors);
        let mut chosen = 0; // 元数据取哪个条目：封面用了哪个就取哪个
        if l.wants_cover() {
            if let Some((i, bytes, ext)) = self.cover(l.net, &hits) {
                chosen = i;
                let (url, label) = self.cover_origin(&hits[i]);
                l.cover = Some(CoverResult::Found(l.lib.store_cover(l.meta, &bytes, ext, url, label)?));
            }
        }
        if l.wants_info() {
            l.info = self.info(l.net, &hits, chosen, &l.meta.authors);
        }
        Ok(())
    }
}

/// 查找的先后：① 豆瓣（中文版）② QQ 阅读（豆瓣没有的，网络文学多是这样）③ Wikidata（原作）。
const CHAIN: &[&dyn Step] = &[&crate::douban::Douban, &crate::qqread::QqRead, &crate::wikidata::Wikidata];

/// 从标签里去掉作者名（豆瓣标签常带作者），最多 8 个。
pub(crate) fn tags_without_authors<'a>(tags: impl Iterator<Item = &'a String>, authors: impl Iterator<Item = &'a String>) -> Vec<String> {
    let author_keys: Vec<String> = authors.map(|a| crate::matching::norm_author(a)).collect();
    tags.filter(|t| !author_keys.contains(&crate::matching::norm_author(t))).take(8).cloned().collect()
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
        // 找书只用书里的元数据：书名 + 作者（2026-10-08 用户定，不再从文件名里取书名）
        let mut l = Lookup { lib: self, net, meta, title: meta.title.trim(), need_info, need_cover, info: None, cover: None, work: None, work_err: None };
        for step in CHAIN {
            if l.wants_info() || l.wants_cover() {
                step.run(&mut l)?;
            }
        }
        let Lookup { info, mut cover, work, work_err, .. } = l;
        // 网站上的封面已经存下了：后面出错也不整本报错，如实报"封面已存、元数据没查成"
        let cover_stored = cover.is_some();

        let info_result = if !need_info {
            existing_info()
        } else if let Some(i) = info {
            let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
            m.info = Some(i.clone());
            self.save_meta(&m)?;
            InfoResult::Found(i)
        } else if let Some(e) = net.transient_error().or_else(|| work_err.clone()) {
            let e = format!("网络出错，没查完（{e}），下次再试");
            if !cover_stored {
                return Err(e);
            }
            InfoResult::Failed(e)
        } else {
            InfoResult::NotFound("豆瓣、QQ 阅读、Wikidata 里都找不到书名、作者对得上的书".into())
        };
        if need_cover && cover.is_none() {
            let found = (|| {
                if let Some(e) = &work_err {
                    return Err(incomplete(e));
                }
                Ok(match self.cover_from_work(net, meta, work.as_ref())? {
                    Ok(c) => CoverResult::Found(c),
                    // 没找到是因为网络出错：不生成（生成的会存下来，以后就不找了）
                    Err(_) if net.transient_error().is_some() => return Err(incomplete(&net.transient_error().unwrap_or_default())),
                    // ④ 都没有：生成
                    Err(why) => CoverResult::Generated(self.generate_cover(net, meta)?, why),
                })
            })();
            cover = Some(match found {
                Ok(c) => c,
                // 元数据这次已经存下了：如实报"元数据已存、封面没查成"，不整本报错
                Err(e) if matches!(info_result, InfoResult::Found(_)) => CoverResult::Failed(e),
                Err(e) => return Err(e),
            });
        }
        Ok((info_result, cover.unwrap_or_else(existing_cover)))
    }

    /// 去掉联网找来的元数据和封面（找错了的时候）。返回有没有东西被去掉。
    pub fn clear_metadata(&self, meta: &Meta) -> Result<bool, String> {
        let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
        let had = m.info.take().is_some();
        let cover: Option<CoverInfo> = m.cover.take();
        if had || cover.is_some() {
            self.save_meta(&m)?;
        }
        // meta 存好了才删图：存失败时 meta 还指着它
        if let Some(c) = &cover {
            let _ = std::fs::remove_file(self.entry_dir(&meta.id).join(&c.file));
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

/// 书里已有（非空的）简介、标签：(有简介, 有标签)。[`inject`] 和生成指纹共用这一个判定。
pub(crate) fn own_description_subjects(epub: &Path) -> Result<(bool, bool), String> {
    use bookconv::opfmeta::{self, DcField};
    let current = opfmeta::read_epub(epub)?;
    let has = |f: DcField| current.iter().any(|(x, v)| *x == f && !v.is_empty());
    Ok((has(DcField::Description), has(DcField::Subject)))
}

/// 复制 `src` 到 `dst`，补上书里没有的：封面、`dc:description`、`dc:subject`。书里已有的不动。
/// 改写用 `bookconv::opfmeta`（与 `booklib meta --edit` 同一份实现；不做 EPUB 3 规范整理——优化器的清洗层会做；不改 `dcterms:modified`，产物逐字节可重现）。
/// 什么都不用补时不写 `dst`，返回 `false`。
pub(crate) fn inject(src: &Path, dst: &Path, add: &Additions) -> Result<bool, String> {
    use bookconv::opfmeta::{self, DcField, Edits};
    let (has_description, has_subjects) = own_description_subjects(src)?;
    let mut edits = Edits { cover: add.cover.as_ref().map(|(b, _)| bookconv::opfmeta::CoverEdit::Set(b.clone())), ..Default::default() };
    if let Some(info) = add.info {
        if !has_description && !info.description.is_empty() {
            edits.set.push((DcField::Description, vec![info.description.clone()]));
        }
        if !has_subjects && !info.subjects.is_empty() {
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
    use std::cell::Cell;
    use std::io::Write;

    /// 不联网的假网站：搜到一条，给出元数据；记下被搜了几次。
    struct Fake {
        searched: Cell<usize>,
        found: &'static str,
    }

    impl BookSource for Fake {
        type Hit = ();
        fn queries(&self, _: &str, _: &[String]) -> Vec<String> {
            Vec::new()
        }
        fn search_url(&self, _: &str) -> String {
            String::new()
        }
        fn matching_hits(&self, _: &serde_json::Value, _: &str, _: &[String]) -> Result<Vec<()>, String> {
            Ok(Vec::new())
        }
        fn search(&self, _: &Net, _: &str, _: &[String]) -> Vec<()> {
            self.searched.set(self.searched.get() + 1);
            vec![()]
        }
        fn cover(&self, _: &Net, _: &[()]) -> Option<(usize, Vec<u8>, &'static str)> {
            None
        }
        fn cover_origin(&self, _: &()) -> (String, String) {
            (String::new(), String::new())
        }
        fn info(&self, _: &Net, _: &[()], _: usize, _: &[String]) -> Option<BookInfo> {
            (!self.found.is_empty()).then(|| BookInfo { source: self.found.into(), ..Default::default() })
        }
    }

    /// 责任链：前一个网站找到了就不再问后面的；前面出过临时错误时后面的网站不查（结果存下了就不会再去前面找）。
    #[test]
    fn chain_stops_when_found_and_skips_sites_after_transient_errors() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(d.path()).unwrap();
        let net = Net::new();
        let meta = Meta { title: "书".into(), ..Default::default() };
        let lookup = || Lookup { lib: &lib, net: &net, meta: &meta, title: "书", need_info: true, need_cover: false, info: None, cover: None, work: None, work_err: None };
        let (a, b) = (Fake { searched: Cell::new(0), found: "" }, Fake { searched: Cell::new(0), found: "乙" });
        let c = Fake { searched: Cell::new(0), found: "丙" };
        let mut l = lookup();
        for s in [&a as &dyn Step, &b, &c] {
            if l.wants_info() || l.wants_cover() {
                s.run(&mut l).unwrap();
            }
        }
        assert_eq!(l.info.map(|i| i.source).as_deref(), Some("乙"));
        assert_eq!((a.searched.get(), b.searched.get(), c.searched.get()), (1, 1, 0), "找到了就不再问后面的");

        net.note_transient("豆瓣被拦了");
        let mut l = lookup();
        b.run(&mut l).unwrap();
        assert!(l.info.is_none());
        assert_eq!(b.searched.get(), 1, "出过临时错误：不查");
    }

    fn sample(dir: &Path, description: &str) -> std::path::PathBuf {
        use bookconv::epub::{assemble, Book, BookMeta, Chapter};
        let mut book = Book {
            meta: BookMeta { book_id: "t".into(), title: "书".into(), author: "作者".into(), language: "zh".into(), publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
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
        // 只新加了封面图、改了 OPF；不做规范整理（优化器的清洗层会做），别的条目逐字节原样
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

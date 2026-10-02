//! 封面：书里没有封面时，联网找一张（`booklib meta --fetch` 顺带做），找不到就生成。
//!
//! 来源依次是（参照 Koodo Reader 用的书目源，只借鉴"用哪些源"，代码是自己写的——它是 AGPL-3.0）：
//! 1. **豆瓣**：中文版封面，最贴近用户手上的书（见 `douban`）。挑前几个对得上的条目里分辨率最高的。
//! 2. 豆瓣没有时找**原作**的封面（用户 2026-09-27：原版封面也可以）：Wikidata 找到作品（见 `wikidata`），
//!    Open Library 作品本身的封面 → 按英文名、原文名搜 → Wikimedia Commons 上的作品图片（多是初版封面）。
//! 3. 都找不到：**生成**一张——书名在上、作者头像（Wikidata 人物照片）居中、作者名在下，样式按书的 id 挑
//!    （见 `covergen`）。没有作者照片时中间画作者名的第一个字。
//!
//! 找到的封面存在 `masters/<id>/cover.<ext>`，来源记进 `meta.json`，错了可以 `booklib meta --fetch --clear` 去掉。
//! 生成产物时书里没有封面才放进去（只在 OPF 里声明封面图，不加封面页，正文不变）。原件不动。

use crate::fsutil::sha256_hex;
use crate::net::{enc, Net};
use crate::wikidata::Work;
use crate::{Library, Meta};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// 书库里存着的封面（`Meta::cover`）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CoverInfo {
    /// `masters/<id>/` 下的文件名。
    pub file: String,
    pub sha256: String,
    /// 从哪下载的。
    pub source_url: String,
    /// 匹配到的条目或作品（给人核对）。
    pub work: String,
}

/// 一次找封面的结果。
pub enum CoverResult {
    Found(CoverInfo),
    /// 书里本来就有封面。
    HasCover,
    /// 已经找过、存着了（`force` 才重找）。
    Existing(CoverInfo),
    /// 没找到封面，生成了一张（第二项是没找到的原因）。
    Generated(CoverInfo, String),
    /// 网络出错没查成（原因），没存、下次再查；元数据这一步有结果（已存下）时才这样报，不然整本报错。
    Failed(String),
}

pub(crate) fn cover_urls(net: &Net, w: &Work) -> Vec<String> {
    let mut ids: Vec<i64> = Vec::new();
    let mut add = |id: i64| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    if !w.ol.is_empty() {
        if let Ok(v) = net.json(&format!("https://openlibrary.org/works/{}.json", enc(&w.ol))) {
            v["covers"].as_array().into_iter().flatten().filter_map(Value::as_i64).filter(|&c| c > 0).take(3).for_each(&mut add);
        }
    }
    let mut queries: Vec<String> = Vec::new();
    if !w.en.is_empty() {
        if !w.author_en.is_empty() {
            queries.push(format!("title={}&author={}", enc(&w.en), enc(&w.author_en)));
        }
        queries.push(format!("title={}", enc(&w.en)));
    }
    for t in [&w.original, &w.ja] {
        if !t.is_empty() && t != &w.en {
            queries.push(format!("title={}", enc(t)));
        }
    }
    for q in queries {
        if let Ok(v) = net.json(&format!("https://openlibrary.org/search.json?limit=5&fields=cover_i&{q}")) {
            v["docs"].as_array().into_iter().flatten().filter_map(|d| d["cover_i"].as_i64()).take(3).for_each(&mut add);
        }
    }
    let mut urls: Vec<String> = ids.into_iter().take(8).map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg?default=false")).collect();
    if !w.image.is_empty() {
        urls.push(format!("{}?width=1000", w.image.replace("http://", "https://")));
    }
    urls
}

/// 下载下来的是不是像样的封面：能解码、够大、竖版比例。
pub(crate) fn plausible_cover(bytes: &[u8]) -> Option<&'static str> {
    let (ext, _) = bookconv::convert::common::image_ext_mime(bytes)?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
    (h >= 300 && w >= 180 && (0.45..=1.05).contains(&(w as f32 / h as f32))).then_some(ext)
}

/// EPUB 里有没有封面（有效的封面声明，或第一页里有图——优化时会把它声明成封面）。
pub(crate) fn epub_has_cover(epub: &Path) -> bool {
    bookconv::epubzip::cover_image_of(epub).is_some()
}

impl Library {
    /// 找原作封面：Wikidata 作品 → Open Library / Commons。`work` 是已经找到的作品（没有就返回原因）。
    pub(crate) fn cover_from_work(&self, net: &Net, meta: &Meta, work: Option<&Work>) -> Result<Result<CoverInfo, String>, String> {
        let Some(work) = work else { return Ok(Err("豆瓣、Wikidata 里都找不到书名、作者对得上的书".into())) };
        let name = [&work.en, &work.original, &work.ja].into_iter().find(|n| !n.is_empty()).cloned().unwrap_or_default();
        let label = format!("{} {name}", work.qid);
        for url in cover_urls(net, work) {
            let Ok(bytes) = net.fetch(&url) else { continue };
            let Some(ext) = plausible_cover(&bytes) else { continue };
            return self.store_cover(meta, &bytes, ext, url, label).map(Ok);
        }
        Ok(Err(format!("找到了作品 {label}，但没有可用的封面图")))
    }

    /// 生成封面（书名 + 作者头像）。
    pub(crate) fn generate_cover(&self, net: &Net, meta: &Meta) -> Result<CoverInfo, String> {
        let author = meta.authors.first().map(|a| a.trim().to_string()).unwrap_or_default();
        let portrait = crate::wikidata::author_portrait(net, &meta.authors)?;
        let photo = portrait.as_ref().and_then(|(_, url)| net.fetch(url).ok()).filter(|b| image::load_from_memory(b).is_ok());
        // 作者照片是因为网络出错才没拿到的：不生成（存下来就不会再找了），下次再试
        if let Some(e) = net.transient_error() {
            return Err(incomplete(&e));
        }
        let font = crate::covergen::load_font()?;
        let seed = u64::from_str_radix(meta.id.get(..12).unwrap_or(&meta.id), 16).unwrap_or(0);
        let bytes = crate::covergen::render(&font, &meta.title, &author, photo.as_deref(), seed)?;
        let (url, work) = match (&portrait, &photo) {
            (Some((who, url)), Some(_)) => (url.clone(), format!("生成（书名 + 作者头像 {who}）")),
            _ => (String::new(), "生成（书名 + 作者名字标，没找到作者头像）".to_string()),
        };
        self.store_cover(meta, &bytes, "jpg", url, work)
    }

    pub(crate) fn store_cover(&self, meta: &Meta, bytes: &[u8], ext: &str, source_url: String, work: String) -> Result<CoverInfo, String> {
        let info = CoverInfo { file: format!("cover.{ext}"), sha256: sha256_hex(bytes), source_url, work };
        let dir = self.entry_dir(&meta.id);
        crate::fsutil::write_atomic(&dir.join(&info.file), bytes)?;
        let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
        let old = m.cover.replace(info.clone()).filter(|o| o.file != info.file);
        self.save_meta(&m)?;
        // meta 存好了才删旧图：存失败时 meta 还指着旧图
        if let Some(old) = old {
            let _ = std::fs::remove_file(dir.join(&old.file));
        }
        Ok(info)
    }

    /// 书自己有没有封面（EPUB 看封面声明和第一页的图）。
    /// CBZ 不用转：第一页的图就是封面（`cbz_to_epub`；没有图的 CBZ 入库时就拒收了）。
    pub(crate) fn book_has_own_cover(&self, meta: &Meta) -> Result<bool, String> {
        match meta.content_format() {
            "cbz" => Ok(true),
            "epub" => Ok(epub_has_cover(&self.content_path(meta)?)),
            other => Err(crate::unsupported(other)),
        }
    }
}

/// 因为网络出错没查完时的提示。
pub(crate) fn incomplete(e: &str) -> String {
    format!("网络出错，没查完（{e}）；没有生成封面，下次再试")
}

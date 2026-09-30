//! EPUB → AZW3（KF8）写出器：Kindle USB 侧载的唯一可用格式（EPUB 不认，2026-09-27 真机实测）。
//!
//! clean-room：依 MobileRead 的 MOBI 容器文档，加上对 KF8 样本文件的**黑盒数据分析**（只看文件字节，不看任何
//! 工具的代码）实现，不参考 GPL 的 KindleUnpack / Calibre 代码；读取侧 `bookconv::convert::{palm, kf8}` 做往返校验。
//!
//! 输入应是已经按设备优化过的 EPUB（`epub-optimize --device=kindle`）；这里只做格式转换，不改内容。
//! 不嵌字体（`@font-face` 去掉，字体交给阅读器设置）；SVG 图片暂不支持（引用保持原样）。

mod book;
mod container;
pub mod indx;
pub mod palmdoc;
mod text;

use std::collections::HashMap;

/// 写出器版本：改了产物字节的修改要加一，书库据此判断旧的 AZW3 产物过期。
pub const WRITER_VERSION: &str = "1";

/// Kindle 书库里的归类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdeType {
    /// 个人文档（"文档"分类）：封面用书里自带的缩略图，侧载书最稳。
    Pdoc,
    /// 电子书（"书籍"分类）。
    Ebok,
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub cdetype: CdeType,
    /// 固定唯一 ID 与时间戳（书库用书的 id 和入库时间，重建后 Kindle 仍认作同一本书）；`None` 按书自己派生：
    /// ID 取 OPF 唯一标识符的哈希（没有就取 OPF 原文的哈希），时间取 `dcterms:modified`（没有就 2000-01-01）。
    pub fixed_id: Option<(u32, u32)>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { cdetype: CdeType::Pdoc, fixed_id: None }
    }
}

/// 书里没有可用的 `dcterms:modified` 时的时间戳：2000-01-01T00:00:00Z（和优化器升级 EPUB 3 时补的固定值一致）。
const DEFAULT_TIMESTAMP: u32 = 946_684_800;

/// 缩略图高度（像素）。
const THUMB_H: u32 = 330;

/// 封面缩略图：高度缩到 [`THUMB_H`]，本来就不高于它的不放大。
fn thumbnail(cover: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(cover).ok()?;
    let img = if img.height() > THUMB_H { img.resize(u32::MAX, THUMB_H, image::imageops::FilterType::Lanczos3) } else { img };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img.to_rgb8()).ok()?;
    Some(out)
}

/// EPUB 字节 → AZW3 字节。
pub fn epub_to_azw3(epub: &[u8], opts: &Opts) -> Result<Vec<u8>, String> {
    epub_to_azw3_with_warnings(epub, opts).map(|(b, _)| b)
}

/// 同 [`epub_to_azw3`]，另外返回转换中丢掉或降级的内容（不支持的图片格式、找不到目标的链接），给调用方提示用户。
pub fn epub_to_azw3_with_warnings(epub: &[u8], opts: &Opts) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut book = book::load(epub, &mut warnings)?;
    let mut res_map: HashMap<String, (u32, &'static str)> = HashMap::new();
    let mut records = Vec::with_capacity(book.images.len() + 1);
    for img in std::mem::take(&mut book.images) {
        records.push(img.bytes);
        res_map.insert(img.path, (records.len() as u32, img.mime));
    }
    let cover = book.cover.as_ref().and_then(|c| res_map.get(c)).map(|(n, _)| n - 1);
    let thumb = cover.and_then(|c| thumbnail(&records[c as usize])).map(|t| {
        records.push(t);
        records.len() as u32 - 1
    });
    let layout = text::layout(&book, &res_map, &mut warnings)?;
    // 没给固定 ID 时按书自己派生（不取当前时间）：同一本 EPUB 每次转出来逐字节相同，Kindle 也认作同一本书。
    let (uid, timestamp) = opts.fixed_id.unwrap_or_else(|| {
        let h = book.meta.stable_id;
        ((h >> 32) as u32 ^ h as u32, book.meta.modified.unwrap_or(DEFAULT_TIMESTAMP))
    });
    let asin = format!("{uid:08x}-{timestamp:08x}");
    let meta = container::Meta {
        title: if book.meta.title.is_empty() { "未命名" } else { &book.meta.title },
        authors: &book.meta.authors,
        publisher: &book.meta.publisher,
        language: &book.meta.language,
        date: &book.meta.date,
        description: &book.meta.description,
        rtl: book.meta.rtl,
        cdetype: match opts.cdetype {
            CdeType::Pdoc => "PDOC",
            CdeType::Ebok => "EBOK",
        },
        asin: &asin,
        uid,
        timestamp,
    };
    let out = container::assemble(&meta, layout, container::Resources { records, cover, thumb })?;
    Ok((out, warnings))
}

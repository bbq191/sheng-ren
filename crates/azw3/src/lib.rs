//! EPUB → AZW3（KF8）写出器：Kindle USB 侧载的唯一可用格式（EPUB 不认，2026-09-27 真机实测）。
//!
//! clean-room：依 MobileRead 的 MOBI 容器文档，加上对 KF8 样本文件的**黑盒数据分析**（只看文件字节，不看任何
//! 工具的代码）实现，不参考 GPL 的 KindleUnpack / Calibre 代码；读取侧 `bookconv::convert::{palm, kf8}` 做往返校验。
//!
//! 输入应是已经按设备优化过的 EPUB（`epub-optimize --device=kindle-…`）；这里只做格式转换，不改内容。
//! 不嵌字体（`@font-face` 去掉，字体交给阅读器设置）；SVG 图片暂不支持（引用保持原样）。

mod book;
mod container;
pub mod indx;
pub mod palmdoc;
mod text;

use std::collections::HashMap;

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
    /// 固定唯一 ID 与时间戳（测试要可重复）；`None` 按当前时间生成。
    pub fixed_id: Option<(u32, u32)>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { cdetype: CdeType::Pdoc, fixed_id: None }
    }
}

/// 缩略图高度（像素）。
const THUMB_H: u32 = 330;

fn thumbnail(cover: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(cover).ok()?;
    let img = img.resize(u32::MAX, THUMB_H, image::imageops::FilterType::Lanczos3).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img).ok()?;
    Some(out)
}

/// EPUB 字节 → AZW3 字节。
pub fn epub_to_azw3(epub: &[u8], opts: &Opts) -> Result<Vec<u8>, String> {
    let book = book::load(epub)?;
    let mut res_map: HashMap<String, (u32, &'static str)> = HashMap::new();
    let mut records = Vec::new();
    for img in &book.images {
        records.push(img.bytes.clone());
        res_map.insert(img.path.clone(), (records.len() as u32, img.mime));
    }
    let cover = book.cover.as_ref().and_then(|c| res_map.get(c)).map(|(n, _)| n - 1);
    let thumb = cover.and_then(|c| thumbnail(&records[c as usize])).map(|t| {
        records.push(t);
        records.len() as u32 - 1
    });
    let layout = text::layout(&book, &res_map)?;
    let (uid, timestamp) = opts.fixed_id.unwrap_or_else(|| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        ((now.as_nanos() as u32) ^ 0x5A5A_1234, now.as_secs() as u32)
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
    container::assemble(&meta, &layout, container::Resources { records, cover, thumb })
}

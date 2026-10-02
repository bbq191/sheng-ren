//! MOBI 词典 → StarDict，给 KOReader 查词用（KOReader 只认 StarDict；Kindle 自带阅读器的 MOBI 词典它打不开）。
//! 只用来转换用户自己手上的词典，产物不进仓库。
//!
//! clean-room：依 MobileRead 的 MOBI 容器文档、StarDict 的文件格式说明，加上对两本词典样本的黑盒分析实现，
//! 不看 KindleUnpack / Calibre 的代码。容器读取（PalmDB、PalmDOC 解压、INDX）借 [`azw3::read::palm`]。
//!
//! MOBI 词典的结构（2026-10-02 对《现代汉语词典》《牛津高阶双解》两本样本核过）：
//! - 正文是一整份 HTML，每个词条是其中一段；**词头索引**（orth index）的头 INDX 记录号在 MOBI 头 `+0x18`。
//! - 索引条目的 tag 1 = 词条在解压后正文里的起始字节，tag 2 = 长度。没有这两个、只有一个 tag 的条目是别名，
//!   值是主条目在索引里的序号（《现代汉语词典》tag 22：异体字 㕑 → 厨）。
//! - 词头编码：头 INDX `+0x1C` 是 65002 时，词头每个字 2 字节（大端）；值小于 ORDT 表长（`+0xA8`）的查
//!   ORDT2 表（偏移在 `+0xB0`，`ORDT` 魔数后每项 2 字节）换成码位，否则本身就是码位。65001 时是 UTF-8。
//! - 词形变化索引（inflection index，MOBI 头 `+0x1C`）是按规则变换词头的，没有直接的"变形 → 词头"表，暂不转：
//!   查 `ran` 找不到 `run`。

pub mod stardict;

use azw3::read::palm::{self, be_u32};
use regex::Regex;
use std::sync::OnceLock;

/// 一条词条：词头 + 正文里的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub word: String,
    pub start: usize,
    pub len: usize,
}

/// 读出来的一本 MOBI 词典。
pub struct Dict {
    /// EXTH 书名（没有就用 PalmDB 名）。有的词典这里是乱码（牛津那本是 `ZRQAHFCRCTU7G`），只作参考。
    pub title: String,
    /// 解压后的整份正文（HTML，UTF-8）。
    pub text: Vec<u8>,
    /// 词头索引里的全部词条（按索引顺序）。
    pub entries: Vec<Entry>,
}

/// 读 MOBI 词典：正文 + 词头索引。不是词典（没有词头索引）、加密、HUFF/CDIC 压缩都报错。
pub fn read(data: &[u8]) -> Result<Dict, String> {
    let records = palm::parse_palmdb(data)?;
    let r0 = records.first().ok_or("没有记录")?;
    let h = palm::parse_header(r0)?;
    let codepage = be_u32(h.mobi, 0x0C).unwrap_or(0);
    if codepage != 65001 {
        return Err(format!("正文编码是 {codepage}，只支持 UTF-8（65001）"));
    }
    let (idx, _, raw_entries) = palm::indx_read(&records, h.mobi, 0x18).ok_or("没有词头索引（不是 MOBI 词典？）")?;
    let labels = LabelCodec::from_index_header(records[idx])?;
    let text = palm::decompress_text(&records, &h);
    // 先取每条自己的位置；没有位置、只有一个指向别的条目序号的 tag 的（《现代汉语词典》的异体字 㕑 → 厨，tag 22），
    // 再借目标条目的位置，成为同一段释义的又一个词头。
    let span = |tags: &[(u8, Vec<usize>)]| -> Option<(usize, usize)> {
        let (start, len) = (palm::tag_val(tags, 1, 0)?, palm::tag_val(tags, 2, 0)?);
        (len > 0 && start.checked_add(len).is_some_and(|e| e <= text.len())).then_some((start, len))
    };
    let spans: Vec<Option<(usize, usize)>> = raw_entries.iter().map(|(_, tags)| span(tags)).collect();
    let mut entries = Vec::with_capacity(raw_entries.len());
    for (i, (id, tags)) in raw_entries.iter().enumerate() {
        let pos = spans[i].or_else(|| {
            let has_own = palm::tag_val(tags, 1, 0).is_some() || palm::tag_val(tags, 2, 0).is_some();
            let mut targets = tags.iter().filter(|(_, v)| v.len() == 1).filter_map(|(_, v)| spans.get(v[0]).copied().flatten());
            if has_own { None } else { targets.next() }
        });
        let word = labels.decode(id);
        if let (Some((start, len)), false) = (pos, word.trim().is_empty()) {
            entries.push(Entry { word, start, len });
        }
    }
    if entries.is_empty() {
        return Err("词头索引里没有可用的词条".into());
    }
    let exth = palm::parse_exth(h.mobi, h.mobi_hlen);
    let title = if exth.title.trim().is_empty() { palm::palmdb_name(data) } else { exth.title.trim().to_string() };
    Ok(Dict { title, text, entries })
}

/// 词头的解码方式（见模块说明）。
#[derive(Debug)]
pub enum LabelCodec {
    Utf8,
    /// 65002：每字 2 字节，小于表长的查表。
    Ordt(Vec<u16>),
}

impl LabelCodec {
    /// 从头 INDX 记录读编码和 ORDT2 表。
    pub fn from_index_header(hdr: &[u8]) -> Result<Self, String> {
        match be_u32(hdr, 0x1C) {
            Some(65001) => Ok(LabelCodec::Utf8),
            Some(65002) => {
                let count = be_u32(hdr, 0xA8).ok_or("头 INDX 太短（没有 ORDT 表长）")? as usize;
                let off = be_u32(hdr, 0xB0).ok_or("头 INDX 太短（没有 ORDT2 偏移）")? as usize;
                if hdr.get(off..off + 4) != Some(b"ORDT") {
                    return Err(format!("ORDT2 偏移 {off} 处不是 ORDT 表"));
                }
                let body = hdr.get(off + 4..off + 4 + count * 2).ok_or("ORDT2 表超出记录")?;
                Ok(LabelCodec::Ordt(body.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect()))
            }
            Some(e) => Err(format!("词头编码 {e} 不认识（只认 65001、65002）")),
            None => Err("头 INDX 太短（没有编码字段）".into()),
        }
    }

    pub fn decode(&self, id: &[u8]) -> String {
        match self {
            LabelCodec::Utf8 => String::from_utf8_lossy(id).into_owned(),
            LabelCodec::Ordt(table) => id
                .chunks_exact(2)
                .map(|c| {
                    let v = u16::from_be_bytes([c[0], c[1]]);
                    let cp = table.get(v as usize).copied().unwrap_or(v);
                    char::from_u32(cp as u32).unwrap_or('\u{FFFD}')
                })
                .collect(),
        }
    }
}

/// 按正文位置找词条：给 `filepos` 链接找目标词头。
pub struct PosLookup<'a> {
    /// 按 start 排序的 (start, end, 词头)。
    spans: Vec<(usize, usize, &'a str)>,
}

impl<'a> PosLookup<'a> {
    pub fn new(entries: &'a [Entry]) -> Self {
        let mut spans: Vec<_> = entries.iter().map(|e| (e.start, e.start + e.len, e.word.as_str())).collect();
        spans.sort_by_key(|s| (s.0, s.1));
        PosLookup { spans }
    }

    /// `pos` 落在哪条词条里；不在任何词条里时，取其后 [`LINK_SLACK`] 字节内最近开始的一条
    /// （链接常指向词条前面的分隔标记）。
    pub fn word_at(&self, pos: usize) -> Option<&'a str> {
        let i = self.spans.partition_point(|s| s.0 <= pos);
        if i > 0 {
            let (s, e, w) = self.spans[i - 1];
            if s <= pos && pos < e {
                return Some(w);
            }
        }
        self.spans.get(i).filter(|s| s.0 - pos <= LINK_SLACK).map(|s| s.2)
    }
}

/// [`PosLookup::word_at`] 往后找词条的最大距离（字节）。
pub const LINK_SLACK: usize = 64;

/// 词条 HTML 整理成 StarDict 的 `h` 类型内容：
/// - `<a filepos=N>` 换成 `<a href="bword://词头">`（KOReader 点了接着查那个词）；找不到目标的去掉链接、留文字；
/// - 去掉 `<img>`（图片在 MOBI 资源记录里，没带过来）；
/// - 去掉 `mbp:`、`idx:` 这类 Kindle 专用标签（内容留着）。
pub fn clean_entry_html(html: &str, lookup: &PosLookup) -> String {
    static RA: OnceLock<Regex> = OnceLock::new();
    static RI: OnceLock<Regex> = OnceLock::new();
    static RK: OnceLock<Regex> = OnceLock::new();
    let re_a = RA.get_or_init(|| Regex::new(r#"(?is)<a\b([^>]*?)\bfilepos\s*=\s*["']?0*(\d+)["']?([^>]*)>(.*?)</a\s*>"#).unwrap());
    let re_img = RI.get_or_init(|| Regex::new(r#"(?is)<img\b[^>]*>"#).unwrap());
    let re_kindle = RK.get_or_init(|| Regex::new(r#"(?is)</?(?:mbp|idx):[^>]*>"#).unwrap());
    let s = re_a.replace_all(html, |c: &regex::Captures| {
        let inner = &c[4];
        match c[2].parse::<usize>().ok().and_then(|p| lookup.word_at(p)) {
            Some(w) => format!(r#"<a href="bword://{}">{inner}</a>"#, escape_attr(w)),
            None => inner.to_string(),
        }
    });
    let s = re_img.replace_all(&s, "");
    re_kindle.replace_all(&s, "").trim().to_string()
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// 转换结果统计。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// 写进 .idx 的词头数（同一词头多条释义各算一条）。
    pub words: usize,
    /// .dict 里不同的释义段数（多个词头指向同一段只存一份）。
    pub articles: usize,
}

/// 整本词典 → StarDict 的三个文件内容。
pub fn to_stardict(dict: &Dict, bookname: &str) -> (stardict::Files, Stats) {
    let lookup = PosLookup::new(&dict.entries);
    let mut w = stardict::Writer::default();
    let mut seen = std::collections::HashMap::new();
    for e in &dict.entries {
        let key = (e.start, e.len);
        let art = match seen.get(&key) {
            Some(&a) => a,
            None => {
                let html = String::from_utf8_lossy(&dict.text[e.start..e.start + e.len]);
                let a = w.add_article(clean_entry_html(&html, &lookup).as_bytes());
                seen.insert(key, a);
                a
            }
        };
        w.add_word(e.word.trim(), art);
    }
    let stats = Stats { words: w.word_count(), articles: seen.len() };
    let description = format!("由 MOBI 词典《{bookname}》转换（mobi-dict-to-stardict），只供自己查词用");
    (w.finish(bookname, &description), stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, start: usize, len: usize) -> Entry {
        Entry { word: word.into(), start, len }
    }

    #[test]
    fn ordt_labels_map_small_values_through_table() {
        // 表：0 → 'A'，1 → 'あ'；0x5236（≥ 表长）就是 '制' 本身
        let mut hdr = vec![0u8; 0xB4];
        hdr[0x1C..0x20].copy_from_slice(&65002u32.to_be_bytes());
        hdr[0xA8..0xAC].copy_from_slice(&2u32.to_be_bytes());
        hdr[0xB0..0xB4].copy_from_slice(&(0xB4u32).to_be_bytes());
        hdr.extend_from_slice(b"ORDT");
        hdr.extend_from_slice(&[0x00, 0x41, 0x30, 0x42]);
        let codec = LabelCodec::from_index_header(&hdr).unwrap();
        assert_eq!(codec.decode(&[0x00, 0x00, 0x00, 0x01, 0x52, 0x36]), "Aあ制");
    }

    #[test]
    fn ordt_offset_must_point_at_magic() {
        let mut hdr = vec![0u8; 0xC0];
        hdr[0x1C..0x20].copy_from_slice(&65002u32.to_be_bytes());
        hdr[0xB0..0xB4].copy_from_slice(&(0xB4u32).to_be_bytes());
        assert!(LabelCodec::from_index_header(&hdr).is_err());
    }

    #[test]
    fn filepos_links_become_bword_links() {
        let entries = vec![entry("because", 100, 50), entry("cause", 200, 40)];
        let lk = PosLookup::new(&entries);
        let html = r#"= <a  filepos=0000000120 >BECAUSE</a>, <a filepos="190">x</a>, <a filepos=9999>gone</a>"#;
        assert_eq!(
            clean_entry_html(html, &lk),
            r#"= <a href="bword://because">BECAUSE</a>, <a href="bword://cause">x</a>, gone"#
        );
    }

    #[test]
    fn kindle_tags_and_images_are_dropped_but_text_kept() {
        let lk = PosLookup::new(&[]);
        let html = r#" <idx:entry><idx:orth value="a">词</idx:orth><img recindex="00001"> 释义<mbp:pagebreak/></idx:entry> "#;
        assert_eq!(clean_entry_html(html, &lk), "词 释义");
    }

    #[test]
    fn duplicate_slices_are_stored_once() {
        let text = b"<b>one</b><b>two</b>".to_vec();
        let dict = Dict { title: "t".into(), text, entries: vec![entry("a", 0, 10), entry("b", 0, 10), entry("c", 10, 10)] };
        let (files, stats) = to_stardict(&dict, "t");
        assert_eq!(stats, Stats { words: 3, articles: 2 });
        assert_eq!(files.dict, b"<b>one</b><b>two</b>");
        let idx = stardict::parse_idx(&files.idx).unwrap();
        assert_eq!(idx, vec![("a".into(), 0, 10), ("b".into(), 0, 10), ("c".into(), 10, 10)]);
    }
}

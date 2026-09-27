//! INDX 索引记录（MOBI 通用结构，KF8 的片段/骨架/目录索引都用它）。布局（MobileRead MOBI 文档 + 对 KF8 样本的黑盒分析）：
//! - **头记录**：192 字节 INDX 头 → TAGX（标签定义）→ 每个数据记录一块"末条键 + 条目数（u16）"→ 补齐 4 字节 → IDXT（各块偏移，u16）。
//! - **数据记录**：192 字节 INDX 头（+0x0C=1）→ 各条目 → 补齐 4 字节 → IDXT（各条目偏移，u16）。
//! - 条目：键长（1 字节）+ 键 + 控制字节 + 各标签的值（前向变长整数，按 TAGX 顺序）。
//! - **CNCX**：字符串池，每串 = 前向变长整数长度 + UTF-8 字节；偏移 = 记录序号 << 16 | 记录内偏移。

const HEADER_LEN: usize = 192;
/// 单个数据记录的上限（IDXT 偏移是 u16，留余量）。
const MAX_RECORD: usize = 0xF000;

/// 前向变长整数：大端 7 位一组，最后一个字节最高位置 1（与读取侧共用一份）。
pub use bookconv::convert::palm::fwd_varint;

/// TAGX 里的一个标签：(标签号, 每次出现的值个数, 控制字节掩码)。
#[derive(Clone, Copy)]
pub struct TagDef {
    pub tag: u8,
    pub values: u8,
    pub mask: u8,
}

pub struct Entry {
    pub key: Vec<u8>,
    /// (标签号, 值)。值个数必须是该标签 `values` 的整数倍，倍数写进控制字节（掩码位里放得下）。
    pub tags: Vec<(u8, Vec<u32>)>,
}

fn encode_entry(tagx: &[TagDef], e: &Entry) -> Vec<u8> {
    let mut ctrl = 0u8;
    let mut vals = Vec::new();
    for d in tagx {
        let Some((_, v)) = e.tags.iter().find(|(t, _)| *t == d.tag) else { continue };
        let count = (v.len() / d.values as usize) as u8;
        ctrl |= (count << d.mask.trailing_zeros()) & d.mask;
        for x in v {
            vals.extend(fwd_varint(*x));
        }
    }
    let mut out = vec![e.key.len() as u8];
    out.extend_from_slice(&e.key);
    out.push(ctrl);
    out.extend(vals);
    out
}

fn pad4(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

fn indx_header(record_type: u32, idxt_off: usize, count: usize, total: u32, ncncx: u32, is_meta: bool) -> Vec<u8> {
    let mut h = vec![0u8; HEADER_LEN];
    let mut put = |off: usize, v: u32| h[off..off + 4].copy_from_slice(&v.to_be_bytes());
    put(0x04, HEADER_LEN as u32);
    put(0x0C, record_type);
    put(0x14, idxt_off as u32);
    put(0x18, count as u32);
    if is_meta {
        put(0x10, 2);
        put(0x1C, 65001);
        put(0x20, 0xFFFF_FFFF);
        put(0x24, total);
        put(0x34, ncncx);
        put(0xB4, HEADER_LEN as u32);
    } else {
        put(0x1C, 0xFFFF_FFFF);
        put(0x20, 0xFFFF_FFFF);
    }
    h[0..4].copy_from_slice(b"INDX");
    h
}

/// 组出一个索引的全部记录：[头记录, 数据记录…]（CNCX 另给）。
pub fn build(tagx: &[TagDef], entries: &[Entry], ncncx: u32) -> Vec<Vec<u8>> {
    // 条目编码后按大小分到各数据记录
    let mut blocks: Vec<Vec<Vec<u8>>> = vec![Vec::new()];
    let mut size = HEADER_LEN + 8;
    let mut last_keys: Vec<Vec<u8>> = Vec::new();
    for e in entries {
        let enc = encode_entry(tagx, e);
        if size + enc.len() + 2 > MAX_RECORD && !blocks.last().unwrap().is_empty() {
            blocks.push(Vec::new());
            size = HEADER_LEN + 8;
        }
        size += enc.len() + 2;
        blocks.last_mut().unwrap().push(enc);
        if last_keys.len() < blocks.len() {
            last_keys.push(Vec::new());
        }
        *last_keys.last_mut().unwrap() = e.key.clone();
    }
    let mut data_records = Vec::new();
    for b in &blocks {
        let mut body = Vec::new();
        let mut offs = Vec::new();
        for enc in b {
            offs.push(HEADER_LEN + body.len());
            body.extend_from_slice(enc);
        }
        let mut rec = Vec::new();
        pad4(&mut body);
        let idxt_off = HEADER_LEN + body.len();
        rec.extend(indx_header(1, idxt_off, b.len(), 0, 0, false));
        rec.extend(body);
        rec.extend_from_slice(b"IDXT");
        for o in offs {
            rec.extend_from_slice(&(o as u16).to_be_bytes());
        }
        pad4(&mut rec);
        data_records.push(rec);
    }
    // 头记录
    let mut tagx_bytes = b"TAGX".to_vec();
    tagx_bytes.extend(((12 + 4 * (tagx.len() + 1)) as u32).to_be_bytes());
    tagx_bytes.extend(1u32.to_be_bytes());
    for d in tagx {
        tagx_bytes.extend([d.tag, d.values, d.mask, 0]);
    }
    tagx_bytes.extend([0, 0, 0, 1]);
    let mut geo = Vec::new();
    let mut geo_offs = Vec::new();
    for (k, b) in blocks.iter().enumerate() {
        geo_offs.push(HEADER_LEN + tagx_bytes.len() + geo.len());
        let key = &last_keys[k];
        geo.push(key.len() as u8);
        geo.extend_from_slice(key);
        geo.extend((b.len() as u16).to_be_bytes());
    }
    let mut after = tagx_bytes;
    after.extend(geo);
    pad4(&mut after);
    let idxt_off = HEADER_LEN + after.len();
    let mut meta = indx_header(0, idxt_off, blocks.len(), entries.len() as u32, ncncx, true);
    meta.extend(after);
    meta.extend_from_slice(b"IDXT");
    for o in geo_offs {
        meta.extend((o as u16).to_be_bytes());
    }
    pad4(&mut meta);
    let mut out = vec![meta];
    out.extend(data_records);
    out
}

/// CNCX 字符串池。
#[derive(Default)]
pub struct Cncx {
    records: Vec<Vec<u8>>,
}

impl Cncx {
    pub fn add(&mut self, s: &str) -> u32 {
        let mut enc = fwd_varint(s.len() as u32);
        enc.extend_from_slice(s.as_bytes());
        if self.records.last().is_none_or(|r| r.len() + enc.len() > MAX_RECORD) {
            self.records.push(Vec::new());
        }
        let rec_no = (self.records.len() - 1) as u32;
        let r = self.records.last_mut().unwrap();
        let off = r.len() as u32;
        r.extend(enc);
        (rec_no << 16) | off
    }
    pub fn into_records(self) -> Vec<Vec<u8>> {
        self.records.into_iter().map(|mut r| { pad4(&mut r); r }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_matches_documented_example() {
        // MobileRead 文档：0x11111 前向编码为 04 22 91
        assert_eq!(fwd_varint(0x11111), [0x04, 0x22, 0x91]);
        assert_eq!(fwd_varint(0), [0x80]);
        assert_eq!(fwd_varint(132), [0x01, 0x84]);
    }

    #[test]
    fn fragment_entry_encodes_like_sample() {
        // 样本片段条目：键 "0000000395"、tags 2/3/4=0、6=[0,132] → 0a 30…35 0f 80 80 80 80 01 84
        let tagx = [TagDef { tag: 2, values: 1, mask: 1 }, TagDef { tag: 3, values: 1, mask: 2 }, TagDef { tag: 4, values: 1, mask: 4 }, TagDef { tag: 6, values: 2, mask: 8 }];
        let e = Entry { key: b"0000000395".to_vec(), tags: vec![(2, vec![0]), (3, vec![0]), (4, vec![0]), (6, vec![0, 132])] };
        let hex: String = encode_entry(&tagx, &e).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "0a303030303030303339350f808080800184");
        // 骨架条目：tag1 两次、tag6 两组 → 控制字节 0x0a
        let tagx = [TagDef { tag: 1, values: 1, mask: 3 }, TagDef { tag: 6, values: 2, mask: 12 }];
        let e = Entry { key: b"SKEL0000000000".to_vec(), tags: vec![(1, vec![1, 1]), (6, vec![0, 411, 0, 411])] };
        let hex: String = encode_entry(&tagx, &e).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "0e534b454c303030303030303030300a818180039b80039b");
    }
}

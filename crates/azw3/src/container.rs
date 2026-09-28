//! PDB 容器与记录组装。字段布局依 MobileRead MOBI 文档；KF8 专有字段（0xC0 FDST、0xF4 NCX、0xF8 片段索引、
//! 0xFC 骨架索引）与 FDST/FCIS 的取值来自对 KF8 样本的黑盒分析。
//!
//! 记录顺序：0 头 → 正文记录（每条 4096 字节未压缩，PalmDOC 压缩 + 多字节字符尾随字节）→ 1 字节补位记录 →
//! 片段索引（头、数据、CNCX）→ 骨架索引 → 目录索引（头、数据、CNCX）→ 图片 → FDST → FLIS → FCIS → EOF。

use crate::indx::{self, Cncx, Entry, TagDef};
use crate::text::{base32, Layout};

const TEXT_RECORD: usize = 4096;
/// 记录 0 末尾的零填充（样本里是 8192 字节，阅读器可能就地改写记录 0）。
const R0_PADDING: usize = 8192;

pub struct Meta<'a> {
    pub title: &'a str,
    pub authors: &'a [String],
    pub publisher: &'a str,
    pub language: &'a str,
    pub date: &'a str,
    pub description: &'a str,
    pub rtl: bool,
    pub cdetype: &'a str,
    pub asin: &'a str,
    pub uid: u32,
    pub timestamp: u32,
}

pub struct Resources {
    pub records: Vec<Vec<u8>>,
    /// 封面、缩略图在资源里的下标（0 起）。
    pub cover: Option<u32>,
    pub thumb: Option<u32>,
}

/// 多字节字符跨记录时的尾随字节：把跨到下一条记录的那几个续字节抄在本记录末尾，再加一个字节写个数（低 2 位）。
fn multibyte_trailer(text: &[u8], end: usize) -> Vec<u8> {
    let mut n = 0;
    while end + n < text.len() && n < 3 && text[end + n] & 0xC0 == 0x80 {
        n += 1;
    }
    let mut t = text[end..end + n].to_vec();
    t.push(n as u8);
    t
}

fn text_records(text: &[u8]) -> Vec<Vec<u8>> {
    (0..text.len().div_ceil(TEXT_RECORD))
        .map(|k| {
            let (s, e) = (k * TEXT_RECORD, ((k + 1) * TEXT_RECORD).min(text.len()));
            let mut r = crate::palmdoc::compress(&text[s..e]);
            r.extend(multibyte_trailer(text, e));
            r
        })
        .collect()
}

fn fragment_index(layout: &Layout) -> Vec<Vec<u8>> {
    let mut cncx = Cncx::default();
    let entries: Vec<Entry> = layout
        .fragments
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let sel = cncx.add(&format!("P-//*[@aid='{}']", f.aid));
            Entry { key: format!("{:010}", f.insert_pos).into_bytes(), tags: vec![(2, vec![sel]), (3, vec![i as u32]), (4, vec![i as u32]), (6, vec![0, f.len])] }
        })
        .collect();
    let tagx = [TagDef { tag: 2, values: 1, mask: 1 }, TagDef { tag: 3, values: 1, mask: 2 }, TagDef { tag: 4, values: 1, mask: 4 }, TagDef { tag: 6, values: 2, mask: 8 }];
    indx::build_with_cncx(&tagx, &entries, cncx)
}

fn skeleton_index(layout: &Layout) -> Vec<Vec<u8>> {
    let entries: Vec<Entry> = layout
        .skeletons
        .iter()
        .enumerate()
        .map(|(i, &(start, len))| Entry { key: format!("SKEL{i:010}").into_bytes(), tags: vec![(1, vec![1, 1]), (6, vec![start, len, start, len])] })
        .collect();
    indx::build(&[TagDef { tag: 1, values: 1, mask: 3 }, TagDef { tag: 6, values: 2, mask: 12 }], &entries, 0)
}

/// 目录索引：先放完所有第 0 层，再放第 1 层……（样本里的顺序）；父子关系用条目下标表示。
fn ncx_index(layout: &Layout, text_len: u32) -> Option<Vec<Vec<u8>>> {
    if layout.ncx.is_empty() {
        return None;
    }
    let items = &layout.ncx;
    // 文档顺序里的父条目与子条目（层级已保证不跳级，见 book::clamp_levels），一趟栈扫描
    let mut parent: Vec<Option<usize>> = vec![None; items.len()];
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        while stack.last().is_some_and(|&k| items[k].level >= it.level) {
            stack.pop();
        }
        if let Some(&p) = stack.last() {
            parent[i] = Some(p);
            kids[p].push(i);
        }
        stack.push(i);
    }
    // 长度：到文档顺序里下一条同级或更高级条目为止（倒着扫，用栈找"下一条不更深的"）
    let mut length = vec![0u32; items.len()];
    let mut next: Vec<usize> = Vec::new();
    for i in (0..items.len()).rev() {
        while next.last().is_some_and(|&k| items[k].level > items[i].level) {
            next.pop();
        }
        let end = next.last().map_or(text_len, |&k| items[k].pos);
        length[i] = end.saturating_sub(items[i].pos);
        next.push(i);
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| (items[i].level, i));
    let mut slot = vec![0usize; items.len()];
    for (s, &i) in order.iter().enumerate() {
        slot[i] = s;
    }
    let width = format!("{:X}", items.len().saturating_sub(1)).len().max(2);
    let mut cncx = Cncx::default();
    let entries: Vec<Entry> = order
        .iter()
        .enumerate()
        .map(|(s, &i)| {
            let it = &items[i];
            let mut tags = vec![(1, vec![it.pos]), (2, vec![length[i]]), (3, vec![cncx.add(&it.label)]), (4, vec![it.level])];
            if let Some(p) = parent[i] {
                tags.push((21, vec![slot[p] as u32]));
            }
            let kid_slots = kids[i].iter().map(|&k| slot[k]);
            if let (Some(first), Some(last)) = (kid_slots.clone().min(), kid_slots.max()) {
                tags.push((22, vec![first as u32]));
                tags.push((23, vec![last as u32]));
            }
            tags.push((6, vec![it.fid, it.off]));
            Entry { key: format!("{s:0width$X}").into_bytes(), tags }
        })
        .collect();
    let tagx = [1u8, 2, 3, 4, 21, 22, 23]
        .iter()
        .enumerate()
        .map(|(k, &tag)| TagDef { tag, values: 1, mask: 1 << k })
        .chain(std::iter::once(TagDef { tag: 6, values: 2, mask: 128 }))
        .collect::<Vec<_>>();
    Some(indx::build_with_cncx(&tagx, &entries, cncx))
}

fn fdst(bounds: &[(u32, u32)]) -> Vec<u8> {
    let mut r = b"FDST".to_vec();
    r.extend(12u32.to_be_bytes());
    r.extend((bounds.len() as u32).to_be_bytes());
    for (s, e) in bounds {
        r.extend(s.to_be_bytes());
        r.extend(e.to_be_bytes());
    }
    r
}

fn flis() -> Vec<u8> {
    let mut r = b"FLIS".to_vec();
    for v in [8u32, 0x0041_0000, 0, 0xFFFF_FFFF, 0x0001_0003, 3, 1, 0xFFFF_FFFF] {
        r.extend(v.to_be_bytes());
    }
    r
}

fn fcis(text_len: u32) -> Vec<u8> {
    let mut r = b"FCIS".to_vec();
    for v in [0x14u32, 0x10, 2, 0, text_len, 0, 0x28, 0, 0x28, 8, 0x0001_0001, 0] {
        r.extend(v.to_be_bytes());
    }
    r
}

fn exth(meta: &Meta, (resource_count, cover, thumb): (u32, Option<u32>, Option<u32>)) -> Vec<u8> {
    let mut recs: Vec<(u32, Vec<u8>)> = Vec::new();
    let s = |v: &str| v.as_bytes().to_vec();
    for a in meta.authors {
        recs.push((100, s(a)));
    }
    if !meta.publisher.is_empty() {
        recs.push((101, s(meta.publisher)));
    }
    if !meta.description.is_empty() {
        recs.push((103, s(meta.description)));
    }
    if !meta.date.is_empty() {
        recs.push((106, s(meta.date)));
    }
    recs.push((113, s(meta.asin)));
    recs.push((501, s(meta.cdetype)));
    recs.push((503, s(meta.title)));
    recs.push((504, s(meta.asin)));
    if !meta.language.is_empty() {
        recs.push((524, s(meta.language)));
    }
    if meta.rtl {
        recs.push((527, s("rtl")));
    }
    recs.push((125, resource_count.to_be_bytes().to_vec()));
    recs.push((131, 0u32.to_be_bytes().to_vec()));
    if let Some(c) = cover {
        recs.push((201, c.to_be_bytes().to_vec()));
        recs.push((203, 0u32.to_be_bytes().to_vec()));
        recs.push((129, s(&format!("kindle:embed:{}", base32(c + 1, 4)))));
    }
    if let Some(t) = thumb {
        recs.push((202, t.to_be_bytes().to_vec()));
    }
    // 生成工具标识，与样本相同（阅读器可能按它决定排版特性）。
    for (t, v) in [(204u32, 201u32), (205, 2), (206, 9), (207, 0)] {
        recs.push((t, v.to_be_bytes().to_vec()));
    }
    recs.push((535, s("0730-890adc2")));
    let mut body = Vec::new();
    for (t, v) in &recs {
        body.extend(t.to_be_bytes());
        body.extend(((v.len() + 8) as u32).to_be_bytes());
        body.extend(v);
    }
    let mut out = b"EXTH".to_vec();
    out.extend(((body.len() + 12) as u32).to_be_bytes());
    out.extend((recs.len() as u32).to_be_bytes());
    out.extend(body);
    indx::pad4(&mut out);
    out
}

fn locale(lang: &str) -> u32 {
    match lang.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase().as_str() {
        "zh" => 4,
        "en" => 9,
        "ja" => 0x11,
        "de" => 7,
        "fr" => 0x0C,
        "ko" => 0x12,
        _ => 0,
    }
}

struct Indices {
    first_non_book: u32,
    frag: u32,
    skel: u32,
    ncx: u32,
    first_image: u32,
    fdst: u32,
    flis: u32,
    fcis: u32,
}

fn record0(meta: &Meta, text_len: u32, text_records: u32, flows: u32, res: (u32, Option<u32>, Option<u32>), idx: &Indices) -> Vec<u8> {
    let mut r = vec![0u8; 16 + 0x108];
    let mut put = |off: usize, v: u32| indx::put_u32(&mut r, off, v);
    // PalmDOC 头
    put(4, text_len);
    let ex = exth(meta, res);
    let name = meta.title.as_bytes();
    let name_off = (16 + 0x108 + ex.len()) as u32;
    // MOBI 头
    put(0x14, 0x108);
    put(0x18, 2);
    put(0x1C, 65001);
    put(0x20, meta.uid);
    put(0x24, 8);
    for off in (0x28..=0x4C).step_by(4) {
        put(off, 0xFFFF_FFFF);
    }
    put(0x50, idx.first_non_book);
    put(0x54, name_off);
    put(0x58, name.len() as u32);
    put(0x5C, locale(meta.language));
    put(0x68, 8);
    put(0x6C, idx.first_image);
    put(0x80, 0x50);
    put(0xA4, 0xFFFF_FFFF);
    put(0xA8, 0xFFFF_FFFF);
    put(0xC0, idx.fdst);
    put(0xC4, flows);
    put(0xC8, idx.fcis);
    put(0xCC, 1);
    put(0xD0, idx.flis);
    put(0xD4, 1);
    put(0xE0, 0xFFFF_FFFF);
    put(0xE8, 0xFFFF_FFFF);
    put(0xEC, 0xFFFF_FFFF);
    put(0xF0, 1); // 尾随字节：只有多字节字符
    put(0xF4, idx.ncx);
    put(0xF8, idx.frag);
    put(0xFC, idx.skel);
    put(0x100, 0xFFFF_FFFF);
    put(0x104, 0xFFFF_FFFF);
    put(0x108, 0xFFFF_FFFF);
    put(0x110, 0xFFFF_FFFF);
    r[0..2].copy_from_slice(&2u16.to_be_bytes()); // PalmDOC 压缩
    r[8..10].copy_from_slice(&(text_records as u16).to_be_bytes());
    r[10..12].copy_from_slice(&(TEXT_RECORD as u16).to_be_bytes());
    r[16..20].copy_from_slice(b"MOBI");
    r.extend(ex);
    r.extend(name);
    r.extend(std::iter::repeat_n(0u8, R0_PADDING));
    indx::pad4(&mut r);
    r
}

fn pdb_name(title: &str, uid: u32) -> Vec<u8> {
    let mut n: String = title.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-').map(|c| if c == ' ' { '_' } else { c }).collect();
    if n.trim_matches('_').is_empty() {
        n = format!("book_{uid:08x}");
    }
    let mut b = n.into_bytes();
    b.truncate(31);
    b.resize(32, 0);
    b
}

pub fn assemble(meta: &Meta, mut layout: Layout, res: Resources) -> Result<Vec<u8>, String> {
    let mut text = std::mem::take(&mut layout.flow0);
    let layout = &layout;
    let mut bounds = vec![(0u32, text.len() as u32)];
    for f in &layout.css_flows {
        let s = text.len() as u32;
        text.extend_from_slice(f);
        bounds.push((s, text.len() as u32));
    }
    let text_len = u32::try_from(text.len()).map_err(|_| "正文超过 4GB")?;
    let trecs = text_records(&text);
    if trecs.len() > u16::MAX as usize {
        return Err("正文记录数超过 65535".into());
    }
    let text_record_count = trecs.len() as u32;
    drop(text);
    let mut records: Vec<Vec<u8>> = vec![Vec::new()]; // 0 号稍后填
    records.extend(trecs);
    records.push(vec![0]);
    // 待核：MOBI 头 0x50「第一条非正文记录」这里指向片段索引（补位记录之后）；样本与公开文档是否一致还没核对，先不动。
    let first_non_book = records.len() as u32;
    let frag = records.len() as u32;
    records.extend(fragment_index(layout));
    let skel = records.len() as u32;
    records.extend(skeleton_index(layout));
    let ncx = match ncx_index(layout, bounds[0].1) {
        Some(r) => {
            let at = records.len() as u32;
            records.extend(r);
            at
        }
        None => 0xFFFF_FFFF,
    };
    let first_image = records.len() as u32;
    let resource_count = res.records.len() as u32;
    let (cover, thumb) = (res.cover, res.thumb);
    records.extend(res.records);
    let fdst_i = records.len() as u32;
    records.push(fdst(&bounds));
    let flis_i = records.len() as u32;
    records.push(flis());
    let fcis_i = records.len() as u32;
    records.push(fcis(text_len));
    records.push(vec![0xE9, 0x8E, 0x0D, 0x0A]);
    let idx = Indices { first_non_book, frag, skel, ncx, first_image, fdst: fdst_i, flis: flis_i, fcis: fcis_i };
    records[0] = record0(meta, text_len, text_record_count, bounds.len() as u32, (resource_count, cover, thumb), &idx);

    let n = records.len();
    if n > u16::MAX as usize {
        return Err("PDB 记录数超过 65535".into());
    }
    let mut out = pdb_name(meta.title, meta.uid);
    out.extend([0u8; 4]); // 属性、版本
    out.extend(meta.timestamp.to_be_bytes());
    out.extend(meta.timestamp.to_be_bytes());
    out.extend([0u8; 16]); // 备份时间、修改号、appInfo、sortInfo
    out.extend(b"BOOKMOBI");
    out.extend(((2 * n - 1) as u32).to_be_bytes());
    out.extend([0u8; 4]);
    out.extend((n as u16).to_be_bytes());
    let total: usize = 78 + 8 * n + 2 + records.iter().map(Vec::len).sum::<usize>();
    if total > u32::MAX as usize {
        return Err("文件超过 4GB".into());
    }
    out.reserve(total - out.len());
    let mut off = 78 + 8 * n + 2;
    for (i, r) in records.iter().enumerate() {
        out.extend((off as u32).to_be_bytes());
        out.extend(((2 * i) as u32).to_be_bytes());
        off += r.len();
    }
    out.extend([0u8; 2]);
    for r in records {
        out.extend(r);
    }
    Ok(out)
}

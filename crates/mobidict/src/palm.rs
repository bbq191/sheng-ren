//! PalmDB / MOBI 容器的读取（词典转换用）：记录表、PalmDOC 解压、extra_data_flags 尾随字节剥离、EXTH 书名、INDX 索引。
//! 2026-10-08 从已删的 AZW3 写出器的读取器里挪过来，只留词典要的部分。
//!
//! 尾随字节标志的位置按「record0 起 +0xF2 == MOBI 头起 +0xE2」定位（[`read_extra_flags`]）；标志位错了跨记录拼接会错位、整本乱码。

/// PalmDB 记录切片表：每条记录一个 `&[u8]`（借 data，零拷贝）。
pub fn parse_palmdb(d: &[u8]) -> Result<Vec<&[u8]>, String> {
    if d.len() < 78 {
        return Err("文件过短，非 PalmDB".into());
    }
    let nrec = u16::from_be_bytes([d[76], d[77]]) as usize;
    if nrec == 0 || d.len() < 78 + nrec * 8 {
        return Err("PalmDB 记录表越界".into());
    }
    let mut offs: Vec<usize> = Vec::with_capacity(nrec + 1);
    for i in 0..nrec {
        let p = 78 + i * 8;
        offs.push(u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize);
    }
    offs.push(d.len());
    let max_off = offs.iter().copied().max().unwrap_or(0);
    if max_off > d.len() {
        return Err(format!("文件不完整：记录偏移超出文件长度（{max_off} > {}），可能是没下载完", d.len()));
    }
    let mut out = Vec::with_capacity(nrec);
    for i in 0..nrec {
        let (a, b) = (offs[i], offs[i + 1]);
        if a > d.len() || b > d.len() || a > b {
            return Err("PalmDB 记录偏移非法".into());
        }
        out.push(&d[a..b]);
    }
    Ok(out)
}

/// PalmDB 数据库名（file[0..32] 空字符截断），做书名兜底。
pub fn palmdb_name(d: &[u8]) -> String {
    if d.len() < 32 {
        return String::new();
    }
    let raw = &d[0..32];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(32);
    String::from_utf8_lossy(&raw[..end]).trim().to_string()
}

/// 从 record0 解出的容器头（compression / 文本记录数 / extra_flags / MOBI 头）。
pub struct Header<'a> {
    pub compression: u16,
    pub text_record_count: usize,
    pub extra_flags: u16,
    /// r0[16..]，即 MOBI 魔数开始处。
    pub mobi: &'a [u8],
    pub mobi_hlen: usize,
}

/// 解析 record0（PalmDOC 头 + MOBI 头）。校验 MOBI 魔数、DRM、HUFF/CDIC。
pub fn parse_header(r0: &[u8]) -> Result<Header<'_>, String> {
    if r0.len() < 16 + 8 {
        return Err("record0 过短".into());
    }
    let compression = u16::from_be_bytes([r0[0], r0[1]]);
    let text_record_count = u16::from_be_bytes([r0[8], r0[9]]) as usize;
    let encryption = u16::from_be_bytes([r0[12], r0[13]]);
    if encryption != 0 {
        return Err("有 DRM 加密，不支持".into());
    }
    if compression == 17480 {
        return Err("HUFF/CDIC 压缩，暂不支持".into());
    }
    let mobi = &r0[16..];
    if mobi.len() < 8 || &mobi[0..4] != b"MOBI" {
        return Err("非 MOBI 容器（无 MOBI 头）".into());
    }
    let mobi_hlen = u32::from_be_bytes([mobi[4], mobi[5], mobi[6], mobi[7]]) as usize;
    let extra_flags = read_extra_flags(mobi);
    Ok(Header { compression, text_record_count, extra_flags, mobi, mobi_hlen })
}

/// 偏移 `o` 处的大端 u32；越界为 `None`。
pub fn be_u32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
}

/// 解压文本记录 1..=text_record_count → 完整正文字节。
/// 每条记录先剥 trailing bytes（extra_data_flags）再 PalmDOC 解压（compression==1 时原样拼）。
pub fn decompress_text(records: &[&[u8]], h: &Header) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let last = h.text_record_count.min(records.len().saturating_sub(1));
    for rec in records.iter().take(last + 1).skip(1) {
        let ts = trailing_size(rec, h.extra_flags);
        let body = &rec[..rec.len().saturating_sub(ts)];
        if h.compression == 2 {
            palmdoc_decompress(body, &mut out);
        } else {
            out.extend_from_slice(body);
        }
    }
    out
}

/// extra_data_flags：MOBI 头起 `+0xE2`（== record0 起 `+0xF2`）的 u16。MOBI 头长度（`+4`）不到 0xE4 时
/// 这个字段不存在，那里已是 EXTH，按 0（没有尾随字节）处理。
pub fn read_extra_flags(mobi: &[u8]) -> u16 {
    match be_u32(mobi, 4) {
        Some(hlen) if hlen >= 0xE4 && mobi.len() >= 0xE4 => u16::from_be_bytes([mobi[0xE2], mobi[0xE3]]),
        _ => 0,
    }
}

/// 每条文本记录尾部 trailing bytes 大小（多字节重叠 + TBS 索引项，须剥离再解压）。
pub fn trailing_size(rec: &[u8], flags: u16) -> usize {
    let mut num = 0usize;
    let mut f = flags >> 1;
    while f != 0 {
        if f & 1 != 0 {
            let end = rec.len().saturating_sub(num);
            num += backward_varint(&rec[..end]);
        }
        f >>= 1;
    }
    if flags & 1 != 0 {
        let idx = rec.len().saturating_sub(num).saturating_sub(1);
        if idx < rec.len() {
            num += (rec[idx] & 0x3) as usize + 1;
        }
    }
    num.min(rec.len())
}

/// 从末尾向前读 7bit/byte 变长整数，遇高位=1 停止。
pub fn backward_varint(data: &[u8]) -> usize {
    let mut val = 0usize;
    let mut shift = 0u32;
    for &b in data.iter().rev() {
        val |= ((b & 0x7f) as usize) << shift;
        shift += 7;
        if b & 0x80 != 0 {
            break;
        }
        if shift > 28 {
            break;
        }
    }
    val
}

/// PalmDOC/LZ77 解压，追加到 out。完全 bounds-safe（坏 back-ref 跳过，不崩）。
pub fn palmdoc_decompress(data: &[u8], out: &mut Vec<u8>) {
    let n = data.len();
    let mut i = 0;
    while i < n {
        let c = data[i];
        i += 1;
        if c == 0 {
            out.push(0);
        } else if c <= 8 {
            let take = (c as usize).min(n - i);
            out.extend_from_slice(&data[i..i + take]);
            i += c as usize;
        } else if c < 0x80 {
            out.push(c);
        } else if c < 0xc0 {
            if i >= n {
                break;
            }
            let di = (((c as usize) << 8) | data[i] as usize) & 0x3fff;
            i += 1;
            let length = (di & 7) + 3;
            let dist = di >> 3;
            if dist > 0 && dist <= out.len() {
                for _ in 0..length {
                    out.push(out[out.len() - dist]);
                }
            }
        } else {
            out.push(b' ');
            out.push(c ^ 0x80);
        }
    }
}

/// EXTH 书名（503，紧跟 MOBI 头之后；有多条取最后一条）。按 UTF-8 解（坏字节换成 U+FFFD）；没有为空串。
pub fn exth_title(mobi: &[u8], mobi_hlen: usize) -> String {
    if mobi_hlen == 0 || mobi.len() < mobi_hlen + 12 || &mobi[mobi_hlen..mobi_hlen + 4] != b"EXTH" {
        return String::new();
    }
    let count = be_u32(mobi, mobi_hlen + 8).unwrap_or(0) as usize;
    let (mut p, mut title) = (mobi_hlen + 12, String::new());
    for _ in 0..count {
        let (Some(rtype), Some(rlen)) = (be_u32(mobi, p), be_u32(mobi, p + 4).map(|v| v as usize)) else { break };
        if rlen < 8 || p + rlen > mobi.len() {
            break;
        }
        if rtype == 503 {
            title = String::from_utf8_lossy(&mobi[p + 8..p + rlen]).into_owned();
        }
        p += rlen;
    }
    title
}

/// 前向读 7bit/byte 变长整数，遇高位=1 停止；推进 `*p`。
fn read_varint_fwd(b: &[u8], p: &mut usize) -> usize {
    let mut val = 0usize;
    while *p < b.len() {
        let c = b[*p];
        *p += 1;
        val = (val << 7) | (c & 0x7f) as usize;
        if c & 0x80 != 0 {
            break;
        }
    }
    val
}

fn rfind_sub(h: &[u8], pat: &[u8]) -> Option<usize> {
    h.windows(pat.len()).rposition(|w| w == pat)
}

// ── INDX 索引族（词头索引、目录索引等都是这套结构）─────────────────
// MOBI 头某偏移存「头 INDX 记录号」；头 INDX 的 +0x18 = 数据块数 ndata；数据 INDX 靠尾部 IDXT 表
// 定位各条目；条目 = [id 长度 1 字节][id 文本][后续 tag/值 字节]。

/// 定位 INDX 索引族：从 MOBI 头 `field_off` 读记录号 → (记录号, 头 INDX 切片, 数据块数 ndata)。
fn indx_locate<'a>(records: &[&'a [u8]], mobi: &[u8], field_off: usize) -> Option<(usize, &'a [u8], usize)> {
    if mobi.len() < field_off + 4 {
        return None;
    }
    let idx = u32::from_be_bytes([mobi[field_off], mobi[field_off + 1], mobi[field_off + 2], mobi[field_off + 3]]) as usize;
    if idx == 0 || idx >= records.len() {
        return None;
    }
    let hdr = records[idx];
    if hdr.len() < 0x1C || &hdr[0..4] != b"INDX" {
        return None;
    }
    let ndata = u32::from_be_bytes([hdr[0x18], hdr[0x19], hdr[0x1A], hdr[0x1B]]) as usize;
    if ndata == 0 || idx + ndata >= records.len() {
        return None;
    }
    Some((idx, hdr, ndata))
}

/// 遍历一个数据 INDX 块的所有条目，返回各条目「id 长度字节起」的字节切片（到下一条目/IDXT）。
fn indx_entries(data: &[u8]) -> Vec<&[u8]> {
    if data.len() < 0x1C || &data[0..4] != b"INDX" {
        return vec![];
    }
    let nent = u32::from_be_bytes([data[0x18], data[0x19], data[0x1A], data[0x1B]]) as usize;
    let idxt = match rfind_sub(data, b"IDXT") {
        Some(x) => x,
        None => return vec![],
    };
    // 条目数来自文件：按 IDXT 之后实际还剩的字节（每项 2 字节）封顶，免得坏文件声明几十亿条时先分配几 GB。
    let mut offs: Vec<usize> = Vec::with_capacity(nent.min(data.len().saturating_sub(idxt + 4) / 2));
    for k in 0..nent {
        let p = idxt + 4 + k * 2;
        if p + 2 > data.len() {
            break;
        }
        offs.push(u16::from_be_bytes([data[p], data[p + 1]]) as usize);
    }
    let mut out = Vec::with_capacity(offs.len());
    for k in 0..offs.len() {
        let s = offs[k];
        let e = if k + 1 < offs.len() { offs[k + 1] } else { idxt };
        if s < e && e <= data.len() {
            out.push(&data[s..e]);
        }
    }
    out
}

/// 拆条目切片为 (id 文本, 其后的 tag/值 字节)。
fn indx_split_entry(entry: &[u8]) -> Option<(&[u8], &[u8])> {
    let id_len = *entry.first()? as usize;
    if 1 + id_len > entry.len() {
        return None;
    }
    Some((&entry[1..1 + id_len], &entry[1 + id_len..]))
}

/// 一族 INDX 解析后的条目：(id 文本, [(tag, 该 tag 的全部值)])。
pub type IndxEntry<'a> = (&'a [u8], Vec<(u8, Vec<usize>)>);

/// 解析一族 INDX（头记录的 `TAGX` 标签表 + 各数据块条目）。返回 (头记录号, 数据块数, 条目)。
/// 条目 = id 文本 + 控制字节（个数见 TAGX `+8`）+ 按 TAGX 顺序的 tag 值（前向变长整数）。某 tag 在控制字节里
/// 对应掩码位上的数 = 出现次数，每次 `nvals` 个值；掩码多位全置 1 的"次数另写"形式没见过样本，遇到时该条目
/// 后面的 tag 不再解析。
pub fn indx_read<'a>(records: &[&'a [u8]], mobi: &[u8], field_off: usize) -> Option<(usize, usize, Vec<IndxEntry<'a>>)> {
    let (idx, hdr, ndata) = indx_locate(records, mobi, field_off)?;
    // TAGX 标签定义表（每项 4 字节：tag / nvals / mask / eof）
    let tagx_at = bookconv::util::memfind(hdr, b"TAGX")?;
    if tagx_at + 12 > hdr.len() {
        return None;
    }
    let tagx_len = be_u32(hdr, tagx_at + 4)? as usize;
    let ctrl_count = (be_u32(hdr, tagx_at + 8)? as usize).max(1);
    let tagx_end = (tagx_at + tagx_len).min(hdr.len());
    let tagtable: Vec<(u8, u8, u8, u8)> = hdr[(tagx_at + 12).min(tagx_end)..tagx_end].as_chunks::<4>().0.iter().map(|t| (t[0], t[1], t[2], t[3])).collect();
    let mut out = Vec::new();
    for blk in 0..ndata {
        for entry in indx_entries(records[idx + 1 + blk]) {
            let Some((id, tagbytes)) = indx_split_entry(entry) else { continue };
            if tagbytes.len() < ctrl_count {
                continue;
            }
            let mut p = ctrl_count;
            let mut ci = 0usize;
            let mut tags = Vec::new();
            for &(tag, nvals, mask, eof) in &tagtable {
                if eof != 0 {
                    ci += 1; // 下一个控制字节
                    continue;
                }
                let Some(&ctrl) = tagbytes.get(ci) else { break };
                if ctrl & mask == 0 {
                    continue;
                }
                if mask.count_ones() > 1 && ctrl & mask == mask {
                    break;
                }
                let occ = ((ctrl & mask) >> mask.trailing_zeros()) as usize;
                let mut vals = Vec::with_capacity(occ * nvals as usize);
                for _ in 0..occ * nvals as usize {
                    if p >= tagbytes.len() {
                        break;
                    }
                    vals.push(read_varint_fwd(tagbytes, &mut p));
                }
                tags.push((tag, vals));
            }
            out.push((id, tags));
        }
    }
    Some((idx, ndata, out))
}

/// 条目里某个 tag 的第 `i` 个值。
pub fn tag_val(tags: &[(u8, Vec<usize>)], tag: u8, i: usize) -> Option<usize> {
    tags.iter().find(|(t, _)| *t == tag).and_then(|(_, v)| v.get(i).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palmdoc_literal_and_backref() {
        let mut out = Vec::new();
        palmdoc_decompress(b"Hi", &mut out);
        assert_eq!(out, b"Hi");
        // 0x80 0x18 = 距离 3、长度 3 的回指
        let mut out = Vec::new();
        palmdoc_decompress(&[b'a', b'b', b'c', 0x80, 0x18], &mut out);
        assert_eq!(out, b"abcabc");
    }

    #[test]
    fn backward_varint_reads_from_end() {
        assert_eq!(backward_varint(&[0x00, 0x83]), 3);
    }

    #[test]
    fn read_extra_flags_needs_long_enough_mobi_header() {
        let mut m = vec![0u8; 0x100];
        m[0..4].copy_from_slice(b"MOBI");
        m[0xE2] = 0x00;
        m[0xE3] = 0x03;
        m[0xF2] = 0x00;
        m[0xF3] = 0x07;
        m[4..8].copy_from_slice(&0xE8u32.to_be_bytes());
        assert_eq!(read_extra_flags(&m), 3);
        // 头只有 0xE0 字节：0xE2 已是 EXTH 的内容，不当标志读，也不去 0xF2 找
        m[4..8].copy_from_slice(&0xE0u32.to_be_bytes());
        assert_eq!(read_extra_flags(&m), 0);
        // 头长度声明够、但切片被截断
        let mut short = m[..0xE2].to_vec();
        short[4..8].copy_from_slice(&0xE8u32.to_be_bytes());
        assert_eq!(read_extra_flags(&short), 0);
    }

    #[test]
    fn varint_fwd_stops_at_high_bit() {
        let b = [0x01, 0x84, 0x7F];
        let mut p = 0;
        assert_eq!(read_varint_fwd(&b, &mut p), 132);
        assert_eq!(p, 2);
    }

    #[test]
    fn exth_title_takes_503() {
        let mut m = vec![0u8; 8];
        m[0..4].copy_from_slice(b"MOBI");
        m.extend_from_slice(b"EXTH\0\0\0\0\0\0\0\x02");
        m.extend_from_slice(&[0, 0, 0, 100, 0, 0, 0, 9, b'x']);
        m.extend_from_slice(&[0, 0, 1, 0xF7, 0, 0, 0, 12]);
        m.extend_from_slice("词典".as_bytes()[..4].as_ref());
        assert_eq!(exth_title(&m, 8), String::from_utf8_lossy(&"词典".as_bytes()[..4]));
        assert_eq!(exth_title(&m, 0), "");
        assert_eq!(exth_title(&m[..20], 8), "", "截断的 EXTH 不越界");
    }

    #[test]
    fn truncated_file_reports_incomplete_download() {
        let mut d = vec![0u8; 78 + 8];
        d[76..78].copy_from_slice(&1u16.to_be_bytes());
        d[78..82].copy_from_slice(&1_000_000u32.to_be_bytes());
        let err = parse_palmdb(&d).unwrap_err();
        assert!(err.contains("文件不完整"), "{err}");
    }

    #[test]
    fn palmdb_name_null_terminated() {
        let mut d = vec![0u8; 78];
        d[0..5].copy_from_slice(b"Book\0");
        assert_eq!(palmdb_name(&d), "Book");
    }
}

//! PalmDB / MOBI 容器的读取（KF8 读取器 [`super::kf8`] 用）：记录表、PalmDOC 解压、extra_data_flags 尾随字节剥离、
//! EXTH、图片、封面、INDX（片段/目录索引）。base32、正向变长整数与写出器共用这一份。
//!
//! 尾随字节标志的位置按「record0 起 +0xF2 == MOBI 头起 +0xE2」定位（[`read_extra_flags`]）；标志位错了跨记录拼接会错位、整本乱码。

use bookconv::convert::common;
use bookconv::epub::{Book, BookMeta, Chapter, Resource};
use regex::Regex;
use std::sync::OnceLock;

/// 剥掉切出来的一段 HTML 的结构壳：XML 声明、`<head>…</head>`、`<html …>`/`</html>`、`<body …>`/`</body>`。
/// KF8 的段是整份 XHTML 文档切开的，切出来的段可能带着这些壳。
pub(super) fn strip_shell(seg: &str) -> String {
    static RX: OnceLock<Regex> = OnceLock::new();
    static RH: OnceLock<Regex> = OnceLock::new();
    static RS: OnceLock<Regex> = OnceLock::new();
    let re_xml = RX.get_or_init(|| Regex::new(r#"(?is)<\?xml[^>]*\?>"#).unwrap());
    let re_head = RH.get_or_init(|| Regex::new(r#"(?is)<head\b.*?</head>"#).unwrap());
    let re_shell = RS.get_or_init(|| Regex::new(r#"(?is)<html\b[^>]*>|</html>|</?body\b[^>]*>"#).unwrap());
    let s = re_xml.replace_all(seg, "");
    let s = re_head.replace_all(&s, "");
    re_shell.replace_all(&s, "").into_owned()
}

/// `re` 在 `seg` 里第一个匹配的第 1 组：去掉标签、首尾空白，取前 80 个字符（段内 `<title>`/`<hN>` 当章名用）。
/// 没有匹配时为空串。
pub(super) fn first_match_text(re: &Regex, seg: &str) -> String {
    static RT: OnceLock<Regex> = OnceLock::new();
    let re_tag = RT.get_or_init(|| Regex::new(r#"(?s)<[^>]+>"#).unwrap());
    re.captures(seg).map(|c| re_tag.replace_all(&c[1], "").trim().chars().take(80).collect()).unwrap_or_default()
}

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

/// 从 record0 解出的容器头（compression / encryption / 文本记录数 / extra_flags / MOBI 头）。
pub struct Header<'a> {
    pub compression: u16,
    pub encryption: u16,
    pub text_record_count: usize,
    pub extra_flags: u16,
    /// r0[16..]，即 MOBI 魔数开始处。
    pub mobi: &'a [u8],
    pub mobi_hlen: usize,
    /// 第一条资源（图片）记录号：MOBI 头 +0x5C（record0 +0x6C）。`kindle:embed:N`、
    /// EXTH 201 封面偏移都从这条记录数起。缺失/非法时为 `None`。
    pub first_resource: Option<usize>,
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
        return Err("有 DRM 加密，不支持（请先去 DRM）".into());
    }
    if compression == 17480 {
        return Err("HUFF/CDIC 压缩，暂不支持".into());
    }
    let mobi = &r0[16..];
    if mobi.len() < 8 || &mobi[0..4] != b"MOBI" {
        return Err("非 MOBI/KF8 容器（无 MOBI 头）".into());
    }
    let mobi_hlen = u32::from_be_bytes([mobi[4], mobi[5], mobi[6], mobi[7]]) as usize;
    let extra_flags = read_extra_flags(mobi);
    let first_resource = be_u32(mobi, 0x5C).map(|v| v as usize).filter(|&v| v > 0 && v != 0xFFFF_FFFF);
    Ok(Header { compression, encryption, text_record_count, extra_flags, mobi, mobi_hlen, first_resource })
}

fn be_u32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
}

/// 解码后的正文 + 「原始字节偏移 → 解码后字节偏移」对照。KF8 的 NCX 位置 / 片段起点都是**原始字节**偏移；
/// 正文含坏 UTF-8 序列（变成 3 字节的 U+FFFD）时解码后位置会漂，必须经 [`RawText::pos`] 换算。
/// 写出器只写 UTF-8（MOBI 头编码 65001），所以这里只按 UTF-8 解；合法 UTF-8 正文不建对照表（恒等）。
pub struct RawText {
    pub text: String,
    /// `map[原始偏移] = 解码后偏移`，长度 = 原始长度 + 1；`None` = 恒等。
    map: Option<Vec<u32>>,
    raw_len: usize,
}

impl RawText {
    pub fn decode(raw: Vec<u8>) -> Self {
        let raw_len = raw.len();
        let raw = match String::from_utf8(raw) {
            Ok(text) => return RawText { text, map: None, raw_len },
            Err(e) => e.into_bytes(),
        };
        // 含坏序列：每段坏字节换成一个 U+FFFD（与 `from_utf8_lossy` 一致），段内字节都指向这个替换符。
        let mut text = String::with_capacity(raw_len + 16);
        let mut map: Vec<u32> = Vec::with_capacity(raw_len + 1);
        for chunk in raw.utf8_chunks() {
            let base = text.len() as u32;
            map.extend((0..chunk.valid().len() as u32).map(|i| base + i));
            text.push_str(chunk.valid());
            if !chunk.invalid().is_empty() {
                let at = text.len() as u32;
                map.extend(std::iter::repeat_n(at, chunk.invalid().len()));
                text.push('\u{FFFD}');
            }
        }
        map.push(text.len() as u32);
        RawText { text, map: Some(map), raw_len }
    }

    /// 原始字节偏移 → 解码后字节偏移（落在字符边界上）；超出正文返回 `None`。
    pub fn pos(&self, raw_off: usize) -> Option<usize> {
        if raw_off > self.raw_len {
            return None;
        }
        Some(match &self.map {
            Some(m) => m[raw_off] as usize,
            None => char_floor(&self.text, raw_off),
        })
    }
}

/// 字节位置下取到字符边界（`str::floor_char_boundary` 未稳定，手写；多字节字符最多回退 3 字节）。
pub fn char_floor(s: &str, mut pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// 解压文本记录 1..=text_record_count → 完整正文字节（KF8 的 rawML）。
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

/// extra_data_flags：正确位置是「record0 起 +0xF2」== 「MOBI 头起 +0xE2」。标准文档常写 0xF2 是相对
/// record0；本函数以 MOBI 头为基。先试 0xE2，再退 0xF2，取「合理小值」（仅低几位是标志位）。
pub fn read_extra_flags(mobi: &[u8]) -> u16 {
    let at = |o: usize| -> Option<u16> {
        if mobi.len() >= o + 2 { Some(u16::from_be_bytes([mobi[o], mobi[o + 1]])) } else { None }
    };
    for o in [0xE2usize, 0xF2] {
        if let Some(v) = at(o) {
            if v <= 0x3F {
                return v;
            }
        }
    }
    0
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

/// 一条图片记录：字节（借自文件，要用时再复制）+ 扩展名 + MIME。
#[derive(Clone, Copy)]
pub struct ImgRec<'a> {
    pub bytes: &'a [u8],
    pub ext: &'static str,
    pub mime: &'static str,
}

impl<'a> ImgRec<'a> {
    fn of(rec: &'a [u8]) -> Option<Self> {
        common::image_ext_mime(rec).map(|(ext, mime)| ImgRec { bytes: rec, ext, mime })
    }
}

/// 扫所有记录收集图片（JPEG/PNG/GIF），按记录序。资源区里只有图片时，这个顺序就是：
/// - KF8 `kindle:embed:NNNN` 的 1-based 索引空间；
/// - EXTH 201 封面偏移（相对首图记录）的 0-based 索引空间（`images[cover_idx]`）。
///
/// 资源区夹着字体等非图片记录时这个顺序会错位，所以优先用 [`resource_image`] 按记录号取。
pub fn collect_images<'a>(records: &[&'a [u8]]) -> Vec<ImgRec<'a>> {
    records.iter().filter_map(|r| ImgRec::of(r)).collect()
}

/// 1-based 资源序号 `n`（`kindle:embed`）→ 图片。优先按 MOBI 头的首个资源记录号直接取
/// `records[first_resource + n - 1]`；那里不是图片（头字段缺失或不可信）时退回 `images[n - 1]`。
pub fn resource_image<'a>(records: &[&'a [u8]], h: &Header, images: &[ImgRec<'a>], n: usize) -> Option<ImgRec<'a>> {
    if n == 0 {
        return None;
    }
    let by_record = h.first_resource.and_then(|f| f.checked_add(n - 1)).and_then(|i| records.get(i)).and_then(|r| ImgRec::of(r));
    by_record.or_else(|| images.get(n - 1).copied())
}

/// EXTH 元数据（title=503 / author=100 / publisher=101 / cover 记录偏移=201 / 缩略图偏移=202）。
#[derive(Default)]
pub struct Exth {
    pub title: String,
    pub author: String,
    pub publisher: String,
    pub language: String,
    pub cover_index: Option<usize>,
    pub thumb_index: Option<usize>,
}

/// EXTH 语言（BCP-47，如 "zh-CN"/"ru"/"en-US"）→ EPUB `dc:language`；空则退 `en`。
pub fn lang_or_default(lang: &str) -> String {
    let t = lang.trim();
    if t.is_empty() { "en".to_string() } else { t.to_string() }
}

/// 解析 EXTH 头（紧跟 MOBI 头之后）。字符串按 UTF-8 解（坏字节换成 U+FFFD）。
pub fn parse_exth(mobi: &[u8], mobi_hlen: usize) -> Exth {
    let mut e = Exth::default();
    if mobi_hlen == 0 || mobi.len() < mobi_hlen + 12 || &mobi[mobi_hlen..mobi_hlen + 4] != b"EXTH" {
        return e;
    }
    let base = mobi_hlen;
    let count =
        u32::from_be_bytes([mobi[base + 8], mobi[base + 9], mobi[base + 10], mobi[base + 11]]) as usize;
    let mut p = base + 12;
    for _ in 0..count {
        if p + 8 > mobi.len() {
            break;
        }
        let rtype = u32::from_be_bytes([mobi[p], mobi[p + 1], mobi[p + 2], mobi[p + 3]]);
        let rlen = u32::from_be_bytes([mobi[p + 4], mobi[p + 5], mobi[p + 6], mobi[p + 7]]) as usize;
        if rlen < 8 || p + rlen > mobi.len() {
            break;
        }
        let val = &mobi[p + 8..p + rlen];
        let as_u32 = || {
            if val.len() >= 4 {
                Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]) as usize)
            } else {
                None
            }
        };
        match rtype {
            503 => e.title = String::from_utf8_lossy(val).into_owned(),
            100 if e.author.is_empty() => e.author = String::from_utf8_lossy(val).into_owned(),
            101 if e.publisher.is_empty() => e.publisher = String::from_utf8_lossy(val).into_owned(),
            201 => e.cover_index = as_u32(),
            202 => e.thumb_index = as_u32(),
            524 if e.language.is_empty() => e.language = String::from_utf8_lossy(val).to_string(),
            _ => {}
        }
        p += rlen;
    }
    e
}

/// 按 EXTH 选封面：201（相对首个资源记录的偏移）优先，取不到退 202 缩略图，再退首图。
/// 偏移换算同 [`resource_image`]（对 AZW3 样本验证过）。
pub fn pick_cover<'a>(records: &[&'a [u8]], h: &Header, images: &[ImgRec<'a>], exth: &Exth) -> Option<ImgRec<'a>> {
    let at = |i: Option<usize>| i.and_then(|i| resource_image(records, h, images, i + 1));
    at(exth.cover_index).or_else(|| at(exth.thumb_index)).or_else(|| resource_image(records, h, images, 1))
}

/// 读回的收尾：书名 + EXTH 元数据 + 封面 + 章节/资源 → EPUB。`scheme` 是 `book_id` 前缀。
pub fn assemble_book(
    scheme: &str,
    title: String,
    exth: Exth,
    cover: Option<ImgRec>,
    chapters: Vec<Chapter>,
    resources: Vec<Resource>,
) -> Result<(Vec<u8>, String), String> {
    let (cover, cover_ext, cover_media_type) = match cover {
        Some(c) => (Some(c.bytes.to_vec()), c.ext.to_string(), c.mime.to_string()),
        None => (None, "jpg".into(), "image/jpeg".into()),
    };
    let mut book = Book {
        meta: BookMeta {
            book_id: format!("{scheme}:{}", common::sanitize_id(&title)),
            title: title.clone(),
            author: exth.author,
            language: lang_or_default(&exth.language),
            publisher: exth.publisher,
            cover,
            cover_ext,
            cover_media_type,
            subjects: Vec::new(),
        },
        chapters,
        resources,
        nav: Vec::new(),
    };
    Ok((bookconv::epub::assemble_master(&mut book)?, title))
}

/// 一条 NCX 目录项：`pos`=章在 rawML 的字节偏移，`label`=真章名，`level`=层级（0=顶层）。
pub struct NcxEntry {
    pub pos: usize,
    pub label: String,
    pub level: u8,
}

/// KF8 引用里的数字（`kindle:embed:XXXX`、`kindle:pos:fid:XXXX:off:YYYYYYYYYY`）是 **base32**：
/// 数字表 `0-9A-V`，高位在前，左侧补 0 到 `width` 位。写出器（azw3 crate）与读取器共用这一份。
const B32: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";

/// base32 编码（见 [`B32`]），不足 `width` 位左补 0。
pub fn base32(mut v: u32, width: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(B32[(v % 32) as usize]);
        v /= 32;
        if v == 0 {
            break;
        }
    }
    while s.len() < width {
        s.push(b'0');
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// base32 解码（大小写都认）；空串、非法字符或溢出返回 `None`。
pub fn base32_decode(s: &str) -> Option<usize> {
    if s.is_empty() {
        return None;
    }
    s.bytes().try_fold(0usize, |acc, b| {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'A'..=b'V' => b - b'A' + 10,
            b'a'..=b'v' => b - b'a' + 10,
            _ => return None,
        };
        acc.checked_mul(32)?.checked_add(d as usize)
    })
}

/// 前向变长整数编码（INDX tag 值 / CNCX 串长）：大端 7 位一组，最后一个字节最高位置 1。
pub fn fwd_varint(mut v: u32) -> Vec<u8> {
    let mut b = vec![(v & 0x7F) as u8 | 0x80];
    v >>= 7;
    while v > 0 {
        b.push((v & 0x7F) as u8);
        v >>= 7;
    }
    b.reverse();
    b
}

/// 前向读 7bit/byte 变长整数（[`fwd_varint`] 的逆），遇高位=1 停止；推进 `*p`。
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

// ── INDX 索引族共享底座（NCX 目录、fragment 索引都是这套结构）─────────────────
// MOBI 头某偏移存「头 INDX 记录号」；头 INDX 的 +0x18 = 数据块数 ndata；数据 INDX 靠尾部 IDXT 表
// 定位各条目；条目 = [id 长度 1 字节][id 文本][后续 tag/值 字节]。以下三个 helper 收敛这套解析，
// `parse_ncx` 与 `parse_fragment_starts` 共用（去重）。

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
type IndxEntry<'a> = (&'a [u8], Vec<(u8, Vec<usize>)>);

/// 解析一族 INDX（头记录的 `TAGX` 标签表 + 各数据块条目）。返回 (头记录号, 数据块数, 条目)。
/// 条目 = id 文本 + 控制字节（个数见 TAGX `+8`）+ 按 TAGX 顺序的 tag 值（前向变长整数）。某 tag 在控制字节里
/// 对应掩码位上的数 = 出现次数，每次 `nvals` 个值；掩码多位全置 1 的"次数另写"形式没见过样本，遇到时该条目
/// 后面的 tag 不再解析。
fn indx_read<'a>(records: &[&'a [u8]], mobi: &[u8], field_off: usize) -> Option<(usize, usize, Vec<IndxEntry<'a>>)> {
    let (idx, hdr, ndata) = indx_locate(records, mobi, field_off)?;
    // TAGX 标签定义表（每项 4 字节：tag / nvals / mask / eof）
    let tagx_at = bookconv::util::memfind(hdr, b"TAGX")?;
    if tagx_at + 12 > hdr.len() {
        return None;
    }
    let tagx_len = be_u32(hdr, tagx_at + 4)? as usize;
    let ctrl_count = (be_u32(hdr, tagx_at + 8)? as usize).max(1);
    let tagx_end = (tagx_at + tagx_len).min(hdr.len());
    let tagtable: Vec<(u8, u8, u8, u8)> = hdr[(tagx_at + 12).min(tagx_end)..tagx_end].chunks_exact(4).map(|t| (t[0], t[1], t[2], t[3])).collect();
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
fn tag_val(tags: &[(u8, Vec<usize>)], tag: u8, i: usize) -> Option<usize> {
    tags.iter().find(|(t, _)| *t == tag).and_then(|(_, v)| v.get(i).copied())
}

/// 解析 KF8 **NCX 目录** → `(组装后文本偏移, 真章名, 层级)` 列表。KF8 章名不在正文 `<title>`（那常=书名），
/// 而在独立索引：NCX 记录号存 MOBI 头 `+0xE4`；结构 = 头 INDX（含 `TAGX` 标签定义 + `+0x18` 数据块数）
/// 加 N 个数据 INDX（每条目 = id 文本 + 按 TAGX 编码的 tag 值）+ CNCX（`ncx+1+ndata`，标签字串池）。
/// tag 1 = pos（**片段插回骨架之后**的文本偏移，要经 [`Kf8Map::to_stored`] 换成 rawML 存储偏移）、
/// tag 3 = CNCX 标签偏移、tag 4 = 层级。
/// 5 本真机样本（俄/日/中，4–30 条，含层级）验证。解析失败/非预期结构 → 返回空（调用方退化，不崩不回归）。
pub fn parse_ncx(records: &[&[u8]], h: &Header) -> Vec<NcxEntry> {
    let Some((ncx, ndata, entries)) = indx_read(records, h.mobi, 0xE4) else { return vec![] };
    // CNCX 紧跟数据块；截断的文件可能没有这条记录。
    let cncx = match records.get(ncx + 1 + ndata) {
        Some(r) => *r,
        None => return vec![],
    };
    // CNCX 文本校验：首串须 UTF-8 可解，否则判定不是我们要的 NCX（防误读别的索引族）
    {
        let mut p = 0usize;
        let l = read_varint_fwd(cncx, &mut p);
        if l == 0 || p + l > cncx.len() || std::str::from_utf8(&cncx[p..p + l]).is_err() {
            return vec![];
        }
    }
    let label_at = |o: usize| -> String {
        if o >= cncx.len() {
            return String::new();
        }
        let mut p = o;
        let l = read_varint_fwd(cncx, &mut p);
        if p + l > cncx.len() {
            return String::new();
        }
        String::from_utf8_lossy(&cncx[p..p + l]).into_owned()
    };
    entries
        .iter()
        .filter_map(|(_, tags)| {
            let (pos, lo) = (tag_val(tags, 1, 0)?, tag_val(tags, 3, 0)?);
            let label = label_at(lo);
            (!label.trim().is_empty()).then(|| NcxEntry { pos, label, level: tag_val(tags, 4, 0).unwrap_or(0) as u8 })
        })
        .collect()
}

/// 解析 KF8 **fragment 索引** → 每个 fragment 的插入位置（下标=fragment id）。
/// fragment 索引记录号存 MOBI 头 `+0xE8`；数据 INDX 每条目的 id 文本就是插入位置的十进制串（如 "0000000400"）。
/// 插入位置是**片段插回骨架之后**的文本坐标，要换成 rawML 存储坐标见 [`Kf8Map`]。
pub fn parse_fragment_starts(records: &[&[u8]], h: &Header) -> Vec<usize> {
    let Some((_, _, entries)) = indx_read(records, h.mobi, 0xE8) else { return vec![] };
    entries.iter().map(|(id, _)| parse_dec(id)).collect()
}

fn parse_dec(id: &[u8]) -> usize {
    std::str::from_utf8(id).unwrap_or("").trim().parse::<usize>().unwrap_or(0)
}

/// KF8 两套坐标的换算。rawML 里按"骨架、它的各片段、下一个骨架……"**存储**；阅读器把片段插回骨架的 `</body>`
/// 之前**组装**成文档。NCX 的 pos、fragment 索引的插入位置都是组装后的坐标，而我们切章用的是存储顺序的 rawML，
/// 两者在片段内差一个"骨架尾巴"（`</body></html>` 那段）的长度——不换算，切点会提前十几个字节，切进前一个标签
/// 中间（真书《福尔摩斯探案全集》7 张插图因此丢失）。
/// 骨架索引（MOBI 头 `+0xEC`）：tag 1 = 片段数，tag 6 = (存储起点, 长度)；fragment 索引 tag 6 = (_, 片段长度)。
pub struct Kf8Map {
    /// 按片段号：(组装后插入位置, 长度, rawML 存储起点)
    frags: Vec<(usize, usize, usize)>,
}

impl Kf8Map {
    /// 两个索引都在且对得上才返回；否则 `None`（调用方退回"两套坐标当一样"的旧近似）。
    pub fn parse(records: &[&[u8]], h: &Header) -> Option<Self> {
        let (_, _, fentries) = indx_read(records, h.mobi, 0xE8)?;
        let (_, _, sentries) = indx_read(records, h.mobi, 0xEC)?;
        let mut frags: Vec<(usize, usize, usize)> = fentries.iter().map(|(id, tags)| Some((parse_dec(id), tag_val(tags, 6, 1)?, 0))).collect::<Option<_>>()?;
        let mut fi = 0usize;
        for (_, tags) in &sentries {
            let (count, start, len) = (tag_val(tags, 1, 0)?, tag_val(tags, 6, 0)?, tag_val(tags, 6, 1)?);
            // 这些数都来自文件：相加一律 checked，溢出就当索引不可信（`None`，调用方退回旧近似）。
            let mut pos = start.checked_add(len)?;
            let end = fi.checked_add(count)?;
            for f in frags.get_mut(fi..end)? {
                f.2 = pos;
                pos = pos.checked_add(f.1)?;
            }
            fi = end;
        }
        let sorted = frags.windows(2).all(|w| w[0].0 <= w[1].0);
        (fi == frags.len() && sorted && !frags.is_empty()).then_some(Kf8Map { frags })
    }

    /// 各片段在 rawML 里的存储起点（下标 = 片段号）。
    pub fn stored_starts(&self) -> Vec<usize> {
        self.frags.iter().map(|f| f.2).collect()
    }

    /// 组装后偏移 → rawML 存储偏移。落在某片段里的按片段换算；落在骨架里（片段之外）的两套坐标相同。
    pub fn to_stored(&self, a: usize) -> usize {
        let i = self.frags.partition_point(|f| f.0 <= a);
        match i.checked_sub(1).map(|k| self.frags[k]) {
            Some((ins, len, stored)) if a < ins.saturating_add(len) => stored.saturating_add(a - ins),
            _ => a,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palmdoc_literal_and_backref() {
        let mut out = Vec::new();
        palmdoc_decompress(b"Hi", &mut out);
        assert_eq!(out, b"Hi");
    }

    #[test]
    fn backward_varint_reads_from_end() {
        assert_eq!(backward_varint(&[0x00, 0x83]), 3);
    }

    #[test]
    fn read_extra_flags_prefers_e2_plausible() {
        // MOBI 头：前 0xE2 填 0，然后 0xE2 处放 0x0003（合理），0xF2 处放 0xffff（不合理，跳过）。
        let mut m = vec![0u8; 0xF4];
        m[0..4].copy_from_slice(b"MOBI");
        m[0xE2] = 0x00;
        m[0xE3] = 0x03;
        m[0xF2] = 0xff;
        m[0xF3] = 0xff;
        assert_eq!(read_extra_flags(&m), 3);
    }

    fn mobi_header(first_resource: u32) -> Vec<u8> {
        let mut r0 = vec![0u8; 16 + 0x80];
        r0[16..20].copy_from_slice(b"MOBI");
        r0[20..24].copy_from_slice(&0x80u32.to_be_bytes());
        r0[16 + 0x5C..16 + 0x60].copy_from_slice(&first_resource.to_be_bytes());
        r0
    }

    #[test]
    fn pick_cover_uses_exth_201_then_falls_back() {
        let (a, b, c) = ([0xFF, 0xD8, 0xFF, 0], [0xFF, 0xD8, 0xFF, 1], [0xFF, 0xD8, 0xFF, 2]);
        let records: Vec<&[u8]> = vec![&[0u8; 4], &a, &b, &c];
        let r0 = mobi_header(1);
        let h = parse_header(&r0).unwrap();
        let images = collect_images(&records);
        let mut e = Exth { cover_index: Some(2), ..Default::default() };
        assert_eq!(pick_cover(&records, &h, &images, &e).unwrap().bytes, &c);
        e.cover_index = Some(9); // 越界 → 退 202
        e.thumb_index = Some(1);
        assert_eq!(pick_cover(&records, &h, &images, &e).unwrap().bytes, &b);
        e.thumb_index = Some(9); // 都越界 → 退首图
        assert_eq!(pick_cover(&records, &h, &images, &e).unwrap().bytes, &a);
    }

    #[test]
    fn resource_index_counts_from_first_resource_record() {
        // 资源区：图、字体、图。按记录号取时序号 3 = 第二张图；只数图片会错成越界。
        let (a, font, b) = ([0xFF, 0xD8, 0xFF, 0], *b"FONT", [0xFF, 0xD8, 0xFF, 1]);
        let records: Vec<&[u8]> = vec![&[0u8; 4], &[1u8; 4], &a, &font, &b];
        let r0 = mobi_header(2);
        let h = parse_header(&r0).unwrap();
        let images = collect_images(&records);
        assert_eq!(resource_image(&records, &h, &images, 3).unwrap().bytes, &b);
        assert!(resource_image(&records, &h, &images, 0).is_none());
        // 头字段缺失 → 退回只数图片
        let r0 = mobi_header(0);
        let h = parse_header(&r0).unwrap();
        assert_eq!(resource_image(&records, &h, &images, 2).unwrap().bytes, &b);
    }

    #[test]
    fn base32_round_trips_kindle_digits() {
        assert_eq!(base32(9, 4), "0009");
        assert_eq!(base32(31, 4), "000V");
        assert_eq!(base32(32, 4), "0010");
        assert_eq!(base32(0, 10), "0000000000");
        for v in [0u32, 1, 9, 10, 31, 32, 1023, 1024, 123_456_789] {
            assert_eq!(base32_decode(&base32(v, 10)), Some(v as usize));
        }
        assert_eq!(base32_decode("000a"), Some(10), "小写也认");
        assert_eq!(base32_decode("00W0"), None);
        assert_eq!(base32_decode(""), None);
    }

    #[test]
    fn fwd_varint_round_trips() {
        for v in [0u32, 1, 127, 128, 132, 0x11111, u32::MAX >> 4] {
            let enc = fwd_varint(v);
            let mut p = 0;
            assert_eq!(read_varint_fwd(&enc, &mut p), v as usize);
            assert_eq!(p, enc.len());
        }
    }

    #[test]
    fn raw_text_maps_bad_utf8_offsets() {
        // UTF-8 里夹坏字节：坏字节换成 U+FFFD，后续偏移照样对得上
        let t = RawText::decode(b"a\xFFb<i>".to_vec());
        assert_eq!(t.text, "a\u{FFFD}b<i>");
        assert_eq!(&t.text[t.pos(3).unwrap()..], "<i>");
        assert_eq!(t.pos(6), Some(t.text.len()));
        assert_eq!(t.pos(7), None);
        // 合法 UTF-8：恒等，落在多字节字符中间时下取到字符边界
        let t = RawText::decode("中<b>".as_bytes().to_vec());
        assert_eq!(t.pos(1), Some(0));
        assert_eq!(t.pos(3), Some(3));
    }

    #[test]
    fn truncated_ncx_without_cncx_record_yields_empty_not_panic() {
        // NCX 头 INDX（1 个数据块，带 TAGX）+ 数据块，但文件在 CNCX 记录前截断。
        let mut r0 = vec![0u8; 16 + 0xE8];
        r0[16..20].copy_from_slice(b"MOBI");
        r0[20..24].copy_from_slice(&0xE8u32.to_be_bytes());
        r0[16 + 0xE4..16 + 0xE8].copy_from_slice(&1u32.to_be_bytes());
        let mut hdr = vec![0u8; 0x20];
        hdr[..4].copy_from_slice(b"INDX");
        hdr[0x18..0x1C].copy_from_slice(&1u32.to_be_bytes());
        hdr.extend_from_slice(b"TAGX\0\0\0\x10\0\0\0\x01\x01\x01\x01\0");
        let data = b"INDX".to_vec();
        let records: Vec<&[u8]> = vec![&r0, &hdr, &data];
        let h = parse_header(&r0).unwrap();
        assert!(parse_ncx(&records, &h).is_empty());
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

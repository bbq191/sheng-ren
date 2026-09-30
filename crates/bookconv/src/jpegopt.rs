//! JPEG 无损瘦身：只重做哈夫曼编码表，图像数据（量化后的 DCT 系数）一个不动，解码出来逐像素相同（用户 2026-09-29：换压缩更好的编码、画质必须不变）。
//!
//! `image` 库的 JPEG 编码器用 JPEG 标准附录 K 的通用哈夫曼表；按这张图实际出现的符号频率重建最优表（附录 K.2 的算法，码长限 16 位），
//! 同样的系数能少占约 7%–16%（2026-09-29《亂馬½》漫画页 13%–16%；2026-09-30 页面缩到 1104×1546、q95：
//! 《死亡筆記》愛藏版卷01 灰度 8.0%，《哆啦A夢全彩版》卷01 彩色 7.5%、转灰度 9.0%）。做法同 `jpegtran -optimize`，这里自己实现（只用 Rust）：
//!
//! 1. 解析头部：SOF0（基线顺序式）、DQT、DHT、SOS；
//! 2. 按原表把熵编码数据解成符号流（DC 差值类别、AC 游程/类别 + 附加位），同时统计每张表的符号频率；
//! 3. 按频率生成最优码长与码字，用新 DHT 替换旧的，按新表重新编码符号流（附加位原样）。
//!
//! 只处理本 crate 自己编出来的这类 JPEG（基线、单次扫描、没有重启间隔）；别的形态（渐进式、算术编码、多次扫描、DRI）原样返回 `None`。
//! 调用方（`imgopt`）还会把新旧两份都解码逐像素比对，不一致就用旧的——正确性不只靠这里的实现。
//!
//! 速度（2026-09-30）：熵编码数据按 64 位缓冲读写、哈夫曼码先查 9 位表（长码再逐位），结果与逐位实现逐字节相同；
//! 537 张 1104×1546 q95 漫画页 `optimize` 平均约 17ms → 6ms/张（逐位读写时它占整页处理时间四成多）。

/// 一张哈夫曼表：按码长排好的符号（附录 C 的 BITS/HUFFVAL）。
#[derive(Clone, Default)]
struct Table {
    bits: [u8; 17], // bits[1..=16]：各码长的码字数
    vals: Vec<u8>,
}

/// 查表解码一次看的位数：码长不超过它的码字一次查出（通用表和按图重做的表里绝大多数符号都在 9 位以内）。
const LUT_BITS: u32 = 9;
/// 查表项：高 8 位是码长（0 = 前缀不够长，走逐位解码），低 8 位是符号；[`LUT_BAD`] = 码字对不上表（数据损坏）。
const LUT_BAD: u16 = 0xFFFF;

/// 解码用：附录 F.2.2.3 的 MAXCODE/VALPTR/MINCODE，外加 [`LUT_BITS`] 位的查找表。
struct Decoder {
    maxcode: [i32; 18],
    valptr: [i32; 17],
    mincode: [i32; 17],
    vals: Vec<u8>,
    lut: Vec<u16>,
}

impl Decoder {
    fn new(t: &Table) -> Decoder {
        let (mut maxcode, mut valptr, mut mincode) = ([-1i32; 18], [0i32; 17], [0i32; 17]);
        let (mut code, mut k) = (0i32, 0i32);
        for l in 1..=16 {
            let n = t.bits[l] as i32;
            if n > 0 {
                valptr[l] = k;
                mincode[l] = code;
                code += n;
                k += n;
                maxcode[l] = code - 1;
            }
            code <<= 1;
        }
        maxcode[17] = i32::MAX;
        let mut d = Decoder { maxcode, valptr, mincode, vals: t.vals.clone(), lut: Vec::new() };
        // 查找表：对每个 LUT_BITS 位前缀按附录 F.2.2.3 的逐位算法走到 LUT_BITS 位为止，结果和逐位解码完全一样（包括坏表的情形）。
        d.lut = (0..1u32 << LUT_BITS)
            .map(|v| {
                for l in 1..=LUT_BITS as usize {
                    let code = (v >> (LUT_BITS as usize - l)) as i32;
                    if code <= d.maxcode[l] {
                        return d.symbol(l, code).map_or(LUT_BAD, |s| ((l as u16) << 8) | s as u16);
                    }
                }
                0
            })
            .collect();
        d
    }

    /// 码长 `l`、码值 `code` 对应的符号（附录 F.2.2.3 的 VALPTR 取值）。
    fn symbol(&self, l: usize, code: i32) -> Option<u8> {
        self.vals.get((self.valptr[l] + code - self.mincode[l]) as usize).copied()
    }
}

/// 熵编码数据的位读取器（跳过 0xFF 后的填充 0x00）。一次预读多个字节进 64 位缓冲；遇到标记（0xFF 后不是 0x00）
/// 或数据读完就不再预读，之后再要位 = 损坏（与逐位读的判定一致）。
struct BitReader<'a> {
    data: &'a [u8],
    /// 下一个没读进缓冲的字节。
    pos: usize,
    /// 低 `nbits` 位是还没用的位，下一位是第 `nbits - 1` 位。
    acc: u64,
    nbits: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, acc: 0, nbits: 0 }
    }

    /// 缓冲补到至少 57 位（数据读完或遇到标记时可能更少）。
    #[inline]
    fn refill(&mut self) {
        while self.nbits <= 56 {
            let Some(&b) = self.data.get(self.pos) else { return };
            if b == 0xFF {
                // 填充字节 0x00；别的就是标记（或数据截断），停在这里
                if self.data.get(self.pos + 1) != Some(&0) {
                    return;
                }
                self.pos += 2;
            } else {
                self.pos += 1;
            }
            self.acc = (self.acc << 8) | b as u64;
            self.nbits += 8;
        }
    }

    /// 读 `n`（≤ 16）位；不够 → `None`。
    #[inline]
    fn bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if self.nbits < n {
            self.refill();
            if self.nbits < n {
                return None;
            }
        }
        self.nbits -= n;
        Some(((self.acc >> self.nbits) & ((1u64 << n) - 1)) as u32)
    }

    #[inline]
    fn decode(&mut self, d: &Decoder) -> Option<u8> {
        if self.nbits < LUT_BITS {
            self.refill();
        }
        if self.nbits >= LUT_BITS {
            let e = d.lut[((self.acc >> (self.nbits - LUT_BITS)) & ((1 << LUT_BITS) - 1)) as usize];
            if e == LUT_BAD {
                return None;
            }
            if e >> 8 != 0 {
                self.nbits -= (e >> 8) as u32;
                return Some(e as u8);
            }
        }
        // 码长超过 LUT_BITS，或数据快读完了：逐位走附录 F.2.2.3
        let mut code = self.bits(1)? as i32;
        for l in 1..=16 {
            if code <= d.maxcode[l] {
                return d.symbol(l, code);
            }
            code = (code << 1) | self.bits(1)? as i32;
        }
        None
    }

    /// 已经用掉的数据到哪为止（最后一个用到的位所在字节之后；缓冲里整字节没用的退回去，填充的 0x00 一并算）。
    fn consumed(&self) -> usize {
        let mut pos = self.pos;
        for i in 0..self.nbits / 8 {
            let b = (self.acc >> (8 * i)) as u8;
            pos -= if b == 0xFF { 2 } else { 1 };
        }
        pos
    }
}

/// 位写入器（写出时给 0xFF 补填充 0x00，结尾补 1）。
struct BitWriter {
    out: Vec<u8>,
    /// 低 `nbits` 位是还没写出的位（每次 `put` 之后 `nbits` < 32）。
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    /// 写 `value` 的低 `n`（≤ 32）位。攒够 32 位整批写出 4 个字节。
    #[inline]
    fn put(&mut self, value: u32, n: u32) {
        self.acc = (self.acc << n) | (value as u64 & ((1u64 << n) - 1));
        self.nbits += n;
        if self.nbits >= 32 {
            self.nbits -= 32;
            let w = ((self.acc >> self.nbits) as u32).to_be_bytes();
            if w.contains(&0xFF) {
                for b in w {
                    self.out.push(b);
                    if b == 0xFF {
                        self.out.push(0);
                    }
                }
            } else {
                self.out.extend_from_slice(&w);
            }
        }
    }

    /// 剩下的位补 1 凑满字节写出。
    fn finish(mut self) -> Vec<u8> {
        let pad = (8 - self.nbits % 8) % 8;
        self.acc = (self.acc << pad) | ((1u64 << pad) - 1);
        self.nbits += pad;
        while self.nbits > 0 {
            self.nbits -= 8;
            let b = (self.acc >> self.nbits) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0);
            }
        }
        self.out
    }
}

/// 符号流里的一项：用哪张表（`slot` = 类别*4 + 表号）、符号、附加位。
struct Sym {
    slot: u8,
    sym: u8,
    extra: u16,
    nextra: u8,
}

/// 附录 K.2：按频率生成码长（限 16 位），返回 BITS/HUFFVAL。
fn optimal_table(freq_in: &[u32; 256]) -> Table {
    let mut freq = [0i64; 257];
    for (i, f) in freq_in.iter().enumerate() {
        freq[i] = *f as i64;
    }
    freq[256] = 1; // 保留一个码字，保证不会出现全 1 的码
    let mut codesize = [0usize; 257];
    let mut others = [-1i32; 257];
    loop {
        // 最小的两个非零频率（相同取序号大的，同附录 K.2）
        let mut v1: i32 = -1;
        for i in 0..257 {
            if freq[i] > 0 && (v1 < 0 || freq[i] <= freq[v1 as usize]) {
                v1 = i as i32;
            }
        }
        let mut v2: i32 = -1;
        for i in 0..257 {
            if freq[i] > 0 && i as i32 != v1 && (v2 < 0 || freq[i] <= freq[v2 as usize]) {
                v2 = i as i32;
            }
        }
        if v2 < 0 {
            break;
        }
        let (mut a, mut b) = (v1 as usize, v2 as usize);
        freq[a] += freq[b];
        freq[b] = 0;
        codesize[a] += 1;
        while others[a] >= 0 {
            a = others[a] as usize;
            codesize[a] += 1;
        }
        others[a] = v2;
        codesize[b] += 1;
        while others[b] >= 0 {
            b = others[b] as usize;
            codesize[b] += 1;
        }
    }
    let mut bits = [0i32; 33];
    for &c in codesize.iter() {
        if c > 0 {
            bits[c.min(32)] += 1;
        }
    }
    // 码长超过 16 的往下挪（附录 K.3 Adjust_BITS）
    for i in (17..=32).rev() {
        while bits[i] > 0 {
            let mut j = i - 2;
            while bits[j] == 0 {
                j -= 1;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
    }
    let mut i = 16;
    while bits[i] == 0 {
        i -= 1;
    }
    bits[i] -= 1; // 去掉保留的那个
    let mut t = Table::default();
    for (dst, &n) in t.bits.iter_mut().zip(bits.iter()).skip(1) {
        *dst = n as u8;
    }
    // HUFFVAL：按原始码长从短到长、同码长按符号值排（附录 K.2 Sort_input）
    for size in 1..=32 {
        for (v, &c) in codesize.iter().enumerate().take(256) {
            if c == size {
                t.vals.push(v as u8);
            }
        }
    }
    t
}

/// 附录 C：由 BITS/HUFFVAL 生成每个符号的（码字, 码长）。
fn codes(t: &Table) -> [(u16, u8); 256] {
    let mut out = [(0u16, 0u8); 256];
    let (mut code, mut k) = (0u32, 0usize);
    for l in 1..=16u8 {
        for _ in 0..t.bits[l as usize] {
            if let Some(&v) = t.vals.get(k) {
                out[v as usize] = (code as u16, l);
            }
            code += 1;
            k += 1;
        }
        code <<= 1;
    }
    out
}

fn be16(b: &[u8], i: usize) -> Option<usize> {
    Some(((*b.get(i)? as usize) << 8) | *b.get(i + 1)? as usize)
}

/// 重做哈夫曼表。不是本模块能处理的形态、数据损坏、或者没变小，返回 `None`（调用方用原来的）。
pub fn optimize(jpeg: &[u8]) -> Option<Vec<u8>> {
    if jpeg.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    // ── 头部 ──
    let mut pos = 2;
    let mut kept: Vec<&[u8]> = Vec::new(); // 原样保留的段（DHT 以外）
    let mut tables: [Option<Table>; 8] = Default::default();
    struct Comp {
        id: u8,
        h: usize,
        v: usize,
    }
    let mut comps: Vec<Comp> = Vec::new();
    let (mut width, mut height) = (0usize, 0usize);
    let sos_start;
    loop {
        if *jpeg.get(pos)? != 0xFF {
            return None;
        }
        let marker = *jpeg.get(pos + 1)?;
        let len = be16(jpeg, pos + 2)?;
        let seg = jpeg.get(pos..pos + 2 + len)?;
        match marker {
            0xC0 => {
                // 基线 SOF0：精度、高、宽、分量数，每个分量 (id, HV, Tq)
                height = be16(seg, 5)?;
                width = be16(seg, 7)?;
                let n = *seg.get(9)? as usize;
                for c in 0..n {
                    let p = 10 + 3 * c;
                    let hv = *seg.get(p + 1)?;
                    comps.push(Comp { id: *seg.get(p)?, h: (hv >> 4) as usize, v: (hv & 15) as usize });
                }
                kept.push(seg);
            }
            0xC4 => {
                let mut p = 4;
                while p < seg.len() {
                    let tc_th = seg[p];
                    let (class, id) = ((tc_th >> 4) as usize, (tc_th & 15) as usize);
                    if class > 1 || id > 3 {
                        return None;
                    }
                    let mut t = Table::default();
                    let mut total = 0usize;
                    for l in 1..=16 {
                        t.bits[l] = *seg.get(p + l)?;
                        total += t.bits[l] as usize;
                    }
                    t.vals = seg.get(p + 17..p + 17 + total)?.to_vec();
                    tables[class * 4 + id] = Some(t);
                    p += 17 + total;
                }
            }
            0xDA => {
                sos_start = pos;
                break;
            }
            // 渐进式、无损、算术编码、重启间隔：不处理
            0xC1..=0xC3 | 0xC5..=0xCF | 0xDD => return None,
            _ => kept.push(seg),
        }
        pos += 2 + len;
    }
    if comps.is_empty() || width == 0 || height == 0 {
        return None;
    }
    let sos_len = be16(jpeg, sos_start + 2)?;
    let sos = jpeg.get(sos_start..sos_start + 2 + sos_len)?;
    let ns = *sos.get(4)? as usize;
    if ns != comps.len() {
        return None; // 多次扫描
    }
    // 扫描里各分量用哪张 DC/AC 表
    let mut scan: Vec<(usize, usize, usize, usize)> = Vec::new(); // (h, v, dc slot, ac slot)
    for s in 0..ns {
        let cid = *sos.get(5 + 2 * s)?;
        let t = *sos.get(6 + 2 * s)?;
        let c = comps.iter().find(|c| c.id == cid)?;
        scan.push((c.h, c.v, (t >> 4) as usize, 4 + (t & 15) as usize));
    }
    // 频谱选择与逐次逼近：基线必须是 0..63、0
    if sos.get(5 + 2 * ns..8 + 2 * ns)? != [0, 63, 0] {
        return None;
    }
    let decoders: Vec<Option<Decoder>> = tables.iter().map(|t| t.as_ref().map(Decoder::new)).collect();

    // ── 解出符号流 ──
    let data = &jpeg[sos_start + 2 + sos_len..];
    let mut br = BitReader::new(data);
    let (hmax, vmax) = (scan.iter().map(|s| s.0).max()?, scan.iter().map(|s| s.1).max()?);
    // 单分量扫描不交错：按块数算；多分量按 MCU 算
    let (mcus, blocks_per): (usize, Vec<usize>) = if ns == 1 {
        (width.div_ceil(8) * height.div_ceil(8), vec![1])
    } else {
        (width.div_ceil(8 * hmax) * height.div_ceil(8 * vmax), scan.iter().map(|s| s.0 * s.1).collect())
    };
    let mut syms: Vec<Sym> = Vec::with_capacity(mcus * 64);
    let mut freq = [[0u32; 256]; 8];
    for _ in 0..mcus {
        for (ci, &nb) in blocks_per.iter().enumerate() {
            let (_, _, dc, ac) = scan[ci];
            for _ in 0..nb {
                let s = br.decode(decoders[dc].as_ref()?)?;
                if s > 11 {
                    return None;
                }
                let extra = br.bits(s as u32)?;
                freq[dc][s as usize] += 1;
                syms.push(Sym { slot: dc as u8, sym: s, extra: extra as u16, nextra: s });
                let mut k = 1;
                while k < 64 {
                    let rs = br.decode(decoders[ac].as_ref()?)?;
                    let (run, size) = ((rs >> 4) as usize, rs & 15);
                    if size > 10 {
                        return None;
                    }
                    let extra = br.bits(size as u32)?;
                    freq[ac][rs as usize] += 1;
                    syms.push(Sym { slot: ac as u8, sym: rs, extra: extra as u16, nextra: size });
                    if size == 0 {
                        if run == 15 {
                            k += 16; // ZRL：16 个零
                            continue;
                        }
                        break; // EOB
                    }
                    k += run + 1;
                }
                if k > 64 {
                    return None;
                }
            }
        }
    }
    // 熵编码数据之后应当紧跟 EOI（中间允许补位用的 1）
    let rest = &data[br.consumed()..];
    let eoi = rest.windows(2).position(|w| w == [0xFF, 0xD9])?;
    if rest[..eoi].iter().any(|&b| b != 0xFF) {
        return None;
    }

    // ── 新表、重新编码 ──
    let mut new_tables: [Option<Table>; 8] = Default::default();
    let mut new_codes: [[(u16, u8); 256]; 8] = [[(0, 0); 256]; 8];
    for slot in 0..8 {
        if freq[slot].iter().any(|&f| f > 0) {
            let t = optimal_table(&freq[slot]);
            new_codes[slot] = codes(&t);
            new_tables[slot] = Some(t);
        }
    }
    let mut bw = BitWriter { out: Vec::with_capacity(data.len()), acc: 0, nbits: 0 };
    for s in &syms {
        let (code, len) = new_codes[s.slot as usize][s.sym as usize];
        if len == 0 {
            return None;
        }
        // 码字和附加位一起写（最多 16 + 11 位）
        bw.put(((code as u32) << s.nextra) | s.extra as u32, (len + s.nextra) as u32);
    }
    let entropy = bw.finish();

    let mut out = Vec::with_capacity(jpeg.len());
    out.extend_from_slice(&[0xFF, 0xD8]);
    for seg in &kept {
        out.extend_from_slice(seg);
    }
    let mut dht = Vec::new();
    for (slot, t) in new_tables.iter().enumerate() {
        if let Some(t) = t {
            dht.push(((slot / 4) << 4 | (slot % 4)) as u8);
            dht.extend_from_slice(&t.bits[1..=16]);
            dht.extend_from_slice(&t.vals);
        }
    }
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);
    out.extend_from_slice(sos);
    out.extend_from_slice(&entropy);
    out.extend_from_slice(&[0xFF, 0xD9]);
    (out.len() < jpeg.len()).then_some(out)
}

/// [`optimize`] 之后把新旧两份都解码、逐像素比对：完全相同才用新的，否则用原来的（画质不变的保证不只靠上面的实现）。
pub fn optimize_verified(jpeg: Vec<u8>) -> Vec<u8> {
    let Some(new) = optimize(&jpeg) else { return jpeg };
    let same = match (image::load_from_memory(&jpeg), image::load_from_memory(&new)) {
        (Ok(a), Ok(b)) => a.color() == b.color() && a.width() == b.width() && a.height() == b.height() && a.as_bytes() == b.as_bytes(),
        _ => false,
    };
    if same {
        new
    } else {
        jpeg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::jpeg::JpegEncoder, DynamicImage, GrayImage, RgbImage};

    fn encode(img: &DynamicImage, q: u8) -> Vec<u8> {
        let mut b = Vec::new();
        JpegEncoder::new_with_quality(&mut b, q).encode_image(img).unwrap();
        b
    }

    /// 带纹理的测试图（纯色图压缩后几乎没有 AC 符号，测不出东西）。
    fn textured(w: u32, h: u32) -> (GrayImage, RgbImage) {
        let mut s = 12345u32;
        let mut rnd = move || {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            (s >> 16) as u8
        };
        let g = GrayImage::from_fn(w, h, |x, y| image::Luma([((x * 7 + y * 3) as u8).wrapping_add(rnd() % 40)]));
        let c = RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 5) as u8, (y * 3) as u8, rnd()]));
        (g, c)
    }

    #[test]
    fn pixels_identical_and_smaller() {
        // 尺寸不是 8、16 的倍数也要对（边上的 MCU 补齐）
        for (w, h) in [(64, 64), (100, 37), (257, 129), (1, 1), (17, 300)] {
            let (g, c) = textured(w, h);
            for img in [DynamicImage::ImageLuma8(g.clone()), DynamicImage::ImageRgb8(c.clone())] {
                for q in [75, 95, 100] {
                    let orig = encode(&img, q);
                    let a = image::load_from_memory(&orig).unwrap();
                    if let Some(new) = optimize(&orig) {
                        let b = image::load_from_memory(&new).unwrap();
                        assert_eq!(a.as_bytes(), b.as_bytes(), "{w}x{h} q{q} {:?}：解码结果要逐像素相同", img.color());
                        assert!(new.len() < orig.len());
                    } else {
                        assert!(w * h < 64 * 64, "{w}x{h} q{q} 这么大的图应当能变小");
                    }
                    let v = optimize_verified(orig.clone());
                    assert_eq!(image::load_from_memory(&v).unwrap().as_bytes(), a.as_bytes());
                }
            }
        }
    }

    #[test]
    fn buffered_bit_io_matches_bit_by_bit_with_stuffing() {
        // 逐位的参照：先拼成位串，再按字节切、0xFF 后补 0x00、结尾补 1
        let mut s = 7u32;
        let mut items = Vec::new();
        for i in 0..5000u32 {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            let n = 1 + (s >> 8) % 27;
            // 常出现全 1，逼出 0xFF 填充
            let v = if i % 5 == 0 { u32::MAX } else { s >> 3 };
            items.push((v & ((1u32 << n) - 1), n));
        }
        let mut bitstr = Vec::new();
        for &(v, n) in &items {
            for i in (0..n).rev() {
                bitstr.push((v >> i) & 1);
            }
        }
        while bitstr.len() % 8 != 0 {
            bitstr.push(1);
        }
        let mut want = Vec::new();
        for c in bitstr.chunks(8) {
            let b = c.iter().fold(0u8, |a, &x| (a << 1) | x as u8);
            want.push(b);
            if b == 0xFF {
                want.push(0);
            }
        }
        let mut w = BitWriter { out: Vec::new(), acc: 0, nbits: 0 };
        for &(v, n) in &items {
            w.put(v, n);
        }
        let got = w.finish();
        assert_eq!(got, want);
        // 读回来：每段位一样；后面跟着 EOI 时读完的位置停在它前面
        let mut data = got.clone();
        data.extend_from_slice(&[0xFF, 0xD9]);
        let mut r = BitReader::new(&data);
        for &(v, n) in &items {
            let got = if n > 16 { (r.bits(n - 16).unwrap() << 16) | r.bits(16).unwrap() } else { r.bits(n).unwrap() };
            assert_eq!(got, v);
        }
        assert_eq!(r.consumed(), got.len(), "最后一个字节用到了就算读过");
        while r.bits(1).is_some() {}
        assert_eq!(r.consumed(), got.len(), "填充的 1 读完后停在标记前");
    }

    #[test]
    fn optimal_table_is_valid_prefix_code() {
        let mut f = [0u32; 256];
        for (i, v) in f.iter_mut().enumerate().take(200) {
            *v = (i as u32 * 37 % 1000) + 1; // 很多符号：码长会被限到 16
        }
        f[0] = 1_000_000;
        let t = optimal_table(&f);
        assert_eq!(t.vals.len(), 200);
        let c = codes(&t);
        assert!(c.iter().filter(|x| x.1 > 0).all(|x| x.1 <= 16));
        // Kraft 不等式：严格小于 1（保留了全 1 的码字）
        let kraft: f64 = c.iter().filter(|x| x.1 > 0).map(|x| 0.5f64.powi(x.1 as i32)).sum();
        assert!(kraft < 1.0, "{kraft}");
        // 只有一个符号也要能编
        let mut one = [0u32; 256];
        one[5] = 10;
        assert_eq!(codes(&optimal_table(&one))[5].1, 1);
    }

    #[test]
    fn unsupported_forms_left_alone() {
        assert!(optimize(b"not a jpeg").is_none());
        let (g, _) = textured(64, 64);
        let mut j = encode(&DynamicImage::ImageLuma8(g), 90);
        j.truncate(j.len() / 2); // 截断的
        assert!(optimize(&j).is_none());
        assert_eq!(optimize_verified(j.clone()), j);
    }
}

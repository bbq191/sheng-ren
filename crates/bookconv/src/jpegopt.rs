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
//! [`optimize_verified`] 还会把产物按新表重新解析、与原符号流逐项比对，不一致就用旧的——正确性不只靠编码这一半的实现。
//!
//! 速度（2026-09-30）：熵编码数据按 64 位缓冲读写、哈夫曼码先查 9 位表（长码再逐位），结果与逐位实现逐字节相同；
//! 537 张 1104×1546 q95 漫画页 `optimize` 平均约 17ms → 6ms/张（逐位读写时它占整页处理时间四成多）。
//! 核对（2026-10-03）：从"新旧两份都整张解码逐像素比"改成"按新表重新解析、逐项比符号流"，合成的 1104×1546 q95 带噪页上
//! 核对开销约 22ms → 9ms/张，整页处理约少两成；产物字节不变。
//!
//! 本 crate 自己编 JPEG 时（[`crate::imgopt::Page8::encode`]）不再"先用 `image` 按通用表编一遍、再按通用表解回符号流"：
//! [`encode_optimized`] 照 `image` 编码器的算法直接从像素算出符号流，再走同一个重做表、核对的流程，产物逐字节相同
//! （2026-10-07，合成的 1104×1546 线稿页：灰度 q95 约 12ms → 7ms/张，彩色 q85 约 19ms → 13ms/张；见文末一节）。

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
#[derive(PartialEq, Eq)]
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

/// 解析出来的 JPEG：除 DHT 以外原样保留的段、SOS 段、按原表解出的符号流、每张表的符号频率。
struct Parsed<'a> {
    kept: Vec<&'a [u8]>,
    sos: &'a [u8],
    syms: Vec<Sym>,
    freq: [[u32; 256]; 8],
}

/// 重做哈夫曼表。不是本模块能处理的形态、数据损坏、或者没变小，返回 `None`（调用方用原来的）。
pub fn optimize(jpeg: &[u8]) -> Option<Vec<u8>> {
    rebuild(&parse(jpeg)?, jpeg.len())
}

/// 解析头部、按文件里的哈夫曼表把熵编码数据解成符号流（同时统计频率）。不是本模块能处理的形态、数据损坏 → `None`。
fn parse(jpeg: &[u8]) -> Option<Parsed<'_>> {
    let mut syms: Vec<Sym> = Vec::with_capacity(jpeg.len());
    let mut freq = [[0u32; 256]; 8];
    let (kept, sos) = parse_with(jpeg, |s| {
        freq[s.slot as usize][s.sym as usize] += 1;
        syms.push(s);
        Some(())
    })?;
    Some(Parsed { kept, sos, syms, freq })
}

/// [`parse`] 的主体：符号逐个交给 `emit`（它返回 `None` 就中止），返回 DHT 以外原样保留的段和 SOS 段。
fn parse_with(jpeg: &[u8], mut emit: impl FnMut(Sym) -> Option<()>) -> Option<(Vec<&[u8]>, &[u8])> {
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
                    let (h, v) = ((hv >> 4) as usize, (hv & 15) as usize);
                    if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
                        return None; // 采样因子只能是 1–4（0 会在算 MCU 数时除零）：坏图，原图照用
                    }
                    comps.push(Comp { id: *seg.get(p)?, h, v });
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
        let (td, ta) = ((t >> 4) as usize, (t & 15) as usize);
        if td > 3 || ta > 3 {
            return None; // 表号只有 0–3（大了会越界）：坏图，原图照用
        }
        scan.push((c.h, c.v, td, 4 + ta));
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
    for _ in 0..mcus {
        for (ci, &nb) in blocks_per.iter().enumerate() {
            let (_, _, dc, ac) = scan[ci];
            for _ in 0..nb {
                let s = br.decode(decoders[dc].as_ref()?)?;
                if s > 11 {
                    return None;
                }
                let extra = br.bits(s as u32)?;
                emit(Sym { slot: dc as u8, sym: s, extra: extra as u16, nextra: s })?;
                let mut k = 1;
                while k < 64 {
                    let rs = br.decode(decoders[ac].as_ref()?)?;
                    let (run, size) = ((rs >> 4) as usize, rs & 15);
                    if size > 10 {
                        return None;
                    }
                    let extra = br.bits(size as u32)?;
                    emit(Sym { slot: ac as u8, sym: rs, extra: extra as u16, nextra: size })?;
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
    Some((kept, sos))
}

/// 按 `p` 的符号频率生成最优表、重新编码，拼回完整的 JPEG；没比 `orig_len` 小 → `None`。
fn rebuild(p: &Parsed, orig_len: usize) -> Option<Vec<u8>> {
    rebuild_any(p, orig_len).filter(|out| out.len() < orig_len)
}

/// [`rebuild`] 不比长短：按 `p` 的符号频率生成最优表、重新编码，拼回完整的 JPEG（`cap` 只是预分配的大小）。
/// 有符号编不出来（坏表）→ `None`。
fn rebuild_any(p: &Parsed, cap: usize) -> Option<Vec<u8>> {
    let Parsed { kept, sos, syms, freq } = p;

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
    let mut bw = BitWriter { out: Vec::with_capacity(cap), acc: 0, nbits: 0 };
    for s in syms {
        let (code, len) = new_codes[s.slot as usize][s.sym as usize];
        if len == 0 {
            return None;
        }
        // 码字和附加位一起写（最多 16 + 11 位）
        bw.put(((code as u32) << s.nextra) | s.extra as u32, (len + s.nextra) as u32);
    }
    let entropy = bw.finish();

    let mut out = Vec::with_capacity(cap);
    out.extend_from_slice(&[0xFF, 0xD8]);
    for seg in kept {
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
    Some(out)
}

/// [`optimize`] 并自证无损：把产物按它自己的新表重新解析一遍，与原文件的解析结果逐项比对——DHT 以外的段（SOF、DQT 等）
/// 逐字节相同、SOS 相同、符号流（每一项的表、符号、附加位）完全相同，才用新的，否则用原来的。
///
/// 为什么这等于"解码逐像素相同"：解码结果只取决于帧头、量化表、扫描头和熵编码数据还原出的 DCT 系数；前三者逐字节相同，
/// 系数由符号流（游程/类别 + 附加位）唯一决定，哈夫曼表只是符号的编码方式。所以比符号流就是比系数，不必把两份都解码成像素
/// （2026-10-03 以前就是两份都整张解码逐像素比，占整页处理时间约 28%）。像素级比对留在测试里。
pub fn optimize_verified(jpeg: Vec<u8>) -> Vec<u8> {
    let Some(old) = parse(&jpeg) else { return jpeg };
    let Some(new) = rebuild(&old, jpeg.len()) else { return jpeg };
    if same_coefficients(&old, &new) {
        new
    } else {
        jpeg
    }
}

/// `new` 按它自己的哈夫曼表解析出来，与 `old` 的非 DHT 段、SOS、符号流逐项相同。
fn same_coefficients(old: &Parsed, new: &[u8]) -> bool {
    // 边解边比，不另存一份符号流
    let mut i = 0usize;
    let same_syms = |s: Sym| {
        let ok = old.syms.get(i) == Some(&s);
        i += 1;
        ok.then_some(())
    };
    parse_with(new, same_syms).is_some_and(|(kept, sos)| kept == old.kept && sos == old.sos) && i == old.syms.len()
}

// ───────────────────────── 从像素直接编出重做过表的 JPEG ─────────────────────────
//
// 以前 [`crate::imgopt::Page8::encode`] 先用 `image` 0.25 的 `JpegEncoder` 按通用哈夫曼表编一遍（逐位写出、逐像素 `get_pixel`、
// 量化时每个系数调一次 `roundf`），再 [`optimize_verified`] 把它按通用表解回符号流、重做表、重新编码——一张图编两遍、解一遍。
// [`encode_optimized`] 照 `image` 编码器的做法（帧头、量化表、扫描头、色彩转换、DCT、量化、游程编码）直接从像素算出同一个
// 符号流，省掉通用表那一遍的逐位写出和解回。产物和"`image` 编码 + `optimize_verified`"逐字节相同：
// - 按通用表写出的那一份照样拼出来——重做后没变小、或者核对不过时就用它，和 `optimize_verified` 的取舍一样；
// - 重做的产物照旧按新表重新解析、与符号流逐项比对（[`same_coefficients`]），正确性不只靠编码这一半；
// - 测试里拿 `image` 的编码器逐字节对照（各种尺寸、灰度和彩色、几种质量）。
//
// DCT 是 `image` 编码器里那份 IJG `jfdctint.c`（libjpeg 9a，Thomas G. Lane、Guido Vollbeding）的 Rust 译本，照抄、一个运算不改
// （包括第二遍奇数部分的舍入量与 libjpeg 不同之处），这样系数才逐个相同。本软件部分基于 Independent JPEG Group 的工作。

/// 附录 K 的亮度、色度量化表（自然顺序），质量缩放见 [`encode_optimized`]。
#[rustfmt::skip]
const STD_QTABLES: [[u8; 64]; 2] = [
    [
        16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55,
        14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62,
        18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92,
        49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
    ],
    [
        17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99,
        24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99,
        99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
        99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    ],
];

/// 之字形顺序第 i 个系数在 8×8 块（自然顺序）里的位置。
#[rustfmt::skip]
const ZIGZAG: [u8; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10, 17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// 附录 K.3–K.6 的通用哈夫曼表的 BITS（码长 1–16 各几个）：亮度 DC、色度 DC、亮度 AC、色度 AC。
const STD_BITS: [[u8; 16]; 4] = [
    [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
    [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D],
    [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
];
const STD_DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
#[rustfmt::skip]
const STD_LUMA_AC_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7,
    0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5,
    0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
    0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
    0xF9, 0xFA,
];
#[rustfmt::skip]
const STD_CHROMA_AC_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0,
    0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26,
    0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
    0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3,
    0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
    0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
    0xF9, 0xFA,
];

/// 通用哈夫曼表（槽位＝类别*4+表号：0 亮度 DC、1 色度 DC、4 亮度 AC、5 色度 AC）。
fn std_table(slot: usize) -> Table {
    let (i, vals): (usize, &[u8]) = match slot {
        0 => (0, &STD_DC_VALS),
        1 => (1, &STD_DC_VALS),
        4 => (2, &STD_LUMA_AC_VALS),
        _ => (3, &STD_CHROMA_AC_VALS),
    };
    let mut bits = [0u8; 17];
    bits[1..].copy_from_slice(&STD_BITS[i]);
    Table { bits, vals: vals.to_vec() }
}

/// 一个段的完整字节：`FF 标记`、长度、数据。
fn segment(marker: u8, data: &[u8]) -> Vec<u8> {
    let mut s = Vec::with_capacity(data.len() + 4);
    s.extend_from_slice(&[0xFF, marker]);
    s.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
    s.extend_from_slice(data);
    s
}

/// 整数前向 DCT（IJG `jfdctint.c` 经 `image` 0.25 的 Rust 译本，照抄），系数放大 8 倍。
#[allow(clippy::identity_op, clippy::erasing_op)]
fn fdct(samples: &[u8; 64], coeffs: &mut [i32; 64]) {
    const CONST_BITS: i32 = 13;
    const PASS1_BITS: i32 = 2;
    const FIX_0_298631336: i32 = 2446;
    const FIX_0_390180644: i32 = 3196;
    const FIX_0_541196100: i32 = 4433;
    const FIX_0_765366865: i32 = 6270;
    const FIX_0_899976223: i32 = 7373;
    const FIX_1_175875602: i32 = 9633;
    const FIX_1_501321110: i32 = 12_299;
    const FIX_1_847759065: i32 = 15_137;
    const FIX_1_961570560: i32 = 16_069;
    const FIX_2_053119869: i32 = 16_819;
    const FIX_2_562915447: i32 = 20_995;
    const FIX_3_072711026: i32 = 25_172;
    // 第一遍：行
    for y in 0usize..8 {
        let y0 = y * 8;
        let s = |i: usize| i32::from(samples[y0 + i]);
        let t0 = s(0) + s(7);
        let t1 = s(1) + s(6);
        let t2 = s(2) + s(5);
        let t3 = s(3) + s(4);
        let t10 = t0 + t3;
        let t12 = t0 - t3;
        let t11 = t1 + t2;
        let t13 = t1 - t2;
        let t0 = s(0) - s(7);
        let t1 = s(1) - s(6);
        let t2 = s(2) - s(5);
        let t3 = s(3) - s(4);
        coeffs[y0] = (t10 + t11 - 8 * 128) << PASS1_BITS;
        coeffs[y0 + 4] = (t10 - t11) << PASS1_BITS;
        let z1 = (t12 + t13) * FIX_0_541196100 + (1 << (CONST_BITS - PASS1_BITS - 1));
        coeffs[y0 + 2] = (z1 + t12 * FIX_0_765366865) >> (CONST_BITS - PASS1_BITS);
        coeffs[y0 + 6] = (z1 - t13 * FIX_1_847759065) >> (CONST_BITS - PASS1_BITS);
        let t12 = t0 + t2;
        let t13 = t1 + t3;
        let z1 = (t12 + t13) * FIX_1_175875602 + (1 << (CONST_BITS - PASS1_BITS - 1));
        let t12 = t12 * (-FIX_0_390180644) + z1;
        let t13 = t13 * (-FIX_1_961570560) + z1;
        let z1 = (t0 + t3) * (-FIX_0_899976223);
        let t0 = t0 * FIX_1_501321110 + z1 + t12;
        let t3 = t3 * FIX_0_298631336 + z1 + t13;
        let z1 = (t1 + t2) * (-FIX_2_562915447);
        let t1 = t1 * FIX_3_072711026 + z1 + t13;
        let t2 = t2 * FIX_2_053119869 + z1 + t12;
        coeffs[y0 + 1] = t0 >> (CONST_BITS - PASS1_BITS);
        coeffs[y0 + 3] = t1 >> (CONST_BITS - PASS1_BITS);
        coeffs[y0 + 5] = t2 >> (CONST_BITS - PASS1_BITS);
        coeffs[y0 + 7] = t3 >> (CONST_BITS - PASS1_BITS);
    }
    // 第二遍：列（去掉 PASS1_BITS 的放大，留下 8 倍）
    for x in 0usize..8 {
        let c = |r: usize| coeffs[x + 8 * r];
        let t0 = c(0) + c(7);
        let t1 = c(1) + c(6);
        let t2 = c(2) + c(5);
        let t3 = c(3) + c(4);
        let t10 = t0 + t3 + (1 << (PASS1_BITS - 1));
        let t12 = t0 - t3;
        let t11 = t1 + t2;
        let t13 = t1 - t2;
        let t0 = c(0) - c(7);
        let t1 = c(1) - c(6);
        let t2 = c(2) - c(5);
        let t3 = c(3) - c(4);
        coeffs[x] = (t10 + t11) >> PASS1_BITS;
        coeffs[x + 8 * 4] = (t10 - t11) >> PASS1_BITS;
        let z1 = (t12 + t13) * FIX_0_541196100 + (1 << (CONST_BITS + PASS1_BITS - 1));
        coeffs[x + 8 * 2] = (z1 + t12 * FIX_0_765366865) >> (CONST_BITS + PASS1_BITS);
        coeffs[x + 8 * 6] = (z1 - t13 * FIX_1_847759065) >> (CONST_BITS + PASS1_BITS);
        let t12 = t0 + t2;
        let t13 = t1 + t3;
        // 舍入量照 `image` 的译本（libjpeg 原文是 CONST_BITS + PASS1_BITS - 1）：照抄才和它逐字节相同
        let z1 = (t12 + t13) * FIX_1_175875602 + (1 << (CONST_BITS - PASS1_BITS - 1));
        let t12 = t12 * (-FIX_0_390180644) + z1;
        let t13 = t13 * (-FIX_1_961570560) + z1;
        let z1 = (t0 + t3) * (-FIX_0_899976223);
        let t0 = t0 * FIX_1_501321110 + z1 + t12;
        let t3 = t3 * FIX_0_298631336 + z1 + t13;
        let z1 = (t1 + t2) * (-FIX_2_562915447);
        let t1 = t1 * FIX_3_072711026 + z1 + t13;
        let t2 = t2 * FIX_2_053119869 + z1 + t12;
        coeffs[x + 8] = t0 >> (CONST_BITS + PASS1_BITS);
        coeffs[x + 8 * 3] = t1 >> (CONST_BITS + PASS1_BITS);
        coeffs[x + 8 * 5] = t2 >> (CONST_BITS + PASS1_BITS);
        coeffs[x + 8 * 7] = t3 >> (CONST_BITS + PASS1_BITS);
    }
}

/// 量化一个块：`image` 的编码器逐个系数算 `((x / 8) as f32 / q as f32).round() as i32`（`x` 是放大 8 倍的系数），这里是同一个
/// f32 除法，只是取整不调 `roundf`（x86-64 基线没有取整指令，每个系数一次函数调用）：截断成整数，`商 - 整数部分`（|商| < 2^23 时
/// 是精确的）到了 ±0.5 再进一位，和 `round` 的远离零一致。64 个系数一起算，编译器能整块用 SIMD。测试里逐个对照过。
#[inline]
fn quantize_block(coef: &mut [i32; 64], q: &[f32; 64]) {
    // 分成三个简单的循环，编译器才肯整块向量化
    let mut v = [0f32; 64];
    for (v, c) in v.iter_mut().zip(coef.iter()) {
        *v = (*c / 8) as f32;
    }
    for (v, q) in v.iter_mut().zip(q) {
        *v /= q;
    }
    for (c, v) in coef.iter_mut().zip(v) {
        let t = v as i32;
        let f = v - t as f32;
        *c = t + i32::from(f >= 0.5) - i32::from(f <= -0.5);
    }
}

/// 系数的（类别, 附加位），同附录 F.1.2.1（负数取反码）。
#[inline]
fn magnitude(v: i32) -> (u8, u16) {
    let a = v.unsigned_abs();
    let n = (32 - a.leading_zeros()) as u8;
    let mask = ((1u32 << n) - 1) as u16;
    (n, if v < 0 { (v - 1) as u16 & mask } else { v as u16 & mask })
}

/// 把 8 位灰度（`gray`）或 RGB 像素编成基线 JPEG（质量 `quality`，1–100），哈夫曼表按这张图重做。
/// 结果和 `image` 0.25 的 `JpegEncoder::new_with_quality(.., quality).write_image(..)` 编出来再过 [`optimize_verified`] 逐字节相同
/// （做法见本节开头）。`image` 编码器报错的情形（宽或高为 0、超过 65535）返回 `None`；它会 panic 的情形（量化后系数超出通用
/// 哈夫曼表能编的范围：DC 差值类别 > 11、AC 类别 > 10，只有极端的高频图在质量接近 100 时才会出现）也返回 `None`。
pub(crate) fn encode_optimized(raw: &[u8], width: u32, height: u32, gray: bool, quality: u8) -> Option<Vec<u8>> {
    let e = encode_syms(raw, width, height, gray, quality)?;
    let std_min = e.std_len_lower_bound();
    let mut kept: Vec<&[u8]> = vec![&e.app0, &e.sof];
    kept.extend(e.dqts.iter().map(Vec::as_slice));
    let parsed = Parsed { kept, sos: &e.sos, syms: e.syms, freq: e.freq };
    let new = rebuild_any(&parsed, std_min)?;
    // 重做的要比按通用表写的短才用（同 `optimize_verified`）。通用表那份的长度先按位数估下限，比下限还短就不用一个个数（数要再过一遍符号流）
    let smaller = new.len() < std_min || new.len() < std_len(e.gray, &e.app0, &e.sof, &e.dqts, &e.sos, &parsed.syms);
    Some(if smaller && same_coefficients(&parsed, &new) {
        new
    } else {
        // 用按通用表写的那一份（很少见：小图、纯色图，DHT 段占了大头）
        std_bytes(e.gray, &e.app0, &e.sof, &e.dqts, &e.sos, &parsed.syms)
    })
}

/// [`encode_syms`] 的结果：头部各段、符号流、频率。
struct Encoded {
    gray: bool,
    app0: Vec<u8>,
    sof: Vec<u8>,
    dqts: Vec<Vec<u8>>,
    sos: Vec<u8>,
    syms: Vec<Sym>,
    freq: [[u32; 256]; 8],
}

impl Encoded {
    /// 按通用表写出的完整 JPEG（和 `image` 编码器的输出逐字节相同）。
    #[cfg(test)]
    fn std_bytes(&self) -> Vec<u8> {
        std_bytes(self.gray, &self.app0, &self.sof, &self.dqts, &self.sos, &self.syms)
    }

    /// 按通用表写出来的长度的下限：按频率算出熵编码数据的位数（码长 + 附加位，不算 0xFF 后填充的 0x00），只看 8 张表的频率，
    /// 不过符号流。
    fn std_len_lower_bound(&self) -> usize {
        let (tables, codes) = std_codes(self.gray);
        let mut bits = 0u64;
        for (slot, (freq, codes)) in self.freq.iter().zip(&codes).enumerate() {
            for (sym, (&f, &(_, len))) in freq.iter().zip(codes).enumerate() {
                // 附加位：DC 是类别本身，AC 是低 4 位
                let extra = if slot < 4 { sym } else { sym & 15 };
                bits += u64::from(f) * (u64::from(len) + extra as u64);
            }
        }
        std_header(&self.app0, &self.sof, &self.dqts, &self.sos, &tables).len() + bits.div_ceil(8) as usize + 2
    }

    #[cfg(test)]
    fn std_len(&self) -> usize {
        std_len(self.gray, &self.app0, &self.sof, &self.dqts, &self.sos, &self.syms)
    }
}

/// 按通用表写出来有多长，只数不写（重做的产物要比它小才用）。
fn std_len(gray: bool, app0: &[u8], sof: &[u8], dqts: &[Vec<u8>], sos: &[u8], syms: &[Sym]) -> usize {
    let (tables, codes) = std_codes(gray);
    let mut bc = BitCounter::default();
    for s in syms {
        let (code, len) = codes[s.slot as usize][s.sym as usize];
        bc.put(((code as u32) << s.nextra) | s.extra as u32, (len + s.nextra) as u32);
    }
    std_header(app0, sof, dqts, sos, &tables).len() + bc.finish() + 2
}

/// 各槽位（类别*4+表号）每个符号的（码字, 码长）。
type SlotCodes = [[(u16, u8); 256]; 8];

/// 通用哈夫曼表（按 `image` 编码器写 DHT 的顺序）与各槽位的（码字, 码长）。
fn std_codes(gray: bool) -> (Vec<(usize, Table)>, SlotCodes) {
    let slots: &[usize] = if gray { &[0, 4] } else { &[0, 4, 1, 5] };
    let mut by_slot = [[(0u16, 0u8); 256]; 8];
    let tables: Vec<(usize, Table)> = slots.iter().map(|&s| (s, std_table(s))).collect();
    for (s, t) in &tables {
        by_slot[*s] = codes(t);
    }
    (tables, by_slot)
}

/// 熵编码数据之前的部分（SOI 到 SOS；DHT 是通用表，每张表一个段，同 `image` 的编码器）。
fn std_header(app0: &[u8], sof: &[u8], dqts: &[Vec<u8>], sos: &[u8], tables: &[(usize, Table)]) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    out.extend_from_slice(app0);
    out.extend_from_slice(sof);
    for d in dqts {
        out.extend_from_slice(d);
    }
    for (slot, t) in tables {
        let mut d = vec![(((slot / 4) << 4) | (slot % 4)) as u8];
        d.extend_from_slice(&t.bits[1..]);
        d.extend_from_slice(&t.vals);
        out.extend_from_slice(&segment(0xC4, &d));
    }
    out.extend_from_slice(sos);
    out
}

/// 按通用表写出完整的 JPEG（和 `image` 编码器的输出逐字节相同）。
fn std_bytes(gray: bool, app0: &[u8], sof: &[u8], dqts: &[Vec<u8>], sos: &[u8], syms: &[Sym]) -> Vec<u8> {
    let (tables, codes) = std_codes(gray);
    let mut bw = BitWriter { out: std_header(app0, sof, dqts, sos, &tables), acc: 0, nbits: 0 };
    for s in syms {
        let (code, len) = codes[s.slot as usize][s.sym as usize];
        bw.put(((code as u32) << s.nextra) | s.extra as u32, (len + s.nextra) as u32);
    }
    let mut out = bw.finish();
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// 只数字节的 [`BitWriter`]：同样按 32 位一批、0xFF 后补 0x00、结尾补 1，算出写出来有多长。
#[derive(Default)]
struct BitCounter {
    acc: u64,
    nbits: u32,
    bytes: usize,
}

impl BitCounter {
    #[inline]
    fn put(&mut self, value: u32, n: u32) {
        self.acc = (self.acc << n) | (value as u64 & ((1u64 << n) - 1));
        self.nbits += n;
        if self.nbits >= 32 {
            self.nbits -= 32;
            let w = (self.acc >> self.nbits) as u32;
            self.bytes += 4 + w.to_be_bytes().iter().filter(|&&b| b == 0xFF).count();
        }
    }

    fn finish(mut self) -> usize {
        let pad = (8 - self.nbits % 8) % 8;
        self.acc = (self.acc << pad) | ((1u64 << pad) - 1);
        self.nbits += pad;
        while self.nbits > 0 {
            self.nbits -= 8;
            self.bytes += if (self.acc >> self.nbits) as u8 == 0xFF { 2 } else { 1 };
        }
        self.bytes
    }
}

/// 照 `image` 0.25 的编码器把像素编成符号流（连同它的头部各段）。见 [`encode_optimized`]。
fn encode_syms(raw: &[u8], width: u32, height: u32, gray: bool, quality: u8) -> Option<Encoded> {
    let n = if gray { 1 } else { 3 };
    let (w16, h16) = (u16::try_from(width).ok().filter(|&v| v > 0)?, u16::try_from(height).ok().filter(|&v| v > 0)?);
    let (w, h) = (width as usize, height as usize);
    if raw.len() != w * h * n {
        return None;
    }
    // 量化表按质量缩放（libjpeg 的算法）
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - q * 2 };
    let mut qt = STD_QTABLES;
    for v in qt.iter_mut().flatten() {
        *v = ((u32::from(*v) * scale + 50) / 100).clamp(1, 255) as u8;
    }
    // 头部各段（和 `image` 的编码器一样：JFIF 1.02、像素比 1:1；分量 1–3、采样 1×1）
    let app0 = segment(0xE0, b"JFIF\0\x01\x02\x00\x00\x01\x00\x01\x00\x00");
    // (分量 id, 量化表, DC 槽位, AC 槽位)
    let comps: &[(u8, usize, u8, u8)] = if gray { &[(1, 0, 0, 4)] } else { &[(1, 0, 0, 4), (2, 1, 1, 5), (3, 1, 1, 5)] };
    let mut sof = vec![8];
    sof.extend_from_slice(&h16.to_be_bytes());
    sof.extend_from_slice(&w16.to_be_bytes());
    sof.push(comps.len() as u8);
    for &(id, t, ..) in comps {
        sof.extend_from_slice(&[id, 0x11, t as u8]);
    }
    let sof = segment(0xC0, &sof);
    let dqts: Vec<Vec<u8>> = (0..if gray { 1 } else { 2 })
        .map(|i| {
            let mut d = vec![i as u8];
            d.extend(ZIGZAG.iter().map(|&k| qt[i][k as usize]));
            segment(0xDB, &d)
        })
        .collect();
    let mut sos = vec![comps.len() as u8];
    for &(id, t, ..) in comps {
        sos.extend_from_slice(&[id, ((t as u8) << 4) | t as u8]);
    }
    sos.extend_from_slice(&[0, 63, 0]);
    let sos = segment(0xDA, &sos);

    // 符号流：逐块（边上不满 8×8 的块用最近的像素补）色彩转换 → DCT → 量化 → 游程编码
    let qf: [[f32; 64]; 2] = [qt[0].map(f32::from), qt[1].map(f32::from)];
    let mut syms: Vec<Sym> = Vec::with_capacity(w * h * n / 4);
    let mut freq = [[0u32; 256]; 8];
    let mut planes = [[0u8; 64]; 3];
    let mut coef = [0i32; 64];
    let mut prev_dc = [0i32; 3];
    let stride = w * n;
    // 每 8 行先整条转成各分量的平面（宽补到 8 的倍数，补的列和不满 8 行时补的行都取最近的像素，同 `image` 的 `pixel_at_or_near`），
    // 再按块取：逐行连续地算色彩转换，不在每个块里逐像素判断边界
    let wp = w.div_ceil(8) * 8;
    let mut strips = vec![vec![0u8; 8 * wp]; n];
    for by in (0..h).step_by(8) {
        for y in 0..8 {
            let row = &raw[(by + y).min(h - 1) * stride..][..stride];
            let o = y * wp;
            if let [ys] = strips.as_mut_slice() {
                ys[o..o + w].copy_from_slice(row);
            } else if let [ys, cbs, crs] = strips.as_mut_slice() {
                let (ys, cbs, crs) = (&mut ys[o..o + w], &mut cbs[o..o + w], &mut crs[o..o + w]);
                for (((p, yv), cb), cr) in row.as_chunks::<3>().0.iter().zip(ys).zip(cbs).zip(crs) {
                    let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
                    // BT.601 全范围，16 位定点（同 `image` 编码器的 `rgb_to_ycbcr`）
                    const UV: i32 = (128 << 16) + (1 << 15) - 1;
                    *yv = ((19595 * r + 38469 * g + 7471 * b + (1 << 15) - 1) >> 16) as u8;
                    *cb = ((-11059 * r - 21709 * g + 32768 * b + UV) >> 16) as u8;
                    *cr = ((32768 * r - 27439 * g - 5329 * b + UV) >> 16) as u8;
                }
            }
            for st in strips.iter_mut() {
                let last = st[o + w - 1];
                st[o + w..o + wp].fill(last);
            }
        }
        for bx in (0..w).step_by(8) {
            for (plane, st) in planes.iter_mut().zip(&strips) {
                for y in 0..8 {
                    plane[y * 8..y * 8 + 8].copy_from_slice(&st[y * wp + bx..y * wp + bx + 8]);
                }
            }
            for (c, &(_, t, dc, ac)) in comps.iter().enumerate() {
                fdct(&planes[c], &mut coef);
                quantize_block(&mut coef, &qf[t]);
                let (size, extra) = magnitude(coef[0] - prev_dc[c]);
                prev_dc[c] = coef[0];
                if size > 11 {
                    return None; // 通用 DC 表没有这个类别（`image` 编码器在这里 panic）
                }
                let mut emit = |slot: u8, sym: u8, extra: u16, nextra: u8| {
                    freq[slot as usize][sym as usize] += 1;
                    syms.push(Sym { slot, sym, extra, nextra });
                };
                emit(dc, size, extra, size);
                // 之字形顺序里非零的 AC 系数记成位图，按位找下一个非零的（不逐个系数判断零不零，大多是零时少很多分支）
                let zz: [i32; 64] = ZIGZAG.map(|k| coef[k as usize]);
                let mut nonzero = zz.iter().enumerate().skip(1).fold(0u64, |m, (i, &v)| m | (u64::from(v != 0) << i));
                let mut last = 0u32;
                while nonzero != 0 {
                    let i = nonzero.trailing_zeros();
                    nonzero &= nonzero - 1;
                    let mut run = (i - last - 1) as u8;
                    while run > 15 {
                        emit(ac, 0xF0, 0, 0);
                        run -= 16;
                    }
                    let (size, extra) = magnitude(zz[i as usize]);
                    if size > 10 {
                        return None; // 通用 AC 表没有这个类别
                    }
                    emit(ac, (run << 4) | size, extra, size);
                    last = i;
                }
                if zz[63] == 0 {
                    emit(ac, 0x00, 0, 0); // EOB
                }
            }
        }
    }
    Some(Encoded { gray, app0, sof, dqts, sos, syms, freq })
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

    /// 头部第一个 `marker` 段的起点（`FF xx` 的位置）。
    fn seg_pos(jpeg: &[u8], marker: u8) -> usize {
        let mut pos = 2;
        loop {
            if jpeg[pos + 1] == marker {
                return pos;
            }
            pos += 2 + be16(jpeg, pos + 2).unwrap();
        }
    }

    /// 坏图不 panic、返回 `None`（原图照用）：采样因子为 0（算 MCU 数时会除零）、扫描里的表号大于 3（会越界）。
    #[test]
    fn malformed_sampling_or_table_id_rejected() {
        let (_, c) = textured(64, 64);
        let orig = encode(&DynamicImage::ImageRgb8(c), 90);
        assert!(optimize(&orig).is_some(), "好图本来能处理");
        for hv in [0x00, 0x10, 0x01, 0x51] {
            let mut bad = orig.clone();
            bad[seg_pos(&orig, 0xC0) + 11] = hv; // 第一个分量的 HV
            assert_eq!(optimize(&bad), None, "HV={hv:#04x}");
        }
        for t in [0x40, 0x04, 0xFF] {
            let mut bad = orig.clone();
            bad[seg_pos(&orig, 0xDA) + 6] = t; // 第一个分量的 DC/AC 表号
            assert_eq!(optimize(&bad), None, "表号={t:#04x}");
        }
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
                    assert_eq!(image::load_from_memory(&v).unwrap().as_bytes(), a.as_bytes(), "{w}x{h} q{q}：核对过的产物解码逐像素相同");
                    // 符号流核对不误拒：能优化的都采用了优化结果
                    assert_eq!(Some(v), optimize(&orig), "{w}x{h} q{q}");
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
        // 渐进式（SOF2）、带重启间隔（DRI）的：不处理，原样返回
        let base = encode(&DynamicImage::ImageRgb8(textured(64, 64).1), 90);
        let sof = base.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        let mut prog = base.clone();
        prog[sof + 1] = 0xC2;
        assert!(optimize(&prog).is_none());
        assert_eq!(optimize_verified(prog.clone()), prog);
        let mut dri = base[..2].to_vec();
        dri.extend_from_slice(&[0xFF, 0xDD, 0, 4, 0, 0]); // 重启间隔 0（不重启），解码结果不变
        dri.extend_from_slice(&base[2..]);
        assert_eq!(image::load_from_memory(&dri).unwrap().as_bytes(), image::load_from_memory(&base).unwrap().as_bytes());
        assert!(optimize(&dri).is_none());
        assert_eq!(optimize_verified(dri.clone()), dri);
    }

    /// 符号流核对真能拦下错误的产物：熵编码数据改一位、或量化表改一个字节，都判成不一致。
    #[test]
    fn verification_rejects_tampered_output() {
        let (g, c) = textured(120, 90);
        for img in [DynamicImage::ImageLuma8(g), DynamicImage::ImageRgb8(c)] {
            let orig = encode(&img, 90);
            let old = parse(&orig).unwrap();
            let new = rebuild(&old, orig.len()).unwrap();
            assert!(same_coefficients(&old, &new));
            let sos = new.windows(2).rposition(|w| w == [0xFF, 0xDA]).unwrap();
            let mut bad = new.clone();
            // 改熵编码数据的一位（避开 0xFF 及其填充字节，不造出标记）
            let i = (sos + 40..new.len()).find(|&i| new[i] < 0xFE && new[i - 1] != 0xFF).unwrap();
            bad[i] ^= 1;
            assert!(!same_coefficients(&old, &bad));
            let dqt = new.windows(2).position(|w| w == [0xFF, 0xDB]).unwrap();
            let mut bad = new.clone();
            bad[dqt + 10] ^= 1;
            assert!(!same_coefficients(&old, &bad));
        }
    }

    /// 从像素直接编（[`encode_optimized`]）和"`image` 编码器编一遍 + [`optimize_verified`]"逐字节相同；按通用表写的那份和 `image` 编码器的输出逐字节相同。
    /// `image` 编码器 panic 的（质量 100 时系数超出通用表）这边返回 `None`。
    #[test]
    fn direct_encoder_matches_image_encoder_byte_for_byte() {
        use image::{ExtendedColorType, ImageEncoder};
        let mut s = 99u32;
        let mut rnd = move || {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            (s >> 16) as u8
        };
        let mut cases = 0;
        for (w, h) in [(1, 1), (2, 3), (7, 9), (8, 8), (9, 8), (16, 17), (17, 33), (100, 37), (257, 129), (320, 241)] {
            for kind in 0..4 {
                // 0 平滑渐变、1 带噪纹理、2 纯噪声（高频，逼出大系数）、3 纯色
                let px = |x: u32, y: u32, c: u32, r: u8| -> u8 {
                    match kind {
                        0 => ((x * 3 + y * 2 + c * 40) % 256) as u8,
                        1 => ((x * 7 + y * 3 + c * 11) as u8).wrapping_add(r % 40),
                        2 => r,
                        _ => 200 - c as u8 * 50,
                    }
                };
                let gray: Vec<u8> = (0..w * h).map(|i| px(i % w, i / w, 0, rnd())).collect();
                let rgb: Vec<u8> = (0..w * h * 3).map(|i| px((i / 3) % w, (i / 3) / w, i % 3, rnd())).collect();
                for (raw, ct) in [(&gray, ExtendedColorType::L8), (&rgb, ExtendedColorType::Rgb8)] {
                    for q in [1, 30, 50, 75, 85, 95, 100] {
                        let image_std = std::panic::catch_unwind(|| {
                            let mut b = Vec::new();
                            JpegEncoder::new_with_quality(&mut b, q).write_image(raw, w, h, ct).unwrap();
                            b
                        });
                        let mine = encode_syms(raw, w, h, ct == ExtendedColorType::L8, q);
                        let got = encode_optimized(raw, w, h, ct == ExtendedColorType::L8, q);
                        match image_std {
                            Ok(b) => {
                                let mine = mine.expect("image 能编的这边也能编");
                                assert_eq!(mine.std_bytes(), b, "{w}x{h} kind{kind} {ct:?} q{q}：通用表那份");
                                assert_eq!(mine.std_len(), b.len(), "{w}x{h} kind{kind} {ct:?} q{q}：只数不写的长度");
                                assert!(mine.std_len_lower_bound() <= b.len(), "{w}x{h} kind{kind} {ct:?} q{q}：下限");
                                assert_eq!(got, Some(optimize_verified(b)), "{w}x{h} kind{kind} {ct:?} q{q}：重做过表的");
                                cases += 1;
                            }
                            Err(_) => assert!(got.is_none(), "{w}x{h} kind{kind} {ct:?} q{q}：image 编码器 panic 的这边该是 None"),
                        }
                    }
                }
            }
        }
        assert!(cases > 400, "{cases}");
        // image 编码器报错的尺寸
        assert!(encode_optimized(&[], 0, 0, true, 90).is_none());
        assert!(encode_optimized(&vec![0; 70_000], 70_000, 1, true, 90).is_none());
    }

    /// 量化和 `image` 编码器的 `((x / 8) as f32 / q as f32).round() as i32` 处处相同（系数范围放宽到 ±2 万，所有量化值）。
    #[test]
    fn quantize_matches_float_round() {
        for q in 1..=255u8 {
            let qs = [f32::from(q); 64];
            for start in (-20_000..=20_000i32).step_by(64) {
                let xs: [i32; 64] = std::array::from_fn(|i| start + i as i32);
                let mut got = xs;
                quantize_block(&mut got, &qs);
                for (x, g) in xs.iter().zip(got) {
                    assert_eq!(g, ((x / 8) as f32 / f32::from(q)).round() as i32, "x={x} q={q}");
                }
            }
        }
    }
}

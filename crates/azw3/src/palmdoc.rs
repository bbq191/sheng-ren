//! PalmDOC（LZ77 变体）压缩，每个 4096 字节文本块独立压缩。编码规则（MobileRead MOBI 文档）：
//! - `0x00`、`0x09`–`0x7F`：原样一个字节；
//! - `0x01`–`0x08`：后面跟 1–8 个原样字节（用来放 `0x80`–`0xFF` 等不能直接写的字节）；
//! - `0x80`–`0xBF` 两字节：`0x8000 | 距离 << 3 | (长度 - 3)`，距离 1–2047、长度 3–10，回指前面的数据；
//! - `0xC0`–`0xFF`：空格 + (字节 ^ 0x80)，字节在 `0x40`–`0x7F`。

const MAX_DIST: usize = 2047;
const MIN_LEN: usize = 3;
const MAX_LEN: usize = 10;

/// 每个 3 字节前缀最多回看的候选数（最近的 64 个同前缀位置）。
const MAX_CANDIDATES: usize = 64;
/// 哈希链的桶数（2 的幂）。不同前缀可能落进同一个桶，走链时按前缀本身过滤。
const HASH_BITS: u32 = 12;
const NIL: u32 = u32::MAX;

fn hash3(b: &[u8]) -> usize {
    let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    // 哈希链：head[桶] = 该桶最近插入的位置，prev[位置] = 同桶上一个位置。候选按位置从近到远走，
    // 只数前缀真正相同的，最多 MAX_CANDIDATES 个、不出窗口——与"每个前缀留最近 64 个位置"的做法取到的候选完全相同。
    let mut head = vec![NIL; 1 << HASH_BITS];
    let mut prev = vec![NIL; input.len()];
    let mut i = 0;
    let mut pending: Vec<u8> = Vec::new(); // 待用 0x01–0x08 打包的原样字节
    let flush = |pending: &mut Vec<u8>, out: &mut Vec<u8>| {
        for chunk in pending.chunks(8) {
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
        pending.clear();
    };
    while i < input.len() {
        // 回指：找窗口内最长匹配
        let mut best = (0usize, 0usize); // (长度, 距离)
        if i + MIN_LEN <= input.len() {
            let key = &input[i..i + MIN_LEN];
            let mut p = head[hash3(key)];
            let mut seen = 0;
            while p != NIL && seen < MAX_CANDIDATES {
                let pu = p as usize;
                let dist = i - pu;
                if dist > MAX_DIST {
                    break;
                }
                p = prev[pu];
                if &input[pu..pu + MIN_LEN] != key {
                    continue; // 同桶不同前缀
                }
                seen += 1;
                let mut l = 0;
                while l < MAX_LEN && i + l < input.len() && input[pu + l] == input[i + l] {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, dist);
                    if l == MAX_LEN {
                        break;
                    }
                }
            }
        }
        let advance = if best.0 >= MIN_LEN {
            flush(&mut pending, &mut out);
            let v: u16 = 0x8000 | ((best.1 as u16) << 3) | (best.0 - MIN_LEN) as u16;
            out.extend_from_slice(&v.to_be_bytes());
            best.0
        } else {
            let c = input[i];
            if c == b' ' && i + 1 < input.len() && (0x40..=0x7F).contains(&input[i + 1]) {
                flush(&mut pending, &mut out);
                out.push(input[i + 1] ^ 0x80);
                2
            } else if c == 0 || (0x09..=0x7F).contains(&c) {
                flush(&mut pending, &mut out);
                out.push(c);
                1
            } else {
                pending.push(c);
                1
            }
        };
        for k in i..(i + advance) {
            if k + MIN_LEN <= input.len() {
                let h = hash3(&input[k..k + MIN_LEN]);
                prev[k] = head[h];
                head[h] = k as u32;
            }
        }
        i += advance;
    }
    flush(&mut pending, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let c = compress(data);
        let mut out = Vec::new();
        crate::read::palm::palmdoc_decompress(&c, &mut out);
        assert_eq!(out, data, "压缩后解压必须逐字节一致");
    }

    /// 改哈希链之前的写法（每个前缀一个位置表，留最近 64 个）：新写法的产物必须与它逐字节一致。
    fn compress_reference(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut head: std::collections::HashMap<[u8; 3], Vec<usize>> = std::collections::HashMap::new();
        let mut pending: Vec<u8> = Vec::new();
        let flush = |pending: &mut Vec<u8>, out: &mut Vec<u8>| {
            for chunk in pending.chunks(8) {
                out.push(chunk.len() as u8);
                out.extend_from_slice(chunk);
            }
            pending.clear();
        };
        let mut i = 0;
        while i < input.len() {
            let mut best = (0usize, 0usize);
            if i + MIN_LEN <= input.len() {
                if let Some(cands) = head.get(&[input[i], input[i + 1], input[i + 2]]) {
                    for &p in cands.iter().rev() {
                        let dist = i - p;
                        if dist > MAX_DIST {
                            break;
                        }
                        let mut l = 0;
                        while l < MAX_LEN && i + l < input.len() && input[p + l] == input[i + l] {
                            l += 1;
                        }
                        if l > best.0 {
                            best = (l, dist);
                            if l == MAX_LEN {
                                break;
                            }
                        }
                    }
                }
            }
            let advance = if best.0 >= MIN_LEN {
                flush(&mut pending, &mut out);
                out.extend_from_slice(&(0x8000 | ((best.1 as u16) << 3) | (best.0 - MIN_LEN) as u16).to_be_bytes());
                best.0
            } else if input[i] == b' ' && i + 1 < input.len() && (0x40..=0x7F).contains(&input[i + 1]) {
                flush(&mut pending, &mut out);
                out.push(input[i + 1] ^ 0x80);
                2
            } else if input[i] == 0 || (0x09..=0x7F).contains(&input[i]) {
                flush(&mut pending, &mut out);
                out.push(input[i]);
                1
            } else {
                pending.push(input[i]);
                1
            };
            for k in i..(i + advance) {
                if k + MIN_LEN <= input.len() {
                    let list = head.entry([input[k], input[k + 1], input[k + 2]]).or_default();
                    list.push(k);
                    if list.len() > 64 {
                        list.remove(0);
                    }
                }
            }
            i += advance;
        }
        flush(&mut pending, &mut out);
        out
    }

    #[test]
    fn hash_chain_matches_reference_byte_for_byte() {
        // 伪随机小字母表（大量重复前缀，超过 64 个候选、跨出窗口都会碰到）+ 真实感的 HTML
        let mut x = 0x1234_5678u32;
        let mut noise = Vec::new();
        for _ in 0..4096 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            noise.push(b"ab \xE4\xB8"[(x % 5) as usize]);
        }
        let html = "<p class=\"a\">第一段，重复的中文正文 text。</p>\n".repeat(120);
        for data in [&noise[..], &html.as_bytes()[..4096], &[b'a'; 4096][..], &(0u8..=255).cycle().take(4096).collect::<Vec<_>>()[..]] {
            assert_eq!(compress(data), compress_reference(data));
        }
    }

    #[test]
    fn roundtrips_text_binary_and_utf8() {
        roundtrip(b"");
        roundtrip(b"Hello Hello Hello world, hello world. A B C a b c");
        roundtrip("中文正文，中文正文，重复的中文正文。".as_bytes());
        roundtrip(&(0u8..=255).collect::<Vec<_>>());
        let html = "<p class=\"a\">段落</p>\n".repeat(200);
        roundtrip(&html.as_bytes()[..4096]);
        let c = compress(&html.as_bytes()[..4096]);
        assert!(c.len() < 1500, "重复内容要压得动: {}", c.len());
    }
}

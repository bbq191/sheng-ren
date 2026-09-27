//! PalmDOC（LZ77 变体）压缩，每个 4096 字节文本块独立压缩。编码规则（MobileRead MOBI 文档）：
//! - `0x00`、`0x09`–`0x7F`：原样一个字节；
//! - `0x01`–`0x08`：后面跟 1–8 个原样字节（用来放 `0x80`–`0xFF` 等不能直接写的字节）；
//! - `0x80`–`0xBF` 两字节：`0x8000 | 距离 << 3 | (长度 - 3)`，距离 1–2047、长度 3–10，回指前面的数据；
//! - `0xC0`–`0xFF`：空格 + (字节 ^ 0x80)，字节在 `0x40`–`0x7F`。

const MAX_DIST: usize = 2047;
const MIN_LEN: usize = 3;
const MAX_LEN: usize = 10;

pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    // 3 字节前缀 → 最近出现过的位置（链表，只看窗口内）
    let mut head: std::collections::HashMap<[u8; 3], Vec<usize>> = std::collections::HashMap::new();
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
            let key = [input[i], input[i + 1], input[i + 2]];
            if let Some(cands) = head.get(&key) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let c = compress(data);
        let mut out = Vec::new();
        bookconv::convert::palm::palmdoc_decompress(&c, &mut out);
        assert_eq!(out, data, "压缩后解压必须逐字节一致");
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

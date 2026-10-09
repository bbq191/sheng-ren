//! StarDict 2.4.2 写出（依 StarDict 的 doc/StarDictFileFormat）：
//! - `.dict`：释义内容首尾相接；
//! - `.idx`：每个词 = 词头 UTF-8 + `\0` + 释义在 .dict 里的偏移（4 字节大端）+ 长度（4 字节大端），
//!   按 [`stardict_cmp`] 排好序（查词靠二分，顺序错了会查不到）；
//! - `.ifo`：文本头，`sametypesequence=h`（释义全是 HTML）。

use std::cmp::Ordering;

/// 三个文件的内容。
pub struct Files {
    pub ifo: String,
    pub idx: Vec<u8>,
    pub dict: Vec<u8>,
}

/// 释义在 .dict 里的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Article {
    offset: u32,
    size: u32,
}

#[derive(Default)]
pub struct Writer {
    dict: Vec<u8>,
    words: Vec<(String, Article)>,
}

impl Writer {
    /// 追加一段释义，返回它的位置（多个词头可以共用）。.idx 里偏移、长度都是 4 字节，
    /// 追加后 .dict 超过 4 GiB（`u32::MAX` 字节）就报错——以前 `as u32` 会悄悄截断，偏移指到错的释义。
    pub fn add_article(&mut self, content: &[u8]) -> Result<Article, String> {
        let end = self.dict.len() as u64 + content.len() as u64;
        Self::check_fits(end)?;
        // end 装得下 u32，起点和长度都不大于它
        let a = Article { offset: self.dict.len() as u32, size: content.len() as u32 };
        self.dict.extend_from_slice(content);
        Ok(a)
    }

    fn check_fits(end: u64) -> Result<(), String> {
        u32::try_from(end).map(|_| ()).map_err(|_| format!(".dict 超过 StarDict 的 4 GiB 上限（{end} 字节），.idx 的 4 字节偏移装不下"))
    }

    /// 加一个词头。词头里的 `\0` 去掉再存：.idx 用 `\0` 当词头结尾，留着会让后面的偏移、长度错位、整个索引读歪。
    /// 选去掉而不是跳过整条：去掉控制字符后词头仍能查到释义（MOBI 词头里出现 `\0` 只会是脏数据）。去掉后为空的不加。
    pub fn add_word(&mut self, word: &str, article: Article) {
        let word = if word.contains('\0') { word.replace('\0', "") } else { word.to_string() };
        if !word.is_empty() {
            self.words.push((word, article));
        }
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    pub fn finish(mut self, bookname: &str, description: &str) -> Files {
        self.words.sort_by(|a, b| stardict_cmp(&a.0, &b.0).then(a.1.offset.cmp(&b.1.offset)));
        self.words.dedup();
        let mut idx = Vec::new();
        for (w, a) in &self.words {
            idx.extend_from_slice(w.as_bytes());
            idx.push(0);
            idx.extend_from_slice(&a.offset.to_be_bytes());
            idx.extend_from_slice(&a.size.to_be_bytes());
        }
        let one_line = |s: &str| s.replace(['\n', '\r'], " ");
        let ifo = format!(
            "StarDict's dict ifo file\nversion=2.4.2\nbookname={}\nwordcount={}\nidxfilesize={}\nsametypesequence=h\ndescription={}\n",
            one_line(bookname),
            self.words.len(),
            idx.len(),
            one_line(description)
        );
        Files { ifo, idx, dict: self.dict }
    }
}

/// StarDict 的词头顺序：先按 ASCII 字母不分大小写逐字节比（`g_ascii_strcasecmp`），相同再按原字节比（`strcmp`）。
pub fn stardict_cmp(a: &str, b: &str) -> Ordering {
    let fold = |s: &str| s.bytes().map(|c| c.to_ascii_lowercase()).collect::<Vec<u8>>();
    fold(a).cmp(&fold(b)).then_with(|| a.as_bytes().cmp(b.as_bytes()))
}

/// 读回 .idx（测试和自检用）：(词头, 偏移, 长度)。
pub fn parse_idx(idx: &[u8]) -> Result<Vec<(String, u32, u32)>, String> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < idx.len() {
        let end = idx[p..].iter().position(|&b| b == 0).ok_or("词头没有结尾的 \\0")? + p;
        let word = std::str::from_utf8(&idx[p..end]).map_err(|_| "词头不是 UTF-8")?.to_string();
        let nums = idx.get(end + 1..end + 9).ok_or(".idx 截断")?;
        out.push((word, u32::from_be_bytes([nums[0], nums[1], nums[2], nums[3]]), u32::from_be_bytes([nums[4], nums[5], nums[6], nums[7]])));
        p = end + 9;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_ascii_case_insensitive_then_bytewise() {
        let mut v = vec!["b", "B", "a", "Ab", "aa", "啊", "abc"];
        v.sort_by(|a, b| stardict_cmp(a, b));
        assert_eq!(v, vec!["a", "aa", "Ab", "abc", "B", "b", "啊"]);
    }

    #[test]
    fn writer_roundtrips_and_ifo_matches_idx() {
        let mut w = Writer::default();
        let x = w.add_article(b"<b>x</b>").unwrap();
        let y = w.add_article(b"yy").unwrap();
        w.add_word("zeta", x);
        w.add_word("Alpha", y);
        w.add_word("alpha", x);
        let f = w.finish("测试\n词典", "说明");
        assert_eq!(parse_idx(&f.idx).unwrap(), vec![("Alpha".into(), 8, 2), ("alpha".into(), 0, 8), ("zeta".into(), 0, 8)]);
        assert!(f.ifo.starts_with("StarDict's dict ifo file\nversion=2.4.2\nbookname=测试 词典\nwordcount=3\n"));
        assert!(f.ifo.contains(&format!("idxfilesize={}\n", f.idx.len())));
        assert_eq!(f.dict, b"<b>x</b>yy");
    }

    #[test]
    fn nul_in_headword_is_removed_so_idx_stays_aligned() {
        let mut w = Writer::default();
        let a = w.add_article(b"x").unwrap();
        w.add_word("a\0b", a);
        w.add_word("\0", a);
        w.add_word("c", a);
        let f = w.finish("t", "d");
        assert_eq!(parse_idx(&f.idx).unwrap(), vec![("ab".into(), 0, 1), ("c".into(), 0, 1)]);
        assert!(f.ifo.contains("wordcount=2\n"));
    }

    #[test]
    fn dict_over_4gib_is_an_error_not_truncated() {
        assert!(Writer::check_fits(u32::MAX as u64).is_ok());
        assert!(Writer::check_fits(u32::MAX as u64 + 1).unwrap_err().contains("4 GiB"));
    }
}

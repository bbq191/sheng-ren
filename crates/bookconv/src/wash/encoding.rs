//! 不是 UTF-8 的 XHTML、OPF、NCX 先转成 UTF-8（2026-10-09 审计）：后面各步一律按 UTF-8 读（不少地方 `from_utf8_lossy` 读了再写回），
//! 不先转的话 GBK、Big5、UTF-16 的文件会被替换字符改坏。按 BOM、XML 声明的 `encoding`、`<meta charset>` 定编码，字一个不改，
//! 声明一起改成 UTF-8（EPUB 3 本来就要求 UTF-8）。认不出编码、或按声明的编码解出错的不动——拿不准就不处理。
//! 样式表不在这里：CSS 的编码另有 `@charset`，非 UTF-8 的按单字节处理（`util::latin1_decode`）。

use super::{Entry, WashReport};
use crate::epubzip::is_html_entry;
use regex::Regex;
use std::sync::OnceLock;

fn is_xml_doc(e: &Entry) -> bool {
    let n = e.name.to_ascii_lowercase();
    n.ends_with(".opf") || n.ends_with(".ncx") || is_html_entry(&e.name, &e.data)
}

/// 文件开头声明的编码：XML 声明的 `encoding`，没有就找 `<meta charset>`、`<meta http-equiv content="…charset=…">`。
/// 只看开头 1024 字节、按 ASCII 找（UTF-16 的由 BOM 认，不走这里）。
fn declared(data: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    static RE: OnceLock<regex::bytes::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::bytes::Regex::new(r#"(?i)<\?xml[^>]*?\bencoding\s*=\s*["']([A-Za-z0-9._:-]+)|<meta\b[^>]*?\bcharset\s*=\s*["']?([A-Za-z0-9._:-]+)"#).unwrap());
    let c = re.captures(&data[..data.len().min(1024)])?;
    let label = c.get(1).or_else(|| c.get(2))?;
    encoding_rs::Encoding::for_label(label.as_bytes())
}

/// 解码后的文字里，把声明的编码改成 UTF-8（XML 声明和 meta 里的都改）。
fn declare_utf8(text: &str) -> String {
    static XML: OnceLock<Regex> = OnceLock::new();
    static META: OnceLock<Regex> = OnceLock::new();
    let xml = XML.get_or_init(|| Regex::new(r#"(?i)(<\?xml[^>]*?\bencoding\s*=\s*["'])[A-Za-z0-9._:-]+"#).unwrap());
    let meta = META.get_or_init(|| Regex::new(r#"(?i)(<meta\b[^>]*?\bcharset\s*=\s*["']?)[A-Za-z0-9._:-]+"#).unwrap());
    let t = xml.replace(text, "${1}UTF-8");
    meta.replace(&t, "${1}UTF-8").into_owned()
}

/// 一个文件转成 UTF-8；已经是、认不出编码、解码出错时 `None`（原样不动）。
fn to_utf8(data: &[u8]) -> Option<String> {
    if let Ok(t) = std::str::from_utf8(data) {
        // 内容已经是 UTF-8、声明却写着别的（`gb2312`……）：按声明解码的阅读器会显示乱码，只改声明（2026-10-09 审计）。
        // 全是 ASCII 的两种读法一样，不动
        let wrong = !t.is_ascii() && declared(data).is_some_and(|e| e != encoding_rs::UTF_8);
        return wrong.then(|| declare_utf8(t));
    }
    let (enc, body) = match encoding_rs::Encoding::for_bom(data) {
        Some((enc, bom)) => (enc, &data[bom..]),
        None => (declared(data)?, data),
    };
    if enc == encoding_rs::UTF_8 {
        return None; // 声明是 UTF-8 却不是合法 UTF-8：坏文件，不猜
    }
    let text = enc.decode_without_bom_handling_and_without_replacement(body)?;
    Some(declare_utf8(&text))
}

/// 全书的 XHTML、OPF、NCX 里不是 UTF-8 的转成 UTF-8，记进报告。
pub(super) fn transcode_to_utf8(entries: &mut [Entry], rep: &mut WashReport) {
    rep.transcoded_to_utf8 += transcode_entries(entries);
}

/// 同 [`transcode_to_utf8`]，返回转了几个。优化器在清洗层之前就要按 UTF-8 改 OPF（补封面声明、封面页），所以读完书先转一次
/// （以前 GBK 的 OPF 先被 `from_utf8_lossy` 读成替换字符再写回，书名作者全坏；2026-10-09 审计）。
pub fn transcode_entries(entries: &mut [Entry]) -> usize {
    let mut n = 0;
    for e in entries.iter_mut().filter(|e| is_xml_doc(e)) {
        if let Some(t) = to_utf8(&e.data) {
            e.data = t.into_bytes();
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gbk_big5_utf16_become_utf8_with_declaration_updated() {
        let (gbk, _, _) = encoding_rs::GBK.encode(r#"<?xml version="1.0" encoding="gbk"?><html><head><meta charset="gbk"/></head><body><p>中文正文</p></body></html>"#);
        assert_eq!(to_utf8(&gbk).as_deref(), Some(r#"<?xml version="1.0" encoding="UTF-8"?><html><head><meta charset="UTF-8"/></head><body><p>中文正文</p></body></html>"#));
        let (big5, _, _) = encoding_rs::BIG5.encode(r#"<html><head><meta http-equiv="Content-Type" content="text/html; charset=big5"/></head><body>繁體</body></html>"#);
        assert_eq!(to_utf8(&big5).as_deref(), Some(r#"<html><head><meta http-equiv="Content-Type" content="text/html; charset=UTF-8"/></head><body>繁體</body></html>"#));
        let mut u16 = vec![0xFF, 0xFE];
        for c in "<?xml version=\"1.0\" encoding=\"UTF-16\"?><package>书</package>".encode_utf16() {
            u16.extend(c.to_le_bytes());
        }
        assert_eq!(to_utf8(&u16).as_deref(), Some("<?xml version=\"1.0\" encoding=\"UTF-8\"?><package>书</package>"));
    }

    #[test]
    fn utf8_content_declared_gbk_gets_declaration_fixed() {
        let t = r#"<?xml version="1.0" encoding="gb2312"?><html><body><p>中文</p></body></html>"#;
        assert_eq!(to_utf8(t.as_bytes()).as_deref(), Some(r#"<?xml version="1.0" encoding="UTF-8"?><html><body><p>中文</p></body></html>"#));
        assert_eq!(to_utf8(br#"<?xml version="1.0" encoding="gb2312"?><p>abc</p>"#), None, "全 ASCII 不动");
    }

    #[test]
    fn utf8_unknown_or_malformed_left_alone() {
        assert_eq!(to_utf8("<p>已经是 UTF-8</p>".as_bytes()), None);
        assert_eq!(to_utf8(b"<p>\xff\xfe\xfd no declaration</p>"), None, "认不出编码不猜");
        assert_eq!(to_utf8(b"<?xml version=\"1.0\" encoding=\"utf-8\"?><p>\xff</p>"), None, "声明 UTF-8 却不合法：不猜");
        assert_eq!(to_utf8(b"<?xml version=\"1.0\" encoding=\"shift_jis\"?><p>\x82</p>"), None, "按声明的编码解出错：不动");
    }
}

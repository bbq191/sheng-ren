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
    if std::str::from_utf8(data).is_ok() {
        return None;
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

/// 全书的 XHTML、OPF、NCX 里不是 UTF-8 的转成 UTF-8；返回转了几个。
pub(super) fn transcode_to_utf8(entries: &mut [Entry], rep: &mut WashReport) {
    for e in entries.iter_mut().filter(|e| is_xml_doc(e)) {
        if let Some(t) = to_utf8(&e.data) {
            e.data = t.into_bytes();
            rep.transcoded_to_utf8 += 1;
        }
    }
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
    fn utf8_unknown_or_malformed_left_alone() {
        assert_eq!(to_utf8("<p>已经是 UTF-8</p>".as_bytes()), None);
        assert_eq!(to_utf8(b"<p>\xff\xfe\xfd no declaration</p>"), None, "认不出编码不猜");
        assert_eq!(to_utf8(b"<?xml version=\"1.0\" encoding=\"utf-8\"?><p>\xff</p>"), None, "声明 UTF-8 却不合法：不猜");
        assert_eq!(to_utf8(b"<?xml version=\"1.0\" encoding=\"shift_jis\"?><p>\x82</p>"), None, "按声明的编码解出错：不动");
    }
}

//! 各种源格式 → **与设备无关的母版**（入库用）。`booklib`（`crates/library`）入库时调这里：
//! MOBI6（.mobi/.prc/.azw）、AZW3（KF8，clean-room 自研，见 `kf8`）、FB2 → EPUB，CBZ → 每页一张原图的 EPUB
//! （`cbz::cbz_to_epub`）。按设备的优化是入库之后的独立步骤，这里不做。
//!
//! 另有 `cbz::cbz_to_pdf`（`cbz2pdf` 命令用，按设备出 PDF）和 `pdfwrite`（手写 PDF，漫画 PDF / 入库 PDF 裁边共用）。
//! 不支持：HUFF/CDIC 压缩的 AZW3、带 DRM 的文件（`precheck` 直接拒）；CBR（RAR 解包要 C++ 依赖和受限许可，先用外部工具解成 CBZ）。

pub mod cbz;
pub mod common;
pub mod fb2;
pub mod kf8;
pub mod mobi;
pub mod palm;
pub mod pdfwrite;

/// 不用转换、原样作母版的格式。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ContentType {
    Epub,
    Pdf,
}

/// 转换产物：母版 EPUB 字节 + 建议书名（不含扩展名）。
pub struct Converted {
    pub data: Vec<u8>,
    pub title: String,
}

/// 旧的"墨水屏色调档"参数，已无作用（CBZ→PDF 的色调现在由设备 profile 的 `color` 决定，见 `cbz::cbz_to_pdf`）。
/// 只为 [`convert_file`] 的调用方签名不变而保留，调用方改掉后删除。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum EinkTone {
    #[default]
    Off,
}

/// 该文件扩展名是否为「可转换的源格式」。.mobi/.prc/.azw = 旧 MOBI6；.azw3 = KF8（见 kf8）。
pub fn is_convertible(filename: &str) -> bool {
    let l = filename.to_ascii_lowercase();
    l.ends_with(".cbz")
        || l.ends_with(".fb2")
        || l.ends_with(".mobi")
        || l.ends_with(".prc")
        || l.ends_with(".azw3")
        || l.ends_with(".azw")
}

/// EPUB/PDF 不转换，原样作母版。返回其类型。
pub fn direct_content_type(filename: &str) -> Option<ContentType> {
    let l = filename.to_ascii_lowercase();
    if l.ends_with(".epub") {
        Some(ContentType::Epub)
    } else if l.ends_with(".pdf") {
        Some(ContentType::Pdf)
    } else {
        None
    }
}

/// **廉价预检**（不做完整转换/解压）：判断该文件能否转换，能则 `Ok`，否则给可读原因。
/// 入库时**先验后转**——不支持的（DRM / HUFF-CDIC / 未知）立即报错，不白跑重活。
/// MOBI/AZW3 只解 PalmDB+record0 头（含 DRM/压缩类型），秒级；epub/pdf/cbz/fb2 结构无法廉价预判则放行。
pub fn precheck(filename: &str, data: &[u8]) -> Result<(), String> {
    let l = filename.to_ascii_lowercase();
    if direct_content_type(&l).is_some() {
        return Ok(()); // epub/pdf 原样作母版
    }
    if l.ends_with(".mobi") || l.ends_with(".prc") || l.ends_with(".azw") || l.ends_with(".azw3") {
        // 只解容器头即可判 DRM / HUFF-CDIC（parse_header 早失败，不触发解压）
        let records = palm::parse_palmdb(data)?;
        if records.is_empty() {
            return Err("空文件或非 PalmDB".into());
        }
        palm::parse_header(records[0]).map(|_| ())
    } else if is_convertible(&l) {
        Ok(()) // cbz/fb2 无法廉价预判，交由转换阶段
    } else {
        Err("不支持的格式（支持 EPUB/PDF 原样入库，AZW3/MOBI/FB2/CBZ 转换）".into())
    }
}

/// 按扩展名分派转换，产出与设备无关的母版 EPUB。返回 `None` = 非可转换源；`Some(Err)` = 是源但转换失败。
/// `_tone`、`_screen` 已无作用（母版不按设备处理），保留只为调用方签名不变。
pub fn convert_file(filename: &str, data: &[u8], _tone: EinkTone, _screen: crate::imgopt::Screen) -> Option<Result<Converted, String>> {
    let l = filename.to_ascii_lowercase();
    let title = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("book")
        .to_string();
    // 书自带的书名（FB2 `<book-title>`、MOBI/AZW3 EXTH）比文件名准；取不到时退回文件名。
    let with_title = |r: Result<(Vec<u8>, String), String>| {
        r.map(|(data, book_title)| Converted { data, title: if book_title.trim().is_empty() { title.clone() } else { book_title } })
    };
    if l.ends_with(".cbz") {
        Some(cbz::cbz_to_epub(data, &title).map(|data| Converted { data, title: title.clone() }))
    } else if l.ends_with(".fb2") {
        Some(with_title(fb2::fb2_to_epub(data)))
    } else if l.ends_with(".mobi") || l.ends_with(".prc") || l.ends_with(".azw") {
        // .azw（初代 Kindle）≈ 旧 MOBI6；.mobi/.prc 同。
        Some(with_title(mobi::mobi_to_epub(data)))
    } else if l.ends_with(".azw3") {
        // AZW3 = KF8（现代 Kindle）。
        Some(with_title(kf8::azw3_to_epub(data)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_and_convertible_classification() {
        assert_eq!(direct_content_type("a.epub"), Some(ContentType::Epub));
        assert_eq!(direct_content_type("A.PDF"), Some(ContentType::Pdf));
        assert_eq!(direct_content_type("a.azw3"), None);
        assert!(is_convertible("x.azw3") && is_convertible("x.cbz") && is_convertible("x.FB2"));
        assert!(!is_convertible("x.txt") && !is_convertible("x.epub"));
    }

    #[test]
    fn precheck_rejects_unknown_and_accepts_direct() {
        assert!(precheck("x.epub", b"anything").is_ok()); // 原样入库，不预判内容
        assert!(precheck("x.pdf", b"%PDF").is_ok());
        assert!(precheck("x.txt", b"...").is_err()); // 未知格式
        assert!(precheck("x.mobi", b"tooshort").is_err()); // 坏 PalmDB 早失败
    }
}

//! 源格式 → **与设备无关的母版 EPUB**（入库用）。现在只支持 CBZ → 每页一张原图的 EPUB（`cbz::cbz_to_epub`）；
//! EPUB 本身不转换。按阅读模式的优化是入库之后的独立步骤，这里不做。
//! 不支持：CBR（RAR 解包要 C++ 依赖和受限许可，先用外部工具解成 CBZ）；其它格式（MOBI/AZW3/FB2/PDF 等）2026-09-29 起不再支持。

/// 格式转换（CBZ → EPUB）的版本：改了会影响转换结果的代码要加一。书库只把它放进**需要转换的来源**（CBZ）的指纹，
/// 原本就是 EPUB 的书不受影响。
/// - 1（2026-09-28）：当时还覆盖 MOBI/AZW3/FB2/PDF 的转换；那些格式 2026-09-29 删掉，CBZ 的转换结果没有变，版本号不动。
pub const CONVERT_VERSION: &str = "1";

pub mod cbz;
pub mod common;
/// KF8（AZW3）读取器：**只给 AZW3 写出器（`azw3` crate）回读自检和测试用**，不是输入格式（入库只收 EPUB、CBZ）。
pub mod kf8;
/// MOBI/PalmDB 容器的公共部分（`kf8` 用）。
pub mod palm;

/// 不用转换、原样作母版的格式。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ContentType {
    Epub,
}

/// 转换产物：母版 EPUB 字节 + 建议书名（不含扩展名）。
pub struct Converted {
    pub data: Vec<u8>,
    pub title: String,
}

/// 该文件扩展名是否为「可转换的源格式」（现在只有 CBZ）。
pub fn is_convertible(filename: &str) -> bool {
    filename.to_ascii_lowercase().ends_with(".cbz")
}

/// EPUB 不转换，原样作母版。返回其类型。
pub fn direct_content_type(filename: &str) -> Option<ContentType> {
    filename.to_ascii_lowercase().ends_with(".epub").then_some(ContentType::Epub)
}

/// **廉价预检**（不做完整转换/解压）：只看扩展名，EPUB、CBZ 放行（结构交给读取/转换阶段），其余拒绝。
pub fn precheck(filename: &str, _data: &[u8]) -> Result<(), String> {
    if direct_content_type(filename).is_some() || is_convertible(filename) {
        Ok(())
    } else {
        Err("只支持 EPUB 和 CBZ".into())
    }
}

/// 按扩展名分派转换，产出与设备无关的母版 EPUB。返回 `None` = 非可转换源；`Some(Err)` = 是源但转换失败。
pub fn convert_file(filename: &str, data: &[u8]) -> Option<Result<Converted, String>> {
    let title = std::path::Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("book").to_string();
    if is_convertible(filename) {
        Some(cbz::cbz_to_epub(data, &title).map(|data| Converted { data, title }))
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
        assert_eq!(direct_content_type("A.EPUB"), Some(ContentType::Epub));
        assert_eq!(direct_content_type("a.pdf"), None);
        assert!(is_convertible("x.cbz") && is_convertible("x.CBZ"));
        assert!(!is_convertible("x.azw3") && !is_convertible("x.fb2") && !is_convertible("x.epub"));
    }

    #[test]
    fn precheck_accepts_only_epub_and_cbz() {
        assert!(precheck("x.epub", b"anything").is_ok());
        assert!(precheck("x.cbz", b"anything").is_ok());
        for other in ["x.pdf", "x.mobi", "x.azw3", "x.fb2", "x.txt"] {
            assert_eq!(precheck(other, b"...").unwrap_err(), "只支持 EPUB 和 CBZ", "{other}");
        }
    }
}

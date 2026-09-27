//! convert 各转换器共用的小工具——集中一处，避免 fb2/mobi/kf8/cbz 各写一遍漂移。

use crate::epub::{self, Book};

/// 图片魔数 → (扩展名, MIME)。非已知图片返回 None。
pub fn image_ext_mime(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.len() >= 3 && b[0] == 0xFF && b[1] == 0xD8 && b[2] == 0xFF {
        Some(("jpg", "image/jpeg"))
    } else if b.len() >= 8 && b[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        Some(("png", "image/png"))
    } else if b.len() >= 6 && (&b[..6] == b"GIF87a" || &b[..6] == b"GIF89a") {
        Some(("gif", "image/gif"))
    } else {
        None
    }
}

/// 资源/书名 id 安全化：非字母数字/`.`/`-`/`_` 一律换成下划线。
pub fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// 组装成**与设备无关的母版 EPUB**——转换器统一收尾路。按设备的优化（图片缩放、字体解锁等）在入库之后
/// 按 profile 另做，不在转换时写死某台设备。
pub fn assemble_master(book: &mut Book) -> Result<Vec<u8>, String> {
    epub::assemble(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_magic() {
        assert_eq!(image_ext_mime(&[0xFF, 0xD8, 0xFF, 0]), Some(("jpg", "image/jpeg")));
        assert_eq!(image_ext_mime(b"GIF89a...."), Some(("gif", "image/gif")));
        assert_eq!(image_ext_mime(b"not an image"), None);
    }

    #[test]
    fn sanitize_replaces_non_ascii_word() {
        assert_eq!(sanitize_id("a/b 书.c"), "a_b__.c");
    }
}

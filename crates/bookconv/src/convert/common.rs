//! convert 共用的小工具（图片格式识别、id 安全化）。

/// 图片魔数 → (扩展名, MIME)，只认 JPEG/PNG/GIF；非已知图片返回 None。识别本身是 [`crate::util::image_kind`]，这里是给
/// 封面、网络图、KF8 资源这些只收这三种的调用方留的薄包装。
pub fn image_ext_mime(b: &[u8]) -> Option<(&'static str, &'static str)> {
    crate::util::image_kind(b).filter(|k| k.format != image::ImageFormat::WebP).map(|k| (k.ext, k.mime))
}

/// WebP（RIFF 容器、格式标记 `WEBP`）。不放进 [`image_ext_mime`]：那里是各处都认的 JPEG/PNG/GIF（封面、网络图、KF8 资源），
/// WebP 只在 CBZ 页面（之后由优化器转成 JPEG/PNG）和写 AZW3 时（转成 PNG）另外认。
pub fn is_webp(b: &[u8]) -> bool {
    crate::util::image_kind(b).is_some_and(|k| k.format == image::ImageFormat::WebP)
}

/// 资源/书名 id 安全化：非字母数字/`.`/`-`/`_` 一律换成下划线。
pub fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_magic() {
        assert_eq!(image_ext_mime(&[0xFF, 0xD8, 0xFF, 0]), Some(("jpg", "image/jpeg")));
        assert_eq!(image_ext_mime(b"GIF89a...."), Some(("gif", "image/gif")));
        assert_eq!(image_ext_mime(b"not an image"), None);
        assert!(is_webp(b"RIFF\x10\0\0\0WEBPVP8L") && image_ext_mime(b"RIFF\x10\0\0\0WEBPVP8L").is_none());
        assert!(!is_webp(b"RIFF\x10\0\0\0WAVEfmt "));
        assert!(!is_webp(b"RIFF\x10\0\0\0WEB"), "太短");
        assert_eq!(image_ext_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), Some(("png", "image/png")));
        assert_eq!(image_ext_mime(&[0xFF, 0xD8]), None, "JPEG 魔数要三个字节");
    }

    #[test]
    fn sanitize_replaces_non_ascii_word() {
        assert_eq!(sanitize_id("a/b 书.c"), "a_b__.c");
    }
}

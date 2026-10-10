//! 内容层的错误类型 [`BookError`]：调用方按变体分清取消、超上限、文件损坏、IO 失败、DRM，不再看错误串开头。
//!
//! 给用户看的文字（`Display`）和改成枚举以前的错误串逐字相同；还在用 `String` 往上传的代码照旧 `?`
//! （[`From<BookError> for String`](BookError)），返回 `String` 的老函数在新函数里 `?` 进来成 [`BookError::Other`]。
use std::fmt;

/// 取消优化时的错误文字（[`BookError::Cancelled`] 的 `Display`）。
pub const CANCELLED_MSG: &str = "已取消";

/// 内容层的错误。`Display` 就是给用户看的中文错误文字。
#[derive(Debug)]
pub enum BookError {
    /// 调用方要求中途取消（`optimize_epub_file_streaming_with_cancel` 的 `cancel()` 返回真）。
    Cancelled,
    /// 文件系统读写失败：`context` 说明在做什么（空串＝只显示底层错误）。
    Io { context: String, source: std::io::Error },
    /// zip 层报的错（打不开、条目损坏、读写失败）：`context` 同上。zip 库的错误自带「i/o error:」等前缀，照原样显示。
    Zip { context: String, source: zip::result::ZipError },
    /// 文件内容不对（缺 OPF、缺条目、spine 里没有文档……），整句文字。
    Corrupt(String),
    /// 单个 zip 条目解压后超过上限（`epubzip::MAX_ENTRY_BYTES`，损坏或恶意的压缩包）。
    LimitExceeded { name: String, cap_mb: u64 },
    /// 不支持的输入，整句文字。
    Unsupported(String),
    /// 加密了正文的 EPUB（真 DRM），整句文字。
    Drm(String),
    /// 其它（多是还返回 `String` 的老函数报的错），整句文字。
    Other(String),
}

impl BookError {
    /// 是不是调用方要求的取消。
    pub fn is_cancelled(&self) -> bool {
        matches!(self, BookError::Cancelled)
    }

    /// 在前面加一段说明，显示成 `{ctx}: {原文字}`（和以前 `format!("{ctx}: {e}")` 一样），变体不变。
    /// 取消没有可加说明的地方，加了就成 [`BookError::Other`]（文字照样是 `{ctx}: 已取消`）。
    pub fn context(self, ctx: impl fmt::Display) -> Self {
        let join = |s: String| if s.is_empty() { ctx.to_string() } else { format!("{ctx}: {s}") };
        match self {
            BookError::Cancelled => BookError::Other(format!("{ctx}: {CANCELLED_MSG}")),
            BookError::Io { context, source } => BookError::Io { context: join(context), source },
            BookError::Zip { context, source } => BookError::Zip { context: join(context), source },
            BookError::Corrupt(s) => BookError::Corrupt(format!("{ctx}: {s}")),
            BookError::LimitExceeded { name, cap_mb } => BookError::LimitExceeded { name: format!("{ctx}: {name}"), cap_mb },
            BookError::Unsupported(s) => BookError::Unsupported(format!("{ctx}: {s}")),
            BookError::Drm(s) => BookError::Drm(format!("{ctx}: {s}")),
            BookError::Other(s) => BookError::Other(format!("{ctx}: {s}")),
        }
    }

    /// 带说明的 IO 错误。
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        BookError::Io { context: context.into(), source }
    }

    /// 带说明的 zip 错误。
    pub fn zip(context: impl Into<String>, source: zip::result::ZipError) -> Self {
        BookError::Zip { context: context.into(), source }
    }
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookError::Cancelled => f.write_str(CANCELLED_MSG),
            BookError::Io { context, source } if context.is_empty() => write!(f, "{source}"),
            BookError::Io { context, source } => write!(f, "{context}: {source}"),
            BookError::Zip { context, source } if context.is_empty() => write!(f, "{source}"),
            BookError::Zip { context, source } => write!(f, "{context}: {source}"),
            BookError::LimitExceeded { name, cap_mb } => write!(f, "{name}: 解压后超过单个条目上限 {cap_mb} MB（损坏或恶意的压缩包？）"),
            BookError::Corrupt(s) | BookError::Unsupported(s) | BookError::Drm(s) | BookError::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for BookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BookError::Io { source, .. } => Some(source),
            BookError::Zip { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BookError {
    fn from(e: std::io::Error) -> Self {
        BookError::io("", e)
    }
}

impl From<zip::result::ZipError> for BookError {
    fn from(e: zip::result::ZipError) -> Self {
        BookError::zip("", e)
    }
}

/// 还返回 `String` 的老函数报的错。
impl From<String> for BookError {
    fn from(s: String) -> Self {
        BookError::Other(s)
    }
}

/// 还用 `String` 往上传错误的调用方照旧 `?`：文字就是 `Display`。
impl From<BookError> for String {
    fn from(e: BookError) -> Self {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 给用户看的文字和改成枚举以前的错误串逐字相同。
    #[test]
    fn display_matches_old_messages() {
        assert_eq!(BookError::Cancelled.to_string(), "已取消");
        let nf = || std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory (os error 2)");
        assert_eq!(BookError::io("打开输入失败", nf()).to_string(), "打开输入失败: No such file or directory (os error 2)");
        assert_eq!(BookError::from(nf()).to_string(), "No such file or directory (os error 2)");
        assert_eq!(BookError::zip("解 EPUB(非 zip?)", zip::result::ZipError::InvalidArchive("Could not find EOCD")).to_string(), "解 EPUB(非 zip?): invalid Zip archive: Could not find EOCD");
        assert_eq!(BookError::from(zip::result::ZipError::FileNotFound).to_string(), "specified file not found in archive");
        assert_eq!(BookError::LimitExceeded { name: "a.xhtml".into(), cap_mb: 256 }.to_string(), "a.xhtml: 解压后超过单个条目上限 256 MB（损坏或恶意的压缩包？）");
        assert_eq!(BookError::Corrupt("找不到 OPF".into()).to_string(), "找不到 OPF");
    }

    /// 加说明：文字同 `format!("{ctx}: {e}")`，变体不变。
    #[test]
    fn context_prefixes_text_and_keeps_variant() {
        let e = BookError::LimitExceeded { name: "i/p.jpg".into(), cap_mb: 256 }.context("重读图片失败");
        assert_eq!(e.to_string(), "重读图片失败: i/p.jpg: 解压后超过单个条目上限 256 MB（损坏或恶意的压缩包？）");
        assert!(matches!(e, BookError::LimitExceeded { .. }));
        let e = BookError::zip("i/p.jpg", zip::result::ZipError::FileNotFound).context("重读图片失败");
        assert_eq!(e.to_string(), "重读图片失败: i/p.jpg: specified file not found in archive");
        let e = BookError::from(zip::result::ZipError::FileNotFound).context("x");
        assert_eq!(e.to_string(), "x: specified file not found in archive");
        assert_eq!(BookError::Corrupt("zip 里没有条目 a".into()).context("重读图片失败").to_string(), "重读图片失败: zip 里没有条目 a");
    }
}

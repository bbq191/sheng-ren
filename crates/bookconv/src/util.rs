//! reading 线通用小工具——集中一处，消除各处重复（XML 转义、书名→文件名安全化）。
//! HTTP Agent 见 `crate::netimg::http_agent`。

/// 现在的 UTC 时间，写成 `2026-09-30T08:05:09Z`（EPUB 3 的 `dcterms:modified` 格式）。
pub fn utc_now_w3c() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    utc_w3c(secs)
}

/// Unix 秒数 → `YYYY-MM-DDThh:mm:ssZ`（公历换算按 Howard Hinnant 的 civil_from_days）。
pub fn utc_w3c(secs: u64) -> String {
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// XML/XHTML 文本与属性通用转义：`& < > "`（转义 `"` 对文本无害、对属性必需，故一个函数通吃）。
/// epub 章节组装、稍后读正文、来源脚注等全共用，替代原先散落的 `xesc`/`xml_escape`。
/// 顺带丢弃 XML 1.0 不允许出现的字符（见 [`is_xml_char`]）——转义救不了它们，留着整份文档就不是合法
/// XML：2026-09-23 真机《T.E.双语》PDF 标题是 UTF-16BE，被当 UTF-8 解出一串 `\0`，写进 OPF 的
/// `dc:title` 后 xochitl 解析 OPF 失败、整本只渲染出 1 页。
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        push_xml_escaped(&mut out, c);
    }
    out
}

/// 单个字符按 [`xml_escape`] 的规则追加进 `out`（逐字符生成正文的调用方用，免得每个字符分配临时 `String`）。
pub fn push_xml_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        c if !is_xml_char(c) => {}
        _ => out.push(c),
    }
}

/// [`xml_escape`] 的反向：把 XML 文本里的字符引用还原（`&amp; &lt; &gt; &quot; &apos;` 与 `&#N;`/`&#xH;`）；认不出的
/// `&…` 原样保留。从 OPF/正文**读出**文字再**写进**别处时用——不先还原就再转义一遍，`A &amp; B` 会变成
/// `A &amp;amp; B`，设备上显示成字面的 "A &amp; B"。
pub fn xml_unescape(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('&') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // 只在 `&` 后面 12 字节内找 `;`（最长的字符引用 `&#x10FFFF;` 也够）：此前 `find` 一路找到文末，裸 `&` 多、`;` 少的
        // 长文本是平方级。
        let decoded = rest.as_bytes().iter().take(13).position(|&b| b == b';').and_then(|j| {
            let ent = &rest[1..j];
            let c = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => ent
                    .strip_prefix("#x")
                    .or_else(|| ent.strip_prefix("#X"))
                    .map(|h| u32::from_str_radix(h, 16))
                    .or_else(|| ent.strip_prefix('#').map(|d| d.parse::<u32>()))
                    .and_then(|r| r.ok())
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, j + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

/// XML 1.0 §2.2 允许的字符：`#x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]`
/// （Rust `char` 本来就不含代理区）。
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// 路径/文件名是不是常见位图（按扩展名，忽略大小写）：jpg/jpeg/png/gif/webp。封面声明、占位封面探测、CBZ 收页、
/// 优化器挑要处理的图（`imgopt::is_page_image`）共用。
pub fn is_image_ext(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".jpg") || l.ends_with(".jpeg") || l.ends_with(".png") || l.ends_with(".gif") || l.ends_with(".webp")
}

/// 图片路径的扩展名（小写、不带点）：取**文件名**最后一个 `.` 之后；文件名没有扩展名时当 `jpg`（EPUB 里绝大多数图是
/// JPEG）。读封面、占位封面、优化器给图片重新落名共用——此前各处直接 `rsplit('.')`，无扩展名的路径会把整段路径连同 `/`
/// 当扩展名，写出 `cover.images/x`、`images/0001.oebps/images/x` 这种条目名。
pub(crate) fn image_ext_of(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() => e.to_ascii_lowercase(),
        _ => "jpg".into(),
    }
}

/// 认得的位图格式：`image` 库的格式、条目扩展名（不带点）、media-type。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageKind {
    pub format: image::ImageFormat,
    pub ext: &'static str,
    pub mime: &'static str,
}

/// 书里会出现、本 crate 能解码的位图格式（`image` 只开了这四种）。按魔数认（[`image_kind`]）、按扩展名查 media-type
/// （[`image_media_type_of_ext`]）都用这一张表——此前魔数识别有三套（`convert::common`、`imgopt` 里的 `image::guess_format`、
/// 这里按扩展名的），各写各的。
const IMAGE_KINDS: [ImageKind; 4] = [
    ImageKind { format: image::ImageFormat::Jpeg, ext: "jpg", mime: "image/jpeg" },
    ImageKind { format: image::ImageFormat::Png, ext: "png", mime: "image/png" },
    ImageKind { format: image::ImageFormat::Gif, ext: "gif", mime: "image/gif" },
    ImageKind { format: image::ImageFormat::WebP, ext: "webp", mime: "image/webp" },
];

/// 按文件头魔数认图片格式（JPEG `FF D8 FF`、PNG 8 字节签名、`GIF87a`/`GIF89a`、RIFF 容器里标 `WEBP`）；不认得 → `None`。
/// 与 `image::guess_format` 对这四种的判定相同。
pub fn image_kind(b: &[u8]) -> Option<ImageKind> {
    let i = if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        0
    } else if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        1
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        2
    } else if b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        3
    } else {
        return None;
    };
    Some(IMAGE_KINDS[i])
}

/// 图片扩展名（不带点、小写）→ media-type；认不出的当 JPEG（EPUB 里绝大多数图是 JPEG）。
pub fn image_media_type_of_ext(ext: &str) -> &'static str {
    IMAGE_KINDS.iter().find(|k| k.ext == ext).map_or("image/jpeg", |k| k.mime)
}

/// 全角 ASCII（U+FF01–U+FF5E）转成对应的半角字符，其余不变。
pub fn to_halfwidth(c: char) -> char {
    if ('\u{FF01}'..='\u{FF5E}').contains(&c) { char::from_u32(c as u32 - 0xFEE0).unwrap_or(c) } else { c }
}

/// "先产出到临时文件、成功才落盘改名覆盖目标、失败清掉半成品"的统一外壳（命令行工具和书库写文件都走它）。`produce(tmp)` 负责把产物写到 `tmp` 并返回任意结果（如统计报告）；
/// 它出错或最后 `rename` 失败，`tmp` 都会被删掉，不在目录里留半成品。`tmp` 应与 `target` 同分区（rename 才原子）。
/// 输入输出是同一个文件时也安全：产出期间原文件不动，改名那一刻才换掉。
pub fn produce_then_replace<T>(tmp: &std::path::Path, target: &std::path::Path, produce: impl FnOnce(&std::path::Path) -> Result<T, String>) -> Result<T, String> {
    let value = match produce(tmp) {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(tmp);
            return Err(e);
        }
    };
    if let Err(e) = commit(tmp, target) {
        let _ = std::fs::remove_file(tmp);
        return Err(format!("改名覆盖 {} 失败: {e}", target.display()));
    }
    Ok(value)
}

/// 把写好的临时文件 `tmp` 换到 `target`：先落盘 `tmp`（fsync），再改名，最后落盘所在目录。
/// 中途断电不会留下写了一半的目标文件。失败时 `tmp` 留给调用方删。
pub fn commit(tmp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(tmp)?.sync_all()?;
    std::fs::rename(tmp, target)?;
    sync_parent(target)
}

/// 落盘 `path` 所在的目录（改名、删除之后，让目录项的变化也落盘）。
pub fn sync_parent(path: &std::path::Path) -> std::io::Result<()> {
    match path.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => std::fs::File::open(dir)?.sync_all(),
        None => Ok(()),
    }
}

/// 与 `target` 同目录的临时文件名 `<文件名>.<tag>.tmp`（同分区，[`produce_then_replace`] 的改名才原子）。
pub fn tmp_beside(target: &std::path::Path, tag: &str) -> std::path::PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{tag}.tmp"));
    target.with_file_name(name)
}

/// 整份字节原子地写到 `target`（先写同目录临时文件再改名，失败不留半成品、不截断原文件）。
pub fn write_atomic(target: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    produce_then_replace(&tmp_beside(target, "writing"), target, |t| std::fs::write(t, bytes).map_err(|e| format!("写 {}: {e}", target.display())))
}

/// 命令行工具（`bookconv` 的各个 bin）共用的样板：出错退出、读写文件、SIGPIPE。
/// 退出码约定：1 = 用法错，2 = 读写或处理失败，3 起各工具自定。
pub mod cli {
    pub use super::restore_sigpipe;
    use std::path::Path;

    /// 用法错的退出码。
    pub const USAGE: i32 = 1;
    /// 读写或处理失败的退出码。
    pub const FAILED: i32 = 2;

    /// 把 `msg` 打到 stderr，以 `code` 退出。
    pub fn die(code: i32, msg: impl std::fmt::Display) -> ! {
        eprintln!("{msg}");
        std::process::exit(code)
    }

    /// 读整个文件；失败以 [`FAILED`] 退出。
    pub fn read_or_die(path: impl AsRef<Path>) -> Vec<u8> {
        let path = path.as_ref();
        std::fs::read(path).unwrap_or_else(|e| die(FAILED, format!("读 {}: {e}", path.display())))
    }

    /// 原子地写整个文件（[`super::write_atomic`]）；失败以 [`FAILED`] 退出。
    pub fn write_or_die(path: impl AsRef<Path>, bytes: &[u8]) {
        super::write_atomic(path.as_ref(), bytes).unwrap_or_else(|e| die(FAILED, e))
    }
}

/// 书名 → 安全文件名：控制字符与路径字符（`/\:*?"<>|`）换下划线、去首尾空白、开头的 `.` 换下划线（免得成了隐藏文件）；
/// 截断到 80 个字符且不超过 [`MAX_NAME_BYTES`] 字节（ext4 等文件名上限是 255 **字节**，中文一个字 3 字节，
/// 还要给调用方留出 ` [id].epub` 这类后缀）。空则用 `default`。
pub fn sanitize_filename(title: &str, default: &str) -> String {
    let t: String = title
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let t = t.trim();
    if t.is_empty() {
        return default.to_string();
    }
    let mut out = String::new();
    for c in t.chars().take(80) {
        if out.len() + c.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        out.push(if out.is_empty() && c == '.' { '_' } else { c });
    }
    out.trim_end().to_string()
}

/// 命令行工具的输出接到 `head` 这类提前关闭的管道时，像别的命令一样安静退出（Rust 缺省忽略 SIGPIPE，`println!` 会 panic）。
/// 在 `main` 一开头、还没有其它线程时调用。`booklib` 和所有往 stdout 打印的 bin 共用（也经 [`cli`] 导出）。
pub fn restore_sigpipe() {
    #[cfg(unix)]
    {
        extern "C" {
            fn signal(sig: i32, handler: usize) -> usize;
        }
        const SIGPIPE: i32 = 13;
        const SIG_DFL: usize = 0;
        // SAFETY: 进程启动时、还没有其它线程时恢复 SIGPIPE 的缺省处理。
        unsafe {
            signal(SIGPIPE, SIG_DFL);
        }
    }
}

/// [`sanitize_filename`] 结果的字节上限。
pub const MAX_NAME_BYTES: usize = 200;

/// 字节串里第一次出现 `needle` 的位置；`needle` 为空时返回 `None`。
pub fn memfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// FNV-1a 64 位（稳定的书 ID 用，结果进产物，别改算法）。
pub fn fnv64(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &c| (h ^ c as u64).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produce_then_replace_swaps_on_success_and_cleans_tmp_on_failure() {
        let d = tempfile::tempdir().unwrap();
        let (tmp, target) = (d.path().join(".t.tmp"), d.path().join("t.bin"));
        std::fs::write(&target, b"old").unwrap();
        let n = produce_then_replace(&tmp, &target, |t| std::fs::write(t, b"new").map(|_| 7).map_err(|e| e.to_string())).unwrap();
        assert_eq!(n, 7);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!tmp.exists(), "rename 后 tmp 不该残留");
        // 产出失败：原文件不动，半成品被清
        let err = produce_then_replace(&tmp, &target, |t| -> Result<(), String> {
            std::fs::write(t, b"half").unwrap();
            Err("boom".into())
        })
        .unwrap_err();
        assert_eq!(err, "boom");
        assert_eq!(std::fs::read(&target).unwrap(), b"new", "失败不动原文件");
        assert!(!tmp.exists(), "失败清掉半成品");
        // rename 失败（目标是个非空目录）：报改名失败并清 tmp
        let dir_target = d.path().join("dir");
        std::fs::create_dir_all(dir_target.join("x")).unwrap();
        let err = produce_then_replace(&tmp, &dir_target, |t| std::fs::write(t, b"z").map_err(|e| e.to_string())).unwrap_err();
        assert!(err.contains("改名覆盖"), "{err}");
        assert!(!tmp.exists());
    }

    #[test]
    fn write_atomic_replaces_and_tmp_sits_beside_target() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("书.epub");
        assert_eq!(tmp_beside(&target, "x"), d.path().join("书.epub.x.tmp"));
        std::fs::write(&target, b"old").unwrap();
        write_atomic(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "不留临时文件");
    }

    #[test]
    fn image_ext_of_takes_file_extension_only() {
        assert_eq!(image_ext_of("OEBPS/images/Cv.JPG"), "jpg");
        assert_eq!(image_ext_of("a.b/images/cover"), "jpg", "文件名没扩展名时不能把目录里的点当扩展名");
        assert_eq!(image_ext_of("cover."), "jpg");
        assert_eq!(image_ext_of("x.png"), "png");
    }

    #[test]
    fn image_ext_and_media_type() {
        assert!(is_image_ext("a/B.JPG") && is_image_ext("c.webp") && is_image_ext("x.gif"));
        assert!(!is_image_ext("cover.txt") && !is_image_ext("style.css"));
        assert_eq!(image_media_type_of_ext("png"), "image/png");
        assert_eq!(image_media_type_of_ext("gif"), "image/gif");
        assert_eq!(image_media_type_of_ext("jpg"), "image/jpeg");
        assert_eq!(image_media_type_of_ext("bmp"), "image/jpeg", "认不出当 JPEG");
        assert_eq!(image_media_type_of_ext("webp"), "image/webp");
        assert_eq!(image_media_type_of_ext("jpeg"), "image/jpeg");
    }

    #[test]
    fn utc_w3c_formats_dates() {
        assert_eq!(utc_w3c(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_w3c(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_w3c(1_790_755_509), "2026-09-30T08:05:09Z");
    }

    #[test]
        fn xml_escape_covers_amp_lt_gt_quote() {
        assert_eq!(xml_escape(r#"a&b<c>d"e"#), "a&amp;b&lt;c&gt;d&quot;e");
        assert_eq!(xml_escape("纯文本"), "纯文本");
        assert_eq!(xml_escape("a\u{0}b\u{1}c\td\u{FFFE}e"), "abc\tde", "XML 1.0 不允许的字符直接丢弃");
    }

    #[test]
    fn xml_unescape_reverses_escape_and_char_refs() {
        let raw = "A & B <c> \"d\" 中";
        assert_eq!(xml_unescape(&xml_escape(raw)), raw);
        assert_eq!(xml_unescape("&#20013;&#x6587;&apos;"), "中文'");
        assert_eq!(xml_unescape("a & b &unknown; &#xZZ; &"), "a & b &unknown; &#xZZ; &", "认不出的原样保留");
        assert!(matches!(xml_unescape("plain"), std::borrow::Cow::Borrowed(_)));
        // 裸 `&` 很多、`;` 在很远处：只在 `&` 后面 12 字节内找 `;`，结果不变、不再平方级
        let s = format!("{}x;&amp;", "a & ".repeat(20_000));
        assert!(xml_unescape(&s).ends_with("a & x;&"));
        assert_eq!(xml_unescape("&#x10FFFF;&abcdefghijkl;"), "\u{10FFFF}&abcdefghijkl;");
    }

    #[test]
    fn sanitize_filename_strips_and_defaults() {
        assert_eq!(sanitize_filename("a/b:c?", "book"), "a_b_c_");
        assert_eq!(sanitize_filename("  ", "book"), "book");
        assert_eq!(sanitize_filename("   ", "article"), "article");
        assert_eq!(sanitize_filename("正常书名", "book"), "正常书名");
    }
}

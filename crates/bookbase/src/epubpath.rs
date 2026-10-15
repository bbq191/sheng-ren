//! EPUB（zip）里的路径：zip 内 posix 路径的规范化、相对/绝对换算，链接属性原文 → (zip 路径, 锚点)。
//! [`resolve_link`] 是全书解析链接的统一入口。

use crate::util::percent_decode;

pub fn posix_norm(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

pub fn dir_of(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

pub fn resolve(base_dir: &str, rel: &str) -> String {
    if base_dir.is_empty() {
        posix_norm(rel)
    } else {
        posix_norm(&format!("{base_dir}/{rel}"))
    }
}

/// 已还原字符引用的相对路径（还可能百分号编码，不带 `#锚点`）→ zip 路径：先百分号解码，再按 `base_dir` 解析。
/// OPF manifest 项用 `bookconv::wash::ManifestItem::path`（属性原文还要先还原 `&amp;` 等）；带锚点、以所在文件为基准的链接用 [`resolve_href`]。
pub fn resolve_rel(base_dir: &str, href: &str) -> String {
    resolve(base_dir, &percent_decode(href))
}

/// `target` 相对 `base_dir` 的路径（都是 zip 内绝对路径）。
pub fn relative_to(base_dir: &str, target: &str) -> String {
    let b: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    let t: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = b.iter().zip(t.iter()).take_while(|(x, y)| x == y).count();
    let mut out: Vec<String> = vec!["..".into(); b.len() - common];
    out.extend(t[common..].iter().map(|s| s.to_string()));
    out.join("/")
}

/// 链接值 → (目标文件的 zip 路径, 锚点原文)。`base_file` 是链接所在文件的 zip 路径；路径部分为空（`#x`）时目标就是
/// `base_file` 自己。路径先百分号解码再按 `base_file` 所在目录解析、规整（全书各处"链接指向哪个文件"一律走这里）。
/// 书外链接（`http:`、`mailto:`…）不该传进来，调用方先用 [`crate::html::is_external`] 筛掉。
pub fn resolve_href<'a>(base_file: &str, href: &'a str) -> (String, Option<&'a str>) {
    let (p, frag) = crate::html::split_href(href);
    let path = if p.is_empty() { base_file.to_string() } else { resolve_rel(dir_of(base_file), p) };
    (path, frag)
}

/// 链接**属性原文** → (目标文件的 zip 路径, 解码后的锚点)。全书"属性原文里的链接指向哪里"一律走这里：依次还原字符引用
/// （`a&amp;b.xhtml` 是文件 `a&b.xhtml`）、拆出锚点、路径与锚点各自百分号解码、按 `base_file` 所在目录解析规整；路径部分为空
/// （`#x`）时目标就是 `base_file`。锚点拿去对 id 时，id 也要先还原字符引用（`util::xml_unescape`）。没有 `#` 时锚点是 `None`。
/// 要把锚点**原样写回**链接的调用方（目录、改链）别用这里的锚点，用 [`crate::html::split_href`] 拆出的原文。
/// 书外链接不该传进来，调用方先用 [`crate::html::is_external`] 筛掉。
pub fn resolve_link(base_file: &str, raw: &str) -> (String, Option<String>) {
    let v = crate::util::xml_unescape(raw);
    let (p, frag) = crate::html::split_href(&v);
    let path = if p.is_empty() { base_file.to_string() } else { resolve_rel(dir_of(base_file), p) };
    (path, frag.map(percent_decode))
}

/// zip 内路径写进 `href`/`src` 用的百分号编码：字母数字与 `-._~/` 原样，其余（空格、`#`、`%`、非 ASCII）按 UTF-8 字节编码。
pub fn encode_href_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 从 `from_dir`（zip 内目录）指向 `target`（zip 内路径）的链接值：相对路径百分号编码，`frag`（锚点原文）非空时带 `#frag`。
/// 结果是属性值原文，写进 XML 前调用方仍要 `xml_escape`（锚点里可能有 `&`）。
pub fn href_to(from_dir: &str, target: &str, frag: &str) -> String {
    let rel = encode_href_path(&relative_to(from_dir, target));
    if frag.is_empty() { rel } else { format!("{rel}#{frag}") }
}

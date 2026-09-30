//! 伪 DRM：`encryption.xml` 只加密字体/样式/脚本的"伪加密"剥掉，真加密（加密了正文等）报错。
use super::*;

/// 伪 DRM 允许加密的扩展名（= strip_pseudo_drm.py SAFE_EXTS）。
pub const PSEUDO_DRM_SAFE_EXTS: &[&str] = &[".css", ".ttf", ".otf", ".woff", ".woff2", ".js"];
// ───────────────────────── 1. 伪 DRM ─────────────────────────

/// `META-INF/encryption.xml` 里的加密目标（zip 内路径）：每个 `<CipherReference URI="…">`（单双引号、带前缀都认）。
/// 没有 `encryption.xml` 返回 `None`。质量门 `check`、入库、清洗层共用。
pub fn encrypted_targets(entries: &[Entry]) -> Option<Vec<String>> {
    let enc = entries.iter().find(|e| e.name == "META-INF/encryption.xml")?;
    let t = String::from_utf8_lossy(&enc.data);
    Some(
        html::tags(&t)
            .filter(|tag| tag.is_start() && opf::is_local(tag.name, "CipherReference"))
            .filter_map(|tag| html::attr_value(&t[tag.start..tag.end], "URI"))
            .map(|u| posix_norm(&percent_decode(&crate::util::xml_unescape(u))))
            .filter(|u| !u.is_empty() && !u.starts_with('#'))
            .collect(),
    )
}

/// 真 DRM 判据：加密了非样式/字体/脚本的文件。返回违规项。
pub fn real_drm_items(targets: &[String]) -> Vec<String> {
    targets.iter().filter(|t| { let l = t.to_ascii_lowercase(); !PSEUDO_DRM_SAFE_EXTS.iter().any(|e| l.ends_with(e)) }).cloned().collect()
}

pub(super) fn strip_pseudo_drm(entries: &mut Vec<Entry>, rep: &mut WashReport) -> Result<(), String> {
    let Some(targets) = encrypted_targets(entries) else { return Ok(()) };
    let bad = real_drm_items(&targets);
    if !bad.is_empty() {
        return Err(format!("加密 EPUB（真 DRM，加密了 {} 等 {} 项），阅读器都读不了", bad.iter().take(3).cloned().collect::<Vec<_>>().join("、"), bad.len()));
    }
    let drop: HashSet<String> = targets.iter().cloned().chain(std::iter::once("META-INF/encryption.xml".to_string())).collect();
    if let Some(oi) = find_opf(entries) {
        let opf_dir = dir_of(&entries[oi].name).to_string();
        let text = String::from_utf8_lossy(&entries[oi].data).into_owned();
        // manifest 里指向被剥文件的 `<item>`（连同后面的空白）一趟删掉。
        if let Some(t) = opf::remove_items(&text, |it| drop.contains(&resolve(&opf_dir, &percent_decode(it.href)))) {
            entries[oi].data = t.into_bytes();
        }
    }
    entries.retain(|e| !drop.contains(&e.name));
    rep.pseudo_drm_stripped = targets;
    Ok(())
}

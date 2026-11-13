//! 文件操作小工具（拆分自 `generate.rs`）：大小写别名、删产物、搬 `.sdr`、拷目录。

use std::path::Path;

/// 产物换了目录（文件名没变）：阅读器放在书旁边的同名 `.sdr` 文件夹（Kindle 的阅读进度、书签，`docs/kfx.md#阅读进度2026-10-06-真机`）
/// 跟着挪过去，免得进度留在旧目录（2026-10-14 定：原件目录改名后《哆啦A梦》的进度丢在旧目录）。文件名变了的不挪
/// （`.sdr` 里的文件按书名起名，改了名阅读器认不认没验证过），新位置已经有 `.sdr` 的不动，挪不了的不管（旧目录里留着）。
/// KEPUB（`x.kepub.epub`）不挪：Kobo 的进度在设备数据库里，没有 `.sdr`，`with_extension` 还会算成 `x.kepub.sdr`。
pub(super) fn carry_sidecar(old: &Path, new: &Path) {
    if old.to_string_lossy().ends_with(".kepub.epub") {
        return;
    }
    if old == new || !old.is_absolute() || old.file_name() != new.file_name() {
        return;
    }
    let (from, to) = (old.with_extension("sdr"), new.with_extension("sdr"));
    if !from.is_dir() || to.exists() {
        return;
    }
    if std::fs::rename(&from, &to).is_ok() {
        let _ = crate::fsutil::sync_parent(&to);
        return;
    }
    // 改不了名（跨文件系统：旧位置在电脑上；或者 MTP 不支持）：拷过去再删旧的；拷不成的删掉半截，旧的留着
    if copy_dir(&from, &to).is_ok() {
        let _ = std::fs::remove_dir_all(&from);
        let _ = crate::fsutil::sync_parent(&to);
    } else {
        let _ = std::fs::remove_dir_all(&to);
    }
}

/// 整个目录拷一份（只拷普通文件和子目录）。
pub(super) fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}
/// 同一目录里、文件名只差大小写的两个不同路径：是 [`case_alias`] 的特例（父目录相同、只有文件名差大小写）。
pub(crate) fn case_twin(a: &Path, b: &Path) -> bool {
    a != b
        && a.parent() == b.parent()
        && match (a.file_name(), b.file_name()) {
            (Some(x), Some(y)) => {
                x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase()
            }
            _ => false,
        }
}

/// 两个路径逐段只差大小写（不是同一个路径）：例如 `…/documents/x.epub` 和 `…/Documents/x.epub`。不要求在同一目录，
/// 可以跨目录；同一目录、只差文件名时是 [`case_twin`]。
/// 不分大小写的设备（jmtpfs、安卓存储）上它们是同一个文件；分大小写的设备上不是，所以调用方还要看文件在不在。
pub(crate) fn case_alias(a: &Path, b: &Path) -> bool {
    if a == b {
        return false;
    }
    let (mut ca, mut cb) = (a.components(), b.components());
    loop {
        match (ca.next(), cb.next()) {
            (None, None) => return true,
            (Some(x), Some(y))
                if x.as_os_str().to_string_lossy().to_lowercase()
                    == y.as_os_str().to_string_lossy().to_lowercase() => {}
            _ => return false,
        }
    }
}

/// `path` 和只差大小写的 `twins` 在磁盘上是不是各是各的文件：列目录，`path` 的原名和某个 twin 的原名都列得出来才算
/// （分大小写的文件系统）。不分大小写的文件系统上目录里只会有其中一个名字，算同一个文件，不能删。读不了目录返回 `None`（记着、下次再看）。
/// 只列 `path` 所在的一个目录，判断不了跨目录的别名（那要用 [`case_alias`]），所以调用方只拿同一目录的 twins（[`case_twin`]）来调它。
pub(super) fn distinct_on_disk(path: &Path, twins: &[&Path]) -> Option<bool> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Some(false);
    };
    let rd = std::fs::read_dir(dir).ok()?;
    let names: Vec<std::ffi::OsString> = rd.flatten().map(|e| e.file_name()).collect();
    Some(
        names.iter().any(|n| n == name)
            && twins.iter().any(|t| {
                t.file_name()
                    .is_some_and(|tn| names.iter().any(|n| n == tn))
            }),
    )
}

/// 删掉一个产物文件：删掉了或本来就不在返回 `true`。删的时候报"不在"不立即当真：jmtpfs 刚挪完文件偶尔 stat 不到，
/// 重新列一次目录，里面还有这个名字就再删一次（删不掉的算没删，下次再删）。
pub(super) fn remove_product(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => {
            let _ = crate::fsutil::sync_parent(path);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
                return true;
            };
            let listed = match std::fs::read_dir(dir) {
                Ok(rd) => rd.flatten().any(|e| e.file_name() == name),
                // 目录都不在了：文件当然也不在
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(_) => return false,
            };
            !listed || std::fs::remove_file(path).is_ok()
        }
        Err(_) => false,
    }
}

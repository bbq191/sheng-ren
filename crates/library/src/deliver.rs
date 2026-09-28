//! 把产物拷到别处（U 盘、挂载的阅读器）：`build --out=目录`。
//!
//! 产物总是先生成在书库的 `output/<设备>/`（本地缓存），再拷过去。拷过的记在 `deliveries.json`
//! （目标路径 → 书 id、设备、指纹、大小）：没变的不再拷，书名变了删掉旧文件，`remove` 时一起删。
//!
//! MTP 挂载（gvfs，如 `/run/user/1000/gvfs/mtp:host=…`）不支持普通的写文件、改名（2026-09-27 Kindle PW12 实测：
//! `open(O_CREAT)` 返回"Operation not supported"），只能整文件推送，所以这种情况退回 `gio copy` / `gio remove`。
//! 别的错误（没权限、磁盘满）直接报错，不去动目标目录里的旧文件。

use crate::fsutil::tmp_sibling;
use crate::{Library, Meta};
use profile::Profile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default, Clone, Serialize, Deserialize)]
pub(crate) struct Deliveries {
    /// 目标文件路径 → 拷过去的是什么。
    files: BTreeMap<PathBuf, Delivery>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Delivery {
    id: String,
    device: String,
    fingerprint: String,
    size: u64,
}

/// 一次拷贝的结果。
pub enum Delivered {
    Copied(PathBuf),
    /// 目标那份和现在的产物一样，没拷。
    Unchanged(PathBuf),
}

/// 一本书拷出去的一份（`list` 用）。
pub struct DeliveryStatus {
    pub device: String,
    pub path: PathBuf,
    /// 目标那份是不是现在的产物；`None` = 目标不在（设备没连上，或被删了）。
    pub fresh: Option<bool>,
}

/// 这个错误是不是"文件系统不支持普通写"（MTP 挂载）：错误码 EOPNOTSUPP，或者路径在 gvfs 下。
fn is_mtp(path: &Path, e: &std::io::Error) -> bool {
    const EOPNOTSUPP: i32 = 95; // Linux
    e.raw_os_error() == Some(EOPNOTSUPP) || e.kind() == std::io::ErrorKind::Unsupported || path.to_string_lossy().contains("/gvfs/")
}

/// 拷文件：先按普通文件系统写（临时文件 + 改名，中途失败不留半个文件）；只有 MTP 挂载才改用 `gio copy`。
/// 其它错误直接返回，目标位置的旧文件不动。
fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    let tmp = tmp_sibling(dst);
    let Err(e) = std::fs::copy(src, &tmp).and_then(|_| std::fs::rename(&tmp, dst)) else { return Ok(()) };
    let _ = std::fs::remove_file(&tmp);
    if !is_mtp(dst, &e) {
        return Err(format!("写 {}: {e}", dst.display()));
    }
    // MTP 覆盖已有文件不可靠：先删再推
    if dst.exists() {
        delete_file(dst)?;
    }
    let out = Command::new("gio").arg("copy").arg(src).arg(dst).output().map_err(|g| format!("写 {}: {e}（gio 也用不了: {g}）", dst.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("写 {}: {e}；gio copy: {}", dst.display(), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// 删文件（不在算成功）；MTP 挂载上用 `gio remove`。
fn delete_file(p: &Path) -> Result<(), String> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) if is_mtp(p, &e) => {
            let out = Command::new("gio").arg("remove").arg(p).output().map_err(|g| format!("删 {}: {e}（gio 也用不了: {g}）", p.display()))?;
            if out.status.success() { Ok(()) } else { Err(format!("删 {}: {e}；gio remove: {}", p.display(), String::from_utf8_lossy(&out.stderr).trim())) }
        }
        Err(e) => Err(format!("删 {}: {e}", p.display())),
    }
}

impl Library {
    fn deliveries_path(&self) -> PathBuf {
        self.root.join("deliveries.json")
    }

    /// 把这本书在该设备下的产物（要先 `build` 过）拷进 `dest_dir`。和上次拷的一样就跳过（`force` 照拷）。
    pub fn deliver(&self, meta: &Meta, device: &Profile, dest_dir: &Path, force: bool) -> Result<Delivered, String> {
        let (src, fingerprint) = self.built_output(meta, device).ok_or("还没有生成产物")?;
        let size = std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?.len();
        let file = src.file_name().ok_or("产物文件名不对")?;
        let dest = dest_dir.join(file);
        if dest.to_str().is_none() {
            return Err(format!("{}：目录名不是 UTF-8，请改名", dest_dir.display()));
        }
        let path = self.deliveries_path();
        let rec = self.deliveries_json.get(&path);
        let same = rec.files.get(&dest).is_some_and(|d| d.id == meta.id && d.fingerprint == fingerprint && d.size == size);
        if !force && same && std::fs::metadata(&dest).is_ok_and(|m| m.len() == size) {
            return Ok(Delivered::Unchanged(dest));
        }
        if !dest_dir.is_dir() {
            std::fs::create_dir_all(dest_dir).map_err(|e| format!("{}: {e}", dest_dir.display()))?;
        }
        copy_file(&src, &dest)?;
        let mut rec = (*rec).clone();
        // 同一目录里这本书这台设备以前拷过的旧文件名（书名变了）删掉
        let stale: Vec<PathBuf> =
            rec.files.iter().filter(|(p, d)| **p != dest && d.id == meta.id && d.device == device.id && p.parent() == Some(dest_dir)).map(|(p, _)| p.clone()).collect();
        for p in stale {
            if delete_file(&p).is_ok() {
                rec.files.remove(&p);
            }
        }
        rec.files.insert(dest.clone(), Delivery { id: meta.id.clone(), device: device.id.clone(), fingerprint, size });
        self.deliveries_json.put(&path, rec)?;
        Ok(Delivered::Copied(dest))
    }

    /// 这本书拷出去的各份，以及是不是现在的产物。
    pub fn deliveries(&self, meta: &Meta) -> Vec<DeliveryStatus> {
        self.deliveries_json
            .get(&self.deliveries_path())
            .files
            .iter()
            .filter(|(_, d)| d.id == meta.id)
            .map(|(path, d)| {
                let fresh = std::fs::metadata(path).ok().map(|m| {
                    let current = self.registry.get(&d.device).and_then(|p| self.built_output(meta, p)).is_some_and(|(_, fp)| fp == d.fingerprint);
                    current && m.len() == d.size
                });
                DeliveryStatus { device: d.device.clone(), path: path.clone(), fresh }
            })
            .collect()
    }

    /// 删掉拷出去的各份（`remove` 用）。目标不在（设备没连上）的记录保留，下次再删。
    pub(crate) fn remove_deliveries(&self, id: &str) -> Result<(), String> {
        let path = self.deliveries_path();
        let cur = self.deliveries_json.get(&path);
        let mine: Vec<PathBuf> = cur.files.iter().filter(|(_, d)| d.id == id).map(|(p, _)| p.clone()).collect();
        if mine.is_empty() {
            return Ok(());
        }
        let mut rec = (*cur).clone();
        for p in mine {
            if p.parent().is_some_and(Path::is_dir) {
                delete_file(&p)?;
                rec.files.remove(&p);
            }
        }
        self.deliveries_json.put(&path, rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn plain_copy_errors_do_not_fall_back_to_gio_or_delete_old_copy() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("new.azw3");
        std::fs::write(&src, b"new").unwrap();
        let dest_dir = d.path().join("ro");
        std::fs::create_dir(&dest_dir).unwrap();
        let dst = dest_dir.join("book.azw3");
        std::fs::write(&dst, b"old").unwrap();
        std::fs::set_permissions(&dest_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(dest_dir.join("probe"), b"").is_ok() {
            return; // 以 root 运行：只读目录挡不住，测不了
        }
        let r = copy_file(&src, &dst);
        std::fs::set_permissions(&dest_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = r.unwrap_err();
        assert!(!e.contains("gio"), "普通错误不走 gio：{e}");
        assert_eq!(std::fs::read(&dst).unwrap(), b"old", "旧副本不删");
        assert_eq!(std::fs::read_dir(&dest_dir).unwrap().count(), 1, "不留临时文件");
        // 正常情况：覆盖
        copy_file(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
    }

    #[test]
    fn mtp_is_recognized_by_errno_or_gvfs_path() {
        let unsupported = std::io::Error::from_raw_os_error(95);
        assert!(is_mtp(Path::new("/media/usb/x"), &unsupported));
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(!is_mtp(Path::new("/media/usb/x"), &denied));
        assert!(is_mtp(Path::new("/run/user/1000/gvfs/mtp:host=Kindle/Internal Storage/documents/x"), &denied));
    }
}

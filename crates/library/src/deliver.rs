//! 把产物拷到别处（U 盘、挂载的阅读器）：`build --out=目录`。
//!
//! 产物总是先生成在书库的 `output/<设备>/`（本地缓存），再拷过去。拷过的记在 `deliveries.json`
//! （目标路径 → 书 id、设备、指纹、大小）：没变的不再拷，书名变了删掉旧文件，`remove` 时一起删。
//!
//! MTP 挂载（gvfs，如 `/run/user/1000/gvfs/mtp:host=…`）不支持普通的写文件、改名（2026-09-27 Kindle PW12 实测：
//! `open(O_CREAT)` 返回"Operation not supported"），只能整文件推送，所以普通写失败时退回 `gio copy` / `gio remove`。

use crate::fsutil::write_atomic;
use crate::{Library, Meta};
use profile::Profile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default, Serialize, Deserialize)]
struct Deliveries {
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

/// 拷文件：先按普通文件系统写（临时文件 + 改名，中途失败不留半个文件）；文件系统不支持时（MTP）用 `gio copy`。
fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    let mut tmp_name = dst.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = dst.with_file_name(tmp_name);
    match std::fs::copy(src, &tmp).and_then(|_| std::fs::rename(&tmp, dst)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
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
    }
}

fn delete_file(p: &Path) -> Result<(), String> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            let out = Command::new("gio").arg("remove").arg(p).output().map_err(|g| format!("删 {}: {e}（gio 也用不了: {g}）", p.display()))?;
            if out.status.success() { Ok(()) } else { Err(format!("删 {}: {e}；gio remove: {}", p.display(), String::from_utf8_lossy(&out.stderr).trim())) }
        }
    }
}

impl Library {
    fn load_deliveries(&self) -> Deliveries {
        std::fs::read(self.root.join("deliveries.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_deliveries(&self, d: &Deliveries) -> Result<(), String> {
        write_atomic(&self.root.join("deliveries.json"), serde_json::to_string_pretty(d).unwrap().as_bytes())
    }

    /// 把这本书在该设备下的产物（要先 `build` 过）拷进 `dest_dir`。和上次拷的一样就跳过（`force` 照拷）。
    pub fn deliver(&self, meta: &Meta, device: &Profile, dest_dir: &Path, force: bool) -> Result<Delivered, String> {
        let (src, fingerprint) = self.built_output(meta, device).ok_or("还没有生成产物")?;
        let size = std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?.len();
        let file = src.file_name().ok_or("产物文件名不对")?;
        let dest = dest_dir.join(file);
        let mut rec = self.load_deliveries();
        let same = rec.files.get(&dest).is_some_and(|d| d.id == meta.id && d.fingerprint == fingerprint && d.size == size);
        if !force && same && std::fs::metadata(&dest).is_ok_and(|m| m.len() == size) {
            return Ok(Delivered::Unchanged(dest));
        }
        if !dest_dir.is_dir() {
            std::fs::create_dir_all(dest_dir).map_err(|e| format!("{}: {e}", dest_dir.display()))?;
        }
        copy_file(&src, &dest)?;
        // 同一目录里这本书这台设备以前拷过的旧文件名（书名变了）删掉
        let stale: Vec<PathBuf> =
            rec.files.iter().filter(|(p, d)| **p != dest && d.id == meta.id && d.device == device.id && p.parent() == Some(dest_dir)).map(|(p, _)| p.clone()).collect();
        for p in stale {
            let _ = delete_file(&p);
            rec.files.remove(&p);
        }
        rec.files.insert(dest.clone(), Delivery { id: meta.id.clone(), device: device.id.clone(), fingerprint, size });
        self.save_deliveries(&rec)?;
        Ok(Delivered::Copied(dest))
    }

    /// 这本书拷出去的各份，以及是不是现在的产物。
    pub fn deliveries(&self, meta: &Meta) -> Vec<DeliveryStatus> {
        self.load_deliveries()
            .files
            .into_iter()
            .filter(|(_, d)| d.id == meta.id)
            .map(|(path, d)| {
                let fresh = std::fs::metadata(&path).ok().map(|m| {
                    let current = self.registry.get(&d.device).and_then(|p| self.built_output(meta, p)).is_some_and(|(_, fp)| fp == d.fingerprint);
                    current && m.len() == d.size
                });
                DeliveryStatus { device: d.device, path, fresh }
            })
            .collect()
    }

    /// 删掉拷出去的各份（`remove` 用）。目标不在（设备没连上）的记录保留，下次再删。
    pub(crate) fn remove_deliveries(&self, id: &str) -> Result<(), String> {
        let mut rec = self.load_deliveries();
        let mine: Vec<PathBuf> = rec.files.iter().filter(|(_, d)| d.id == id).map(|(p, _)| p.clone()).collect();
        for p in mine {
            if p.parent().is_some_and(Path::is_dir) {
                delete_file(&p)?;
                rec.files.remove(&p);
            }
        }
        self.save_deliveries(&rec)
    }
}

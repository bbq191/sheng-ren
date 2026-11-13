//! 产物位置与认领（拆分自 `generate.rs`）：产物放哪、文件名、记录里的旧位置，以及没变时的认领。

use super::types::{Built, Made, RecordState, Slot, StateEntry};
use crate::deliver::Target;
use crate::{Library, Meta};
use profile::Profile;
use std::path::{Path, PathBuf};

impl Library {
    /// 原件被跟踪着（有给这台设备的条目覆盖它）的话：(子目录以哪个目录为准（书根目录）, 设备上的子目录)，见
    /// [`crate::sources::Sources::placement`]。
    pub(crate) fn tracked_rel(&self, meta: &Meta, device: &str) -> Option<(PathBuf, PathBuf)> {
        if meta.source() == crate::Source::Stored || meta.source_path.is_empty() {
            return None;
        }
        self.records
            .sources
            .get(&self.sources_path())
            .placement(Path::new(&meta.source_path), device)
    }

    /// 这本书在该模式下的产物根目录和产物所在目录（绝对路径），见模块注释。`target` 是 MTP 设备时根目录是设备上的
    /// `<存储>/<dir>`，否则是电脑上跟踪目录旁的 `<模式 id>/`（add 进来的书、网址书在书库 `output/<模式 id>/`）。
    fn output_dir(
        &self,
        meta: &Meta,
        device: &Profile,
        target: &Target,
    ) -> Result<(PathBuf, PathBuf), String> {
        let device_root = match target {
            Target::Dir { root, .. } => Some(root.clone()),
            _ => None,
        };
        let lib_root = device_root.clone().unwrap_or_else(|| {
            std::path::absolute(&self.root)
                .unwrap_or_else(|_| self.root.clone())
                .join("output")
                .join(&device.id)
        });
        let Some((d, rel)) = self.tracked_rel(meta, &device.id) else {
            return Ok((lib_root.clone(), lib_root));
        };
        let on_device = device_root.is_some();
        let root = match device_root {
            Some(r) => r,
            // 电脑上：书根目录旁的 `<模式 id>/`
            None => {
                let Some(parent) = d.parent() else {
                    return Ok((lib_root.clone(), lib_root));
                };
                let root = parent.join(&device.id);
                let sources = self.records.sources.get(&self.sources_path());
                if let Some(t) = sources.dirs().find(|t| crate::sources::overlaps(&root, t)) {
                    return Err(output_overlap(&root, t));
                }
                root
            }
        };
        // MTP 设备（Kindle 是 FAT 系、掌阅是安卓存储）上目录名不能有 `:?*"<>|`、不能以空格或点结尾：这样的一段换成安全的名字，
        // 不然每次 sync 都在建目录时失败（2026-10-09 审计）。别的目录名原样，已经在设备上的位置不变
        let dir = if on_device {
            root.join(device_safe_rel(&rel))
        } else {
            root.join(rel)
        };
        Ok((root, dir))
    }
    /// 这本书在这个模式下的产物该放哪：产物根目录、所在目录、按记录算出来的文件名，以及记录里的旧条目、以前放在电脑上
    /// 被用户拷上设备的那份拷贝的文件名、同一轮里别的书占着的文件名。
    pub(super) fn resolve_placement(
        &self,
        meta: &Meta,
        device: &Profile,
        target: &Target,
        sp: &Path,
        ext: &str,
    ) -> Result<Slot, String> {
        let (root, dir) = self.output_dir(meta, device, target)?;
        let state = self.records.states.get(sp);
        let prev = state.books.get(&meta.id).cloned();
        // 记录里这本书的产物以前放在电脑上、用户自己拷上设备的（2026-10-07 以前的做法）：设备上同一子目录里叫那个名字的文件
        // 就是那份拷贝，当成本书的（覆盖它），不另起名字、不留重复
        let copied = prev
            .as_ref()
            .filter(|p| !p.root.as_os_str().is_empty() && p.root != root)
            .and_then(|p| {
                let rel = p.path.strip_prefix(&p.root).ok()?;
                (dir.strip_prefix(&root).ok()? == rel.parent()?)
                    .then(|| rel.file_name()?.to_str().map(str::to_string))
                    .flatten()
            });
        let taken = self.build.reserved_names(&dir, &meta.id);
        let out = dir.join(state.file_name_for(
            meta,
            &dir,
            ext,
            None,
            copied.as_deref(),
            &taken,
        )?);
        Ok(Slot {
            root,
            dir,
            out,
            prev,
            copied,
            taken,
        })
    }

    /// 记录里的产物还是最新的就收工：位置没变、有没删掉的旧位置就补删；记录里的位置空了、设备上那份是用户拷上去的拷贝，
    /// 就认领它。返回 `Some` 表示这轮不用生成。
    pub(super) fn settle_fresh(
        &self,
        meta: &Meta,
        sp: &Path,
        slot: &Slot,
        fingerprint: &str,
        record: RecordState,
        staged: bool,
    ) -> Result<Option<Built>, String> {
        if let Some(p) = record.done.filter(|p| p.path == slot.out) {
            if !p.old.is_empty() {
                self.finish(sp, &meta.id, p.clone(), staged)?; // 上次没删掉的旧位置
            }
            return Ok(Some(Built::UpToDate(slot.out.clone())));
        }
        // 先比较：指纹没变、记录里的位置空了（以前放电脑上、用户自己挪上了设备），设备上对应的位置就是那份 → 认领，不重新生成
        if let Some(p) = record
            .unchanged
            .filter(|_| record.done.is_none() && slot.copied.is_some() && slot.out.is_file())
            // 记着哈希的要核对：同名的不一定是那份拷贝（别的书、拷了一半的）；对不上就当普通的旧文件，重新生成覆盖
            .filter(|p| {
                p.sha.is_empty() || crate::fsutil::sha256_file(&slot.out).is_ok_and(|h| h == p.sha)
            })
        {
            let old = p
                .placements()
                .into_iter()
                .filter(|x| x.path != slot.out)
                .collect();
            let mut entry = StateEntry::file(
                slot.out.clone(),
                slot.root.clone(),
                fingerprint.to_string(),
                old,
            );
            entry.sha = p.sha.clone();
            entry.audit = p.audit.clone();
            self.finish(sp, &meta.id, entry, staged)?;
            return Ok(Some(Built::Adopted(slot.out.clone())));
        }
        Ok(None)
    }

    /// 写之前先把这次的位置、指纹留空记进生成记录（旧位置记进待删）：中途被打断的话，下次还认得新位置上的文件是这本书的，
    /// 旧位置也还会删。MTP 直写正式文件名（[`direct_write`]）时半截文件只能靠这条记录认出来；电脑上不预登记（省一次整份
    /// 记录的写），下次重新生成出逐字节相同的产物时认领它（`file_name_for` 的 `same_as`）。
    /// 返回要记进生成记录的条目（指纹留空）和「设备上已经是这次要传的字节、不用传」。
    pub(super) fn stage_registration(
        &self,
        meta: &Meta,
        sp: &Path,
        slot: &Slot,
        out: &Path,
        made: &Made,
        direct: bool,
    ) -> Result<(StateEntry, bool), String> {
        let mut entry =
            StateEntry::file(out.to_path_buf(), slot.root.clone(), String::new(), Vec::new());
        // 设备上这个位置放的就是我们上次传的、和这次逐字节相同（看记录的哈希和文件大小，不把设备上的文件读回来）
        let mut same = false;
        match &slot.prev {
            Some(p) => {
                entry.old = p
                    .placements()
                    .into_iter()
                    .filter(|x| x.path != out)
                    .collect();
                if p.path == out {
                    entry.fingerprint = p.fingerprint.clone(); // 同一位置重建：记录不用预先改
                    entry.sha = p.sha.clone();
                    same = p.sha == made.sha
                        && std::fs::metadata(&made.src).ok().map(|m| m.len())
                            == std::fs::metadata(out).ok().map(|m| m.len());
                    if direct && !same {
                        entry.fingerprint.clear(); // 指纹没变（强制重建）时，写到一半被打断的半截会被当成传好了
                    }
                }
                // 换位置：先把记录改成新位置（指纹留空＝没完成），旧位置记进待删——中途被打断的话，下次还认得新位置上的
                // 文件是这本书的，旧位置也还会删
                if entry.fingerprint.is_empty() || entry.old != p.old {
                    // 没完成的：哈希也留空（原地覆写到一半，设备上的已不是记着的那份，下次不能凭哈希当成一样）
                    let mut pre = entry.clone();
                    if pre.fingerprint.is_empty() {
                        pre.sha.clear();
                    }
                    self.put_entry(sp, &meta.id, pre)?;
                }
            }
            // 新书：传的过程中这个文件名先在内存里占住，同一轮里同名的别的书不会选它。电脑上不预登记（省一次整份记录的写），
            // 中途被打断的话，下次重新生成出逐字节相同的产物时认领它（`file_name_for` 的 `same_as`）；MTP 设备上直接写正式
            // 文件名，半截文件认领不了，要预登记
            None => {
                self.build.reserve(out, &meta.id);
                if direct {
                    self.put_entry(sp, &meta.id, entry.clone())?;
                }
            }
        }
        Ok((entry, same))
    }
}
/// 产物根目录和跟踪目录互相包含时的提示（生成和 `track` 共用）。
pub(crate) fn output_overlap(root: &Path, tracked: &Path) -> String {
    format!(
        "产物目录 {} 和跟踪的目录 {} 互相包含，生成出来的书会被当成新书入库、层层嵌套（跟踪的目录不要用模式 id 命名，也不要放在别的跟踪目录的产物目录里）",
        root.display(),
        tracked.display()
    )
}
/// 相对目录逐段换成设备上能建的名字（见 [`Library::output_dir`]）：只换有不能用的字符、或以空格或点结尾的那段。
pub(super) fn device_safe_rel(rel: &Path) -> PathBuf {
    rel.components()
        .map(|c| {
            let s = c.as_os_str().to_string_lossy();
            let bad = s
                .chars()
                .any(|ch| ch.is_control() || "\\:*?\"<>|".contains(ch))
                || s.ends_with([' ', '.']);
            if bad {
                bookconv::util::sanitize_filename(s.trim_end_matches([' ', '.']), "_")
            } else {
                s.into_owned()
            }
        })
        .collect()
}

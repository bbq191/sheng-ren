//! 生成记录的结构与位置改写（拆分自 `generate.rs`）：记录里的产物、前缀与大小写迁移、删文件与文件名。

use super::fsops::{case_alias, case_twin, distinct_on_disk, remove_product};
use super::types::{Placement, State, StateEntry};
use crate::deliver;
use crate::session::Devices;
use crate::Meta;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

impl StateEntry {
    /// 文件产物（电脑上、MTP 设备上）的记录。
    pub(super) fn file(path: PathBuf, root: PathBuf, fingerprint: String, old: Vec<Placement>) -> StateEntry {
        StateEntry {
            path,
            root,
            fingerprint,
            sha: String::new(),
            old,
            audit: Vec::new(),
        }
    }

    /// 这条记录名下的所有产物：现在的位置和待删的旧位置。
    pub(super) fn placements(&self) -> Vec<Placement> {
        let mut v = vec![Placement {
            path: self.path.clone(),
            root: self.root.clone(),
        }];
        v.extend(self.old.iter().cloned());
        v
    }
}

impl State {
    /// 设备接上了，把记录里旧前缀开头的绝对路径改写成新的 `devices_base/<链接名>` 前缀（`path`、`root`、`old`、`orphans`）。
    /// 只改记录：不碰文件、不重传、不删。改写后指向同一个文件的旧位置（路径和当前位置一样）从 `old` / `orphans` 里去掉，
    /// 免得后面当成"换位置"去删现在这份。返回改没改。
    pub(super) fn migrate_prefixes(&mut self, mappings: &[(PathBuf, PathBuf)]) -> bool {
        if mappings.is_empty() {
            return false;
        }
        let mut changed = false;
        for e in self.books.values_mut() {
            changed |= migrate_placement_prefix(&mut e.path, &mut e.root, mappings);
            for old in &mut e.old {
                changed |= migrate_placement_prefix(&mut old.path, &mut old.root, mappings);
            }
        }
        for o in &mut self.orphans {
            changed |= migrate_placement_prefix(&mut o.path, &mut o.root, mappings);
        }
        // 旧位置经过链接就是当前位置：不是两份文件，不该走"换位置删除"
        let mut current = BTreeSet::new();
        for e in self.books.values_mut() {
            current.insert(e.path.clone());
            let before = e.old.len();
            let mut seen = BTreeSet::new();
            e.old
                .retain(|o| o.path != e.path && seen.insert(o.path.clone()));
            changed |= e.old.len() != before;
        }
        let before = self.orphans.len();
        self.orphans.retain(|o| !current.contains(&o.path));
        changed |= self.orphans.len() != before;
        changed
    }

    /// 产物根目录只差大小写、新路径上的文件也在（不分大小写的设备）时，把记录原样挪到新根目录：
    /// 只改记录，不碰设备上的文件。返回改没改。
    ///
    /// 新路径上文件不在（大小写敏感的设备上是两个真目录）就不换，交给原来的"挪位置"流程。
    pub(super) fn rebase_case_root(&mut self, new_root: &Path) -> bool {
        let mut changed = false;
        for e in self.books.values_mut() {
            changed |= rebase_case_placement(&mut e.path, &mut e.root, new_root);
            for old in &mut e.old {
                changed |= rebase_case_placement(&mut old.path, &mut old.root, new_root);
            }
        }
        for o in &mut self.orphans {
            changed |= rebase_case_placement(&mut o.path, &mut o.root, new_root);
        }
        changed
    }

    /// 这个产物现在（或待删的旧位置里）是不是记在别的书名下（按路径认）。
    pub(super) fn claimed(&self, p: &Placement) -> bool {
        self.books
            .values()
            .any(|e| e.path == p.path || e.old.iter().any(|o| o.path == p.path))
    }

    /// 删掉这些产物（已经不在记录里的书名下），文件在各自的根目录以内删掉变空的目录。返回删不掉的（下次再删）：
    /// 可拔下的设备（MTP、USB 大容量存储的挂载目录下）上的文件，设备没挂上（[`Devices::unplugged`]）时不当成已删；相对路径（2026-10-31 以前 Move 上的旧记录）不动文件、记录直接丢掉。
    /// 调用前要先把本书的记录从 `self` 里拿掉，别的书还记着的不删。
    /// 删掉这些产物（[`State::delete_placements`]），删不掉的记进 `orphans`（下次接上设备再删）。
    pub(super) fn delete_into_orphans(&mut self, list: Vec<Placement>, devices: &Devices) {
        let left = self.delete_placements(list, None, devices);
        self.orphans.extend(left);
    }

    /// `current`：调用方手上、还没放回 `self` 的本书当前位置（`finish`），和别的书的一样不删。
    pub(super) fn delete_placements(
        &self,
        list: Vec<Placement>,
        current: Option<&Path>,
        devices: &Devices,
    ) -> Vec<Placement> {
        let mut left = Vec::new();
        for p in list {
            if self.claimed(&p) || current == Some(p.path.as_path()) {
                continue;
            }
            // 逐段只差大小写的另一个位置（汉王 documents/Documents、跟踪目录 Manga/manga）：不分大小写的设备上就是同一个文件，
            // 删它等于删现在这份。不删文件、不删目录，记录丢掉；大小写敏感的设备上会留下一个旧文件，宁可多留一份。
            if current.is_some_and(|c| case_alias(c, &p.path) && !case_twin(c, &p.path))
                || self.books.values().any(|e| {
                    e.placements()
                        .iter()
                        .any(|q| case_alias(&q.path, &p.path) && !case_twin(&q.path, &p.path))
                })
            {
                continue;
            }
            // 只差大小写的另一个名字正被用着（书名只改了大小写）：阅读器的文件系统多半不分大小写，删旧名字就是删现在这份
            let twins: Vec<&Path> = self
                .books
                .values()
                .flat_map(|e| {
                    std::iter::once(e.path.as_path()).chain(e.old.iter().map(|o| o.path.as_path()))
                })
                .chain(current)
                .filter(|q| case_twin(q, &p.path))
                .collect();
            // 相对路径：2026-10-31 以前 Move 上的旧记录（`文件夹/文件名`，Move 已经删了），不动、也不再记着
            if !p.path.is_absolute() {
                continue;
            }
            // 设备没挂上（[`Devices::unplugged`]：要真的挂着，不只看根目录在不在）
            if devices.unplugged(&p.path, &p.root) {
                left.push(p);
                continue;
            }
            if !twins.is_empty() {
                match distinct_on_disk(&p.path, &twins) {
                    Some(true) => {}
                    Some(false) => continue, // 同一个文件：不删，旧名字的记录丢掉
                    None => {
                        left.push(p); // 读不了目录：下次再看
                        continue;
                    }
                }
            }
            if !remove_product(&p.path) {
                left.push(p);
                continue;
            }
            // Kindle 书旁的 `.sdr`（进度）：书删了就没用了（删书再拷回 Kindle 也当新书），留着的话这个目录永远删不空。
            // 这本书只是换了目录或改了大小写（`carry_sidecar` 没挪过去时）不删，进度还留在旧位置。只看这本书的新位置：
            // 别的书同名不算
            let moved = current.is_some_and(|c| {
                let n = |x: &Path| x.file_name().map(|n| n.to_string_lossy().to_lowercase());
                n(c) == n(&p.path)
            });
            if !moved && p.path.extension().is_some_and(|e| e == "kfx") {
                let sdr = p.path.with_extension("sdr");
                if sdr.is_dir() {
                    let _ = std::fs::remove_dir_all(&sdr);
                }
            }
            // 传输线程可能正往这些目录里放别的书（刚建好目录、还没写进文件）：它那边碰到目录没了会重建（`deliver::put_file`）
            let mut dir = p.path.parent();
            while let Some(d) = dir.filter(|d| *d != p.root && d.starts_with(&p.root)) {
                if std::fs::remove_dir(d).is_err() {
                    break; // 不空（或删不了）：到此为止
                }
                dir = d.parent();
            }
        }
        left
    }

    /// 产物文件名：`书名.<ext>`。同一目录里和别的书撞名（不分大小写：阅读器的文件系统多半不分），或者目录里已有
    /// 一个不是本书产物的同名文件时，用 `书名 [id 前 6 位].<ext>`，不覆盖别人的文件。
    /// 本书在这个目录里已经用着其中一个名字的，一直用下去（名字稳定，重新生成后覆盖设备上的旧文件就行）。
    /// 给了 `same_as`（这次要放上去的产物）时，目录里不认识的同名文件和它逐字节相同的（以前手工拷上设备的）也可以用：认领它。
    /// `copied` 是本书以前放在电脑上的产物的文件名：设备上叫这个名字的文件是用户拷上去的那份，也算本书的。
    /// `taken` 是同一轮里别的书正在传、还没记进来的文件名（小写），也算别人的。
    pub(super) fn file_name_for(
        &self,
        meta: &Meta,
        dir: &Path,
        ext: &str,
        same_as: Option<&Path>,
        copied: Option<&str>,
        taken: &BTreeSet<String>,
    ) -> Result<String, String> {
        // 文件名跟修正后的书名（网站书名 + 卷次，2026-10-12 定）
        let base =
            bookconv::util::sanitize_filename(&crate::metadata::corrected_title(meta), &meta.id);
        let plain = format!("{base}.{ext}");
        let suffixed = format!("{base} [{}].{ext}", meta.id.get(..6).unwrap_or(&meta.id));
        if let Some(e) = self
            .books
            .get(&meta.id)
            .filter(|e| e.path.parent() == Some(dir))
        {
            if let Some(name) = e
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| *n == plain || *n == suffixed)
            {
                return Ok(name.to_string());
            }
        }
        let fold = |n: &str| n.to_lowercase();
        let existing: BTreeSet<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| fold(&e.file_name().to_string_lossy()))
            .collect();
        let others: BTreeSet<String> = self
            .books
            .iter()
            .filter(|(id, _)| *id != &meta.id)
            .flat_map(|(_, e)| e.placements())
            .filter(|p| p.path.parent() == Some(dir))
            .filter_map(|p| p.path.file_name().map(|n| fold(&n.to_string_lossy())))
            .collect();
        // 本书待删的旧文件正好叫这个名字（书名改回去了）：算本书的，可以用
        // 待删的孤儿（删书时设备没接上、这轮末尾才删的）也算：是本工具的旧产物，覆盖它就行，不用另起 `[id]` 名字
        let mine: BTreeSet<String> = self
            .books
            .get(&meta.id)
            .into_iter()
            .flat_map(StateEntry::placements)
            .chain(self.orphans.iter().cloned())
            .filter(|p| p.path.parent() == Some(dir))
            .filter_map(|p| p.path.file_name().map(|n| fold(&n.to_string_lossy())))
            .collect();
        let adopt = |n: &str| {
            copied == Some(n) || same_as.is_some_and(|s| deliver::same_bytes(s, &dir.join(n)))
        };
        let free = |n: &str| {
            !others.contains(&fold(n))
                && !taken.contains(&fold(n))
                && (!existing.contains(&fold(n)) || mine.contains(&fold(n)) || adopt(n))
        };
        if free(&plain) {
            Ok(plain)
        } else if free(&suffixed) {
            Ok(suffixed)
        } else {
            Err(format!(
                "{} 里已经有 {plain} 和 {suffixed}，都不是这本书的产物，不覆盖",
                dir.display()
            ))
        }
    }
}
/// 一个产物记录的 (path, root) 跟着设备前缀换：旧前缀开头的改成新前缀，只改字符串。
fn migrate_placement_prefix(
    path: &mut PathBuf,
    root: &mut PathBuf,
    mappings: &[(PathBuf, PathBuf)],
) -> bool {
    let mut changed = false;
    if let Some(new) = rebase_prefix(path, mappings) {
        *path = new;
        changed = true;
    }
    if let Some(new) = rebase_prefix(root, mappings) {
        *root = new;
        changed = true;
    }
    changed
}

fn rebase_prefix(path: &Path, mappings: &[(PathBuf, PathBuf)]) -> Option<PathBuf> {
    mappings
        .iter()
        .find_map(|(old, new)| path.strip_prefix(old).ok().map(|rest| new.join(rest)))
}

/// 一个产物记录的 (path, root) 跟着根目录的大小写改：根目录只差大小写、`new_root + 相对路径` 的文件在才换。
fn rebase_case_placement(path: &mut PathBuf, root: &mut PathBuf, new_root: &Path) -> bool {
    if !case_alias(root, new_root) {
        return false;
    }
    let Ok(rel) = path.strip_prefix(&*root) else {
        return false;
    };
    let new_path = new_root.join(rel);
    if !new_path.is_file() {
        return false;
    }
    *root = new_root.to_path_buf();
    *path = new_path;
    true
}

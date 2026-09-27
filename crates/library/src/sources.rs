//! 跟踪原件目录（`track`）并把它镜像进书库（`sync`）。

use crate::fsutil::write_atomic;
use crate::{Added, Library, Meta};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 能入库的文件扩展名（小写）。
pub const SUPPORTED_EXTS: &[&str] = &["epub", "pdf", "cbz", "mobi", "azw", "azw3", "prc", "fb2"];

fn is_supported(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// 目录下（递归，不跟符号链接，跳过隐藏文件和目录）所有能入库的文件，按路径排序。
pub fn book_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() && is_supported(&e.path()) {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

/// 跟踪的原件目录，以及其中每个文件上次看到时的样子（`sources.json`）。
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Sources {
    dirs: Vec<PathBuf>,
    #[serde(default)]
    files: BTreeMap<PathBuf, Seen>,
}

/// 大小和修改时间都没变就认为内容没变，不重读、不重算哈希（大漫画一本上百 MB）。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    size: u64,
    mtime_ns: u64,
    /// 空 = 上次入库失败（DRM、损坏等）：文件没变就不再重试、不再重复报错。
    id: String,
}


/// `sync` 过程中每个文件的结果（给命令行逐条打印）。
pub enum SyncEvent<'a> {
    Added(&'a Path, &'a Meta),
    /// 原件内容变了：旧版本的条目已换成新的。参数是旧书名。
    Updated(&'a Path, &'a Meta, &'a str),
    /// 原件不在了（`prune` 时已删掉对应条目）。
    Missing(&'a Path, &'a str, bool),
    Failed(&'a Path, &'a str),
}

/// `sync` 的汇总。
#[derive(Default, Debug)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub pruned: usize,
    pub failed: usize,
}

impl Library {
    fn load_sources(&self) -> Sources {
        std::fs::read(self.root.join("sources.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_sources(&self, s: &Sources) -> Result<(), String> {
        write_atomic(&self.root.join("sources.json"), serde_json::to_string_pretty(s).unwrap().as_bytes())
    }

    /// 跟踪的原件目录。
    pub fn tracked(&self) -> Vec<PathBuf> {
        self.load_sources().dirs
    }

    /// 开始跟踪一个目录（之后 `sync` 会把它镜像进书库）。已跟踪返回 `false`。
    pub fn track(&self, dir: &Path) -> Result<bool, String> {
        let dir = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if !dir.is_dir() {
            return Err(format!("{} 不是目录", dir.display()));
        }
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        if dir.starts_with(&root) || root.starts_with(&dir) {
            return Err("不能跟踪书库自己所在的目录".into());
        }
        let mut s = self.load_sources();
        if s.dirs.contains(&dir) {
            return Ok(false);
        }
        s.dirs.push(dir);
        self.save_sources(&s).map(|_| true)
    }

    /// 不再跟踪一个目录。已经入库的书保留。没在跟踪返回 `false`。
    pub fn untrack(&self, dir: &Path) -> Result<bool, String> {
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let mut s = self.load_sources();
        let before = s.dirs.len();
        s.dirs.retain(|d| d != &dir);
        if s.dirs.len() == before {
            return Ok(false);
        }
        s.files.retain(|p, _| !p.starts_with(&dir));
        self.save_sources(&s).map(|_| true)
    }

    /// 把跟踪的目录镜像进书库：
    /// - 新文件入库；大小和修改时间没变的文件跳过（不重读）；
    /// - 内容变了的文件入库新版本，旧版本的条目（只有这个原件用它时）连同产物删掉；
    /// - 移动、改名的文件按内容认出来，不重复入库；
    /// - 原件不在了的只报告；`prune` 时删掉对应条目（别的原件还用着同一内容的不删）。
    pub fn sync(&self, prune: bool, mut on: impl FnMut(SyncEvent)) -> Result<SyncReport, String> {
        let mut src = self.load_sources();
        let mut rep = SyncReport::default();
        let mut present: BTreeMap<PathBuf, Seen> = BTreeMap::new();
        let mut replaced: Vec<(PathBuf, String)> = Vec::new(); // (原件, 旧 id)
        for dir in src.dirs.clone() {
            if !dir.is_dir() {
                // 目录整个不见了（U 盘没插等）：里面的文件既不算新增也不算删除，原样保留记录
                present.extend(src.files.iter().filter(|(p, _)| p.starts_with(&dir)).map(|(p, s)| (p.clone(), s.clone())));
                continue;
            }
            for path in book_files(&dir) {
                let Some((size, mtime_ns)) = crate::file_stat(&path) else { continue };
                let old = src.files.get(&path);
                if let Some(o) = old.filter(|o| o.size == size && o.mtime_ns == mtime_ns && (o.id.is_empty() || self.entry_dir(&o.id).is_dir())) {
                    present.insert(path, o.clone());
                    rep.unchanged += 1;
                    continue;
                }
                let old = old.filter(|o| !o.id.is_empty());
                match self.add_file(&path) {
                    Ok(added) => {
                        let (m, is_new) = match added {
                            Added::New(m) => (m, true),
                            Added::Existing(m) => (m, false),
                        };
                        match old.filter(|o| o.id != m.id) {
                            Some(o) => {
                                replaced.push((path.clone(), o.id.clone()));
                                let old_title = self.read_meta(&o.id).map(|x| x.title).unwrap_or_default();
                                on(SyncEvent::Updated(&path, &m, &old_title));
                                rep.updated += 1;
                            }
                            None if is_new => {
                                on(SyncEvent::Added(&path, &m));
                                rep.added += 1;
                            }
                            None => rep.unchanged += 1, // 已在库里（改名、移动，或之前手动 add 过）
                        }
                        present.insert(path, Seen { size, mtime_ns, id: m.id });
                    }
                    Err(e) => {
                        on(SyncEvent::Failed(&path, &e));
                        rep.failed += 1;
                        present.insert(path, Seen { size, mtime_ns, id: String::new() });
                    }
                }
            }
        }
        let live_ids: std::collections::HashSet<&str> = present.values().map(|s| s.id.as_str()).filter(|id| !id.is_empty()).collect();
        // 内容变了的：旧版本没有别的原件在用就删掉
        for (_, old_id) in &replaced {
            if !live_ids.contains(old_id.as_str()) {
                let _ = self.remove(old_id);
            }
        }
        // 原件不在了的
        let mut kept_missing = BTreeMap::new();
        for (path, seen) in &src.files {
            if seen.id.is_empty() || present.contains_key(path) || replaced.iter().any(|(p, _)| p == path) {
                continue;
            }
            if live_ids.contains(seen.id.as_str()) {
                continue; // 移动或改名了，内容还在
            }
            let title = self.read_meta(&seen.id).map(|m| m.title).unwrap_or_default();
            let removed = prune && self.remove(&seen.id).is_ok();
            on(SyncEvent::Missing(path, &title, removed));
            rep.missing += 1;
            if removed {
                rep.pruned += 1;
            } else if self.entry_dir(&seen.id).is_dir() {
                kept_missing.insert(path.clone(), seen.clone()); // 继续记着，下次还报告
            }
        }
        present.extend(kept_missing);
        src.files = present;
        self.save_sources(&src)?;
        Ok(rep)
    }

}

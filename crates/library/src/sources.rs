//! 跟踪原件目录（`track`）并把它镜像进书库（`sync`）。

use crate::fsutil::{read_json, write_json};
use crate::{Added, Library, Meta, BUSY, NOT_UTF8};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 能入库的文件扩展名（小写）。2026-09-29 起只有 EPUB 和 CBZ（MOBI/AZW3/FB2/PDF 等不再支持）。
pub const SUPPORTED_EXTS: &[&str] = &["epub", "cbz"];

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
#[derive(Default, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Sources {
    dirs: Vec<PathBuf>,
    #[serde(default)]
    files: BTreeMap<PathBuf, Seen>,
    /// 内容变了的文件的旧版本条目，上次没删掉的：下次 `sync` 再删（继续跟踪，不会变成没人管的条目）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stale: Vec<String>,
}

/// 大小和修改时间都没变就认为内容没变，不重读、不重算哈希（大漫画一本上百 MB）。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    size: u64,
    mtime_ns: u64,
    /// 空 = 上次入库失败（DRM、损坏等）：文件没变就不再重试、不再重复报错。
    /// 原来有旧版本、新版本入库失败时，这里仍是旧版本的 id（旧版本继续跟踪），大小和修改时间是新文件的。
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

/// 跨轮次记住已经报过的问题（`sync --watch` 用）：原件不在了、文件名不是 UTF-8 的，只在新出现时报一次
/// （也只在那一轮计数），问题消失后再出现会重新报。
#[derive(Default)]
pub struct SyncMemo {
    missing: HashSet<PathBuf>,
    bad_names: HashSet<PathBuf>,
}

impl Sources {
    /// 跟踪的目录（`track` 时已规范化成绝对路径）。
    pub(crate) fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }
}

impl Library {
    fn load_sources(&self) -> Sources {
        read_json(&self.root.join("sources.json")).unwrap_or_default()
    }

    fn save_sources(&self, s: &Sources) -> Result<(), String> {
        write_json(&self.root.join("sources.json"), s)
    }

    /// 跟踪的原件目录。
    pub fn tracked(&self) -> Vec<PathBuf> {
        self.load_sources().dirs
    }

    /// 开始跟踪一个目录（之后 `sync` 会把它镜像进书库）。已跟踪返回 `false`。
    /// 和已跟踪的目录互相包含的拒绝（同一个文件会被跟踪两遍）。
    pub fn track(&self, dir: &Path) -> Result<bool, String> {
        let dir = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if !dir.is_dir() {
            return Err(format!("{} 不是目录", dir.display()));
        }
        if dir.to_str().is_none() {
            return Err(format!("{}：{NOT_UTF8}", dir.display()));
        }
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        if dir.starts_with(&root) || root.starts_with(&dir) {
            return Err("不能跟踪书库自己所在的目录".into());
        }
        let mut s = self.load_sources();
        if s.dirs.contains(&dir) {
            return Ok(false);
        }
        if let Some(d) = s.dirs.iter().find(|d| dir.starts_with(d) || d.starts_with(&dir)) {
            return Err(format!("{} 和已跟踪的 {} 互相包含（同一个文件会被跟踪两遍）：要换的话先 untrack 那个", dir.display(), d.display()));
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

    /// 把跟踪的目录镜像进书库（一次性运行：每个问题都报）。见 [`Library::sync_with`]。
    pub fn sync(&self, prune: bool, on: impl FnMut(SyncEvent)) -> Result<SyncReport, String> {
        self.sync_with(prune, &mut SyncMemo::default(), on)
    }

    /// 把跟踪的目录镜像进书库：
    /// - 新文件入库；大小和修改时间没变的文件跳过（不重读）；
    /// - 内容变了的文件入库新版本，旧版本的条目（没有别的原件在用时）连同产物删掉；新版本入库失败的，旧版本继续跟踪；
    /// - 移动、改名的文件按内容认出来，不重复入库；同一本书有两份、删了其中一份的，索引改记成还在的那份；
    /// - 原件不在了的只报告；`prune` 时删掉对应条目（别的原件还用着同一内容的、条目记着的原件在别处还在的不删）；
    /// - 文件名不是 UTF-8 的报错，不入库也不记下来（改名后下次同步入库）。
    ///
    /// `memo` 跨轮次记住已经报过的问题（`--watch`），只在新出现时报一次。没有变化时不写 `sources.json`。
    pub fn sync_with(&self, prune: bool, memo: &mut SyncMemo, mut on: impl FnMut(SyncEvent)) -> Result<SyncReport, String> {
        let src = self.load_sources();
        let mut rep = SyncReport::default();
        let mut present: BTreeMap<PathBuf, Seen> = BTreeMap::new();
        // (原件, 旧 id)：内容变了的旧版本，加上以前没删掉的
        let mut replaced: Vec<(PathBuf, String)> = src.stale.iter().map(|id| (self.entry_dir(id), id.clone())).collect();
        let mut bad_now: HashSet<PathBuf> = HashSet::new();
        for dir in &src.dirs {
            if !dir.is_dir() {
                // 目录整个不见了（U 盘没插等）：里面的文件既不算新增也不算删除，原样保留记录
                present.extend(src.files.iter().filter(|(p, _)| p.starts_with(dir)).map(|(p, s)| (p.clone(), s.clone())));
                continue;
            }
            for path in book_files(dir) {
                if present.contains_key(&path) || bad_now.contains(&path) {
                    continue; // 早期版本允许过互相包含的跟踪目录：同一个文件只处理一次
                }
                if path.to_str().is_none() {
                    if memo.bad_names.insert(path.clone()) {
                        on(SyncEvent::Failed(&path, NOT_UTF8));
                        rep.failed += 1;
                    }
                    bad_now.insert(path);
                    continue;
                }
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
                        if e.starts_with(BUSY) {
                            // 正在写入：这次看到的不算数，旧记录照旧（没有就不记），下一轮再试
                            if let Some(o) = src.files.get(&path) {
                                present.insert(path, o.clone());
                            }
                        } else {
                            // 记下这次看到的大小和修改时间：文件没变就不再重试。有旧版本的继续跟踪旧版本
                            let id = old.map(|o| o.id.clone()).unwrap_or_default();
                            present.insert(path, Seen { size, mtime_ns, id });
                        }
                    }
                }
            }
        }
        memo.bad_names.retain(|p| bad_now.contains(p));
        let live: HashMap<String, PathBuf> = present.iter().filter(|(_, s)| !s.id.is_empty()).map(|(p, s)| (s.id.clone(), p.clone())).collect();

        // 内容变了的：旧版本没有别的原件在用、条目记着的原件也不在别处，就删掉；删不掉的下次再删
        let mut stale: Vec<String> = Vec::new();
        for (path, old_id) in &replaced {
            if live.contains_key(old_id) || !self.entry_dir(old_id).is_dir() || stale.contains(old_id) || self.original_elsewhere(old_id, path, &present) {
                continue;
            }
            if let Err(e) = self.remove(old_id) {
                on(SyncEvent::Failed(path, &format!("删旧版本 {old_id} 失败：{e}（下次 sync 再删）")));
                rep.failed += 1;
                stale.push(old_id.clone());
            }
        }

        // 原件不在了的
        let mut kept_missing = BTreeMap::new();
        let mut missing_now: HashSet<PathBuf> = HashSet::new();
        for (path, seen) in &src.files {
            if seen.id.is_empty() || present.contains_key(path) || !self.entry_dir(&seen.id).is_dir() {
                continue;
            }
            if !is_supported(path) && path.is_file() {
                // 早期版本收的、现在不再支持的格式（MOBI/PDF 等）：文件还在，只是不再遍历它。不算原件不在，条目不删
                kept_missing.insert(path.clone(), seen.clone());
                continue;
            }
            if let Some(live_path) = live.get(&seen.id) {
                // 移动、改名了，或者还有一份：条目记着的原件不在了的话，改记成还在的那份
                if let Err(e) = self.repoint(&seen.id, live_path, &present[live_path]) {
                    on(SyncEvent::Failed(path, &e));
                    rep.failed += 1;
                }
                continue;
            }
            if self.original_elsewhere(&seen.id, path, &present) {
                continue; // 条目记着的原件在跟踪目录以外（add 进来的），还在：这本书没丢
            }
            let title = self.read_meta(&seen.id).map(|m| m.title).unwrap_or_default();
            if prune {
                match self.remove(&seen.id) {
                    Ok(_) => {
                        on(SyncEvent::Missing(path, &title, true));
                        rep.missing += 1;
                        rep.pruned += 1;
                    }
                    Err(e) => {
                        on(SyncEvent::Failed(path, &format!("原件不在了，从书库删掉失败：{e}")));
                        rep.failed += 1;
                        kept_missing.insert(path.clone(), seen.clone());
                    }
                }
                continue;
            }
            // 继续记着，下次还报告（watch 时只在新出现时报一次）
            kept_missing.insert(path.clone(), seen.clone());
            missing_now.insert(path.clone());
            if memo.missing.insert(path.clone()) {
                on(SyncEvent::Missing(path, &title, false));
                rep.missing += 1;
            }
        }
        memo.missing.retain(|p| missing_now.contains(p));
        present.extend(kept_missing);
        let new = Sources { dirs: src.dirs.clone(), files: present, stale };
        if new != src {
            self.save_sources(&new)?;
        }
        Ok(rep)
    }

    /// 条目 `id` 记着的原件不是 `path`、不是跟踪目录里现在的文件，而且还在（比如 `add` 进来的另一份）。
    fn original_elsewhere(&self, id: &str, path: &Path, present: &BTreeMap<PathBuf, Seen>) -> bool {
        self.read_meta(id).is_some_and(|m| {
            let p = Path::new(&m.source_path);
            m.master.is_empty() && !m.source_path.is_empty() && p != path && !present.contains_key(p) && p.is_file()
        })
    }

    /// 条目记着的原件不在了，而跟踪目录里还有同一内容的 `live`：改记成它。
    fn repoint(&self, id: &str, live: &Path, seen: &Seen) -> Result<(), String> {
        let Some(mut m) = self.read_meta(id) else { return Ok(()) };
        let Some(s) = live.to_str() else { return Ok(()) };
        if !m.master.is_empty() || m.source_path == s || Path::new(&m.source_path).exists() {
            return Ok(());
        }
        m.source_path = s.to_string();
        m.source = live.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        (m.source_size, m.source_mtime_ns) = (seen.size, seen.mtime_ns);
        self.save_meta(&m)
    }
}

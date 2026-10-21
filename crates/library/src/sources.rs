//! 跟踪原件（`track`）并把它镜像进书库（`sync`）。
//!
//! 跟踪的是一组**条目**（2026-10-19 定：每台设备的跟踪库可以不同，既可以是目录、也可以是单本或多本书，混着来）：
//! 每个条目是一个目录（递归）或一个文件，带着它给哪几台设备（空＝全部）。一本书传给哪些设备，取覆盖它的条目的并集；
//! 不再被任何条目覆盖的书（untrack 了）下次 `sync` 连同产物从书库删掉，从某台的范围里拿掉的书下次 `sync` 从那台上删掉
//! （维护者定：删）。设备上的子目录一律是原件所在目录相对**书根目录**的路径（维护者定：原目录结构是什么就是什么，跟踪
//! `books/漫画/死亡筆記/卷01.epub` 放进设备的 `漫画/死亡筆記/`），见 [`Sources::placement`]。

use crate::{Added, Library, Meta, BUSY, NOT_UTF8};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 能入库的文件扩展名（小写）。2026-09-29 起只有 EPUB 和 CBZ（MOBI/AZW3/FB2/PDF 等不再支持）。
pub const SUPPORTED_EXTS: &[&str] = &["epub", "cbz"];

fn is_supported(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// 目录下（递归，不跟符号链接，跳过隐藏文件和目录）所有能入库的文件，按路径排序。读不了的目录跳过（见 [`scan_books`]）。
pub fn book_files(dir: &Path) -> Vec<PathBuf> {
    scan_books(dir).files
}

/// [`scan_books`] 的结果。
#[derive(Default, Debug)]
pub struct BookScan {
    /// 能入库的文件，按路径排序。
    pub files: Vec<PathBuf>,
    /// 读不了的目录（没有读或进入权限、读的时候出错）和原因：里面有什么不知道，`sync` 对它们下面的书"状态未知"、原样保留。
    pub unreadable: Vec<(PathBuf, String)>,
}

/// 目录下（递归，不跟符号链接，跳过隐藏文件和目录）所有能入库的文件，以及读不了的目录。
/// 列得出文件名、但取不到文件属性的目录（有读权限没有进入权限）也算读不了，其中的文件一个都不报。
pub fn scan_books(dir: &Path) -> BookScan {
    let mut scan = BookScan::default();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = match std::fs::read_dir(&d) {
            Ok(rd) => rd,
            Err(e) => {
                scan.unreadable.push((d, e.to_string()));
                continue;
            }
        };
        let (mut files, mut dirs) = (Vec::new(), Vec::new());
        let mut err = None;
        for e in rd {
            let e = match e {
                Ok(e) => e,
                Err(x) => {
                    err = Some(x.to_string());
                    break;
                }
            };
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let ft = match e.file_type() {
                Ok(ft) => ft,
                Err(x) => {
                    err = Some(format!("{}: {x}", e.path().display()));
                    break;
                }
            };
            if ft.is_dir() {
                dirs.push(e.path());
            } else if ft.is_file() && is_supported(&e.path()) {
                // 取得到属性才算看见了（`sync` 要大小和修改时间）；文件刚好被删了的不算
                match e.metadata() {
                    Ok(_) => files.push(e.path()),
                    Err(x) if x.kind() == std::io::ErrorKind::NotFound => {}
                    Err(x) => {
                        err = Some(format!("{}: {x}", e.path().display()));
                        break;
                    }
                }
            }
        }
        match err {
            Some(e) => scan.unreadable.push((d, e)),
            None => {
                scan.files.extend(files);
                stack.extend(dirs);
            }
        }
    }
    scan.files.sort();
    scan.unreadable.sort();
    scan
}

/// 两个目录互相包含（相同也算）。
pub(crate) fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// 跟踪的一个条目：一个目录（递归）或一个文件，给哪几台设备。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tracked {
    /// 绝对路径（`track` 时规范化过）。
    pub path: PathBuf,
    /// 是单个文件（不是目录）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file: bool,
    /// 给哪几台设备（模式 id，排好序）；空＝全部设备（包括以后加的模式）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<String>,
    /// 放法（`track --top`）：从这一层起放——目录条目在设备上以它自己的名字为最上一层（跟踪 `book/漫画/死亡筆記` → 设备上
    /// `死亡筆記/卷01`），单本放设备的根目录。不写是照书库结构（相对书根目录）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub top: bool,
}

impl Tracked {
    /// 这个条目覆盖这个原件：目录条目是它下面的文件，文件条目是它自己。
    pub fn covers(&self, p: &Path) -> bool {
        if self.file { p == self.path } else { p.starts_with(&self.path) }
    }

    /// 给这台设备。
    pub fn for_device(&self, id: &str) -> bool {
        self.devices.is_empty() || self.devices.iter().any(|d| d == id)
    }

    /// 原件 `p` 按这个条目的放法在设备上的子目录；`root` 是书根目录（没有的话相对条目自己，见 [`Sources::placement`]）。
    fn rel(&self, p: &Path, root: Option<&Path>) -> PathBuf {
        let parent = p.parent().unwrap_or(Path::new(""));
        let base: &Path = match (self.top, self.file) {
            (true, true) => return PathBuf::new(),
            (true, false) => self.path.parent().unwrap_or(&self.path),
            (false, _) => match root.filter(|r| p.starts_with(r)) {
                Some(r) => r,
                // 没有书根目录（以前跟踪过几个目录的书库）：目录相对它自己，单本是所在的文件夹名
                None if self.file => return parent.file_name().map(PathBuf::from).unwrap_or_default(),
                None => &self.path,
            },
        };
        parent.strip_prefix(base).unwrap_or(Path::new("")).to_path_buf()
    }

    /// 同一本书被一台设备的几个条目覆盖时用哪个的放法（大的优先）：单本 > 深的目录 > 浅的目录。
    fn specificity(&self) -> (bool, usize) {
        (self.file, self.path.components().count())
    }

    /// 给人看的设备范围。
    pub fn devices_shown(&self) -> String {
        if self.devices.is_empty() { "全部设备".into() } else { self.devices.join("、") }
    }
}

/// 跟踪的条目，以及其中每个文件上次看到时的样子（`sources.json`）。
#[derive(Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "SourcesFile")]
pub(crate) struct Sources {
    /// 书根目录：设备上的子目录按原件相对它的路径放；跟踪的条目都在它里面。第一次 `track` 时定下（目录就是它，单本是它所在的目录），
    /// `track --root=` 换。2026-10-19 以前的书库读进来时，只跟踪一个目录的就是那个目录。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<PathBuf>,
    entries: Vec<Tracked>,
    #[serde(default)]
    pub(crate) files: BTreeMap<PathBuf, Seen>,
    /// 内容变了的文件的旧版本条目，上次没删掉的：下次 `sync` 再删（继续跟踪，不会变成没人管的条目）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stale: Vec<String>,
}

/// `sources.json` 读进来的样子：2026-10-19 以前只有 `dirs`（目录、全部设备），读进来改成条目。
#[derive(Deserialize)]
struct SourcesFile {
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default)]
    dirs: Vec<PathBuf>,
    #[serde(default)]
    entries: Vec<Tracked>,
    #[serde(default)]
    files: BTreeMap<PathBuf, Seen>,
    #[serde(default)]
    stale: Vec<String>,
}

impl From<SourcesFile> for Sources {
    fn from(f: SourcesFile) -> Sources {
        let root = f.root.or_else(|| (f.entries.is_empty() && f.dirs.len() == 1).then(|| f.dirs[0].clone()));
        let mut entries = f.entries;
        for d in f.dirs {
            if !entries.iter().any(|e| !e.file && e.path == d) {
                entries.push(Tracked { path: d, file: false, devices: Vec::new(), top: false });
            }
        }
        Sources { root, entries, files: f.files, stale: f.stale }
    }
}

/// [`Library::track_with`] 的结果。
#[derive(Debug, PartialEq, Eq)]
pub enum TrackChange {
    /// 新跟踪的。
    New,
    /// 已经在跟踪，设备范围或放法改了。
    Changed,
    /// 已经在跟踪，什么都没变。
    Same,
}

/// [`Library::untrack_with`] 的结果。
#[derive(Debug, PartialEq, Eq)]
pub enum UntrackChange {
    /// 整条去掉了：这么多本下次 `sync` 从书库删掉（没被别的条目覆盖的，按上次看到的算）。
    Removed(usize),
    /// 只去掉了几台设备：这么多本下次 `sync` 从这几台上删掉。
    Narrowed(usize),
    /// 本来就没在跟踪。
    NotTracked,
}

/// 大小和修改时间都没变就认为内容没变，不重读、不重算哈希（大漫画一本上百 MB）。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Seen {
    size: u64,
    mtime_ns: u64,
    /// 空 = 上次入库失败（DRM、损坏等）。
    /// 原来有旧版本、新版本入库失败时，这里仍是旧版本的 id（旧版本继续跟踪），大小和修改时间是新文件的。
    id: String,
    /// 这个文件（这个大小、修改时间）上次入库失败的记号：`<booklib 的提交号> <时刻（Unix 秒）>`；没失败是空。文件没变时
    /// 不马上重试、不重复报错，但升级了 booklib（可能修好了）或过了 [`RETRY_FAILED_AFTER`]（偶发的读错误）就再试一次
    /// （2026-10-09 审计：以前记下就永远不再试）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    failed: String,
}

/// 入库失败的文件，同一版 booklib 隔多久再试一次。
const RETRY_FAILED_AFTER: u64 = 3600;

/// 现在这版 booklib 记下的入库失败记号（见 [`Seen::failed`]）。
fn failed_mark() -> String {
    format!("{} {}", env!("BOOKLIB_GIT_COMMIT"), crate::now())
}

/// 入库失败的记号该不该重试了：换了 booklib，或过了 [`RETRY_FAILED_AFTER`]。读不懂的记号算该重试。
fn retry_due(mark: &str) -> bool {
    match mark.split_once(' ') {
        Some((commit, at)) => commit != env!("BOOKLIB_GIT_COMMIT") || at.parse::<u64>().map_or(true, |t| crate::now().saturating_sub(t) >= RETRY_FAILED_AFTER),
        None => true,
    }
}

/// `sync` 过程中每个文件的结果（给命令行逐条打印）。
pub enum SyncEvent<'a> {
    Added(&'a Path, &'a Meta),
    /// 原件内容变了：旧版本的条目已换成新的。参数是旧书名。
    Updated(&'a Path, &'a Meta, &'a str),
    /// 原件不在了（不是挪动）：原件路径、书名、是不是已从书库删掉（连同产物；[`Prune::Keep`] 时不删）。
    Missing(&'a Path, &'a str, bool),
    /// 不再被任何跟踪条目覆盖（untrack 了）：原件路径、书名、是不是已从书库删掉（连同产物；[`Prune::Keep`] 时不删）。
    Released(&'a Path, &'a str, bool),
    Failed(&'a Path, &'a str),
    /// 读不了的目录（权限、读出错）和原因：里面的书状态未知，原样保留（不删、不报"原件不在"、不入库新的）。
    Unreadable(&'a Path, &'a str),
}

/// 原件不在了（不是挪动）的书怎么处理。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Prune {
    /// 缺省：书库严格镜像跟踪目录，跟踪目录里登记过的书原件不在了就连同产物从书库删掉（2026-10-06 定）。
    /// 删的只是索引和能重建的产物，原件不动；删错了（比如文件只是暂时挪开）放回去再 `sync` 就回来。
    #[default]
    Auto,
    /// `--keep`：这一次不删，只报告。
    Keep,
}

/// `sync` 的汇总。
#[derive(Default, Debug)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub pruned: usize,
    /// 不再跟踪的（见 [`SyncEvent::Released`]），删了的也算进 `pruned`。
    pub released: usize,
    pub failed: usize,
    /// 读不了的目录数（见 [`SyncEvent::Unreadable`]）。
    pub unreadable: usize,
}

/// 跨轮次记住已经报过的问题（`sync --watch` 用）：原件不在了、文件名不是 UTF-8 的，只在新出现时报一次
/// （也只在那一轮计数），问题消失后再出现会重新报。
#[derive(Default)]
pub struct SyncMemo {
    missing: HashSet<PathBuf>,
    bad_names: HashSet<PathBuf>,
    unreadable: HashSet<PathBuf>,
}

/// 一次 [`Library::sync_with`] 的中间结果，几步之间传递。
struct SyncRound<'s> {
    /// 上次的 `sources.json`。
    src: &'s Sources,
    rep: SyncReport,
    /// 这一轮看到的（或原样当作还在的）跟踪目录里的文件。
    present: BTreeMap<PathBuf, Seen>,
    /// (原件, 旧 id)：内容变了的旧版本，加上以前没删掉的。
    replaced: Vec<(PathBuf, String)>,
    /// 这一轮文件名不是 UTF-8 的、读不了的目录（`SyncMemo` 据此忘掉已经好了的）。
    bad_now: HashSet<PathBuf>,
    unreadable_now: HashSet<PathBuf>,
}

impl<'s> SyncRound<'s> {
    fn new(src: &'s Sources, replaced: Vec<(PathBuf, String)>) -> SyncRound<'s> {
        SyncRound { src, rep: SyncReport::default(), present: BTreeMap::new(), replaced, bad_now: HashSet::new(), unreadable_now: HashSet::new() }
    }

    /// 这个目录下（或就是这个文件）登记过的文件原样当作还在（不新增、不删除、记录不变）。
    fn keep_under(&mut self, dir: &Path) {
        self.present.extend(self.src.files.iter().filter(|(p, _)| p.starts_with(dir)).map(|(p, s)| (p.clone(), s.clone())));
    }
}

/// 条目去掉这几台设备（`all` 是全部阅读模式：给全部设备的条目改成剩下的几台）；一台不剩返回 `None`。
fn without_devices(mut e: Tracked, drop: &[String], all: &[String]) -> Option<Tracked> {
    let current = if e.devices.is_empty() { all.to_vec() } else { std::mem::take(&mut e.devices) };
    e.devices = current.into_iter().filter(|d| !drop.contains(d)).collect();
    (!e.devices.is_empty()).then_some(e)
}

impl Sources {
    /// 跟踪的目录（条目里的目录）。
    pub(crate) fn dirs(&self) -> impl Iterator<Item = &PathBuf> {
        self.entries.iter().filter(|e| !e.file).map(|e| &e.path)
    }

    /// 原件 `p` 被跟踪着（有条目覆盖它）。
    fn covered(&self, p: &Path) -> bool {
        self.entries.iter().any(|e| e.covers(p))
    }

    /// 这本书（`id`，原件 `source`）给不给这台设备：覆盖它的（原件路径，以及跟踪目录里同一内容的别的文件）条目里有给这台的。
    /// 没有条目覆盖：`add` 进来的、网址书给全部设备；以前跟踪、现在不跟踪了的（还记在 `files` 里，下次 `sync` 删）谁都不给。
    pub(crate) fn wants(&self, id: &str, source: Option<&Path>, device: &str) -> bool {
        let paths: Vec<&Path> = source.into_iter().chain(self.files.iter().filter(|(_, s)| s.id == id).map(|(p, _)| p.as_path())).collect();
        let covering: Vec<&Tracked> = self.entries.iter().filter(|e| paths.iter().any(|p| e.covers(p))).collect();
        if covering.is_empty() {
            return !paths.iter().any(|p| self.files.contains_key(*p));
        }
        covering.iter().any(|e| e.for_device(device))
    }

    /// 原件 `p` 在这台设备上放哪：(电脑上的产物目录以哪个目录为准, 子目录)。用覆盖它、给这台的最具体的条目（[`Tracked::specificity`]）
    /// 的放法（[`Tracked::rel`]）。没有给这台的条目覆盖它返回 `None`。
    pub(crate) fn placement(&self, p: &Path, device: &str) -> Option<(PathBuf, PathBuf)> {
        let e = self.entries.iter().filter(|e| e.covers(p) && e.for_device(device)).max_by_key(|e| e.specificity())?;
        let base = self.root.clone().filter(|r| p.starts_with(r)).unwrap_or_else(|| if e.file { p.parent().and_then(Path::parent).unwrap_or(Path::new("")).to_path_buf() } else { e.path.clone() });
        Some((base, e.rel(p, self.root.as_deref())))
    }

    /// `add` 把 `path` 换成了新版本 `id`（旧版本 `old_ids` 已删）：这个文件在跟踪目录里、记着的是旧版本的话，改记成新版本
    /// （大小、修改时间取现在的），下次 `sync` 不再当成又改了一次。返回改了没有。
    pub(crate) fn record_new_version<'a>(&mut self, path: &Path, id: &str, mut old_ids: impl Iterator<Item = &'a str>) -> bool {
        let Some(seen) = self.files.get_mut(path) else { return false };
        if !old_ids.any(|o| o == seen.id) {
            return false;
        }
        let Some((size, mtime_ns)) = crate::file_stat(path) else { return false };
        *seen = Seen { size, mtime_ns, id: id.to_string(), failed: String::new() };
        true
    }
}

impl Library {
    /// `sources.json`（经缓存读：一次生成里每本书都要查跟踪目录）。
    pub(crate) fn load_sources(&self) -> Sources {
        (*self.records.sources.get(&self.sources_path())).clone()
    }

    pub(crate) fn save_sources(&self, s: &Sources) -> Result<(), String> {
        self.records.sources.put(&self.sources_path(), s.clone())
    }

    pub(crate) fn sources_path(&self) -> PathBuf {
        self.root.join("sources.json")
    }

    /// 跟踪的条目的路径（目录和单本）。
    pub fn tracked(&self) -> Vec<PathBuf> {
        self.tracked_entries().into_iter().map(|e| e.path).collect()
    }

    /// 跟踪的条目。
    pub fn tracked_entries(&self) -> Vec<Tracked> {
        self.records.sources.get(&self.sources_path()).entries.clone()
    }

    /// 这本书给不给这台设备（见 [`Sources::wants`]）。
    pub fn wants(&self, meta: &Meta, device: &str) -> bool {
        let source = (meta.source() != crate::Source::Stored && !meta.source_path.is_empty()).then(|| Path::new(&meta.source_path));
        self.records.sources.get(&self.sources_path()).wants(&meta.id, source, device)
    }

    /// 这本书的原件在跟踪目录里、还在的话，返回它（`remove` 提示用：下次 `sync` 会把它再入库）。
    pub fn tracked_original(&self, id: &str) -> Option<PathBuf> {
        let m = self.read_meta(id)?;
        let p = PathBuf::from(&m.source_path);
        (m.master.is_empty() && !m.source_path.is_empty() && self.tracked().iter().any(|d| p.starts_with(d)) && p.is_file()).then_some(p)
    }

    /// 开始跟踪一个目录，给全部设备（[`Library::track_with`]）。已跟踪（什么都没变）返回 `false`。
    pub fn track(&self, path: &Path) -> Result<bool, String> {
        self.track_with(path, &[], false).map(|c| c != TrackChange::Same)
    }

    /// 书根目录（见 `sources.json` 的 `root`）。
    pub fn books_root(&self) -> Option<PathBuf> {
        self.records.sources.get(&self.sources_path()).root.clone()
    }

    /// 换书根目录：跟踪的条目都要在它里面。换了以后设备上的书下次 `sync` 挪到按新书根算的位置。返回换了没有。
    pub fn set_books_root(&self, dir: &Path) -> Result<bool, String> {
        let dir = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if !dir.is_dir() {
            return Err(format!("{} 不是目录", dir.display()));
        }
        if dir.to_str().is_none() {
            return Err(format!("{}：{NOT_UTF8}", dir.display()));
        }
        let mut s = self.load_sources();
        if let Some(e) = s.entries.iter().find(|e| !e.path.starts_with(&dir)) {
            return Err(format!("跟踪的 {} 不在 {} 里：书根目录要包住所有跟踪的目录和书", e.path.display(), dir.display()));
        }
        if s.root.as_deref() == Some(dir.as_path()) {
            return Ok(false);
        }
        s.root = Some(dir);
        self.save_sources(&s).map(|_| true)
    }

    /// 开始跟踪一个目录或一个文件（之后 `sync` 会把它镜像进书库），给 `devices` 这几台（空＝全部设备），`top` 是这几台的放法（见
    /// [`Tracked::top`]）。同一个路径每台设备有自己的放法：同一路径照书库结构、从这一层起的各是一个条目，这次给的设备从另一种放法的
    /// 条目里挪出来；同一种放法的设备范围取并集（有一边是全部设备就是全部设备）。
    /// 目录和目录、目录和其中的文件可以同时跟踪（同一个文件只入库一次）。
    /// 要在书根目录里（还没有书根目录的，这次的目录就是书根，单本是它所在的目录）。
    pub fn track_with(&self, path: &Path, devices: &[String], top: bool) -> Result<TrackChange, String> {
        let path = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = path.is_file();
        if !file && !path.is_dir() {
            return Err(format!("{} 不是目录也不是文件", path.display()));
        }
        if path.to_str().is_none() {
            return Err(format!("{}：{NOT_UTF8}", path.display()));
        }
        if file && !is_supported(&path) {
            return Err(format!("{}：只支持 EPUB 和 CBZ", path.display()));
        }
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        if path.starts_with(&root) || root.starts_with(&path) {
            return Err("不能跟踪书库自己所在的目录".into());
        }
        let mut s = self.load_sources();
        match s.root.as_deref() {
            Some(r) if !path.starts_with(r) => {
                return Err(format!("{} 不在书根目录 {} 里（设备上按相对书根目录的路径放）：要换书根目录用 booklib track --root=新目录", path.display(), r.display()));
            }
            Some(_) => {}
            None if s.entries.is_empty() => s.root = Some(if file { path.parent().unwrap_or(&path).to_path_buf() } else { path.clone() }),
            None => {}
        }
        let mut devices: Vec<String> = devices.to_vec();
        devices.sort();
        devices.dedup();
        let existed = s.entries.iter().any(|e| e.path == path && e.file == file);
        // 产物放在电脑上的模式（没写 `[deliver]` 的自定义模式；内置模式都直接送设备）产物在跟踪目录旁边的 `<模式 id>/`（见 `generate`）：
        // 新目录和任何一个跟踪目录（包括它自己）的产物根目录互相包含的，生成出来的书会被当成新书入库、层层嵌套
        // （只查和新目录有关的组合：已有的组合生成时会报）
        if !file && !existed {
            let all: Vec<&PathBuf> = s.dirs().chain(std::iter::once(&path)).collect();
            for d in &all {
                let Some(parent) = d.parent() else { continue };
                for dev in self.devices().iter().filter(|p| p.deliver.is_none()) {
                    let root = parent.join(&dev.id);
                    if let Some(t) = all.iter().find(|t| (**d == path || ***t == path) && overlaps(&root, t)) {
                        return Err(crate::generate::output_overlap(&root, t));
                    }
                }
            }
        }
        let all_ids = self.device_ids();
        let before = s.entries.clone();
        // 这几台从同一路径另一种放法的条目里挪出来
        s.entries = std::mem::take(&mut s.entries)
            .into_iter()
            .filter_map(|e| match e.path == path && e.file == file && e.top != top {
                true if devices.is_empty() => None,
                true => without_devices(e, &devices, &all_ids),
                false => Some(e),
            })
            .collect();
        match s.entries.iter_mut().find(|e| e.path == path && e.file == file && e.top == top) {
            Some(e) if e.devices.is_empty() || devices.is_empty() => e.devices.clear(),
            Some(e) => {
                e.devices.extend(devices);
                e.devices.sort();
                e.devices.dedup();
            }
            None => s.entries.push(Tracked { path, file, devices, top }),
        }
        if s.entries == before {
            return Ok(TrackChange::Same);
        }
        self.save_sources(&s)?;
        Ok(if existed { TrackChange::Changed } else { TrackChange::New })
    }

    /// 全部阅读模式的 id。
    fn device_ids(&self) -> Vec<String> {
        self.devices().iter().map(|p| p.id.clone()).collect()
    }

    /// 不再跟踪一个目录或文件（给全部设备都不跟踪了，[`Library::untrack_with`]）。没在跟踪返回 `false`。
    pub fn untrack(&self, path: &Path) -> Result<bool, String> {
        self.untrack_with(path, &[]).map(|c| c != UntrackChange::NotTracked)
    }

    /// 不再跟踪一个目录或文件：`devices` 空＝这个路径的条目整条去掉（里面没被别的条目覆盖的书下次 `sync` 连同产物从书库删掉），
    /// 给了就只从这几台的范围里拿掉（下次 `sync` 从这几台上删掉；全部设备的条目改成剩下的几台，一台不剩就整条去掉）。
    /// 跟踪目录里的单本没有自己的条目时报错（不能从目录里单独去掉一本）。
    pub fn untrack_with(&self, path: &Path, devices: &[String]) -> Result<UntrackChange, String> {
        // 已经删了时 canonicalize 会失败：退回绝对路径（记录里存的都是绝对路径）
        let path = std::fs::canonicalize(path).or_else(|_| std::path::absolute(path)).unwrap_or_else(|_| path.to_path_buf());
        let mut s = self.load_sources();
        if !s.entries.iter().any(|e| e.path == path) {
            if let Some(d) = s.entries.iter().find(|e| !e.file && e.covers(&path)) {
                return Err(format!("{} 在跟踪的目录 {} 里，不能单独去掉（要不跟踪就 untrack 那个目录）", path.display(), d.path.display()));
            }
            return Ok(UntrackChange::NotTracked);
        }
        let all_ids = self.device_ids();
        let before = s.entries.clone();
        s.entries = std::mem::take(&mut s.entries)
            .into_iter()
            .filter_map(|e| if e.path != path { Some(e) } else if devices.is_empty() { None } else { without_devices(e, devices, &all_ids) })
            .collect();
        if s.entries == before {
            return Ok(UntrackChange::NotTracked);
        }
        let covered = |entries: &[Tracked], p: &Path, d: &str| entries.iter().any(|e| e.covers(p) && e.for_device(d));
        let under: Vec<&PathBuf> = s.files.keys().filter(|p| before.iter().any(|e| e.path == path && e.covers(p))).collect();
        let change = if s.entries.iter().any(|e| e.path == path) {
            UntrackChange::Narrowed(under.iter().filter(|p| all_ids.iter().any(|d| covered(&before, p, d) && !covered(&s.entries, p, d))).count())
        } else {
            UntrackChange::Removed(under.iter().filter(|p| !s.covered(p)).count())
        };
        self.save_sources(&s).map(|_| change)
    }

    /// 把跟踪的目录镜像进书库（一次性运行：每个问题都报）。见 [`Library::sync_with`]。
    pub fn sync(&self, prune: Prune, on: impl FnMut(SyncEvent)) -> Result<SyncReport, String> {
        self.sync_with(prune, &mut SyncMemo::default(), on)
    }

    /// 把跟踪的目录镜像进书库：
    /// - 新文件入库；大小和修改时间没变的文件跳过（不重读）；
    /// - 内容变了的文件入库新版本，旧版本的条目（没有别的原件在用时）连同产物删掉；新版本入库失败的，旧版本继续跟踪；
    /// - 移动、改名的文件按内容认出来，不重复入库；同一本书有两份、删了其中一份的，索引改记成还在的那份；
    /// - 原件不在了的（别的原件还用着同一内容的、条目记着的原件在别处还在的不算）：缺省（[`Prune::Auto`]）连同产物从书库删掉，
    ///   [`Prune::Keep`] 只报告、继续记着下次再看。删产物只删生成记录里记在这本名下的文件，已经记在别的书名下的路径不删
    ///   （见 `generate::State::delete_placements`）；这一步在生成之前，同名的新书接着生成时可以用上旧书腾出来的文件名；
    /// - 文件名不是 UTF-8 的报错，不入库也不记下来（改名后下次同步入库）；
    /// - 读不了的目录（权限等）报出来，里面已登记的书状态未知、原样保留、不删；整个跟踪目录不在的什么都不动。
    ///
    /// `memo` 跨轮次记住已经报过的问题（`--watch`），只在新出现时报一次。没有变化时不写 `sources.json`。
    pub fn sync_with(&self, prune: Prune, memo: &mut SyncMemo, mut on: impl FnMut(SyncEvent)) -> Result<SyncReport, String> {
        let src = self.load_sources();
        // 以前没删掉的旧版本也再删一次（"原件"记成条目目录：不会和跟踪目录里的文件相同）
        let leftover = src.stale.iter().map(|id| (self.entry_dir(id), id.clone())).collect();
        let mut round = SyncRound::new(&src, leftover);
        // ① 遍历跟踪的目录和单本：新的、改过的入库，没变的照旧
        for e in &src.entries {
            if e.file {
                self.scan_one(&e.path, &mut round, &mut on);
            } else {
                self.scan_dir(&e.path, &mut round, memo, &mut on);
            }
        }
        memo.bad_names.retain(|p| round.bad_now.contains(p));
        memo.unreadable.retain(|p| round.unreadable_now.contains(p));
        // ② 内容变了的：删旧版本
        let stale = self.retire_replaced(&mut round, &mut on);
        // ③ 原件不在了的
        let kept_missing = self.handle_missing(prune, &mut round, memo, &mut on);
        let SyncRound { mut present, rep, .. } = round;
        present.extend(kept_missing);
        let new = Sources { root: src.root.clone(), entries: src.entries.clone(), files: present, stale };
        if new != src {
            self.save_sources(&new)?;
        }
        Ok(rep)
    }

    /// 遍历一个跟踪目录（[`Library::sync_with`] 第一步）：读不了的子目录、整个不在的目录里登记过的文件原样当作还在；
    /// 每个能入库的文件交给 [`Library::sync_file`]。
    fn scan_dir(&self, dir: &Path, round: &mut SyncRound, memo: &mut SyncMemo, on: &mut dyn FnMut(SyncEvent)) {
        if !dir.is_dir() {
            // 目录整个不见了（U 盘没插等）：里面的文件既不算新增也不算删除，原样保留记录
            round.keep_under(dir);
            return;
        }
        let scan = scan_books(dir);
        for (u, why) in &scan.unreadable {
            // 读不了的目录（权限不够等）：和目录整个不见了一样，里面的记录原样保留——不能当成书都删了（原件不在的会连同产物从书库删掉）
            round.keep_under(u);
            round.unreadable_now.insert(u.clone());
            if memo.unreadable.insert(u.clone()) {
                on(SyncEvent::Unreadable(u, why));
                round.rep.unreadable += 1;
            }
        }
        for path in scan.files {
            if round.present.contains_key(&path) || round.bad_now.contains(&path) {
                continue; // 早期版本允许过互相包含的跟踪目录：同一个文件只处理一次
            }
            if path.to_str().is_none() {
                if memo.bad_names.insert(path.clone()) {
                    on(SyncEvent::Failed(&path, NOT_UTF8));
                    round.rep.failed += 1;
                }
                round.bad_now.insert(path);
                continue;
            }
            self.sync_file(path, round, on);
        }
    }

    /// 跟踪的单本（[`Library::sync_with`] 第一步）：所在的目录整个不在（U 盘没插）时原样当作还在；文件不在了的不管（第三步当成删了）。
    fn scan_one(&self, path: &Path, round: &mut SyncRound, on: &mut dyn FnMut(SyncEvent)) {
        if !path.parent().is_some_and(Path::is_dir) {
            round.keep_under(path);
            return;
        }
        if path.is_file() && !round.present.contains_key(path) {
            self.sync_file(path.to_path_buf(), round, on);
        }
    }

    /// 跟踪目录里的一个文件：大小、修改时间没变（上次也没失败，或还没到重试的时候）就照旧；否则入库，内容变了的记下旧版本待删。
    fn sync_file(&self, path: PathBuf, round: &mut SyncRound, on: &mut dyn FnMut(SyncEvent)) {
        let src = round.src;
        let Some((size, mtime_ns)) = crate::file_stat(&path) else {
            // 列出来以后取不到属性（刚被删、刚改了权限）：这一轮状态未知，旧记录照旧，下一轮再看
            if let Some(o) = src.files.get(&path) {
                round.present.insert(path, o.clone());
            }
            return;
        };
        let old = src.files.get(&path);
        // 没变：大小、修改时间都一样，上次没失败（以前的版本记下的失败没有记号、id 是空的：再试一次）或还没到重试的时候
        let tried = |o: &Seen| if o.failed.is_empty() { !o.id.is_empty() } else { !retry_due(&o.failed) };
        let settled = |o: &&Seen| o.size == size && o.mtime_ns == mtime_ns && tried(o) && (o.id.is_empty() || self.entry_dir(&o.id).is_dir());
        if let Some(o) = old.filter(settled) {
            round.present.insert(path, o.clone());
            round.rep.unchanged += 1;
            return;
        }
        let old = old.filter(|o| !o.id.is_empty());
        match self.add_file_only(&path) {
            Ok(added) => {
                let (m, is_new) = match added {
                    Added::New(m) | Added::Replaced(m, _) => (m, true),
                    Added::Existing(m) => (m, false),
                };
                match old.filter(|o| o.id != m.id) {
                    Some(o) => {
                        round.replaced.push((path.clone(), o.id.clone()));
                        let old_title = self.read_meta(&o.id).map(|x| x.title).unwrap_or_default();
                        on(SyncEvent::Updated(&path, &m, &old_title));
                        round.rep.updated += 1;
                    }
                    None if is_new => {
                        on(SyncEvent::Added(&path, &m));
                        round.rep.added += 1;
                    }
                    None => round.rep.unchanged += 1, // 已在库里（改名、移动，或之前手动 add 过）
                }
                round.present.insert(path, Seen { size, mtime_ns, id: m.id, failed: String::new() });
            }
            Err(e) => {
                on(SyncEvent::Failed(&path, &e));
                round.rep.failed += 1;
                if e.starts_with(BUSY) {
                    // 正在写入：这次看到的不算数，旧记录照旧（没有就不记），下一轮再试
                    if let Some(o) = src.files.get(&path) {
                        round.present.insert(path, o.clone());
                    }
                } else {
                    // 记下这次看到的大小和修改时间：文件没变就不再重试。有旧版本的继续跟踪旧版本
                    let id = old.map(|o| o.id.clone()).unwrap_or_default();
                    round.present.insert(path, Seen { size, mtime_ns, id, failed: failed_mark() });
                }
            }
        }
    }

    /// 内容变了的（[`Library::sync_with`] 第二步）：旧版本没有别的原件在用、条目记着的原件也不在别处，就删掉。
    /// 返回删不掉的（记进 `stale`，下次再删）。
    fn retire_replaced(&self, round: &mut SyncRound, on: &mut dyn FnMut(SyncEvent)) -> Vec<String> {
        let mut stale: Vec<String> = Vec::new();
        for (path, old_id) in &round.replaced {
            if stale.contains(old_id) {
                continue;
            }
            let new_id = round.present.get(path).map(|s| s.id.clone()).unwrap_or_default();
            if let Err(e) = self.retire_old_version(old_id, &new_id, path, &round.present) {
                on(SyncEvent::Failed(path, &format!("删旧版本 {old_id} 失败：{e}（下次 sync 再删）")));
                round.rep.failed += 1;
                stale.push(old_id.clone());
            }
        }
        stale
    }

    /// 上次登记过、这一轮没看到的文件（[`Library::sync_with`] 第三步）：挪了的改记新位置，删了的按 `prune` 从书库删掉或继续记着。
    /// 返回要继续记着的（并进 `sources.json`）。
    fn handle_missing(&self, prune: Prune, round: &mut SyncRound, memo: &mut SyncMemo, on: &mut dyn FnMut(SyncEvent)) -> BTreeMap<PathBuf, Seen> {
        let src = round.src;
        let present = &round.present;
        let rep = &mut round.rep;
        let live: HashMap<String, PathBuf> = present.iter().filter(|(_, s)| !s.id.is_empty()).map(|(p, s)| (s.id.clone(), p.clone())).collect();
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
            if self.original_elsewhere(&seen.id, path, present) {
                continue; // 条目记着的原件在跟踪目录以外（add 进来的），还在：这本书没丢
            }
            let title = self.read_meta(&seen.id).map(|m| m.title).unwrap_or_default();
            // 不再被任何条目覆盖的（untrack 了）：同原件删了，连同产物从书库删掉（2026-10-19 定）
            let released = !src.covered(path);
            let event = |removed| if released { SyncEvent::Released(path, &title, removed) } else { SyncEvent::Missing(path, &title, removed) };
            if prune == Prune::Auto {
                match self.remove(&seen.id) {
                    Ok(_) => {
                        on(event(true));
                        if released {
                            rep.released += 1;
                        } else {
                            rep.missing += 1;
                        }
                        rep.pruned += 1;
                    }
                    Err(e) => {
                        on(SyncEvent::Failed(path, &format!("原件不在了，从书库删掉失败：{e}（下次 sync 再删）")));
                        rep.failed += 1;
                        kept_missing.insert(path.clone(), seen.clone());
                    }
                }
                continue;
            }
            // 继续记着，下次再看（watch 时只在新出现时报一次）
            kept_missing.insert(path.clone(), seen.clone());
            missing_now.insert(path.clone());
            if memo.missing.insert(path.clone()) {
                on(event(false));
                if released {
                    rep.released += 1;
                } else {
                    rep.missing += 1;
                }
            }
        }
        memo.missing.retain(|p| missing_now.contains(p));
        kept_missing
    }

    /// 原件 `path` 换成了新版本 `new_id`（`sync` 看到内容变了，或 `add` 了改过的文件）：旧版本的条目 `old_id` 没人用了就删掉，
    /// 设备上的产物交给新版本（[`Library::hand_over_outputs`]），
    /// 返回删了没有。不删的：`present`（这一轮看到的跟踪目录里的文件）里还有别的文件是旧内容、条目记着的原件在别处还在（见
    /// [`Library::original_elsewhere`]）、条目已经没了。
    pub(crate) fn retire_old_version(&self, old_id: &str, new_id: &str, path: &Path, present: &BTreeMap<PathBuf, Seen>) -> Result<bool, String> {
        let in_use = present.iter().any(|(p, s)| s.id == old_id && p != path);
        if in_use || !self.entry_dir(old_id).is_dir() || self.original_elsewhere(old_id, path, present) {
            return Ok(false);
        }
        // 设备上的产物交给新版本（原地覆盖、原地替换），不先删
        if !new_id.is_empty() {
            self.hand_over_outputs(old_id, new_id)?;
        }
        self.remove(old_id).map(|_| true)
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

#[cfg(test)]
mod scope_tests {
    use super::*;

    fn dir(p: &str, devices: &[&str]) -> Tracked {
        Tracked { path: PathBuf::from(p), file: false, devices: devices.iter().map(|s| s.to_string()).collect(), top: false }
    }

    fn file(p: &str, devices: &[&str]) -> Tracked {
        Tracked { path: PathBuf::from(p), file: true, devices: devices.iter().map(|s| s.to_string()).collect(), top: false }
    }

    fn seen(id: &str) -> Seen {
        Seen { size: 1, mtime_ns: 1, id: id.to_string(), failed: String::new() }
    }

    #[test]
    fn placement_keeps_library_structure_or_starts_at_the_entry() {
        let top = |mut t: Tracked| {
            t.top = true;
            t
        };
        let p = Path::new("/b/漫画/死亡筆記/卷03.epub");
        let s = Sources {
            root: Some(PathBuf::from("/b")),
            entries: vec![
                dir("/b/漫画", &["kindle", "ireader", "xochitl"]),
                top(dir("/b/漫画/死亡筆記", &["ireader"])),
                top(file("/b/漫画/死亡筆記/卷03.epub", &["xochitl"])),
            ],
            ..Default::default()
        };
        let at = |d: &str| s.placement(p, d).map(|(_, r)| r.display().to_string());
        assert_eq!(at("kindle").as_deref(), Some("漫画/死亡筆記"), "照书库结构（相对书根目录）");
        assert_eq!(at("ireader").as_deref(), Some("死亡筆記"), "--top 的目录：从它这一层起；比浅的目录具体");
        assert_eq!(at("xochitl").as_deref(), Some(""), "--top 的单本：放根目录；单本比目录具体");
        assert_eq!(at("other"), None, "没有给这台的条目");
        let keep = Sources { root: Some(PathBuf::from("/b")), entries: vec![file("/b/文字/科幻/三体.epub", &[])], ..Default::default() };
        assert_eq!(keep.placement(Path::new("/b/文字/科幻/三体.epub"), "kindle").unwrap().1, PathBuf::from("文字/科幻"), "单本照书库结构");
        // 没有书根目录（以前跟踪过几个目录的书库）：目录相对它自己，单本是所在的文件夹名
        let old = Sources { entries: vec![dir("/b/漫画", &[]), file("/x/死亡筆記/卷03.epub", &[])], ..Default::default() };
        assert_eq!(old.placement(p, "kindle").unwrap().1, PathBuf::from("死亡筆記"));
        assert_eq!(old.placement(Path::new("/x/死亡筆記/卷03.epub"), "kindle").unwrap().1, PathBuf::from("死亡筆記"));
    }

    #[test]
    fn wants_is_the_union_of_covering_entries() {
        let mut s = Sources { entries: vec![dir("/b/文字", &["ireader"]), file("/b/文字/a.epub", &["kindle"])], ..Default::default() };
        s.files.insert(PathBuf::from("/b/文字/a.epub"), seen("A"));
        s.files.insert(PathBuf::from("/b/文字/b.epub"), seen("B"));
        s.files.insert(PathBuf::from("/old/c.epub"), seen("C"));
        let w = |id: &str, p: &str, d: &str| s.wants(id, Some(Path::new(p)), d);
        assert!(w("A", "/b/文字/a.epub", "ireader") && w("A", "/b/文字/a.epub", "kindle") && !w("A", "/b/文字/a.epub", "xochitl"));
        assert!(w("B", "/b/文字/b.epub", "ireader") && !w("B", "/b/文字/b.epub", "kindle"));
        assert!(!w("C", "/old/c.epub", "ireader"), "不再跟踪了的谁都不给（下次 sync 删）");
        assert!(w("D", "/别处/d.epub", "xochitl") && s.wants("E", None, "kindle"), "add 进来的、网址书给全部设备");
        // 同一内容在跟踪目录里还有一份：按那份的条目算
        s.files.insert(PathBuf::from("/b/文字/a 副本.epub"), seen("D"));
        assert!(s.wants("D", Some(Path::new("/别处/d.epub")), "ireader") && !s.wants("D", Some(Path::new("/别处/d.epub")), "xochitl"));
    }
}

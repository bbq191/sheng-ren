//! [`crate::Library`] 里的缓存和运行状态，按**活多久**分成四组（以前 8 个 `RefCell`/`Cell` 平铺在 `Library` 上，
//! 看不出哪个跨轮、哪个一轮就该清）：
//!
//! | 组 | 活多久 | 为什么可以这么久 |
//! |---|---|---|
//! | [`Records`] 记录文件与锁 | 整个进程 | 读缓存按文件身份（inode、大小、修改时间）核对，别的进程改了会重读 |
//! | [`ContentFacts`] 按内容记住的事实 | 整个进程 | 键里带内容哈希（或原件的路径、大小、修改时间），内容变了就对不上 |
//! | [`Devices`] 设备连接 | 跨轮（`sync --watch`） | Move 的 SSH 隧道每轮重建太慢；每轮开头 [`Devices::refresh`] 核一次 |
//! | [`BuildCtx`] 生成过程中的临时状态 | 一轮生成 / 一件传输 | 中间文件一轮结束时删（[`crate::Library::release_prepared`]）；占着的文件名那件传完就放开 |
//!
//! 联网用的 HTTP 客户端（`net::Net`，节流、离线状态跨书共用）整个进程一个，第一次用时建，不在这里。

use crate::deliver::{self, DeviceEnv, Target};
use crate::fsutil::JsonCache;
use crate::{generate, sources, OwnDc};
use profile::Profile;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// 书库的记录文件（`output-state/<模式>.json`、`sources.json`）的读缓存，和进程锁的状态。整个进程有效。
pub(crate) struct Records {
    /// 本进程现在持有书库的锁（[`crate::Library::lock`]）。不持锁时（`list`）不往书库写补算的字段。
    pub(crate) locked: Cell<bool>,
    /// 残留临时文件清理过了（每个进程第一次拿到锁时清一次）。
    pub(crate) cleaned: Cell<bool>,
    /// 各模式的生成记录、`sources.json`：一次生成、list 里每本书都要查，读一次缓存起来。
    pub(crate) states: JsonCache<generate::State>,
    pub(crate) sources: JsonCache<sources::Sources>,
}

impl Records {
    pub(crate) fn new() -> Records {
        Records { locked: Cell::new(false), cleaned: Cell::new(false), states: JsonCache::new(), sources: JsonCache::new() }
    }
}

/// 核对过的原件：路径，和核对时的大小、修改时间。
type Verified = (String, (u64, u64));

/// 按内容记住的事实（早期条目 meta.json 里没存的，当场算一次记下）。整个进程有效：键里带着内容，内容变了自然对不上。
#[derive(Default)]
pub(crate) struct ContentFacts {
    /// 核对过哈希的原件：书 id → (路径, 大小与修改时间)。多台设备生成同一本书时不重算哈希。
    verified: RefCell<HashMap<String, Verified>>,
    /// 内容哈希 → 书里有没有简介、标签（见 `Library::own_dc_of`）。
    own_dc: RefCell<HashMap<String, OwnDc>>,
    /// 内容哈希 → 是不是漫画（指纹分文字书、漫画，profile 给漫画另配格式时也要用）。
    comic: RefCell<HashMap<String, bool>>,
}

impl ContentFacts {
    /// 这本书的原件在 `path`、大小与修改时间是 `st` 时核对过（内容就是入库时那本）。
    pub(crate) fn verified(&self, id: &str, path: &str, st: (u64, u64)) -> bool {
        self.verified.borrow().get(id).is_some_and(|(p, s)| p == path && *s == st)
    }

    pub(crate) fn note_verified(&self, id: &str, path: &str, st: (u64, u64)) {
        self.verified.borrow_mut().insert(id.to_string(), (path.to_string(), st));
    }

    /// 书从书库删了：忘掉核对过的原件。
    pub(crate) fn forget(&self, id: &str) {
        self.verified.borrow_mut().remove(id);
    }

    pub(crate) fn own_dc(&self, sha: &str) -> Option<OwnDc> {
        self.own_dc.borrow().get(sha).copied()
    }

    pub(crate) fn note_own_dc(&self, sha: &str, o: OwnDc) {
        self.own_dc.borrow_mut().insert(sha.to_string(), o);
    }

    pub(crate) fn comic(&self, sha: &str) -> Option<bool> {
        self.comic.borrow().get(sha).copied()
    }

    pub(crate) fn note_comic(&self, sha: &str, c: bool) {
        self.comic.borrow_mut().insert(sha.to_string(), c);
    }
}

/// 一个模式现在送到哪：连上了（[`Target`]），或没接上的原因。同一轮里共用一份（`Rc`）。
pub(crate) type Connection = Rc<Result<Target, String>>;

/// 设备在哪找，和连上的设备。跨轮保留（`sync --watch` 不每轮重连 Move），每轮开头 [`Devices::refresh`] 核一次。
pub(crate) struct Devices {
    /// 设备在哪找（见 [`DeviceEnv`]）。
    env: DeviceEnv,
    /// 连过的设备：模式 id → 送到哪，或没接上的原因。
    targets: RefCell<HashMap<String, Connection>>,
}

impl Devices {
    pub(crate) fn new(env: DeviceEnv) -> Devices {
        Devices { env, targets: RefCell::default() }
    }

    /// 换设备在哪找：连过的都作废。
    pub(crate) fn set_env(&mut self, env: DeviceEnv) {
        self.env = env;
        self.targets.borrow_mut().clear();
    }

    /// MTP 挂载点的上级目录（删产物时判断文件在不在 MTP 设备上）。
    pub(crate) fn mtp_base(&self) -> &Path {
        &self.env.mtp_base
    }

    /// 这个模式的产物送到哪（一轮里只连一次）。
    pub(crate) fn target(&self, p: &Profile) -> Connection {
        if let Some(t) = self.targets.borrow().get(&p.id) {
            return t.clone();
        }
        let t = Rc::new(deliver::connect(&self.env, p));
        self.targets.borrow_mut().insert(p.id.clone(), t.clone());
        t
    }

    /// 新一轮开始：MTP 设备、没接上的下次用到时重新看；Move 的连接还通就留着（这一轮重新查书在不在），断了的扔掉。
    /// 返回这次发现断了的 Move（模式 id）。
    pub(crate) fn refresh(&self) -> Vec<String> {
        let mut dropped = Vec::new();
        self.targets.borrow_mut().retain(|id, t| match t.as_ref().as_ref().ok().and_then(Target::xochitl) {
            Some(x) => {
                let alive = x.alive();
                if alive {
                    x.forget_presence();
                } else {
                    dropped.push(id.clone());
                }
                alive
            }
            None => false,
        });
        dropped
    }
}

/// 生成过程中的临时状态：中间文件按书留到这本书所有模式都做完（最迟一轮结束），占着的文件名留到那件传完。
#[derive(Default)]
pub(crate) struct BuildCtx {
    /// 与模式无关的中间文件（见 [`PreparedInput`]），按书留着：同一本书接着给别的模式生成时直接用。
    prepared: RefCell<Vec<PreparedInput>>,
    /// 正在传、还没记进生成记录的新产物：设备上的路径 → 书 id（同一轮里别的书不选这个文件名，见 `Library::prepare_file`）。
    reserved: RefCell<HashMap<PathBuf, String>>,
}

/// 与模式无关的中间文件：CBZ 转出来的 EPUB、补了元数据的 EPUB（在书库的 `.tmp-<id>-src/` 里）。
/// 一本书要给几个模式生成时只做一次（一本 135MB 的漫画以前每个模式都整本转一遍）。丢掉时删目录。
/// 按书 id 留着，直到这本书所有模式都做完（`sync` 里某台设备排满了、这本书留到最后补的，也不用再转一遍；
/// 调用方做完一本调 [`crate::Library::release_prepared_of`]）；留着的总大小超过 [`PREPARED_BUDGET`] 时丢掉最早的（之后用到再做）。
pub(crate) struct PreparedInput {
    pub(crate) book: String,
    /// 内容哈希 + 补的封面 + 补的元数据：任何一样变了就重做。
    pub(crate) key: String,
    pub(crate) dir: PathBuf,
    pub(crate) epub: PathBuf,
    /// `epub` 的大小（算留着的总量）。
    pub(crate) bytes: u64,
}

/// 留着给后面的模式用的中间文件总共最多多大（书库所在的盘上）：超过时丢掉最早的，不让推迟的书把中间文件堆满盘。
const PREPARED_BUDGET: u64 = 1 << 30;

impl Drop for PreparedInput {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl BuildCtx {
    /// 这本书同样补充（`key`）的中间文件，做过的话。
    pub(crate) fn prepared(&self, book: &str, key: &str) -> Option<PathBuf> {
        self.prepared.borrow().iter().find(|p| p.book == book && p.key == key).map(|p| p.epub.clone())
    }

    /// 留下刚做的中间文件；留着的太多时丢掉最早的（刚做的这本不丢）。
    pub(crate) fn keep_prepared(&self, p: PreparedInput) {
        let mut kept = self.prepared.borrow_mut();
        kept.push(p);
        while kept.len() > 1 && kept.iter().map(|p| p.bytes).sum::<u64>() > PREPARED_BUDGET {
            kept.remove(0);
        }
    }

    /// 丢掉这本书的中间文件（删目录）。
    pub(crate) fn release_prepared_of(&self, book: &str) {
        let gone: Vec<PreparedInput> = {
            let mut kept = self.prepared.borrow_mut();
            let (gone, keep) = std::mem::take(&mut *kept).into_iter().partition(|p| p.book == book);
            *kept = keep;
            gone
        };
        drop(gone);
    }

    /// 丢掉所有中间文件（一轮生成结束）。
    pub(crate) fn release_prepared(&self) {
        let gone = std::mem::take(&mut *self.prepared.borrow_mut());
        drop(gone);
    }

    /// 新书传的过程中先占住这个文件名。
    pub(crate) fn reserve(&self, out: &Path, book: &str) {
        self.reserved.borrow_mut().insert(out.to_path_buf(), book.to_string());
    }

    /// 传完（或失败）：放开占着的文件名。
    pub(crate) fn unreserve(&self, out: &Path) {
        self.reserved.borrow_mut().remove(out);
    }

    /// 同一轮里别的书正在传、还没记进生成记录的新文件：`dir` 里的文件名（小写）。
    pub(crate) fn reserved_names(&self, dir: &Path, book: &str) -> BTreeSet<String> {
        self.reserved
            .borrow()
            .iter()
            .filter(|(p, id)| *id != book && p.parent() == Some(dir))
            .filter_map(|(p, _)| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
            .collect()
    }
}

//! [`crate::Library`] 里的缓存和运行状态，按**活多久**分成四组（以前 8 个 `RefCell`/`Cell` 平铺在 `Library` 上，
//! 看不出哪个跨轮、哪个一轮就该清）：
//!
//! | 组 | 活多久 | 为什么可以这么久 |
//! |---|---|---|
//! | [`Records`] 记录文件与锁 | 整个进程 | 读缓存按文件身份（inode、大小、修改时间）核对，别的进程改了会重读 |
//! | [`ContentFacts`] 按内容记住的事实 | 整个进程 | 键里带内容哈希（或原件的路径、大小、修改时间），内容变了就对不上 |
//! | [`Devices`] 设备连接 | 一轮（`sync --watch` 每轮开头 [`Devices::refresh`] 清掉）；"这次连接清过临时文件"的那份跨轮留着 | 一轮里每台只连一次 |
//! | [`BuildCtx`] 生成过程中的临时状态 | 一轮生成 / 一件传输 | 中间文件一轮结束时删（[`crate::Library::release_prepared`]）；占着的文件名那件传完就放开 |
//!
//! 联网用的 HTTP 客户端（`net::Net`，节流、离线状态跨书共用）整个进程一个，第一次用时建，不在这里。

use crate::deliver::{self, home_dir, user_name, DeviceEnv, Target};
use crate::fsutil::JsonCache;
use crate::{generate, sources, OwnDc};
use profile::Profile;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
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
        Records {
            locked: Cell::new(false),
            cleaned: Cell::new(false),
            states: JsonCache::new(),
            sources: JsonCache::new(),
        }
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
    /// 内容哈希 → 是不是中文文字书（竖排自动判定）。
    zh_text: RefCell<HashMap<String, bool>>,
    /// 这次运行里联网找过元数据的整套漫画（`metadata::series_key`）：`meta --fetch --force` 时同一套别的卷照用，不再每卷联网。
    series_fetched: RefCell<std::collections::HashSet<String>>,
}

impl ContentFacts {
    /// 这本书的原件在 `path`、大小与修改时间是 `st` 时核对过（内容就是入库时那本）。
    pub(crate) fn verified(&self, id: &str, path: &str, st: (u64, u64)) -> bool {
        self.verified
            .borrow()
            .get(id)
            .is_some_and(|(p, s)| p == path && *s == st)
    }

    pub(crate) fn note_verified(&self, id: &str, path: &str, st: (u64, u64)) {
        self.verified
            .borrow_mut()
            .insert(id.to_string(), (path.to_string(), st));
    }

    /// 书从书库删了：忘掉核对过的原件。
    pub(crate) fn forget(&self, id: &str) {
        self.verified.borrow_mut().remove(id);
    }

    pub(crate) fn own_dc(&self, sha: &str) -> Option<OwnDc> {
        self.own_dc.borrow().get(sha).cloned()
    }

    pub(crate) fn note_own_dc(&self, sha: &str, o: OwnDc) {
        self.own_dc.borrow_mut().insert(sha.to_string(), o);
    }

    pub(crate) fn series_fetched(&self, key: &str) -> bool {
        self.series_fetched.borrow().contains(key)
    }

    pub(crate) fn note_series_fetched(&self, key: &str) {
        self.series_fetched.borrow_mut().insert(key.to_string());
    }

    pub(crate) fn comic(&self, sha: &str) -> Option<bool> {
        self.comic.borrow().get(sha).copied()
    }

    pub(crate) fn note_comic(&self, sha: &str, c: bool) {
        self.comic.borrow_mut().insert(sha.to_string(), c);
    }

    pub(crate) fn zh_text(&self, sha: &str) -> Option<bool> {
        self.zh_text.borrow().get(sha).copied()
    }

    pub(crate) fn note_zh_text(&self, sha: &str, zh: bool) {
        self.zh_text.borrow_mut().insert(sha.to_string(), zh);
    }
}

/// 一个模式现在送到哪：连上了（[`Target`]），或没接上的原因。同一轮里共用一份（`Rc`）。
pub(crate) type Connection = Rc<Result<Target, String>>;

/// 设备在哪找，和连上的设备。一轮里每台只连一次，每轮开头 [`Devices::refresh`] 清掉。
pub(crate) struct Devices {
    /// 设备在哪找（见 [`DeviceEnv`]）。
    env: RefCell<DeviceEnv>,
    /// `env` 是从环境变量读的（[`DeviceEnv::from_env`]）：每轮 [`Devices::refresh`] 重新读（`/run/media/$USER` 第一次插 USB 存储才建）。
    auto: bool,
    /// 连过的设备：模式 id → 送到哪，或没接上的原因。
    targets: RefCell<HashMap<String, Connection>>,
    /// 这一轮已经尝试过生成记录大小写迁移的模式。
    rebased: RefCell<HashSet<String>>,
    /// 这次连接已经清过设备上残留临时文件的模式。不随 [`Devices::refresh`] 清：同一连接里 `--watch` 的后续轮次不再清；
    /// 设备没接上时 [`Devices::forget_device_cleaned`] 清掉，下次接上重新清。
    cleaned: RefCell<HashSet<String>>,
}

impl Devices {
    pub(crate) fn new(env: DeviceEnv) -> Devices {
        Devices {
            env: RefCell::new(env),
            auto: false,
            targets: RefCell::default(),
            rebased: RefCell::default(),
            cleaned: RefCell::default(),
        }
    }

    /// 从环境变量读设备在哪找，每轮重新读。
    pub(crate) fn from_env() -> Devices {
        Devices {
            auto: true,
            ..Devices::new(DeviceEnv::from_env())
        }
    }

    /// 换设备在哪找：连过的都作废。
    pub(crate) fn set_env(&mut self, env: DeviceEnv) {
        self.env = RefCell::new(env);
        self.auto = false;
        self.targets.borrow_mut().clear();
        self.cleaned.borrow_mut().clear();
    }

    /// `path` 在可拔下的设备上（`~/Devices/<链接名>`，或旧 MTP、USB 大容量存储挂载目录下）：只看路径前缀，不碰文件系统。
    /// 新旧前缀都认（两次运行之间路径变了，旧记录还记着原来的位置）：认不出是设备上的会当成电脑上的，
    /// 加锁清理时就去读卡死的挂载点（汉王 jmtpfs），删产物时又会把没接上的当成接上了。
    pub(crate) fn on_removable(&self, path: &Path) -> bool {
        self.removable_base(path).is_some()
    }

    /// `path` 在哪个设备挂载目录下（长的先比）：返回匹配的前缀。新前缀是 `devices_base`，旧前缀是 `mtp_base`、
    /// `usbms_base`、`/run/media/$USER`、`/media/$USER` 和废弃的 `~/Hanvon`（后者只对 `~/Hanvon/...` 加 `$HOME`）。
    fn removable_base(&self, path: &Path) -> Option<PathBuf> {
        // 长的先比：旧前缀可能互相包含（关了挂载检查时 usbms_base 曾在 mtp_base 下面）；新前缀和旧前缀都留着
        let env = self.env.borrow();
        let mut bases = vec![
            env.devices_base.clone(),
            env.mtp_base.clone(),
            env.usbms_base.clone(),
        ];
        if let Some(user) = user_name() {
            for base in ["/run/media", "/media"] {
                bases.push(Path::new(base).join(&user));
            }
        }
        // 缺省的 usbms_base 会在 /run/media/$USER 和 /media/$USER 之间换：另一处也认（测试用的假用户也按这个换）
        for (a, b) in [("/run/media", "/media"), ("/media", "/run/media")] {
            if let Ok(rest) = env.usbms_base.strip_prefix(a) {
                bases.push(Path::new(b).join(rest));
            }
        }
        if let Some(home) = home_dir() {
            let old_hanvon = home.join("Hanvon");
            if path.starts_with(&old_hanvon) {
                bases.push(home);
            }
        }
        bases.sort_by_key(|b| std::cmp::Reverse(b.as_os_str().len()));
        bases.into_iter().find(|b| path.starts_with(b))
    }

    /// `path` 在可拔下的设备上（MTP、USB 大容量存储挂载目录下）而设备现在没挂上：删产物时不当成已删，记录留着下次再删。
    /// 挂载点取 `<base>/<path 在 base 下的第一段>`；开着挂载检查时它要真的挂着（[`deliver::mounted`]），同时 `root` 要在。
    /// 只看 `root.is_dir()` 不够：拔掉后 udisks、jmtpfs 留下的空挂载点里正好有同名目录时会被当成接上，删掉了记录、书却还留在机上（agy 发现）。
    pub(crate) fn unplugged(&self, path: &Path, root: &Path) -> bool {
        let Some(base) = self.removable_base(path) else {
            return false;
        };
        let Ok(rest) = path.strip_prefix(&base) else {
            return false;
        };
        let Some(first) = rest.components().next() else {
            return true; // 就是挂载目录本身：不是设备上的文件，按没挂上处理，不删
        };
        let env = self.env.borrow();
        let point = base.join(first);
        if env.mount_check && !deliver::mounted(&point) {
            return true;
        }
        // 新约定：链接指向的挂载点在 mountinfo 里就算接上，不再 stat 链接目标（卡死的 FUSE 上 stat 会挂住）
        if env.mount_check
            && std::fs::symlink_metadata(&point).is_ok_and(|m| m.file_type().is_symlink())
        {
            return false;
        }
        !root.is_dir()
    }

    /// 这次连接还没清过这台设备上的残留临时文件的话，记下清过了、返回 true；已经清过返回 false。
    pub(crate) fn begin_device_cleaned(&self, id: &str) -> bool {
        self.cleaned.borrow_mut().insert(id.to_string())
    }

    /// 设备没接上：下次接上时重新清这台设备上的残留临时文件。
    pub(crate) fn forget_device_cleaned(&self, id: &str) {
        self.cleaned.borrow_mut().remove(id);
    }

    /// 这台设备旧记录前缀 → 新的 `devices_base/<链接名>`（只改记录，见 [`crate::generate`]）。
    pub(crate) fn prefix_migrations(&self, p: &Profile) -> Vec<(PathBuf, PathBuf)> {
        self.env.borrow().prefix_migrations(p)
    }

    /// 这个模式的产物送到哪（一轮里只连一次）。
    pub(crate) fn target(&self, p: &Profile) -> Connection {
        if let Some(t) = self.targets.borrow().get(&p.id) {
            return t.clone();
        }
        let t = Rc::new(deliver::connect(&self.env.borrow(), p));
        self.targets.borrow_mut().insert(p.id.clone(), t.clone());
        t
    }

    /// 这一轮这台设备要不要尝试生成记录的大小写迁移：第一次返回 true，之后 false（每轮最多一次）。
    pub(crate) fn begin_rebase(&self, id: &str) -> bool {
        self.rebased.borrow_mut().insert(id.to_string())
    }

    /// 新一轮开始：连过的都作废，下次用到时重新看（MTP 挂载点每轮可能变）。
    /// `cleaned` 不清：同一台设备没断开过时，这一轮不再清它上面的残留临时文件。
    pub(crate) fn refresh(&self) {
        if self.auto {
            *self.env.borrow_mut() = DeviceEnv::from_env();
        }
        self.targets.borrow_mut().clear();
        self.rebased.borrow_mut().clear();
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
        self.prepared
            .borrow()
            .iter()
            .find(|p| p.book == book && p.key == key)
            .map(|p| p.epub.clone())
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
            let (gone, keep) = std::mem::take(&mut *kept)
                .into_iter()
                .partition(|p| p.book == book);
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
        self.reserved
            .borrow_mut()
            .insert(out.to_path_buf(), book.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn devices(mtp: &str, usbms: &str, mount_check: bool) -> Devices {
        Devices::new(DeviceEnv {
            devices_base: PathBuf::from(mtp),
            mtp_base: PathBuf::from(mtp),
            mount_check,
            usbms_base: PathBuf::from(usbms),
        })
    }

    #[test]
    fn removable_paths_are_recognized_by_prefix_only() {
        let d = tempfile::tempdir().unwrap();
        let mtp = d.path().join("mtp");
        let usbms = d.path().join("usbms");
        let devices = devices(&mtp.to_string_lossy(), &usbms.to_string_lossy(), false);
        assert!(devices.on_removable(&mtp.join("kindle/Internal Storage/书.epub")));
        assert!(devices.on_removable(&usbms.join("Kobo/books/书.epub")));
        assert!(devices.on_removable(&mtp), "挂载目录本身也算");
        assert!(!devices.on_removable(&d.path().join("电脑/书.epub")));
        // 按路径分量比，不是字符串前缀
        assert!(!devices.on_removable(&d.path().join("mtp2/书.epub")));
    }

    #[test]
    fn run_media_and_media_are_both_recognized() {
        let mtp = "/run/user/1000/mtp";
        let for_usbms = |usbms: &str| devices(mtp, usbms, true);

        let d = for_usbms("/run/media/u/Kobo");
        assert!(d.on_removable(Path::new("/run/media/u/Kobo/books/书.epub")));
        assert!(d.on_removable(Path::new("/media/u/Kobo/books/书.epub")));

        let d = for_usbms("/media/u/Kobo");
        assert!(d.on_removable(Path::new("/media/u/Kobo/books/书.epub")));
        assert!(d.on_removable(Path::new("/run/media/u/Kobo/books/书.epub")));
    }

    #[test]
    fn cleaned_mark_survives_refresh_and_is_forgotten_when_unplugged() {
        let mut d = devices("/run/user/1000/mtp", "/run/media/u/Kobo", true);
        assert!(d.begin_device_cleaned("kindle"), "第一次接上要清");
        assert!(!d.begin_device_cleaned("kindle"), "同一次连接只清一次");
        d.refresh();
        assert!(!d.begin_device_cleaned("kindle"), "跨轮仍然记得清过");
        d.forget_device_cleaned("kindle");
        assert!(d.begin_device_cleaned("kindle"), "拔掉后重新接上再清");

        d.begin_device_cleaned("kobo");
        d.set_env(DeviceEnv {
            devices_base: "/run/user/1000/mtp".into(),
            mtp_base: "/run/user/1000/mtp".into(),
            mount_check: true,
            usbms_base: "/run/media/u/Kobo".into(),
        });
        assert!(d.begin_device_cleaned("kobo"), "换设备环境后要重新清");
    }
}

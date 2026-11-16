//! 书库用到的文件操作：原子写、JSON 读写（带缓存）、流式哈希、进程锁、残留临时文件清理。

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// 书库里所有临时文件、临时目录的名字前缀：拿到锁以后清理进程被杀时留下的（见 [`clean_tmp`]）。
pub(crate) const TMP_PREFIX: &str = ".tmp-";

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// 文件的 SHA-256，边读边算（大漫画不整本读进内存）。
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = File::open(path).map_err(|e| format!("读 {}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf)
            .map_err(|e| format!("读 {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        crate::stall::beat();
    }
    Ok(hex(&h.finalize()))
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|x| format!("{x:02x}")).collect()
}

/// 同目录下的临时文件名（和目标在同一文件系统，改名是原子的）。带进程号和计数器，同时写同一个目标也不会撞名。
pub(crate) fn tmp_sibling(path: &Path) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let mut name = std::ffi::OsString::from(format!(
        "{TMP_PREFIX}{}-{}-",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    name.push(path.file_name().unwrap_or_default());
    path.with_file_name(name)
}

/// 书库里的原子写：临时文件 → 落盘 → 改名 → 落盘目录（[`bookconv::util::produce_then_replace`]）。
///
/// 和 `bookconv::util::write_atomic` 做的事一样，只是临时文件的名字不同：这里是 [`tmp_sibling`] 的
/// `.tmp-<进程号>-<计数>-<名>`——带 [`TMP_PREFIX`]，进程被杀时留下的下次拿到锁时会被 [`clean_tmp`] 清掉；带进程号和计数，
/// 同时写同一个目标也不撞名。`bookconv` 那个是 `<名>.writing.tmp`（命令行工具用，不归书库清理）。书库里的文件一律用这个。
pub(crate) fn write_atomic_cleanable(path: &Path, data: &[u8]) -> Result<(), String> {
    bookconv::util::produce_then_replace(&tmp_sibling(path), path, |t| {
        std::fs::write(t, data).map_err(|e| format!("写 {}: {e}", path.display()))
    })
}

pub(crate) use bookconv::util::sync_parent;

/// 读 JSON 文件；不在或读不出来返回 `None`（只读的场合用；会改书库的命令持锁时先用 [`check_json`] 核对过）。
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// 核对 JSON 文件读得出来：不在算好的；在但读不出、解析不了 → 错误。
/// 书库的记录（`sources.json`、生成记录）坏了要停下来，不能当成空的再写回去——那会丢掉全部记录
/// （生成记录丢了，旧产物再也认不出来，每本书在产物目录里变两份）。
pub(crate) fn check_json<T: DeserializeOwned>(path: &Path) -> Result<(), String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("读 {}: {e}", path.display())),
    };
    serde_json::from_slice::<T>(&bytes)
        .map(|_| ())
        .map_err(|e| {
            format!(
                "{} 坏了（{e}）：修好它，或者挪走它（记录会重建，但以前的产物认不出来了）再试",
                path.display()
            )
        })
}

/// 原子写 JSON（缩进格式）。序列化失败（比如路径不是 UTF-8）返回错误，不 panic。
pub(crate) fn write_json<T: Serialize + ?Sized>(path: &Path, v: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(v).map_err(|e| format!("写 {}: {e}", path.display()))?;
    write_atomic_cleanable(path, s.as_bytes())
}

/// 文件的身份：设备号 + inode + 大小 + 修改时间。原子写（改名）总会换 inode，所以别的进程写过一定能看出来。
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileKey(u64, u64, u64, i64, i64);

fn file_key(path: &Path) -> Option<FileKey> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).ok()?;
    Some(FileKey(
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
    ))
}

/// 小 JSON 文件的读缓存（`output-state/<模式>.json`、`sources.json`）：文件没变（见 [`FileKey`]）就不重读、不重新解析。
/// 一次生成（`sync`）、`list` 里每本书都要查一遍，不缓存的话每本书都重读一次。
pub(crate) struct JsonCache<T> {
    map: RefCell<HashMap<PathBuf, Cached<T>>>,
}

type Cached<T> = (Option<FileKey>, Rc<T>);

impl<T: DeserializeOwned + Serialize + Default> JsonCache<T> {
    pub(crate) fn new() -> Self {
        JsonCache {
            map: RefCell::new(HashMap::new()),
        }
    }

    /// 读（不在或读不出来 = 缺省值）。
    pub(crate) fn get(&self, path: &Path) -> Rc<T> {
        let key = file_key(path);
        if let Some((k, v)) = self.map.borrow().get(path) {
            if *k == key {
                return v.clone();
            }
        }
        let v: Rc<T> = Rc::new(key.and_then(|_| read_json(path)).unwrap_or_default());
        self.map
            .borrow_mut()
            .insert(path.to_path_buf(), (key, v.clone()));
        v
    }

    /// 原子写，并更新缓存。
    pub(crate) fn put(&self, path: &Path, v: T) -> Result<(), String> {
        write_json(path, &v)?;
        self.map
            .borrow_mut()
            .insert(path.to_path_buf(), (file_key(path), Rc::new(v)));
        Ok(())
    }
}

/// 书库的进程锁：同一时间只允许一个会改动书库的 `booklib` 进程（两个进程会互删临时目录、互相覆盖生成记录）。
/// 进程退出时锁自动释放；`Lock` 被丢掉时也释放。
pub struct Lock<'a> {
    _file: File,
    held: &'a Cell<bool>,
}

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        crate::stall::note_lock_fd(-1);
        self.held.set(false);
    }
}

pub(crate) fn lock<'a>(root: &Path, held: &'a Cell<bool>) -> Result<Lock<'a>, String> {
    let path = root.join(".lock");
    let f = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => {
            held.set(true);
            {
                use std::os::unix::io::AsRawFd;
                crate::stall::note_lock_fd(f.as_raw_fd());
            }
            Ok(Lock { _file: f, held })
        }
        Err(std::fs::TryLockError::WouldBlock) => Err(format!(
            "另一个 booklib 正在使用书库 {}，等它结束再试",
            root.display()
        )),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

/// 删掉 `dir` 下名字以 [`TMP_PREFIX`] 开头的文件和目录（进程被杀时留下的）。只在持锁时调用。
/// 读目录前先调 `guard`：测试钩子在这里检查“这个目录能不能读”，正式路径传 `|_| {}`。
pub(crate) fn clean_tmp(dir: &Path, guard: impl FnOnce(&Path)) {
    guard(dir);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !e.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
            continue;
        }
        let _ = if e.file_type().is_ok_and(|t| t.is_dir()) {
            std::fs::remove_dir_all(e.path())
        } else {
            std::fs::remove_file(e.path())
        };
    }
}

/// 临时文件名里的进程号（[`tmp_sibling`] 的 `.tmp-<进程号>-<计数>-<名>`）。名字不合规返回 `None`。
fn tmp_owner_pid(name: &std::ffi::OsStr) -> Option<u32> {
    let rest = name.to_str()?.strip_prefix(TMP_PREFIX)?;
    let pid = rest.split('-').next()?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok()
}

/// 临时文件的主人进程还在不在：`/proc/<pid>` 存在就算在。名字里解析不出进程号的当作不在（照删）。
fn tmp_owner_alive(name: &std::ffi::OsStr) -> bool {
    tmp_owner_pid(name).is_some_and(|pid| Path::new("/proc").join(pid.to_string()).exists())
}

/// 只删 `dir` 下名字以 [`TMP_PREFIX`] 开头的**文件**（不删目录）：产物目录在书库外（和原件目录并列），
/// 里面只可能有本工具写产物时留下的 `.tmp-<进程号>-<计数>-<名>`。
/// `skip_live_owner` 为真时跳过名字里的进程号还活着（`/proc/<pid>` 存在）的文件。
/// 名字不合规的照删。**设备连接时的清理不用这个函数**（那边只删名字严格是我们自己的，见 [`clean_device_tmp_files`]）；
/// 这里只用于电脑上的加锁清理，同一书库有进程锁，传 `false`。
/// 只在持锁时调用。读目录前先调 `guard`：测试钩子在这里检查“这个目录能不能读”，正式路径传 `|_| {}`。
pub(crate) fn clean_tmp_files(dir: &Path, skip_live_owner: bool, guard: impl FnOnce(&Path)) {
    guard(dir);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !e.file_name().to_string_lossy().starts_with(TMP_PREFIX)
            || !e.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        if skip_live_owner && tmp_owner_alive(&e.file_name()) {
            continue;
        }
        let _ = std::fs::remove_file(e.path());
        crate::stall::beat();
    }
}

/// 设备上的临时文件名是不是 booklib 自己写的：严格匹配 [`tmp_sibling`] 的 `.tmp-<进程号>-<计数>-<名>`，
/// 两段数字都非空、名字段至少一个字符。返回进程号；不合规（含不是 UTF-8）返回 `None`。
/// 纯判断，不碰文件系统。
fn device_tmp_owner(name: &std::ffi::OsStr) -> Option<u32> {
    let rest = name.to_str()?.strip_prefix(TMP_PREFIX)?;
    let (pid, rest) = rest.split_once('-')?;
    let (count, file) = rest.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(pid) || !digits(count) || file.is_empty() {
        return None;
    }
    pid.parse().ok()
}

/// 设备连接时清 `dir` 本层里 booklib 自己写的残留临时文件。删设备上的文件是破坏性的，只删同时满足的：
/// 名字严格符合 `.tmp-<进程号>-<计数>-<名>`、主人进程不在（`/proc/<pid>`；沿用 [`clean_tmp_files`] 的保护）、
/// 是**普通文件**（不跟符号链接）。用户放在设备上的别的东西一概不碰：`.tmp-` 开头但名字不合规的、没有进程号的、
/// 目录、符号链接（链接和目标都不动）。目录本身不删。每删一个 `stall::beat()` 一次。
pub(crate) fn clean_device_tmp_files(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(pid) = device_tmp_owner(&e.file_name()) else {
            continue;
        };
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        if Path::new("/proc").join(pid.to_string()).exists() {
            continue; // 同一台电脑上别的书库还在写
        }
        let _ = std::fs::remove_file(e.path());
        crate::stall::beat();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_uses_unique_tmp_and_json_errors_are_returned() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        assert_ne!(tmp_sibling(&p), tmp_sibling(&p), "临时名每次不同");
        assert!(tmp_sibling(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(TMP_PREFIX));
        write_json(&p, &vec![1, 2]).unwrap();
        assert_eq!(read_json::<Vec<i32>>(&p), Some(vec![1, 2]));
        // 非 UTF-8 路径做 JSON 键：返回错误而不是 panic
        use std::os::unix::ffi::OsStrExt;
        let bad = PathBuf::from(std::ffi::OsStr::from_bytes(b"/x/\xff.epub"));
        let m: std::collections::BTreeMap<PathBuf, u8> = [(bad, 1)].into();
        assert!(write_json(&p, &m).is_err());
        assert_eq!(
            read_json::<Vec<i32>>(&p),
            Some(vec![1, 2]),
            "失败时原文件不动"
        );
        let names: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.json"], "不留临时文件");
    }

    #[test]
    fn broken_json_is_an_error_missing_is_fine() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.json");
        assert!(check_json::<Vec<i32>>(&p).is_ok(), "不在不算错");
        std::fs::write(&p, b"[1,").unwrap();
        assert!(check_json::<Vec<i32>>(&p).unwrap_err().contains("坏了"));
    }

    #[test]
    fn json_cache_notices_outside_writes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.json");
        let c: JsonCache<Vec<i32>> = JsonCache::new();
        assert!(c.get(&p).is_empty());
        c.put(&p, vec![1]).unwrap();
        assert_eq!(*c.get(&p), [1]);
        write_json(&p, &vec![2]).unwrap(); // 别的进程（原子写，换 inode）
        assert_eq!(*c.get(&p), [2]);
    }

    /// 测试钩子必须接在 `read_dir` 之前：判定在设备路径上 panic 时，目录根本没读、临时文件也还在。
    #[test]
    fn clean_tmp_guard_runs_before_read_dir() {
        let d = tempfile::tempdir().unwrap();
        let device = d.path().join("device");
        std::fs::create_dir_all(&device).unwrap();
        let tmp = device.join(".tmp-1-1-x.epub");
        std::fs::write(&tmp, b"x").unwrap();
        let guard = |dir: &Path| {
            if dir == device {
                panic!("清理读了设备目录");
            }
        };

        let r = std::panic::catch_unwind(|| clean_tmp(&device, guard));
        assert!(r.is_err(), "clean_tmp 读到设备目录要 panic");
        let guard = |dir: &Path| {
            if dir == device {
                panic!("清理读了设备目录");
            }
        };
        let r = std::panic::catch_unwind(|| clean_tmp_files(&device, true, guard));
        assert!(r.is_err(), "clean_tmp_files 读到设备目录要 panic");
        assert!(tmp.is_file(), "panic 在读目录之前，临时文件没删");
    }

    /// 设备清理只删名字严格符合 `.tmp-<进程号>-<计数>-<名>`、主人进程不在的普通文件；
    /// 名字不合规的、没有进程号的、目录、符号链接一律不碰（用户放在设备上的文件不能删）。
    #[test]
    fn device_cleanup_keeps_tmp_files_of_live_owner() {
        let d = tempfile::tempdir().unwrap();
        let me = std::process::id();
        let live = d.path().join(format!(".tmp-{me}-1-x.epub"));
        let dead = d.path().join(format!(".tmp-{}-1-x.epub", u32::MAX));
        let bad = d.path().join(".tmp-nope-1-x.epub");
        let no_pid = d.path().join(".tmp--1-x.epub");
        let no_count = d.path().join(".tmp-12-x.epub");
        let empty_name = d.path().join(".tmp-12-34-");
        let user = d.path().join(".tmp-我的笔记.txt");
        for p in [&live, &dead, &bad, &no_pid, &no_count, &empty_name, &user] {
            std::fs::write(p, b"x").unwrap();
        }
        let dir_tmp = d.path().join(format!(".tmp-{}-0-x", u32::MAX));
        std::fs::create_dir(&dir_tmp).unwrap();
        let target = d.path().join("目标.txt");
        std::fs::write(&target, b"target").unwrap();
        let link = d.path().join(format!(".tmp-{}-0-y", u32::MAX));
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(
            device_tmp_owner(live.file_name().unwrap()).is_some(),
            "当前进程还在"
        );
        assert_eq!(
            device_tmp_owner(bad.file_name().unwrap()),
            None,
            "名字不合规"
        );

        clean_device_tmp_files(d.path());

        assert!(live.is_file(), "主人还在的临时文件要留着");
        assert!(!dead.exists(), "主人不在、名字合规的临时文件要删");
        for p in [&bad, &no_pid, &no_count, &empty_name, &user] {
            assert!(p.is_file(), "不是我们命名的文件不能删：{}", p.display());
        }
        assert!(dir_tmp.is_dir(), "目录不能删");
        assert!(
            link.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink()),
            "符号链接不能删"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"target", "链接目标不动");
    }

    /// 设备清理认的名字：严格 `.tmp-<数字>-<数字>-<非空>`，别的都当成用户的。
    #[test]
    fn device_tmp_name_is_strict() {
        use std::os::unix::ffi::OsStrExt;
        let owner = |n: &str| device_tmp_owner(std::ffi::OsStr::new(n));
        assert_eq!(owner(".tmp-999999-0-丙.kepub.epub"), Some(999999));
        assert_eq!(owner(".tmp-1-0-x"), Some(1));
        assert_eq!(owner(".tmp-12-34-a-b.epub"), Some(12), "名字里可以有 -");
        for n in [
            ".tmp-我的笔记.txt",
            ".tmp-abc-1-x.epub",
            ".tmp--1-x.epub",
            ".tmp-1-x.epub",
            ".tmp-12-34-",
            ".tmp-nope-1-x.epub",
            ".tmp-1",
            ".tmp-1-2",
            "tmp-1-2-x",
            ".tmp-1-2-",
            "笔记.txt",
        ] {
            assert_eq!(owner(n), None, "{n} 不是我们写的临时文件");
        }
        let bad = std::ffi::OsStr::from_bytes(b".tmp-\xff-1-x");
        assert_eq!(device_tmp_owner(bad), None, "不是 UTF-8 的不认");
    }
}

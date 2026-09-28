//! 书库用到的文件操作：原子写、JSON 读写（带缓存）、流式哈希、进程锁、残留临时文件清理。

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
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
        let n = std::io::Read::read(&mut f, &mut buf).map_err(|e| format!("读 {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|x| format!("{x:02x}")).collect()
}

/// 同目录下的临时文件名（和目标在同一文件系统，改名是原子的）。带进程号和计数器，同时写同一个目标也不会撞名。
pub(crate) fn tmp_sibling(path: &Path) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let mut name = std::ffi::OsString::from(format!("{TMP_PREFIX}{}-{}-", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    name.push(path.file_name().unwrap_or_default());
    path.with_file_name(name)
}

/// 原子写：先写临时文件、落盘（fsync），再改名，最后把目录也落盘。中途断电或出错不会留下写了一半的目标文件。
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = tmp_sibling(path);
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            File::open(dir)?.sync_all()?;
        }
        Ok::<(), std::io::Error>(())
    })();
    result.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("写 {}: {e}", path.display())
    })
}

/// 读 JSON 文件；不在或读不出来返回 `None`。
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// 原子写 JSON（缩进格式）。序列化失败（比如路径不是 UTF-8）返回错误，不 panic。
pub(crate) fn write_json<T: Serialize + ?Sized>(path: &Path, v: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(v).map_err(|e| format!("写 {}: {e}", path.display()))?;
    write_atomic(path, s.as_bytes())
}

/// 文件的身份：设备号 + inode + 大小 + 修改时间。原子写（改名）总会换 inode，所以别的进程写过一定能看出来。
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileKey(u64, u64, u64, i64, i64);

fn file_key(path: &Path) -> Option<FileKey> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).ok()?;
    Some(FileKey(m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec()))
}

/// 小 JSON 文件的读缓存（`.state.json`、`deliveries.json`）：文件没变（见 [`FileKey`]）就不重读、不重新解析。
/// 一次 `build`/`list` 里每本书都要查一遍，不缓存的话每本书都重读一次。
pub(crate) struct JsonCache<T> {
    map: RefCell<HashMap<PathBuf, Cached<T>>>,
}

type Cached<T> = (Option<FileKey>, Rc<T>);

impl<T: DeserializeOwned + Serialize + Default> JsonCache<T> {
    pub(crate) fn new() -> Self {
        JsonCache { map: RefCell::new(HashMap::new()) }
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
        self.map.borrow_mut().insert(path.to_path_buf(), (key, v.clone()));
        v
    }

    /// 原子写，并更新缓存。
    pub(crate) fn put(&self, path: &Path, v: T) -> Result<(), String> {
        write_json(path, &v)?;
        self.map.borrow_mut().insert(path.to_path_buf(), (file_key(path), Rc::new(v)));
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
        self.held.set(false);
    }
}

pub(crate) fn lock<'a>(root: &Path, held: &'a Cell<bool>) -> Result<Lock<'a>, String> {
    let path = root.join(".lock");
    let f = File::options().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => {
            held.set(true);
            Ok(Lock { _file: f, held })
        }
        Err(std::fs::TryLockError::WouldBlock) => Err(format!("另一个 booklib 正在使用书库 {}，等它结束再试", root.display())),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

/// 删掉 `dir` 下名字以 [`TMP_PREFIX`] 开头的文件和目录（进程被杀时留下的）。只在持锁时调用。
pub(crate) fn clean_tmp(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !e.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
            continue;
        }
        let _ = if e.file_type().is_ok_and(|t| t.is_dir()) { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
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
        assert!(tmp_sibling(&p).file_name().unwrap().to_string_lossy().starts_with(TMP_PREFIX));
        write_json(&p, &vec![1, 2]).unwrap();
        assert_eq!(read_json::<Vec<i32>>(&p), Some(vec![1, 2]));
        // 非 UTF-8 路径做 JSON 键：返回错误而不是 panic
        use std::os::unix::ffi::OsStrExt;
        let bad = PathBuf::from(std::ffi::OsStr::from_bytes(b"/x/\xff.epub"));
        let m: std::collections::BTreeMap<PathBuf, u8> = [(bad, 1)].into();
        assert!(write_json(&p, &m).is_err());
        assert_eq!(read_json::<Vec<i32>>(&p), Some(vec![1, 2]), "失败时原文件不动");
        let names: Vec<String> = std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a.json"], "不留临时文件");
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
}

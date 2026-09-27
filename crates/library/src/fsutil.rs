//! 书库用到的文件操作：原子写、流式哈希、共享存储、进程锁。

use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::Storage;

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

/// 同目录下的临时文件名（和目标在同一文件系统，改名是原子的）。
fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// 原子写：先写临时文件再改名，中途断电或出错不会留下写了一半的目标文件。
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = tmp_sibling(path);
    std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("写 {}: {e}", path.display())
    })
}

/// 把 `src` 克隆（reflink）到 `dst`：共用磁盘数据、两边互不影响。文件系统不支持时返回 `None`，不做复制。
pub fn try_reflink(src: &Path, dst: &Path) -> Option<Storage> {
    let _ = std::fs::remove_file(dst);
    reflink_copy::reflink(src, dst).ok().map(|_| Storage::Reflink)
}

/// 把 `src` 放到 `dst`：能克隆就克隆，不行就复制。
///
/// 不用硬链接：硬链接和原文件是同一个文件，用户就地改了原文件，母版也跟着变，违反"母版不可变"。
pub fn share_or_copy(src: &Path, dst: &Path) -> Result<Storage, String> {
    if let Some(s) = try_reflink(src, dst) {
        return Ok(s);
    }
    std::fs::copy(src, dst).map_err(|e| format!("复制 {}: {e}", src.display()))?;
    Ok(Storage::Copy)
}

/// 书库的进程锁：同一时间只允许一个会改动书库的 `booklib` 进程（两个进程会互删临时目录、互相覆盖生成记录）。
/// 进程退出时锁自动释放。
pub struct Lock(#[allow(dead_code)] File);

pub fn lock(root: &Path) -> Result<Lock, String> {
    let path = root.join(".lock");
    let f = File::options().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(Lock(f)),
        Err(std::fs::TryLockError::WouldBlock) => Err(format!("另一个 booklib 正在使用书库 {}，等它结束再试", root.display())),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

//! 书库用到的文件操作：原子写、流式哈希、共享存储、进程锁。

use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::{Path, PathBuf};


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

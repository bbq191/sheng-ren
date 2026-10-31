//! 产物送到设备（2026-10-07 定：`sync` 按设备接没接上直接传，电脑上不留产物）。怎么送由 profile 的 `[deliver]` 定：
//!
//! - **MTP**（Kindle、掌阅）：jmtpfs 自动挂载在 `<挂载目录>/<mount>`（缺省 `$XDG_RUNTIME_DIR/mtp`），下面是存储
//!   （「Internal Storage」之类），产物放进存储根目录的 `<dir>/`，按普通文件写。挂载点在、里面有存储就算接上了。
//! - 没写 `[deliver]` 的模式（书库 `profiles/` 里的自定义模式）：产物照旧放在电脑上。
//!
//! 一轮 `sync` 里每台设备只连一次。`--watch` 每轮开头调 [`crate::Library::refresh_devices`]：没接上的下次用到时重新看。
//! （2026-10-31 以前还有 Move：经 SSH 调设备上书架服务的导入接口，Move 卖了以后删了，要找回看 git 历史。）

use profile::{Deliver, Profile};
use std::io::Read;
use std::path::{Path, PathBuf};

/// 设备在哪找（测试、特殊环境可改）。
#[derive(Clone, Debug)]
pub struct DeviceEnv {
    /// MTP 挂载点的上级目录（`<它>/<mount>`）。缺省 `$BOOKLIB_MTP_DIR`，没设就是 `$XDG_RUNTIME_DIR/mtp`。
    pub mtp_base: PathBuf,
    /// MTP 挂载点要真的挂着才算接上、才往里写（缺省开；`$BOOKLIB_MTP_NO_MOUNT_CHECK=1` 关，测试用普通目录当设备）。
    /// 2026-10-21：掌阅在 `sync` 中途断开，jmtpfs 卸掉后挂载点剩个空目录，后面的书建目录、写进了 `/run/user/1000`（tmpfs）直到写满，
    /// 还记成传上了。
    pub mount_check: bool,
}

impl DeviceEnv {
    pub fn from_env() -> DeviceEnv {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        let mtp_base = var("BOOKLIB_MTP_DIR").map(PathBuf::from).unwrap_or_else(|| {
            let run = var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())));
            run.join("mtp")
        });
        DeviceEnv {
            mtp_base,
            mount_check: !var("BOOKLIB_MTP_NO_MOUNT_CHECK").is_some_and(|v| v != "0"),
        }
    }
}

fn uid() -> u32 {
    // /proc/self 的属主就是本进程的 uid（不为这一个数引 libc）
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(1000)
    }
    #[cfg(not(unix))]
    1000
}

/// 一个模式的产物送到哪。
pub(crate) enum Target {
    /// 放在电脑上（没写 `[deliver]`）。
    Local,
    /// MTP 设备上的产物根目录（`<存储>/<dir>`）；`mount` 是要一直挂着的挂载点（不查挂没挂时 `None`）。
    Dir { root: PathBuf, mount: Option<PathBuf> },
}

impl Target {
    /// 给人看的"送到哪"（`booklib devices`、`sync` 开头）：MTP 设备上的产物根目录；放电脑上是 `None`。
    pub(crate) fn describe(&self) -> Option<String> {
        match self {
            Target::Local => None,
            Target::Dir { root, .. } => Some(root.display().to_string()),
        }
    }
}

/// 连设备：接上了返回送到哪，没接上返回原因。
pub(crate) fn connect(env: &DeviceEnv, p: &Profile) -> Result<Target, String> {
    match &p.deliver {
        None => Ok(Target::Local),
        Some(Deliver::Mtp { mount, dir }) => {
            let point = env.mtp_base.join(mount);
            if env.mount_check && !mounted(&point) {
                return Err(format!("没接上（{} 没挂上）", point.display()));
            }
            let storage = mtp_storage(&point).ok_or_else(|| format!("没接上（{} 不在或没挂上）", point.display()))?;
            // `dir` 不在（掌阅原来没有 documents/）的话传书时再建：只看接没接上的命令（devices）不往设备上写
            Ok(Target::Dir { root: storage.join(dir), mount: env.mount_check.then_some(point) })
        }
    }
}

/// `point` 是不是挂着的挂载点：和上一级目录不在同一个文件系统上。jmtpfs 卸掉后挂载点留成上一级（tmpfs）里的普通目录，
/// 这时往里写就写进了内存。
pub(crate) fn mounted(point: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dev = |p: &Path| std::fs::metadata(p).map(|m| m.dev());
        match (dev(point), point.parent().map(dev)) {
            (Ok(a), Some(Ok(b))) => a != b,
            _ => false,
        }
    }
    #[cfg(not(unix))]
    point.is_dir()
}

/// 挂载点下的存储目录：只有一个就是它；几个（插了存储卡）时取名字像内部存储的，否则按名字排第一个。
fn mtp_storage(point: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(point)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    let internal = dirs.iter().position(|d| d.file_name().is_some_and(|n| {
        let n = n.to_string_lossy().to_lowercase();
        n.contains("internal") || n.contains("内部")
    }));
    match internal {
        Some(i) => Some(dirs.swap_remove(i)),
        None => dirs.into_iter().next(),
    }
}

/// 把文件 `src` 放到 `dest`（目录没有就建；同一文件系统直接改名；`keep_src` 时或跨文件系统时复制）。`compare` 时 `dest` 已经是逐字节相同的
/// 文件就不动它（Kindle 覆盖成不同字节会清掉阅读进度，相同的不会；也省得在 MTP 上白拷）——要把 `dest` 读回来，调用方能用记录的
/// 哈希判断时传 `false`。复制缺省先写旁边的临时文件再换上去（先删旧的再改名：MTP 上改名不能覆盖已有文件）；`direct` 时先删旧的、
/// 直接写 `dest`——jmtpfs 的改名是把整个文件下载回来再上传一份，经临时文件等于传三遍（调用方要自己保证写到一半被打断时认得出来，
/// 见 `generate::Library::prepare_file`）。返回有没有真的写。
///
/// 目录刚建好、文件还没放进去时，主线程可能正好把它当成"变空的目录"删掉（删别的书的旧位置时顺带删空目录，
/// `generate::State::delete_placements`）：碰到目录不在了就重建再放，试几次。
pub(crate) fn put_file(src: &Path, dest: &Path, keep_src: bool, compare: bool, direct: bool) -> Result<bool, String> {
    let mut tries = 0;
    loop {
        match put_file_once(src, dest, keep_src, compare, direct) {
            Err((_, true)) if tries < 3 && dest.parent().is_some_and(|d| !d.is_dir()) && src.is_file() => tries += 1,
            r => return r.map_err(|(e, _)| e),
        }
    }
}

/// [`put_file`] 的一次：出错时第二项说是不是"目录（或文件）不在"一类的错误（可以重建目录再试）。
fn put_file_once(src: &Path, dest: &Path, keep_src: bool, compare: bool, direct: bool) -> Result<bool, (String, bool)> {
    let missing = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| (format!("{}: {e}", dir.display()), missing(&e)))?;
    }
    if compare && same_bytes(src, dest) {
        if !keep_src {
            let _ = std::fs::remove_file(src);
        }
        return Ok(false);
    }
    if !keep_src && std::fs::rename(src, dest).is_ok() {
        let _ = crate::fsutil::sync_parent(dest);
        return Ok(true);
    }
    let tmp = if direct {
        let _ = std::fs::remove_file(dest);
        dest.to_path_buf()
    } else {
        crate::fsutil::tmp_sibling(dest)
    };
    let copy = || -> std::io::Result<()> {
        // 大块读写：经 FUSE（jmtpfs）每次写都是一次往返，缺省 8KB 一块太碎
        let mut r = std::io::BufReader::with_capacity(COPY_BUF, std::fs::File::open(src)?);
        let mut w = std::io::BufWriter::with_capacity(COPY_BUF, std::fs::File::create(&tmp)?);
        std::io::copy(&mut r, &mut w)?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()
    };
    if let Err(e) = copy() {
        let _ = std::fs::remove_file(&tmp);
        return Err((format!("写 {}: {e}", dest.display()), missing(&e)));
    }
    if !direct {
        let _ = std::fs::remove_file(dest);
        if let Err(e) = std::fs::rename(&tmp, dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err((format!("改名成 {}: {e}", dest.display()), missing(&e)));
        }
    }
    let _ = crate::fsutil::sync_parent(dest);
    if !keep_src {
        let _ = std::fs::remove_file(src);
    }
    Ok(true)
}

/// 往设备上拷文件的读写块大小。
const COPY_BUF: usize = 1 << 20;

/// 两个文件逐字节相同（`b` 不在算不同）。先比大小，一样才读。
pub(crate) fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else { return false };
    if !mb.is_file() || ma.len() != mb.len() {
        return false;
    }
    let (Ok(fa), Ok(fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut ra, mut rb) = (std::io::BufReader::with_capacity(1 << 16, fa), std::io::BufReader::with_capacity(1 << 16, fb));
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let n = match ra.read(&mut ba) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return rb.read(&mut bb[..1]).is_ok_and(|m| m == 0);
        }
        if rb.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_prefers_internal() {
        let d = tempfile::tempdir().unwrap();
        assert!(mtp_storage(&d.path().join("没有")).is_none());
        assert!(mtp_storage(d.path()).is_none(), "空的挂载点（没挂上）不算接上");
        std::fs::create_dir(d.path().join("SD 卡")).unwrap();
        assert_eq!(mtp_storage(d.path()).unwrap(), d.path().join("SD 卡"));
        std::fs::create_dir(d.path().join("Internal Storage")).unwrap();
        assert_eq!(mtp_storage(d.path()).unwrap(), d.path().join("Internal Storage"));
    }

    #[test]
    fn unmounted_mount_point_is_not_connected() {
        let d = tempfile::tempdir().unwrap();
        assert!(!mounted(d.path()), "普通目录不是挂载点");
        #[cfg(target_os = "linux")]
        assert!(mounted(Path::new("/proc")));
        // 设备断开后挂载点剩下的空目录里有存储目录（上次写进去的）也不算接上
        std::fs::create_dir_all(d.path().join("ireader/内部共享存储空间")).unwrap();
        let p = profile::get("ireader").unwrap();
        let env = DeviceEnv { mtp_base: d.path().into(), mount_check: true };
        assert!(connect(&env, p).err().unwrap().contains("没挂上"));
        let t = connect(&DeviceEnv { mount_check: false, ..env }, p).unwrap();
        assert!(matches!(t, Target::Dir { mount: None, .. }));
    }

    #[test]
    fn put_file_skips_identical_and_replaces_different() {
        let d = tempfile::tempdir().unwrap();
        let (src, dest) = (d.path().join("src"), d.path().join("dest"));
        std::fs::write(&src, b"abc").unwrap();
        assert!(put_file(&src, &dest, true, true, false).unwrap(), "不在：写");
        assert!(!put_file(&src, &dest, true, true, false).unwrap(), "相同：不写");
        assert!(put_file(&src, &dest, true, false, false).unwrap(), "不比较：照写");
        std::fs::write(&src, b"abd").unwrap();
        assert!(put_file(&src, &dest, false, true, false).unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"abd");
        assert!(!src.exists(), "不留来源时挪走");
        assert!(!same_bytes(&dest, &d.path().join("x")));
        // 直接写：换掉旧的、不留临时文件
        std::fs::write(&src, b"xyz1").unwrap();
        assert!(put_file(&src, &dest, true, false, true).unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"xyz1");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2, "只有 src、dest");
    }
}

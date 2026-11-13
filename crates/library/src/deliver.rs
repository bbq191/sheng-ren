//! 产物送到设备（2026-10-07 定：`sync` 按设备接没接上直接传，电脑上不留产物）。怎么送由 profile 的 `[deliver]` 定：
//!
//! - 设备统一在 `~/Devices/<链接名>` 找（缺省 `$HOME/Devices`，可用 `$BOOKLIB_DEVICES_DIR` 改）：链接和挂载都由 dotfiles 管，
//!   booklib **只读**这些链接，绝不创建、删除或修改它们。开着挂载检查时，先确认它是符号链接、读出目标，再要求
//!   `/proc/self/mountinfo` 里有完全相同的挂载点；不 stat 目标、不进入目标、不按文件系统类型猜。
//! - **MTP**（Kindle、掌阅、汉王、Bigme）：链接下面是存储（「Internal Storage」之类），产物放进存储根目录的 `<dir>/`，
//!   按普通文件写；连上后用 `mtp_storage` 探测存储目录。
//! - **USB 大容量存储**（Kobo，2026-11-01 加）：链接指向 udisks 按卷标挂的目录，产物放进根目录的 `<dir>/`。
//!   阅读器自己的 `.kobo/` 一概不碰。
//! - 没写 `[deliver]` 的模式（书库 `profiles/` 里的自定义模式）：产物照旧放在电脑上。
//!
//! 2026-11-13 起不再回退旧挂载路径（`$XDG_RUNTIME_DIR/mtp/<小写>`、`/run/media/$USER/KOBOeReader`、`~/Hanvon`）；
//! 那些旧前缀只在生成记录迁移和 `on_removable` 判前缀时认，不用于找设备。
//!
//! 一轮 `sync` 里每台设备只连一次。`--watch` 每轮开头调 [`crate::Library::refresh_devices`]：没接上的下次用到时重新看。
//! （2026-10-31 以前还有 Move：经 SSH 调设备上书架服务的导入接口，Move 卖了以后删了，要找回看 git 历史。）

use profile::{Deliver, Profile};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// 设备在哪找（测试、特殊环境可改）。
#[derive(Clone, Debug)]
pub struct DeviceEnv {
    /// 设备的链接目录（`<它>/<链接名>`）：缺省 `$BOOKLIB_DEVICES_DIR`，没设就是 `$HOME/Devices`。链接由 dotfiles 建。
    pub devices_base: PathBuf,
    /// MTP 旧挂载点的上级目录（`<它>/<小写 mount>`）。**只用来认旧记录前缀**，不用于找设备。
    /// 缺省 `$BOOKLIB_MTP_DIR`，没设就是 `$XDG_RUNTIME_DIR/mtp`。
    pub mtp_base: PathBuf,
    /// 设备要真的挂上才算接上、才往里写（缺省开；`$BOOKLIB_MTP_NO_MOUNT_CHECK=1` 关，测试用普通目录当设备）。
    /// 2026-10-21：掌阅在 `sync` 中途断开，jmtpfs 卸掉后挂载点剩个空目录，后面的书建目录、写进了 `/run/user/1000`（tmpfs）直到写满，
    /// 还记成传上了。2026-11-13 起先看链接、再对 mountinfo，不 stat 目标。
    pub mount_check: bool,
    /// USB 大容量存储旧挂载点的上级目录（`<它>/<label>`）。**只用来认旧记录前缀**，不用于找设备。
    /// 缺省 `$BOOKLIB_USBMS_DIR`，没设就是 `/run/media/$USER`（它不在、`/media/$USER` 在时用后者）。
    pub usbms_base: PathBuf,
}

impl DeviceEnv {
    pub fn from_env() -> DeviceEnv {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        let devices_base = var("BOOKLIB_DEVICES_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().unwrap_or_default().join("Devices"));
        let mtp_base = var("BOOKLIB_MTP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_mtp_base);
        let mount_check = !var("BOOKLIB_MTP_NO_MOUNT_CHECK").is_some_and(|v| v != "0");
        let usbms_base = var("BOOKLIB_USBMS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if !mount_check {
                    return mtp_base.join("usbms");
                }
                let user = user_name().unwrap_or_default();
                // /run/media/$USER 要等第一次插 USB 存储才由 udisks 建：还不在时只有 /media/$USER 真在（Debian 系）才用它，
                // 否则 `sync --watch` 开着时第一次插上会一直找错地方
                let (run, media) = (
                    Path::new("/run/media").join(&user),
                    Path::new("/media").join(&user),
                );
                if !run.is_dir() && !user.is_empty() && media.is_dir() {
                    media
                } else {
                    run
                }
            });
        DeviceEnv {
            devices_base,
            mtp_base,
            mount_check,
            usbms_base,
        }
    }

    /// 这台设备旧记录可能用的前缀 → 新的 `devices_base/<链接名>`。只改生成记录，不碰文件。
    /// 长的前缀先比（`mtp_base` 和缺省目录可能互相包含）。
    pub(crate) fn prefix_migrations(&self, p: &Profile) -> Vec<(PathBuf, PathBuf)> {
        let Some(d) = &p.deliver else {
            return Vec::new();
        };
        let new = self.devices_base.join(d.link_name());
        let mut old = Vec::new();
        match d {
            Deliver::Mtp { mount, .. } => {
                old.push(self.mtp_base.join(mount));
                let default = default_mtp_base();
                if default != self.mtp_base {
                    old.push(default.join(mount));
                }
            }
            Deliver::Usbms { label, .. } => {
                old.push(self.usbms_base.join(label));
                // 缺省会在 /run/media/$USER 和 /media/$USER 之间换：两处都认（测试里的假用户也按这个换）
                for (a, b) in [("/run/media", "/media"), ("/media", "/run/media")] {
                    if let Ok(rest) = self.usbms_base.strip_prefix(a) {
                        old.push(Path::new(b).join(rest).join(label));
                    }
                }
                if let Some(user) = user_name().filter(|u| !u.is_empty()) {
                    for base in ["/run/media", "/media"] {
                        old.push(Path::new(base).join(&user).join(label));
                    }
                }
            }
        }
        // 废弃的 `~/Hanvon`：文档里出现过，记录里可能还有
        if p.id == "hanvon" {
            if let Some(home) = home_dir() {
                old.push(home.join("Hanvon"));
            }
        }
        old.retain(|o| *o != new);
        old.sort();
        old.dedup();
        old.sort_by_key(|b| std::cmp::Reverse(b.as_os_str().len()));
        old.into_iter().map(|o| (o, new.clone())).collect()
    }
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn user_name() -> Option<String> {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
}

fn default_mtp_base() -> PathBuf {
    let run = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())));
    run.join("mtp")
}

fn uid() -> u32 {
    // /proc/self 的属主就是本进程的 uid（不为这一个数引 libc）
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self")
            .map(|m| m.uid())
            .unwrap_or(1000)
    }
    #[cfg(not(unix))]
    1000
}

/// 一个模式的产物送到哪。
pub(crate) enum Target {
    /// 放在电脑上（没写 `[deliver]`）。
    Local,
    /// 设备上的产物根目录（MTP：`<存储>/<dir>`；USB 大容量存储：`<挂载点>/<dir>`）；`mount` 是要一直挂着的链接
    /// （不查挂没挂时 `None`）；`staged` 是写完临时文件再改名（USB 大容量存储；MTP 直接写正式文件名）。
    Dir {
        root: PathBuf,
        mount: Option<PathBuf>,
        staged: bool,
    },
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
        Some(d @ Deliver::Mtp { dir, .. }) => {
            let link = env.devices_base.join(d.link_name());
            if env.mount_check && !mounted_link(&link) {
                return Err(format!("没接上（{} 不在或没挂上）", link.display()));
            }
            if !env.mount_check && !link.is_dir() {
                return Err(format!("没接上（{} 不在）", link.display()));
            }
            let storage = mtp_storage(&link)
                .ok_or_else(|| format!("没接上（{} 不在或没挂上）", link.display()))?;
            // `dir` 不在（掌阅原来没有 documents/）的话传书时再建：只看接没接上的命令（devices）不往设备上写
            Ok(Target::Dir {
                root: storage.join(dir),
                mount: env.mount_check.then_some(link),
                staged: false,
            })
        }
        Some(d @ Deliver::Usbms { dir, .. }) => {
            // 只认 `~/Devices/<链接名>`：udisks 遇到同名目录占着时会挂成 `<label>1`，链接名却不会变；产物记录存的是绝对路径，
            // 挂载点名一变旧记录就对不上、删不掉。那时报没接上，用户卸掉重插。
            let link = env.devices_base.join(d.link_name());
            if env.mount_check && !mounted_link(&link) {
                return Err(format!("没接上（{} 不在或没挂上）", link.display()));
            }
            if !env.mount_check && !link.is_dir() {
                return Err(format!("没接上（{} 不在）", link.display()));
            }
            Ok(Target::Dir {
                root: link.join(dir),
                mount: env.mount_check.then_some(link),
                staged: true,
            })
        }
    }
}

/// `point` 是不是挂着的设备：符号链接（`~/Devices/<Name>`）看它指向的挂载点在不在 `/proc/self/mountinfo`，不 stat 目标；
/// 普通目录（旧的挂载点、测试）仍看和上一级目录是不是同一个文件系统。jmtpfs 卸掉后挂载点留成上一级（tmpfs）里的普通目录，
/// 这时往里写就写进了内存。
pub(crate) fn mounted(point: &Path) -> bool {
    #[cfg(unix)]
    {
        if std::fs::symlink_metadata(point).is_ok_and(|m| m.file_type().is_symlink()) {
            return mounted_link(point);
        }
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

/// 符号链接 `link` 指向的目标在 `/proc/self/mountinfo` 里挂着：先确认是符号链接、再 readlink，绝不 stat 目标。
fn mounted_link(link: &Path) -> bool {
    let Ok(info) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    link_mounted_in(link, &info)
}

/// [`mounted_link`] 的测试版：挂载信息由调用方给。
fn link_mounted_in(link: &Path, mountinfo: &str) -> bool {
    let Some(target) = link_target(link) else {
        return false;
    };
    mountinfo_points(mountinfo).any(|p| p == target)
}

/// 符号链接的目标，相对路径按**链接所在目录**解析成绝对路径；只做字面规整，不调用 `canonicalize`（那会 stat 目标）。
fn link_target(link: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(link).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let raw = std::fs::read_link(link).ok()?;
    let abs = if raw.is_absolute() {
        raw
    } else {
        link.parent()?.join(raw)
    };
    Some(lexical_normalize(&abs))
}

/// 去掉 `.`、按字面处理 `..`（不碰文件系统）。
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `/proc/self/mountinfo` 里每行的挂载点（第 5 列），先还原 `\040` 等八进制转义。
fn mountinfo_points(mountinfo: &str) -> impl Iterator<Item = PathBuf> + '_ {
    mountinfo.lines().filter_map(|line| {
        line.split_whitespace()
            .nth(4)
            .map(unescape_mountinfo)
    })
}

fn unescape_mountinfo(s: &str) -> PathBuf {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let v = (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0');
            out.push(v);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(out))
    }
    #[cfg(not(unix))]
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

/// 挂载点下的存储目录：只有一个就是它；几个（插了存储卡）时取名字像内部存储的，否则按名字排第一个。
fn mtp_storage(point: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(point)
        .ok()?
        .flatten()
        .filter(|e| {
            e.file_type().is_ok_and(|t| t.is_dir())
                && !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();
    let internal = dirs.iter().position(|d| {
        d.file_name().is_some_and(|n| {
            let n = n.to_string_lossy().to_lowercase();
            n.contains("internal") || n.contains("内部")
        })
    });
    match internal {
        Some(i) => Some(dirs.swap_remove(i)),
        None => dirs.into_iter().next(),
    }
}

/// [`put_file`] 的选项。2026-11-13 之前是四个按位置传的参数（三个 bool 挨着），传错位了编译器也看不出来，改成具名。
#[derive(Clone, Copy, Debug)]
pub(crate) struct PutOptions<'a> {
    /// 保留来源文件（false 时放好后删掉来源）。
    pub(crate) keep_src: bool,
    /// `dest` 已经是逐字节相同的文件就不动它（要把 `dest` 读回来，调用方能用记录的哈希判断时传 `false`）。
    pub(crate) compare: bool,
    /// 不经临时文件：先删 `dest`、直接写（见 [`put_file`]）。
    pub(crate) direct: bool,
    /// 要一直挂着的挂载点（不查挂没挂时 `None`）。
    pub(crate) mount: Option<&'a Path>,
}

/// 把文件 `src` 放到 `dest`（目录没有就建；同一文件系统直接改名；`keep_src` 时或跨文件系统时复制）。`compare` 时 `dest` 已经是逐字节相同的
/// 文件就不动它（Kindle 覆盖成不同字节会清掉阅读进度，相同的不会；也省得在 MTP 上白拷）——要把 `dest` 读回来，调用方能用记录的
/// 哈希判断时传 `false`。复制缺省先写旁边的临时文件再换上去（先删旧的再改名：MTP 上改名不能覆盖已有文件）；`direct` 时先删旧的、
/// 直接写 `dest`——jmtpfs 的改名是把整个文件下载回来再上传一份，经临时文件等于传三遍（调用方要自己保证写到一半被打断时认得出来，
/// 见 `generate::Library::prepare_file`）。返回有没有真的写。各项含义见 [`PutOptions`]。
///
/// 目录刚建好、文件还没放进去时，主线程可能正好把它当成"变空的目录"删掉（删别的书的旧位置时顺带删空目录，
/// `generate::State::delete_placements`）：碰到目录不在了就重建再放，试几次。
pub(crate) fn put_file(
    src: &Path,
    dest: &Path,
    opts: PutOptions<'_>,
) -> Result<bool, crate::error::Error> {
    // 挪到只差大小写的新名字（书名只改了大小写）：不分大小写的文件系统上 `dest` 就是 `src`，下面"先删 dest"会把书删掉。
    // 先改成旁边的临时名字，再从那里放
    if !opts.keep_src && crate::generate::case_twin(src, dest) {
        let staged = crate::fsutil::tmp_sibling(dest);
        std::fs::rename(src, &staged).map_err(|e| format!("改名 {}: {e}", src.display()))?;
        let r = put_file_retry(
            &staged,
            dest,
            PutOptions {
                keep_src: false,
                ..opts
            },
        );
        if r.is_err() && staged.is_file() {
            let _ = std::fs::rename(&staged, src); // 放回原名，记录还对得上
        }
        return r;
    }
    // 逐段只差大小写、父目录不同的两个位置（汉王 documents/Documents）：不分大小写的设备上就是同一个文件。
    // 按 keep_src 处理；并且**强制不走直接写**——直接写会先删 dest，不分大小写时 dest 就是 src，书先没了、
    // 再从已不在的 src 复制失败。经临时文件那条路在临时文件写好之前不碰 dest（agy/验收阻断 1）。
    if !opts.keep_src && crate::generate::case_alias(src, dest) {
        return put_file_retry(
            src,
            dest,
            PutOptions {
                keep_src: true,
                direct: false,
                ..opts
            },
        );
    }
    put_file_retry(src, dest, opts)
}

fn put_file_retry(
    src: &Path,
    dest: &Path,
    opts: PutOptions<'_>,
) -> Result<bool, crate::error::Error> {
    use crate::error::{Error, Kind};
    // 每次写之前、写完都看挂载点还挂着：设备中途断开后挂载点只剩空目录（MTP 的在 tmpfs 上，用户能写），
    // 重试时重建目录会把书写进内存、还记成传上了
    let gone = || {
        opts.mount.filter(|m| !mounted(m)).map(|m| {
            Error::new(
                Kind::Unmounted,
                format!("设备断开了（{} 没挂着），没传", m.display()),
            )
        })
    };
    let mut tries = 0;
    loop {
        if let Some(e) = gone() {
            return Err(e);
        }
        match put_file_once(src, dest, opts) {
            Err((_, true))
                if tries < 3 && dest.parent().is_some_and(|d| !d.is_dir()) && src.is_file() =>
            {
                tries += 1
            }
            r => {
                // 失败时挂载点也不在了：按断开报（断开后的挂载点在 tmpfs 上，写满报的 ENOSPC 不是设备满）
                let r = r.map_err(|(e, _)| match gone() {
                    Some(g) => Error::new(g.kind(), format!("{g}（写的途中断开）：{e}")),
                    None => e,
                })?;
                return match gone() {
                    Some(e) => Err(Error::new(e.kind(), format!("{e}（写的途中断开）"))),
                    None => Ok(r),
                };
            }
        }
    }
}

/// 不分大小写的文件系统上 `dest` 打开的可能是只差大小写的旧名字（书名只改了大小写、内容一样没重写）：目录里没有
/// `dest` 的原名、有只差大小写的那个时，经临时名改成 `dest`，设备上的名字和记录一致。改不了就算了（内容是对的）。
fn fix_case(dest: &Path) {
    let (Some(dir), Some(name)) = (dest.parent(), dest.file_name()) else {
        return;
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let names: Vec<std::ffi::OsString> = rd.flatten().map(|e| e.file_name()).collect();
    if names.iter().any(|n| n == name) {
        return;
    }
    let lower = name.to_string_lossy().to_lowercase();
    let Some(old) = names
        .iter()
        .find(|n| n.to_string_lossy().to_lowercase() == lower)
    else {
        return;
    };
    let (old, tmp) = (dir.join(old), crate::fsutil::tmp_sibling(dest));
    if std::fs::rename(&old, &tmp).is_ok() {
        if std::fs::rename(&tmp, dest).is_err() {
            let _ = std::fs::rename(&tmp, &old);
        }
        let _ = crate::fsutil::sync_parent(dest);
    }
}

#[cfg(test)]
thread_local! {
    /// 测试用：设了就让下一次复制写进一点后报这个 errno（模拟设备写满）。
    static FAIL_WRITE: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

/// 测试用的写错误：单元测试设 [`FAIL_WRITE`]；集成测试设环境变量 `BOOKLIB_TEST_ENOSPC=1`（每次复制都写进一点后报 ENOSPC）。
fn fail_write() -> Option<i32> {
    #[cfg(test)]
    if let Some(e) = FAIL_WRITE.with(|f| f.get()) {
        return Some(e);
    }
    (std::env::var_os("BOOKLIB_TEST_ENOSPC").is_some_and(|v| v == "1")).then_some(28)
}

/// 写设备的错误说成一行：设备满了的带上 [`crate::error::Kind::NoSpace`]（一轮生成据此停下这台设备、不记失败表），
/// 文字照旧以 [`crate::space::NO_SPACE`] 开头。
fn io_error(what: String, e: &std::io::Error) -> crate::error::Error {
    use crate::error::{Error, Kind};
    if crate::space::is_no_space(e) {
        Error::new(
            Kind::NoSpace,
            format!("{}：{what}: {e}", crate::space::NO_SPACE),
        )
    } else {
        Error::other(format!("{what}: {e}"))
    }
}

/// 建一级目录报 EIO 后重试的间隔（毫秒）。
const MKDIR_BACKOFF: &[u64] = &[200, 500, 1000];

/// `p` 是不是目录：先 stat；stat 不到再列一次上级目录看（jmtpfs 刚删过东西时偶尔 stat 不到已有的目录，
/// 列目录会让它刷新这一层，同 `generate::remove_product`）。
fn dir_present(p: &Path) -> bool {
    if p.is_dir() {
        return true;
    }
    let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else {
        return false;
    };
    let listed = std::fs::read_dir(parent).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| e.file_name() == name && e.file_type().is_ok_and(|t| t.is_dir()))
    });
    listed || p.is_dir()
}

/// 确保 `dir` 是目录，从已有的那一层往下逐级建（不用 `create_dir_all`：它只把 EEXIST 当成功）。
/// jmtpfs 刚删过一批文件和空目录（untrack 后的孤儿）时，对已存在的目录 mkdir 偶尔报 EIO（2026-11-02、11-03 掌阅两次
/// 各 12 本）：只对 EIO 按 `backoff` 等一等、列目录再看，在了就算成功，不在就再建；别的错误（无权限、只读、写满、
/// 断开后的 NotFound 等）照原样马上报。`mkdir` 建一级（测试换掉它）。
fn ensure_dir(
    dir: &Path,
    mut mkdir: impl FnMut(&Path) -> std::io::Result<()>,
    backoff: &[u64],
) -> std::io::Result<()> {
    let mut todo = Vec::new();
    let mut cur = Some(dir);
    while let Some(d) = cur.filter(|d| !d.as_os_str().is_empty() && !dir_present(d)) {
        todo.push(d);
        cur = d.parent();
    }
    for d in todo.into_iter().rev() {
        let mut waits = backoff.iter();
        loop {
            match mkdir(d) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && dir_present(d) => break,
                Err(e) if e.raw_os_error() == Some(libc::EIO) => match waits.next() {
                    Some(ms) => {
                        std::thread::sleep(std::time::Duration::from_millis(*ms));
                        if dir_present(d) {
                            break;
                        }
                    }
                    None if dir_present(d) => break,
                    None => return Err(e),
                },
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

/// [`put_file`] 的一次：出错时第二项说是不是"目录（或文件）不在"一类的错误（可以重建目录再试）。
fn put_file_once(
    src: &Path,
    dest: &Path,
    opts: PutOptions<'_>,
) -> Result<bool, (crate::error::Error, bool)> {
    let missing = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    if let Some(dir) = dest.parent() {
        ensure_dir(dir, |d| std::fs::create_dir(d), MKDIR_BACKOFF)
            .map_err(|e| (io_error(dir.display().to_string(), &e), missing(&e)))?;
    }
    if opts.compare && same_bytes(src, dest) {
        if !opts.keep_src {
            let _ = std::fs::remove_file(src);
        }
        fix_case(dest);
        return Ok(false);
    }
    // 测试钩子 BOOKLIB_TEST_ENOSPC 要走复制那条路（同一文件系统上改名不会写满）
    if !opts.keep_src && fail_write().is_none() && std::fs::rename(src, dest).is_ok() {
        let _ = crate::fsutil::sync_parent(dest);
        return Ok(true);
    }
    let tmp = if opts.direct {
        let _ = std::fs::remove_file(dest);
        dest.to_path_buf()
    } else {
        crate::fsutil::tmp_sibling(dest)
    };
    let copy = || -> std::io::Result<()> {
        // 大块读写：经 FUSE（jmtpfs）每次写都是一次往返，缺省 8KB 一块太碎
        let mut r = std::io::BufReader::with_capacity(COPY_BUF, std::fs::File::open(src)?);
        let mut w = std::io::BufWriter::with_capacity(COPY_BUF, std::fs::File::create(&tmp)?);
        if let Some(e) = fail_write() {
            use std::io::Write;
            w.write_all(b"partial")?;
            w.flush()?;
            return Err(std::io::Error::from_raw_os_error(e));
        }
        std::io::copy(&mut r, &mut w)?;
        let f = w.into_inner().map_err(|e| e.into_error())?;
        f.sync_all()?;
        // jmtpfs 在关文件（FUSE flush）时才真正传上设备：关的错误（设备满、断开）要看，`File` 自己 drop 时会把它丢掉
        use std::os::unix::io::IntoRawFd;
        // SAFETY: fd 刚从 File 里拿出来、只关这一次
        if unsafe { libc::close(f.into_raw_fd()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    };
    if let Err(e) = copy() {
        // 写到一半的（临时文件，或直接写的 dest 本身）都删掉。直接写的记录在生成时先记了空指纹（`prepare_file`），
        // 下次认得出没写完、重传
        let _ = std::fs::remove_file(&tmp);
        // 设备满了：种类标成 NoSpace（`io_error`），一轮生成据此停下这台设备、不记进失败表
        return Err((io_error(format!("写 {}", dest.display()), &e), missing(&e)));
    }
    if !opts.direct {
        let _ = std::fs::remove_file(dest);
        if let Err(e) = std::fs::rename(&tmp, dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err((
                io_error(format!("改名成 {}", dest.display()), &e),
                missing(&e),
            ));
        }
    }
    let _ = crate::fsutil::sync_parent(dest);
    if !opts.keep_src {
        let _ = std::fs::remove_file(src);
    }
    Ok(true)
}

#[cfg(test)]
mod space_tests {
    /// 写的途中满了：临时文件删掉、dest 不留半截，错误以 NO_SPACE 开头；直接写的也一样。
    #[test]
    fn enospc_cleans_up() {
        for direct in [false, true] {
            let d = tempfile::tempdir().unwrap();
            let src = d.path().join("src.epub");
            std::fs::write(&src, b"content").unwrap();
            let dest = d.path().join("dev").join("book.epub");
            super::FAIL_WRITE.with(|f| f.set(Some(28)));
            let r = super::put_file(
                &src,
                &dest,
                super::PutOptions {
                    keep_src: true,
                    compare: false,
                    direct,
                    mount: None,
                },
            );
            super::FAIL_WRITE.with(|f| f.set(None));
            let e = r.unwrap_err();
            assert!(e.kind() == crate::error::Kind::NoSpace, "{e}");
            assert!(
                e.as_str().starts_with(crate::space::NO_SPACE),
                "{}",
                e.as_str()
            );
            let left: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
                .unwrap()
                .flatten()
                .collect();
            assert!(left.is_empty(), "direct={direct}: {left:?}");
            assert!(src.is_file());
        }
    }

    /// 别的写错误不标 NO_SPACE。
    #[test]
    fn other_errors_not_no_space() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src.epub");
        std::fs::write(&src, b"x").unwrap();
        super::FAIL_WRITE.with(|f| f.set(Some(5)));
        let r = super::put_file(
            &src,
            &d.path().join("b.epub"),
            super::PutOptions {
                keep_src: true,
                compare: false,
                direct: false,
                mount: None,
            },
        );
        super::FAIL_WRITE.with(|f| f.set(None));
        let e = r.unwrap_err();
        assert!(e.kind() != crate::error::Kind::NoSpace);
        assert!(!e.as_str().starts_with(crate::space::NO_SPACE));
    }
}

#[cfg(test)]
mod case_tests {
    /// 只差大小写的挪动：先挪到临时名字再放，不会因为"先删 dest"把源删掉。
    /// 内容一样不重写时，设备上只差大小写的旧名字改成新名字（模拟不分大小写的文件系统上 `dest` 打开到旧名字）。
    #[test]
    fn same_bytes_fixes_case() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Abc.epub"), b"x").unwrap();
        super::fix_case(&d.path().join("abc.epub"));
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["abc.epub"]);
        super::fix_case(&d.path().join("abc.epub")); // 已经对了：不动
        assert!(d.path().join("abc.epub").is_file());
    }

    #[test]
    fn case_only_move() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("Abc.epub"), d.path().join("abc.epub"));
        std::fs::write(&a, b"book").unwrap();
        assert!(super::put_file(
            &a,
            &b,
            super::PutOptions {
                keep_src: false,
                compare: false,
                direct: true,
                mount: None,
            },
        )
        .unwrap());
        assert_eq!(std::fs::read(&b).unwrap(), b"book");
        assert!(!a.exists());
        let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "没留临时文件");
    }

    /// 给了挂载点而它没挂着（拔掉后留下的空目录）：不建目录、不写，报错。
    #[test]
    fn unmounted_point_not_written() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.epub");
        std::fs::write(&src, b"book").unwrap();
        let point = d.path().join("mtp/dev");
        std::fs::create_dir_all(&point).unwrap();
        let dest = point.join("存储/documents/a.epub");
        let r = super::put_file(
            &src,
            &dest,
            super::PutOptions {
                keep_src: true,
                compare: false,
                direct: true,
                mount: Some(&point),
            },
        );
        assert!(r.is_err());
        assert!(!point.join("存储").exists(), "没在空挂载点里建目录");
    }
}

/// 往设备上拷文件的读写块大小。
const COPY_BUF: usize = 1 << 20;

/// 两个文件逐字节相同（`b` 不在算不同）。先比大小，一样才读。
pub(crate) fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
        return false;
    };
    if !mb.is_file() || ma.len() != mb.len() {
        return false;
    }
    let (Ok(fa), Ok(fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let (mut ra, mut rb) = (
        std::io::BufReader::with_capacity(1 << 16, fa),
        std::io::BufReader::with_capacity(1 << 16, fb),
    );
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
    fn eio() -> std::io::Error {
        std::io::Error::from_raw_os_error(libc::EIO)
    }

    #[test]
    fn ensure_dir_skips_mkdir_when_dir_exists() {
        let d = tempfile::tempdir().unwrap();
        let mut calls = 0;
        super::ensure_dir(
            d.path(),
            |_| {
                calls += 1;
                Err(eio())
            },
            &[0],
        )
        .unwrap();
        assert_eq!(calls, 0);
    }

    #[test]
    fn ensure_dir_builds_missing_levels_and_retries_eio_on_an_ancestor() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("侦探悬疑").join("亚洲侦探");
        let mut seen = Vec::new();
        super::ensure_dir(
            &dir,
            |p| {
                seen.push(p.to_path_buf());
                if seen.len() < 3 {
                    Err(eio())
                } else {
                    std::fs::create_dir(p)
                }
            },
            &[0, 0, 0],
        )
        .unwrap();
        assert!(dir.is_dir());
        // 祖先那一层报了两次 EIO，第三次建成，再建叶子
        let anc = d.path().join("侦探悬疑");
        assert_eq!(seen, vec![anc.clone(), anc.clone(), anc, dir]);
    }

    #[test]
    fn ensure_dir_accepts_dir_that_appeared_after_eio() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("迷宮飯");
        let mut calls = 0;
        super::ensure_dir(
            &dir,
            |p| {
                calls += 1;
                if calls == 1 {
                    std::fs::create_dir(p).unwrap(); // 设备上其实建好了，却报 EIO
                }
                Err(eio())
            },
            &[0, 0],
        )
        .unwrap();
        assert_eq!(calls, 1);
    }

    #[test]
    fn ensure_dir_only_retries_eio() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("x");
        let mut calls = 0;
        let e = super::ensure_dir(
            &dir,
            |_| {
                calls += 1;
                Err(eio())
            },
            &[0, 0],
        )
        .unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::EIO));
        assert_eq!(calls, 3);
        // 写满、无权限、只读、不在（断开）：不等、马上报
        for errno in [
            libc::ENOSPC,
            libc::EACCES,
            libc::EROFS,
            libc::ENOENT,
            libc::ENOTDIR,
        ] {
            let mut calls = 0;
            let e = super::ensure_dir(
                &dir,
                |_| {
                    calls += 1;
                    Err(std::io::Error::from_raw_os_error(errno))
                },
                &[60_000],
            )
            .unwrap_err();
            assert_eq!((e.raw_os_error(), calls), (Some(errno), 1));
        }
        // 报"已存在"但其实不是目录（同名文件）：报错
        std::fs::write(&dir, b"").unwrap();
        assert!(super::ensure_dir(&dir, |p| std::fs::create_dir(p), &[0]).is_err());
    }

    #[test]
    fn ensure_dir_real_mkdir_and_backoff_is_short() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("a").join("b");
        super::ensure_dir(&dir, |p| std::fs::create_dir(p), super::MKDIR_BACKOFF).unwrap();
        assert!(dir.is_dir());
        assert!(super::MKDIR_BACKOFF.iter().sum::<u64>() <= 2000);
    }

    use super::*;

    #[test]
    fn storage_prefers_internal() {
        let d = tempfile::tempdir().unwrap();
        assert!(mtp_storage(&d.path().join("没有")).is_none());
        assert!(
            mtp_storage(d.path()).is_none(),
            "空的挂载点（没挂上）不算接上"
        );
        std::fs::create_dir(d.path().join("SD 卡")).unwrap();
        assert_eq!(mtp_storage(d.path()).unwrap(), d.path().join("SD 卡"));
        std::fs::create_dir(d.path().join("Internal Storage")).unwrap();
        assert_eq!(
            mtp_storage(d.path()).unwrap(),
            d.path().join("Internal Storage")
        );
    }

    #[test]
    fn unmounted_mount_point_is_not_connected() {
        let d = tempfile::tempdir().unwrap();
        assert!(!mounted(d.path()), "普通目录不是挂载点");
        #[cfg(target_os = "linux")]
        assert!(mounted(Path::new("/proc")));
        // 设备断开后链接指向的挂载点不在（或只剩空目录），里面有存储目录也不算接上
        let devices = d.path().join("devices");
        std::fs::create_dir_all(devices.join("Ireader/内部共享存储空间")).unwrap();
        let p = profile::get("ireader").unwrap();
        let env = DeviceEnv {
            devices_base: devices.clone(),
            mtp_base: d.path().join("mtp"),
            mount_check: true,
            usbms_base: d.path().join("usbms"),
        };
        assert!(connect(&env, p).err().unwrap().contains("没挂上"));
        let t = connect(
            &DeviceEnv {
                mount_check: false,
                ..env
            },
            p,
        )
        .unwrap();
        assert!(matches!(&t, Target::Dir { mount: None, root, staged: false } if *root == devices.join("Ireader/内部共享存储空间/Documents")));
        assert!(!devices.join("Ireader").is_symlink(), "booklib 不建链接");
    }

    #[test]
    fn usbms_connects_by_link_name_not_label() {
        let d = tempfile::tempdir().unwrap();
        let p = profile::get("kobo").unwrap();
        let devices = d.path().join("devices");
        let env = DeviceEnv {
            devices_base: devices.clone(),
            mtp_base: d.path().join("mtp"),
            mount_check: true,
            usbms_base: d.path().join("media"),
        };
        assert!(
            connect(&env, p).err().unwrap().contains("没挂上"),
            "链接不在"
        );
        // 拔掉后留下的普通目录不算接上（不是符号链接）
        std::fs::create_dir_all(devices.join("Kobo")).unwrap();
        assert!(connect(&env, p).err().unwrap().contains("没挂上"));
        // 不查挂载（测试）：目录在就算接上，产物放 <链接>/books
        let t = connect(
            &DeviceEnv {
                mount_check: false,
                ..env.clone()
            },
            p,
        )
        .unwrap();
        assert!(
            matches!(&t, Target::Dir { root, mount: None, staged: true } if *root == devices.join("Kobo/books"))
        );
        assert!(connect(
            &DeviceEnv {
                mount_check: false,
                devices_base: d.path().join("无"),
                ..env.clone()
            },
            p
        )
        .err()
        .unwrap()
        .contains("不在"));
        #[cfg(target_os = "linux")]
        {
            // 真的挂载点算接上（/proc 当成 Kobo 链接的目标）；只按链接名找，不拿卷标拼目录
            std::fs::remove_dir_all(devices.join("Kobo")).unwrap();
            std::os::unix::fs::symlink("/proc", devices.join("Kobo")).unwrap();
            assert!(
                matches!(&connect(&env, p).unwrap(), Target::Dir { root, mount: Some(m), staged: true } if *root == devices.join("Kobo/books") && *m == devices.join("Kobo"))
            );
            let mut q = p.clone();
            q.deliver = Some(Deliver::Usbms {
                label: "KOBOeReader".into(),
                link: "Pro".into(),
                dir: "books".into(),
            });
            assert!(connect(&env, &q).is_err(), "链接名是 Pro，不是卷标");
        }
    }

    #[test]
    fn mountinfo_unescapes_octal_and_requires_exact_mount_point() {
        assert_eq!(
            unescape_mountinfo("/a\\040b\\011c\\012d\\134e"),
            PathBuf::from("/a b\tc\nd\\e")
        );
        let d = tempfile::tempdir().unwrap();
        let mnt = d.path().join("mtp dir");
        std::fs::create_dir_all(&mnt).unwrap();
        #[cfg(unix)]
        {
            let link = d.path().join("Kindle");
            std::os::unix::fs::symlink(&mnt, &link).unwrap();
            let escaped = mnt.to_string_lossy().replace(' ', "\\040");
            let info = format!("1 0 0:1 / {escaped} rw - fuse jmtpfs rw\n");
            assert!(link_mounted_in(&link, &info), "转义后完全相等");
            assert!(!link_mounted_in(
                &link,
                "1 0 0:1 / /别的 rw - fuse jmtpfs rw\n"
            ));
            // 相对链接按链接所在目录解析
            let link2 = d.path().join("Kobo");
            std::os::unix::fs::symlink("mtp dir", &link2).unwrap();
            assert!(link_mounted_in(&link2, &info));
            // 链接不在、不是符号链接都不算
            assert!(!link_mounted_in(&d.path().join("没有"), &info));
            std::fs::create_dir(d.path().join("Hanvon")).unwrap();
            assert!(!link_mounted_in(&d.path().join("Hanvon"), &info));
        }
    }

    #[test]
    fn symlink_to_plain_dir_is_not_connected() {
        let d = tempfile::tempdir().unwrap();
        let devices = d.path().join("devices");
        let mnt = d.path().join("mnt");
        std::fs::create_dir_all(devices.join("Kindle")).unwrap();
        std::fs::create_dir_all(mnt.join("Internal Storage")).unwrap();
        std::fs::remove_dir_all(devices.join("Kindle")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&mnt, devices.join("Kindle")).unwrap();
        let p = profile::get("kindle").unwrap();
        let env = DeviceEnv {
            devices_base: devices.clone(),
            mtp_base: d.path().join("mtp"),
            mount_check: true,
            usbms_base: d.path().join("usbms"),
        };
        assert!(
            connect(&env, p).err().unwrap().contains("没挂上"),
            "链接在，但目标不在 mountinfo"
        );
        assert!(connect(
            &DeviceEnv {
                mount_check: false,
                ..env
            },
            p
        )
        .is_ok());
    }

    #[test]
    fn prefix_migrations_include_old_candidates() {
        let env = DeviceEnv {
            devices_base: "/home/u/Devices".into(),
            mtp_base: "/run/user/1000/mtp".into(),
            mount_check: true,
            usbms_base: "/run/media/u".into(),
        };
        let k = env.prefix_migrations(profile::get("kindle").unwrap());
        assert!(k.contains(&(
            PathBuf::from("/run/user/1000/mtp/kindle"),
            PathBuf::from("/home/u/Devices/Kindle")
        )));
        let h = env.prefix_migrations(profile::get("hanvon").unwrap());
        assert!(h.iter().any(|(o, n)| o.ends_with("Hanvon")
            && n == Path::new("/home/u/Devices/Hanvon")));
        let kobo = env.prefix_migrations(profile::get("kobo").unwrap());
        assert!(kobo.iter().any(|(o, n)| o == Path::new("/run/media/u/KOBOeReader")
            && n == Path::new("/home/u/Devices/Kobo")));
        assert!(kobo.iter().any(|(o, _)| o == Path::new("/media/u/KOBOeReader")));
    }

    #[test]
    fn put_file_skips_identical_and_replaces_different() {
        let d = tempfile::tempdir().unwrap();
        let (src, dest) = (d.path().join("src"), d.path().join("dest"));
        std::fs::write(&src, b"abc").unwrap();
        assert!(
            put_file(
                &src,
                &dest,
                PutOptions {
                    keep_src: true,
                    compare: true,
                    direct: false,
                    mount: None,
                },
            )
            .unwrap(),
            "不在：写"
        );
        assert!(
            !put_file(
                &src,
                &dest,
                PutOptions {
                    keep_src: true,
                    compare: true,
                    direct: false,
                    mount: None,
                },
            )
            .unwrap(),
            "相同：不写"
        );
        assert!(
            put_file(
                &src,
                &dest,
                PutOptions {
                    keep_src: true,
                    compare: false,
                    direct: false,
                    mount: None,
                },
            )
            .unwrap(),
            "不比较：照写"
        );
        std::fs::write(&src, b"abd").unwrap();
        assert!(put_file(
            &src,
            &dest,
            PutOptions {
                keep_src: false,
                compare: true,
                direct: false,
                mount: None,
            },
        )
        .unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"abd");
        assert!(!src.exists(), "不留来源时挪走");
        assert!(!same_bytes(&dest, &d.path().join("x")));
        // 直接写：换掉旧的、不留临时文件
        std::fs::write(&src, b"xyz1").unwrap();
        assert!(put_file(
            &src,
            &dest,
            PutOptions {
                keep_src: true,
                compare: false,
                direct: true,
                mount: None,
            },
        )
        .unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"xyz1");
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            2,
            "只有 src、dest"
        );
    }

    /// 父目录只差大小写（documents/Documents，模拟不分大小写的设备）：已一样就不删 src；不一样按 keep_src 复制。
    #[test]
    fn put_file_case_alias_never_deletes_src() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("documents");
        let new = d.path().join("Documents");
        std::fs::create_dir_all(&old).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&old, &new).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir_all(&new).unwrap();
        let src = old.join("书.epub");
        std::fs::write(&src, b"same").unwrap();
        let dest = new.join("书.epub");
        assert!(!put_file(
            &src,
            &dest,
            PutOptions {
                keep_src: false,
                compare: true,
                direct: true,
                mount: None,
            },
        )
        .unwrap());
        assert!(src.exists() && dest.exists(), "同一个文件：不删 src");

        // compare=false、direct=true：也必须强制走临时文件，不能先删 dest（不分大小写时 dest 就是 src）
        let src3 = old.join("另一本.epub");
        std::fs::write(&src3, b"new content").unwrap();
        let dest3 = new.join("另一本.epub");
        assert!(put_file(
            &src3,
            &dest3,
            PutOptions {
                keep_src: false,
                compare: false,
                direct: true,
                mount: None,
            },
        )
        .unwrap());
        assert_eq!(std::fs::read(&dest3).unwrap(), b"new content");
        assert!(src3.exists(), "不删来源");
        let leftovers = std::fs::read_dir(&old)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .count();
        assert_eq!(leftovers, 0, "不留临时文件");

        // 大小写敏感的两个真目录（这里用另一个大小写目录）：复制、不删 src
        let other = d.path().join("DOCUMENTS");
        std::fs::create_dir_all(&other).unwrap();
        let dest2 = other.join("书.epub");
        std::fs::write(&dest2, b"old").unwrap();
        assert!(put_file(
            &src,
            &dest2,
            PutOptions {
                keep_src: false,
                compare: true,
                direct: false,
                mount: None,
            },
        )
        .unwrap());
        assert_eq!(std::fs::read(&dest2).unwrap(), b"same");
        assert!(src.exists(), "不删来源");
    }
}

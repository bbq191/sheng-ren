//! 传之前看设备上放不放得下（2026-11-03 定）：一轮要传的产物估个总大小，和设备剩余空间比，不够的设备这一轮不写。
//! 写的途中满了（ENOSPC）见 `deliver::put_file`。

use std::path::Path;

/// 写的途中设备满了时给人看的那句话的开头：一轮生成按错误种类 [`crate::error::Kind::NoSpace`] 停下这台设备，
/// 不记进失败表（下一轮、下次同步重传）。
pub(crate) const NO_SPACE: &str = "空间满";

/// 留的余量下限：50MB。
const MARGIN_MIN: u64 = 50 * 1024 * 1024;

/// 开了 `comic_upscale` 的漫画还没生成时，按原件大小乘这个倍数估：小页放大到框后 JPEG 变大。
/// 依据：《竹光侍》原件 80.9MB、放大后的产物 164.8MB，约 2.04 倍；取 2.1 留一点富余。
pub(crate) const COMIC_UPSCALE_FACTOR: f64 = 2.1;

/// 一本书这一轮要放上设备的估计：`new` 新产物的大小（已生成的按实际大小，没生成的按原件估），
/// `old` 这次覆盖掉的设备上旧文件的大小（没有就是 0）；`staged`：写的时候新旧两份同时在设备上
/// （不是 MTP 直接写：先写旁边的临时文件、写完才删旧的换上去），这一本写的那一刻要 `new` 的空间、不能先扣掉 `old`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Item {
    pub new: u64,
    pub old: u64,
    pub staged: bool,
}

/// 余量：总容量的 2%，至少 50MB。
pub(crate) fn margin(total: u64) -> u64 {
    (total / 50).max(MARGIN_MIN)
}

/// 放不放得下（纯函数）：要的 = max(Σ新 − Σ旧（不小于 0）, 先写临时文件的那几本里最大的一本) + 余量。
/// 够就 `None`，不够返回差多少字节。没有要写的书不用比。
pub(crate) fn shortfall(free: u64, total: u64, items: &[Item]) -> Option<u64> {
    if items.is_empty() {
        return None;
    }
    let sum = |f: fn(&Item) -> u64| items.iter().fold(0u64, |a, i| a.saturating_add(f(i)));
    let net = sum(|i| i.new).saturating_sub(sum(|i| i.old));
    let peak = items
        .iter()
        .filter(|i| i.staged)
        .map(|i| i.new)
        .max()
        .unwrap_or(0);
    let need = net.max(peak).saturating_add(margin(total));
    (need > free).then(|| need - free)
}

/// `--watch` 时空间不够（或写满）停下的设备，下一轮要不要再试（纯函数）：记下时的剩余空间 `then`、现在的 `now`，
/// 都读得到、而且现在比那时多了至少 50MB（[`MARGIN_MIN`]；掌阅后台清缓存、写日志会让剩余空间来回变几 KB，
/// 不设门槛的话停着一台 `--watch` 就几乎每轮整轮重跑，agy 复审）才再试。
/// 读不到的不记（[`crate::BuildRun::space_held`]）；设备重新接上、书库有变化时调用方清掉记下的。
pub fn space_grew(then: Option<u64>, now: Option<u64>) -> bool {
    matches!((then, now), (Some(t), Some(n)) if t.checked_add(MARGIN_MIN).is_some_and(|x| n >= x))
}

/// 查剩余空间用的路径：`root` 往上第一个存在的目录（新设备上产物目录还没建；SD 卡、内部存储各算各的）。
pub(crate) fn probe_path(root: &Path) -> &Path {
    root.ancestors().find(|p| p.is_dir()).unwrap_or(root)
}

/// 没生成的产物按原件估多大。
pub(crate) fn estimate(source: u64, upscaled_comic: bool) -> u64 {
    if upscaled_comic {
        (source as f64 * COMIC_UPSCALE_FACTOR).ceil() as u64
    } else {
        source
    }
}

/// 设备剩余空间和总容量（字节，statvfs：`f_bavail × f_frsize`、`f_blocks × f_frsize`）。
/// 读不到、或总容量是 0（jmtpfs 可能这样报）返回 `Err(原因)`，调用方只警告、照旧传。
///
/// `BOOKLIB_TEST_FREE_SPACE=<剩余>,<总容量>`（字节）：测试用，不去读真的（集成测试没有小的文件系统可用）。
pub(crate) fn free_space(path: &Path) -> Result<(u64, u64), String> {
    use std::os::unix::ffi::OsStrExt;
    if let Ok(v) = std::env::var("BOOKLIB_TEST_FREE_SPACE") {
        let mut it = v.split(',').map(|x| x.trim().parse::<u64>());
        if let (Some(Ok(free)), Some(Ok(total))) = (it.next(), it.next()) {
            return if total == 0 {
                Err(format!("{}: 报的总容量是 0", path.display()))
            } else {
                Ok((free, total))
            };
        }
    }
    let c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("{}: 路径里有 NUL", path.display()))?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c 是以 NUL 结尾的路径，st 是够大的输出缓冲
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(format!(
            "{}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    // 有的 FUSE 文件系统只填 f_bsize、f_frsize 是 0
    let frsize = if st.f_frsize > 0 {
        st.f_frsize as u64
    } else {
        st.f_bsize as u64
    };
    let total = (st.f_blocks as u64).saturating_mul(frsize);
    if total == 0 {
        return Err(format!("{}: 报的总容量是 0", path.display()));
    }
    Ok(((st.f_bavail as u64).saturating_mul(frsize), total))
}

/// 写文件的错误是不是设备满了（`StorageFull` 或 errno 28 ENOSPC）。
pub(crate) fn is_no_space(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::StorageFull || e.raw_os_error() == Some(28)
}

/// 字节数给人看：MB，一位小数。
pub fn mb(n: u64) -> String {
    format!("{:.1}MB", n as f64 / 1024.0 / 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    const M: u64 = 1024 * 1024;

    #[test]
    fn margin_floor_and_ratio() {
        assert_eq!(margin(1000 * M), 50 * M); // 2% = 20MB < 50MB
        assert_eq!(margin(10_000 * M), 200 * M);
    }

    #[test]
    fn enough_and_short() {
        let items = [Item {
            new: 100 * M,
            old: 0,
            staged: false,
        }];
        // 总 1000MB：要 100 + 50
        assert_eq!(shortfall(150 * M, 1000 * M, &items), None);
        assert_eq!(shortfall(140 * M, 1000 * M, &items), Some(10 * M));
    }

    #[test]
    fn overwrite_subtracts_old() {
        let items = [
            Item {
                new: 100 * M,
                old: 80 * M,
                staged: false,
            },
            Item {
                new: 10 * M,
                old: 0,
                staged: false,
            },
        ];
        // 要 110 − 80 + 50 = 80
        assert_eq!(shortfall(80 * M, 1000 * M, &items), None);
        assert_eq!(shortfall(79 * M, 1000 * M, &items), Some(M));
        // 旧的比新的大：不算负的，只要余量
        let shrink = [Item {
            new: 10 * M,
            old: 100 * M,
            staged: false,
        }];
        assert_eq!(shortfall(50 * M, 1000 * M, &shrink), None);
    }

    #[test]
    fn nothing_to_write_never_short() {
        assert_eq!(shortfall(0, 1000 * M, &[]), None);
        // 有要写的（哪怕估成 0）也要留余量
        let zero = [Item {
            new: 0,
            old: 5,
            staged: false,
        }];
        assert_eq!(shortfall(0, 1000 * M, &zero), Some(50 * M));
    }

    /// 先写临时文件的覆盖：写的那一刻新旧两份都在，不能先扣掉旧的。
    #[test]
    fn staged_overwrite_needs_the_whole_new_file() {
        let staged = [Item {
            new: 100 * M,
            old: 100 * M,
            staged: true,
        }];
        assert_eq!(shortfall(149 * M, 1000 * M, &staged), Some(M));
        assert_eq!(shortfall(150 * M, 1000 * M, &staged), None);
        // MTP 直接写（先删旧的）：只要余量
        let direct = [Item {
            new: 100 * M,
            old: 100 * M,
            staged: false,
        }];
        assert_eq!(shortfall(50 * M, 1000 * M, &direct), None);
    }

    #[test]
    fn sums_do_not_overflow() {
        let big = [Item {
            new: u64::MAX,
            old: 0,
            staged: false,
        }; 3];
        assert_eq!(shortfall(0, u64::MAX, &big), Some(u64::MAX));
    }

    #[test]
    fn retry_only_when_space_was_freed() {
        assert!(space_grew(Some(10), Some(10 + MARGIN_MIN)));
        assert!(
            !space_grew(Some(10), Some(9 + MARGIN_MIN)),
            "来回变几 KB 不算"
        );
        assert!(!space_grew(Some(10), Some(9)));
        assert!(!space_grew(None, Some(u64::MAX)));
        assert!(!space_grew(Some(10), None));
        assert!(!space_grew(Some(u64::MAX), Some(u64::MAX)));
    }

    #[test]
    fn probe_walks_up_to_an_existing_dir() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(probe_path(&d.path().join("a/b/documents")), d.path());
        assert_eq!(probe_path(d.path()), d.path());
    }

    #[test]
    fn comic_estimate() {
        assert_eq!(estimate(100, false), 100);
        assert_eq!(estimate(100, true), 210);
        // 竹光侍：80.9MB → 实际 164.8MB，估的不比实际小
        assert!(estimate(80_900_000, true) >= 164_800_000);
    }

    #[test]
    fn enospc_detected() {
        assert!(is_no_space(&std::io::Error::from_raw_os_error(28)));
        assert!(is_no_space(&std::io::Error::from(
            std::io::ErrorKind::StorageFull
        )));
        assert!(!is_no_space(&std::io::Error::from_raw_os_error(2)));
    }

    #[test]
    fn statvfs_reads_tmp() {
        let d = tempfile::tempdir().unwrap();
        let (free, total) = free_space(d.path()).unwrap();
        assert!(total > 0 && free <= total);
        assert!(free_space(&d.path().join("没有")).is_err());
    }
}

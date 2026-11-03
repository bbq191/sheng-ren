//! 传之前看设备上放不放得下（2026-11-03 定）：一轮要传的产物估个总大小，和设备剩余空间比，不够的设备这一轮不写。
//! 写的途中满了（ENOSPC）见 `deliver::put_file`。

use std::path::Path;

/// 写的途中设备满了的错误开头：一轮生成碰到它就停下这台设备，不记进失败表（下一轮、下次同步重传）。
pub(crate) const NO_SPACE: &str = "空间满";

/// 留的余量下限：50MB。
const MARGIN_MIN: u64 = 50 * 1024 * 1024;

/// 开了 `comic_upscale` 的漫画还没生成时，按原件大小乘这个倍数估：小页放大到框后 JPEG 变大。
/// 依据：《竹光侍》原件 80.9MB、放大后的产物 164.8MB，约 2.04 倍；取 2.1 留一点富余。
pub(crate) const COMIC_UPSCALE_FACTOR: f64 = 2.1;

/// 一本书这一轮要放上设备的估计：`new` 新产物的大小（已生成的按实际大小，没生成的按原件估），
/// `old` 这次覆盖掉的设备上旧文件的大小（没有就是 0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Item {
    pub new: u64,
    pub old: u64,
}

/// 余量：总容量的 2%，至少 50MB。
pub(crate) fn margin(total: u64) -> u64 {
    (total / 50).max(MARGIN_MIN)
}

/// 放不放得下（纯函数）：要的 = Σ新 − Σ旧（不小于 0）+ 余量。够就 `None`，不够返回差多少字节。
pub(crate) fn shortfall(free: u64, total: u64, items: &[Item]) -> Option<u64> {
    let new: u64 = items.iter().map(|i| i.new).sum();
    let old: u64 = items.iter().map(|i| i.old).sum();
    if new == 0 {
        return None; // 没有要写的，不用比
    }
    let need = new.saturating_sub(old).saturating_add(margin(total));
    (need > free).then(|| need - free)
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
pub(crate) fn free_space(path: &Path) -> Result<(u64, u64), String> {
    use std::os::unix::ffi::OsStrExt;
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
    let frsize = st.f_frsize as u64;
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
            },
            Item {
                new: 10 * M,
                old: 0,
            },
        ];
        // 要 110 − 80 + 50 = 80
        assert_eq!(shortfall(80 * M, 1000 * M, &items), None);
        assert_eq!(shortfall(79 * M, 1000 * M, &items), Some(M));
        // 旧的比新的大：不算负的，只要余量
        let shrink = [Item {
            new: 10 * M,
            old: 100 * M,
        }];
        assert_eq!(shortfall(50 * M, 1000 * M, &shrink), None);
    }

    #[test]
    fn nothing_to_write_never_short() {
        assert_eq!(shortfall(0, 1000 * M, &[]), None);
        assert_eq!(shortfall(0, 1000 * M, &[Item { new: 0, old: 5 }]), None);
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

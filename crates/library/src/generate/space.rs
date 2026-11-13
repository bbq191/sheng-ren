//! 设备空间（拆分自 `generate.rs`）：传之前看放不放得下、设备现在剩多少。

use super::flow::direct_write;
use super::types::{SpaceCheck, StateEntry};
use crate::deliver::Target;
use crate::{Library, Meta};
use profile::Profile;
use std::path::Path;

impl Library {
    /// 传之前看这台设备放不放得下 `books` 这一轮要传的产物（2026-11-03 定，见 `space` 模块）。
    /// 每本：指纹没变、设备上已有的不算（`--force` 也不算：重建出来一样大，多半内容一样不再传）；记录里的产物在电脑上
    /// （能直接拿来用）按它的实际大小算；要新生成的按原件大小估（网址入库的没有原件大小，按书库里的母版；开了
    /// `comic_upscale` 的漫画乘 [`crate::space::COMIC_UPSCALE_FACTOR`]）；会覆盖设备上旧版本的（含上次挪位置没删掉的旧位置）
    /// 减掉旧文件大小。待删的孤儿（[`Library::flush_removed`]）不算：调用方不够时先删孤儿再看一次。
    pub fn space_check(&self, device: &Profile, books: &[&Meta], force: bool) -> SpaceCheck {
        let t = self.target(device);
        let Ok(target @ Target::Dir { root, mount, .. }) = t.as_ref().as_ref() else {
            return SpaceCheck::Skip; // 放电脑上的、没接上的不查
        };
        let staged = !direct_write(target);
        if let Err(e) = self.rebase_state_case(device) {
            return SpaceCheck::Unknown(format!("生成记录迁移失败：{e}"));
        }
        let sp = self.state_path(&device.id);
        let state = self.records.states.get(&sp);
        let on_device = |p: &Path| {
            p.is_absolute()
                && (p.starts_with(root) || mount.as_deref().is_some_and(|m| p.starts_with(m)))
        };
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let mut items = Vec::new();
        for m in books {
            let Ok(plan) = self.plan(m, device) else {
                continue; // 算不出的交给 prepare 报错
            };
            let prev = state.books.get(&m.id);
            let fresh = |p: &&StateEntry| {
                !p.fingerprint.is_empty()
                    && (p.fingerprint == plan.fingerprint || p.fingerprint == plan.legacy)
                    && p.path.is_absolute()
                    && p.path.is_file()
            };
            let new = match prev.filter(fresh) {
                Some(p) if on_device(&p.path) => continue, // 已是最新、只在设备上挪位置、--force 重建：不占新空间
                Some(p) if !force => size(&p.path),
                _ => {
                    let comic = device.comic.upscale && self.comic_of(m).unwrap_or(false);
                    let src = if m.source_size > 0 {
                        m.source_size
                    } else {
                        size(&self.content_location(m))
                    };
                    crate::space::estimate(src, comic)
                }
            };
            let old = prev
                .map(|p| p.placements())
                .unwrap_or_default()
                .iter()
                .filter(|x| on_device(&x.path) && x.path.is_file())
                .fold(0u64, |a, x| a.saturating_add(size(&x.path)));
            items.push(crate::space::Item { new, old, staged });
        }
        drop(state);
        if items.is_empty() {
            return SpaceCheck::Enough;
        }
        match crate::space::free_space(crate::space::probe_path(root)) {
            Err(e) => SpaceCheck::Unknown(e),
            Ok((free, total)) => match crate::space::shortfall(free, total, &items) {
                None => SpaceCheck::Enough,
                Some(short) => SpaceCheck::Short {
                    count: items.len(),
                    need: free.saturating_add(short),
                    free,
                    short,
                },
            },
        }
    }

    /// 设备现在的剩余空间（字节）；放电脑上的、没接上的、读不到的 `None`（`sync --watch` 看空间腾出来没有）。
    pub fn device_free(&self, device: &Profile) -> Option<u64> {
        match self.target(device).as_ref().as_ref() {
            Ok(Target::Dir { root, .. }) => {
                crate::space::free_space(crate::space::probe_path(root))
                    .ok()
                    .map(|(free, _)| free)
            }
            _ => None,
        }
    }
}

//! 生成记录的读写与删除（拆分自 `generate.rs`）：`output-state/<模式 id>.json` 的读改写、迁移、删除。

use super::fsops::{carry_sidecar, case_alias};
use super::types::{OutputStatus, State, StateEntry};
use crate::deliver::Target;
use crate::{Library, Meta};
use profile::Profile;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

impl Library {
    fn state_dir(&self) -> PathBuf {
        self.root.join("output-state")
    }

    pub(super) fn state_path(&self, device_id: &str) -> PathBuf {
        self.state_dir().join(format!("{device_id}.json"))
    }

    /// 设备接上了，把这台的生成记录原样挪到新前缀（`~/Devices/<链接名>`）和新的产物根目录上（汉王 `documents` → `Documents`）：
    /// 只改记录，不碰设备上的文件，也不重传、不删。挂在 prepare / space_check / flush_removed / retract 开头
    /// （生成流程；`list` 只读不写）。
    pub(super) fn rebase_state_case(&self, device: &Profile) -> Result<(), String> {
        let Ok(Target::Dir { root, .. }) = &*self.target(device) else {
            return Ok(());
        };
        // 每台设备每轮只迁移一次（`Devices::refresh` 时清掉）。否则一条换不成的记录会让后面每本书都克隆整份记录、
        // 再对它 `is_file()`（MTP 上每次 stat 都是一次往返）。
        if !self.devices.begin_rebase(&device.id) {
            return Ok(());
        }
        let mappings = self.devices.prefix_migrations(device);
        let old_prefix = |p: &Path| mappings.iter().any(|(o, _)| p.starts_with(o));
        let sp = self.state_path(&device.id);
        let state = self.records.states.get(&sp);
        let maybe = state.books.values().any(|e| {
            old_prefix(&e.path)
                || old_prefix(&e.root)
                || case_alias(&e.root, root)
                || e.old.iter().any(|o| {
                    old_prefix(&o.path) || old_prefix(&o.root) || case_alias(&o.root, root)
                })
        }) || state
            .orphans
            .iter()
            .any(|o| old_prefix(&o.path) || old_prefix(&o.root) || case_alias(&o.root, root));
        if !maybe {
            return Ok(());
        }
        self.edit_state(&sp, |state| {
            let mut changed = state.migrate_prefixes(&mappings);
            changed |= state.rebase_case_root(root);
            changed.then_some(())
        })?;
        Ok(())
    }

    /// `output-state/` 下各模式的生成记录（包括 profile 已经删掉的模式）：(模式 id, 记录文件)。
    pub(crate) fn state_files(&self) -> Vec<(String, PathBuf)> {
        let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(self.state_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let id = p.file_stem()?.to_str()?.to_string();
                (!id.starts_with('.') && p.extension().is_some_and(|x| x == "json"))
                    .then_some((id, p))
            })
            .collect();
        v.sort();
        v
    }

    /// 生成记录里产物所在的所有目录（拿到锁时清理残留临时文件用）：电脑上的和设备上的都列出，不看设备挂没挂上（没挂上的目录多半不在，清理时读不到就跳过）。
    pub(crate) fn output_dirs(&self) -> BTreeSet<PathBuf> {
        let mut dirs = BTreeSet::new();
        for (_, sp) in self.state_files() {
            let state = self.records.states.get(&sp);
            let files = state
                .books
                .values()
                .flat_map(StateEntry::placements)
                .chain(state.orphans.iter().cloned());
            dirs.extend(
                files
                    .filter(|p| p.path.is_absolute())
                    .filter_map(|p| p.path.parent().map(Path::to_path_buf)),
            );
        }
        dirs
    }
    /// 删掉一本书在所有模式下的产物（生成记录里记着的，包括还没删掉的旧位置），并从记录里去掉。
    /// 设备没接上、删不掉的记进 `orphans`，下次接上时删（[`Library::flush_removed`]）。
    pub(crate) fn remove_outputs(&self, id: &str) -> Result<(), String> {
        for (_, sp) in self.state_files() {
            self.edit_state(&sp, |state| {
                let entry = state.books.remove(id)?;
                state.delete_into_orphans(entry.placements(), &self.devices);
                Some(())
            })?;
        }
        Ok(())
    }

    /// 原件换了内容（旧条目 `from` 要删、新条目是 `to`）：各模式下旧条目的产物记录交给新条目（指纹留空＝要重新生成），
    /// 新版本生成出来照原地覆盖（MTP 同一个文件名）放上去——以前是先把旧产物删掉、再当新书传，掌阅丢进度、
    /// 新版本生成失败时设备上这本书就没了（2026-10-09 定）。
    /// 新条目在这个模式下已经有自己的产物（同一内容早就在库里）的不交，旧的照常删（[`Library::remove_outputs`]）。
    pub(crate) fn hand_over_outputs(&self, from: &str, to: &str) -> Result<(), String> {
        for (_, sp) in self.state_files() {
            self.edit_state(&sp, |state| {
                // 新条目在这个模式下已经有自己的产物：不交，旧的照常删
                if state.books.contains_key(to) {
                    return None;
                }
                let mut entry = state.books.remove(from)?;
                entry.fingerprint.clear();
                state.books.insert(to.to_string(), entry);
                Some(())
            })?;
        }
        Ok(())
    }

    /// 这本书不再给这台设备（从它的跟踪范围里拿掉了）：删掉它在这台上的产物，从这台的生成记录里去掉；
    /// 删不掉的记进 `orphans`，下次接上再删。这台上本来就没有返回 `false`。书库里的书不动。
    pub fn retract(&self, id: &str, device: &Profile) -> Result<bool, String> {
        self.rebase_state_case(device)?;
        let sp = self.state_path(&device.id);
        let removed = self.edit_state(&sp, |state| {
            let entry = state.books.remove(id)?;
            state.delete_into_orphans(entry.placements(), &self.devices);
            Some(())
        })?;
        Ok(removed.is_some())
    }

    /// 这台设备的生成记录里有这本书。
    pub fn has_output(&self, id: &str, device: &str) -> bool {
        self.records
            .states
            .get(&self.state_path(device))
            .books
            .contains_key(id)
    }

    /// 这台设备上待删的已删书产物个数（[`Library::flush_removed`] 删的那些）。
    pub fn orphan_count(&self, device: &Profile) -> usize {
        self.records
            .states
            .get(&self.state_path(&device.id))
            .orphans
            .len()
    }

    /// 删了的书留在设备上的产物（当时设备没接上）：设备接上了就删。返回删掉几个。
    pub fn flush_removed(&self, device: &Profile) -> Result<usize, String> {
        self.rebase_state_case(device)?;
        let sp = self.state_path(&device.id);
        if self.records.states.get(&sp).orphans.is_empty() {
            return Ok(0);
        }
        if self.target(device).is_err() {
            return Ok(0);
        }
        let n = self.edit_state(&sp, |state| {
            let list = std::mem::take(&mut state.orphans);
            let before = list.len();
            state.delete_into_orphans(list, &self.devices);
            let n = before - state.orphans.len();
            (n > 0).then_some(n)
        })?;
        Ok(n.unwrap_or(0))
    }
    /// 这本书在各模式下的产物，以及是否最新。
    pub fn outputs(&self, meta: &Meta) -> Vec<OutputStatus> {
        let mut out = Vec::new();
        for (dev_id, sp) in self.state_files() {
            let Some(entry) = self.records.states.get(&sp).books.get(&meta.id).cloned() else {
                continue;
            };
            // 可拔下的设备上的产物一律只看路径前缀，不 stat / read_dir / `mounted()`：设备挂载卡死时 `list` 也要马上返回。
            // 不核对文件在不在，也就分不出设备接没接上，所以这类记录只比指纹，显示时注明「未核对设备」。
            let on_device = self.devices.on_removable(&entry.path);
            // 相对路径是 2026-10-31 以前 Move 上的旧记录：不拿当前目录去查。
            let fresh = if !entry.path.is_absolute() {
                None
            } else if on_device {
                // 模式已停用时注册表里没有它，但生成记录还在：照内置参数算指纹，仍能看出记录是不是旧的。
                self.registry
                    .get(&dev_id)
                    .or_else(|| profile::get(&dev_id))
                    .and_then(|p| self.plan(meta, p).ok())
                    .map(|plan| {
                        plan.fingerprint == entry.fingerprint || plan.legacy == entry.fingerprint
                    })
            } else if self.output_unplugged(&entry.path, &entry.root)
                || !self.output_is_file(&entry.path)
            {
                // 电脑上的产物照旧核对文件在不在
                None
            } else {
                self.registry
                    .get(&dev_id)
                    .and_then(|p| self.plan(meta, p).ok())
                    .map(|plan| {
                        plan.fingerprint == entry.fingerprint || plan.legacy == entry.fingerprint
                    })
            };
            out.push(OutputStatus {
                device: dev_id,
                path: entry.path.clone(),
                fresh,
                device_unchecked: on_device,
                audit: entry.audit.clone(),
            });
        }
        out
    }

    /// 测试钩子 `BOOKLIB_TEST_DEVICE_STAT_PANIC=1` 时，`list` 真要对可拔下的设备上的路径做 stat 就 panic：
    /// 证明 `list` / [`Library::outputs`] 不去读设备上的产物（集成测试用；只包着这里的调用，不影响 `sync`）。
    fn output_stat_guard(&self, path: &Path) {
        if self.devices.on_removable(path)
            && std::env::var_os("BOOKLIB_TEST_DEVICE_STAT_PANIC").is_some_and(|v| v != "0")
        {
            panic!(
                "booklib list 对设备上的产物做了 stat：{}（BOOKLIB_TEST_DEVICE_STAT_PANIC=1）",
                path.display()
            );
        }
    }

    fn output_unplugged(&self, path: &Path, root: &Path) -> bool {
        self.output_stat_guard(path);
        self.devices.unplugged(path, root)
    }

    fn output_is_file(&self, path: &Path) -> bool {
        self.output_stat_guard(path);
        path.is_file()
    }
    /// 读这个模式的生成记录、克隆一份交给 `f` 改：`f` 返回 `Some` 才原子写回（`None`＝什么都不用改、不写），并原样返回它的结果。
    fn edit_state<R>(
        &self,
        sp: &Path,
        f: impl FnOnce(&mut State) -> Option<R>,
    ) -> Result<Option<R>, String> {
        let mut state = (*self.records.states.get(sp)).clone();
        match f(&mut state) {
            Some(v) => {
                self.put_state(sp, state)?;
                Ok(Some(v))
            }
            None => Ok(None),
        }
    }

    /// 改写这个模式的生成记录（原子写）。
    fn put_state(&self, sp: &Path, state: State) -> Result<(), String> {
        std::fs::create_dir_all(self.state_dir())
            .map_err(|e| format!("{}: {e}", self.state_dir().display()))?;
        self.records.states.put(sp, state)
    }

    /// 写一条记录（其余不动）。
    pub(super) fn put_entry(&self, sp: &Path, id: &str, entry: StateEntry) -> Result<(), String> {
        let mut state = (*self.records.states.get(sp)).clone();
        state.books.insert(id.to_string(), entry);
        self.put_state(sp, state)
    }

    /// 产物已经到位：删掉记着的旧位置（删不掉的留着下次再删），写上完成的记录。记录和原来一样（旧位置还是删不掉，
    /// `sync --watch` 每轮都会走到这里）时不写。
    pub(super) fn finish(
        &self,
        sp: &Path,
        id: &str,
        mut entry: StateEntry,
        staged: bool,
    ) -> Result<(), String> {
        let mut state = (*self.records.states.get(sp)).clone();
        let prev = state.books.remove(id);
        // `.sdr` 是 Kindle 的（MTP）；USB 大容量存储（Kobo）的进度在阅读器自己的数据库里，产物旁边的东西不动
        // 新旧位置都得真挂着（掉线后挂载点是 tmpfs 上的空目录：拷进去再删旧的就丢了进度）
        if !staged && !self.devices.unplugged(&entry.path, &entry.root) {
            for o in entry
                .old
                .iter()
                .filter(|o| !state.claimed(o) && !self.devices.unplugged(&o.path, &o.root))
            {
                carry_sidecar(&o.path, &entry.path);
            }
        }
        entry.old = state.delete_placements(
            std::mem::take(&mut entry.old),
            Some(&entry.path),
            &self.devices,
        );
        if prev.as_ref() == Some(&entry) {
            return Ok(());
        }
        state.books.insert(id.to_string(), entry);
        self.put_state(sp, state)
    }
}

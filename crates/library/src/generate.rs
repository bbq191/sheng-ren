//! 按阅读模式（设备 profile）生成产物并送到设备上：生成计划与指纹、产物放在哪、跳过没变的、生成记录。
//!
//! 产物位置（每个模式一份，`<名>` 见 [`State::file_name_for`]；2026-10-07 起直接送设备，电脑上不留，见 [`crate::deliver`]）：
//! - Kindle、掌阅（profile `[deliver] kind = "mtp"`）：设备存储根目录的 `documents/<原件所在目录相对跟踪目录的路径>/<名>.<扩展名>`；
//!   `add` 进来的单个文件（不在跟踪目录里）、网址书放 `documents/` 顶层。先在书库的临时目录里生成，再拷上去
//!   （设备上已有逐字节相同的就不拷：Kindle 覆盖成不同字节会清掉阅读进度）。
//! - Kobo（`kind = "usbms"`）：同上，放在 USB 存储根目录的 `books/` 下（2026-11-01 加，未上真机）。
//! - 没写 `[deliver]` 的自定义模式：电脑上，跟踪目录 `D` 旁的 `D/../<模式 id>/<子目录>/`；add 进来的、网址书在 `<书库>/output/<模式 id>/`。
//!
//! 设备没接上的模式不生成（`sync` 跳过，下次接上再传）。
//!
//! 产物格式按模式：EPUB 直接是优化结果；KFX（Kindle）是同一份优化结果再转一次（`kfx` crate）。
//! profile 可以给漫画另配格式（`comic_format`，见 [`Library::output_format`]）；内置模式都不用（kindle 文字书、漫画都出 KFX）。
//!
//! 生成记录 `<书库>/output-state/<模式 id>.json`：书 id → 产物绝对路径、产物根目录、指纹
//! （2026-10-31 以前 Move 上的记的是 uuid 和相对的 `文件夹/文件名`：旧记录里这种相对路径删书时一概不动）。
//! 产物位置变了（原件移动、改名换了目录，书名改了）时删掉旧位置的——**只删记录里记着的**，不认识的文件一概不动；删完顺带删掉
//! 产物根目录以内变空的目录。内容没变、只是位置变了的，把已有产物挪过去，不重新生成（改成送设备以前留在电脑上的产物也这样挪上设备）。
//! 删书时设备没接上、删不掉的记在 `orphans`，接上后删。
//!
//! 换位置时先把记录改成新位置（指纹留空＝没完成，旧位置记进 `old` 待删），产物放好再补上指纹、删旧的：
//! 中途被打断的话，下次生成还认得新位置上的文件是这本书的（不会因为"有个不认识的同名文件"而改名），旧文件也还会删。

use crate::deliver::{self, Target};
use crate::error::{Error, Kind};
use crate::fsutil::TMP_PREFIX;
use crate::session::{Devices, PreparedInput};
use crate::transfer::{Transfer, Work};
use crate::{Library, Meta};
use profile::{Format, Profile};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 生成流程本身（本 crate 的步骤、参数，以及生成时当场做的格式转换）的版本：改了会影响产物的地方要加一，
/// 旧产物随之判为过期。优化器、格式转换各有自己的版本号，也都进指纹。产物放在哪不影响产物内容，不进指纹。
pub(crate) const PIPELINE_VERSION: &str = "5";

#[derive(Debug)]
pub enum Built {
    Written {
        path: PathBuf,
        warnings: Vec<String>,
    },
    UpToDate(PathBuf),
    /// 内容没变、只是位置变了（原件移动改名、书名改了）：已有产物挪了过去，没有重新生成。
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// 重新生成了（规则版本变了、`--force`），但和设备上那份逐字节相同：没有再传（Kindle 进度不受影响）。
    Same {
        path: PathBuf,
        warnings: Vec<String>,
    },
    /// 没变化、记录里的位置空了（以前放电脑上、用户自己挪上了设备），设备上对应位置的那份认领下来，没有重新生成。
    Adopted(PathBuf),
}

/// [`Library::prepare`] 的结果：已经有结论了，或者生成好、要交给传输线程放上设备的一件。
pub enum Step {
    Done(Built),
    Transfer(Box<Transfer>),
}

/// 要放上设备的一份产物：书库临时目录里新生成的（`tmp` 是那个目录，传完删），或者已有的（挪位置）。
struct Made {
    tmp: Option<PathBuf>,
    src: PathBuf,
    sha: String,
    warnings: Vec<String>,
    /// 体检清单（[`bookconv::audit`]，一行一种写法），记进生成记录。
    audit: Vec<String>,
}

/// [`Library::produce`] 的结果：(警告, 产物的 SHA-256（算好了的话）, 体检清单)。
type Produced = (Vec<String>, Option<String>, Vec<String>);

/// 一本书在某模式下的产物状态（`list` 用）。
pub struct OutputStatus {
    pub device: String,
    pub path: PathBuf,
    /// `Some(true)` 最新；`Some(false)` 过期（原件或处理规则变了，或上次生成没完成）；`None` 判断不了（相对路径旧记录、电脑上的文件不在、指纹算不出）。
    pub fresh: Option<bool>,
    /// 在可拔下的设备上：只比了指纹，没核对设备上的文件还在不在（`list` 在状态后注明「未核对设备」）。
    pub device_unchecked: bool,
    /// 生成时的体检清单（[`bookconv::audit::Finding::line`]）：这台上和 Kindle 不一样又没处理的（`✗`）、没验证过的（`?`）写法。
    pub audit: Vec<String>,
}

/// 生成计划：指纹。
struct Plan {
    fingerprint: String,
    /// 同一套规则按 2026-10-08 以前的写法（文字书、漫画不分）算出的指纹：记录里存的是它的，说明规则没变、只是指纹写法改了，
    /// 记录改成新写法、不重新生成（[`Library::prepare`]）。
    legacy: String,
    format: Format,
}

impl Library {
    fn plan(&self, meta: &Meta, device: &Profile) -> Result<Plan, String> {
        if !meta.supported() {
            return Err(crate::unsupported(meta.content_format()));
        }
        if meta.content_sha().is_empty() {
            return Err("条目缺内容哈希（早期版本入库），先运行 booklib dedupe 迁移".into());
        }
        let format = self.output_format(meta, device)?;
        // 优化器用的阅读范围（`OptimizeOpts::for_profile` 取 `output_readable`，漫画另配格式时也是它），不是这本书产物格式的；
        // 内置模式两者相同
        let area = device.output_readable();
        // KFX 再带上写出器的版本（写出器改了也要重建）和写出器求 `@media` 用的屏幕、KFX 阅读范围（`kfx::css::MediaEnv::for_profile`；
        // 书库 profiles/ 里的自定义模式改了屏幕也要重建，2026-10-09 审计补上）
        let format_seg = match format {
            Format::Epub => format.name().to_string(),
            // KEPUB：写出器版本（`bookconv::kepub::VERSION`）；不按屏幕求值，屏幕变了由优化器那段（阅读范围）管
            Format::Kepub => format!("{}{}", format.name(), bookconv::kepub::VERSION),
            Format::Kfx => {
                let r = device.readable(Format::Kfx);
                format!(
                    "{}{}@{}x{}r{}x{}",
                    format.name(),
                    kfx::write::WRITER_VERSION,
                    device.screen.width,
                    device.screen.height,
                    r.width,
                    r.height
                )
            }
        };
        let cover = meta
            .cover
            .as_ref()
            .map_or("-", |c| c.sha256.get(..12).unwrap_or(&c.sha256));
        // 先算是不是漫画：早期条目 `Meta::comic` 可能还没存，按书定方向要和指纹用同一个结果
        let comic = self.comic_of(meta)?;
        let info = self.injected_info_sig(meta).unwrap_or_else(|| "-".into());
        // 补元数据那一步（`metadata::inject`）改了会影响产物时 `bookconv::opfmeta::VERSION` 加一：只让补过东西的书过期（`i5`），
        // 没补过东西的书指纹不变
        let info = if cover != "-" || info != "-" {
            format!("{info}i{}", bookconv::opfmeta::VERSION)
        } else {
            info
        };
        // 按元数据强制成 rtl 的书（Bangumi 日本漫画，`metadata::comic_direction`）在 info 段后接 `r`：
        // 只有这些书指纹变，别的书一个字不变；方向不算 profile 字段，不放进设备段（不然全部漫画都重建）
        let info = if crate::metadata::comic_direction(meta, device, comic).is_some() {
            format!("{info}r")
        } else {
            info
        };
        // 最终竖排的书在 info 段后接 `V`（2026-11-11）；全角 ASCII（`vertical_ascii = fullwidth`）再接 `f`。
        // 不竖排的书不出新字母，指纹除版本号外不变。
        let info = if self.vertical_for(meta, device)? {
            let mut s = format!("{info}V");
            if device.vertical_ascii == profile::VerticalAscii::Fullwidth {
                s.push('f');
            }
            s
        } else {
            info
        };
        // 要当场转换的来源（CBZ）再带上格式转换的版本；写在流程版本后面，EPUB 来源的指纹保持原样（不白重建）
        let pipeline = if meta.content_format() == "epub" {
            PIPELINE_VERSION.to_string()
        } else {
            format!("{PIPELINE_VERSION}c{}", bookconv::convert::CONVERT_VERSION)
        };
        let crate::fingerprint::DeviceSig {
            version,
            area_seg,
            notes,
        } = crate::fingerprint::device_sig(device, area, comic);
        let fingerprint = format!(
            "{}|{cover}|{info}|{pipeline}|{version}|{notes}|{}|{area_seg}|{}|{}",
            meta.content_sha(),
            device.id,
            if device.color { "color" } else { "gray" },
            format_seg,
        );
        // 以前的写法：一个版本号、profile 的字段全带上。版本号用这一路现在的：这一路的版本变过，就对不上，照常重建
        let legacy = {
            let seg = crate::fingerprint::legacy_device_seg(device, area);
            let v = if comic {
                bookconv::optimize::COMIC_VERSION
            } else {
                bookconv::optimize::OPTIMIZE_VERSION
            };
            let gray = if device.color { "color" } else { "gray" };
            format!(
                "{}|{cover}|{info}|{pipeline}|{v}|{notes}|{}|{}x{}+{}{seg}|{gray}|{format_seg}",
                meta.content_sha(),
                device.id,
                area.width,
                area.height,
                device.comic.margin
            )
        };
        Ok(Plan {
            fingerprint,
            legacy,
            format,
        })
    }

    /// 这本书在这个模式下的产物格式：profile 给漫画另配了格式（`comic_format`）时要先判断是不是漫画。
    pub(crate) fn output_format(&self, meta: &Meta, device: &Profile) -> Result<Format, String> {
        if device.format_for(true) == device.format_for(false) {
            return Ok(device.format());
        }
        Ok(device.format_for(self.comic_of(meta)?))
    }

    /// 这本书是不是漫画（同优化器的判定）：入库时存下的（`Meta::comic`）；早期条目当场判、按内容哈希缓存在这次运行里，
    /// 持锁时顺带存进 meta.json（只读的命令不写书库）。
    pub(crate) fn comic_of(&self, meta: &Meta) -> Result<bool, String> {
        if let Some(c) = meta.comic {
            return Ok(c);
        }
        let sha = meta.content_sha().to_string();
        if let Some(c) = self.facts.comic(&sha) {
            return Ok(c);
        }
        let c = self.is_comic(meta)?;
        self.facts.note_comic(&sha, c);
        if self.records.locked.get() {
            if let Some(mut m) = self
                .read_meta(&meta.id)
                .filter(|m| m.content_sha() == meta.content_sha())
            {
                m.comic = Some(c);
                self.save_meta(&m)?;
            }
        }
        Ok(c)
    }

    /// 生成时真正会补进书里的东西（[`crate::metadata::plan`]：简介、标签、带国籍的作者、译者、出版社、日期、ISBN）的指纹；
    /// 什么都不补时 `None`。书里已有什么见 [`Library::own_dc_of`]；不知道（原件暂时不在）、CBZ（当场转换）时按书里只有作者算。
    fn injected_info_sig(&self, meta: &Meta) -> Option<String> {
        crate::metadata::plan_sig(&crate::metadata::plan(meta, self.own_dc_of(meta).as_ref()))
    }

    /// 书里自己的元数据：入库时存进 meta.json 的（`Meta::own_dc`，原件暂时不在也知道）。早期条目没存或是旧版记录：EPUB 当场读 OPF
    /// （按内容哈希缓存），持锁时顺带存进 meta.json（只读的命令不写书库）；读不出来（原件不在了）、不是 EPUB 返回 `None`。
    pub(crate) fn own_dc_of(&self, meta: &Meta) -> Option<crate::OwnDc> {
        if let Some(o) = meta
            .own_dc
            .as_ref()
            .filter(|o| o.v >= crate::OwnDc::VERSION)
        {
            return Some(o.clone());
        }
        if meta.content_format() != "epub" {
            return None;
        }
        let sha = meta.content_sha();
        if let Some(o) = self.facts.own_dc(sha) {
            return Some(o);
        }
        // 读不出来（原件暂时不在）：旧版记录里的简介、标签照用，别的按不知道
        let Ok(own) = crate::metadata::read_own(&self.content_location(meta)) else {
            return meta.own_dc.clone();
        };
        // 原件动过（可能不是这个内容了）的不记：生成前会核对，核对过再算
        if self.original_state(meta) != crate::OriginalState::Touched {
            self.facts.note_own_dc(sha, own.clone());
            if self.records.locked.get() {
                if let Some(mut m) = self.read_meta(&meta.id).filter(|m| {
                    m.content_sha() == sha
                        && m.own_dc
                            .as_ref()
                            .is_none_or(|o| o.v < crate::OwnDc::VERSION)
                }) {
                    m.own_dc = Some(own.clone());
                    let _ = self.save_meta(&m); // 写不回也不影响这次，下次再存
                }
            }
        }
        Some(own)
    }

    /// 这本书给该模式生成的话，产物的指纹（`sync --watch` 用它判断上次失败以后有没有变化）。
    pub fn fingerprint(&self, meta: &Meta, device: &Profile) -> Result<String, String> {
        self.plan(meta, device).map(|p| p.fingerprint)
    }

    fn state_dir(&self) -> PathBuf {
        self.root.join("output-state")
    }

    fn state_path(&self, device_id: &str) -> PathBuf {
        self.state_dir().join(format!("{device_id}.json"))
    }

    /// 设备接上了，把这台的生成记录原样挪到新前缀（`~/Devices/<链接名>`）和新的产物根目录上（汉王 `documents` → `Documents`）：
    /// 只改记录，不碰设备上的文件，也不重传、不删。挂在 prepare / space_check / flush_removed / retract 开头
    /// （生成流程；`list` 只读不写）。
    fn rebase_state_case(&self, device: &Profile) -> Result<(), String> {
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
        let mut state = (*state).clone();
        let mut changed = state.migrate_prefixes(&mappings);
        changed |= state.rebase_case_root(root);
        if changed {
            self.records.states.put(&sp, state)?;
        }
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

    /// 原件被跟踪着（有给这台设备的条目覆盖它）的话：(子目录以哪个目录为准（书根目录）, 设备上的子目录)，见
    /// [`crate::sources::Sources::placement`]。
    pub(crate) fn tracked_rel(&self, meta: &Meta, device: &str) -> Option<(PathBuf, PathBuf)> {
        if meta.source() == crate::Source::Stored || meta.source_path.is_empty() {
            return None;
        }
        self.records
            .sources
            .get(&self.sources_path())
            .placement(Path::new(&meta.source_path), device)
    }

    /// 这本书在该模式下的产物根目录和产物所在目录（绝对路径），见模块注释。`target` 是 MTP 设备时根目录是设备上的
    /// `<存储>/<dir>`，否则是电脑上跟踪目录旁的 `<模式 id>/`（add 进来的书、网址书在书库 `output/<模式 id>/`）。
    fn output_dir(
        &self,
        meta: &Meta,
        device: &Profile,
        target: &Target,
    ) -> Result<(PathBuf, PathBuf), String> {
        let device_root = match target {
            Target::Dir { root, .. } => Some(root.clone()),
            _ => None,
        };
        let lib_root = device_root.clone().unwrap_or_else(|| {
            std::path::absolute(&self.root)
                .unwrap_or_else(|_| self.root.clone())
                .join("output")
                .join(&device.id)
        });
        let Some((d, rel)) = self.tracked_rel(meta, &device.id) else {
            return Ok((lib_root.clone(), lib_root));
        };
        let on_device = device_root.is_some();
        let root = match device_root {
            Some(r) => r,
            // 电脑上：书根目录旁的 `<模式 id>/`
            None => {
                let Some(parent) = d.parent() else {
                    return Ok((lib_root.clone(), lib_root));
                };
                let root = parent.join(&device.id);
                let sources = self.records.sources.get(&self.sources_path());
                if let Some(t) = sources.dirs().find(|t| crate::sources::overlaps(&root, t)) {
                    return Err(output_overlap(&root, t));
                }
                root
            }
        };
        // MTP 设备（Kindle 是 FAT 系、掌阅是安卓存储）上目录名不能有 `:?*"<>|`、不能以空格或点结尾：这样的一段换成安全的名字，
        // 不然每次 sync 都在建目录时失败（2026-10-09 审计）。别的目录名原样，已经在设备上的位置不变
        let dir = if on_device {
            root.join(device_safe_rel(&rel))
        } else {
            root.join(rel)
        };
        Ok((root, dir))
    }

    /// 删掉一本书在所有模式下的产物（生成记录里记着的，包括还没删掉的旧位置），并从记录里去掉。
    /// 设备没接上、删不掉的记进 `orphans`，下次接上时删（[`Library::flush_removed`]）。
    pub(crate) fn remove_outputs(&self, id: &str) -> Result<(), String> {
        for (_, sp) in self.state_files() {
            let state = self.records.states.get(&sp);
            let Some(entry) = state.books.get(id).cloned() else {
                continue;
            };
            let mut state = (*state).clone();
            state.books.remove(id);
            state.delete_into_orphans(entry.placements(), &self.devices);
            self.records.states.put(&sp, state)?;
        }
        Ok(())
    }

    /// 原件换了内容（旧条目 `from` 要删、新条目是 `to`）：各模式下旧条目的产物记录交给新条目（指纹留空＝要重新生成），
    /// 新版本生成出来照原地覆盖（MTP 同一个文件名）放上去——以前是先把旧产物删掉、再当新书传，掌阅丢进度、
    /// 新版本生成失败时设备上这本书就没了（2026-10-09 定）。
    /// 新条目在这个模式下已经有自己的产物（同一内容早就在库里）的不交，旧的照常删（[`Library::remove_outputs`]）。
    pub(crate) fn hand_over_outputs(&self, from: &str, to: &str) -> Result<(), String> {
        for (_, sp) in self.state_files() {
            let state = self.records.states.get(&sp);
            if state.books.contains_key(to) || !state.books.contains_key(from) {
                continue;
            }
            let mut state = (*state).clone();
            if let Some(mut entry) = state.books.remove(from) {
                entry.fingerprint.clear();
                state.books.insert(to.to_string(), entry);
            }
            self.records.states.put(&sp, state)?;
        }
        Ok(())
    }

    /// 这本书不再给这台设备（从它的跟踪范围里拿掉了）：删掉它在这台上的产物，从这台的生成记录里去掉；
    /// 删不掉的记进 `orphans`，下次接上再删。这台上本来就没有返回 `false`。书库里的书不动。
    pub fn retract(&self, id: &str, device: &Profile) -> Result<bool, String> {
        self.rebase_state_case(device)?;
        let sp = self.state_path(&device.id);
        let state = self.records.states.get(&sp);
        let Some(entry) = state.books.get(id).cloned() else {
            return Ok(false);
        };
        let mut state = (*state).clone();
        state.books.remove(id);
        state.delete_into_orphans(entry.placements(), &self.devices);
        self.records.states.put(&sp, state)?;
        Ok(true)
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
        let state = self.records.states.get(&sp);
        if state.orphans.is_empty() {
            return Ok(0);
        }
        let t = self.target(device);
        if t.is_err() {
            return Ok(0);
        }
        let mut state = (*state).clone();
        let list = std::mem::take(&mut state.orphans);
        let before = list.len();
        state.delete_into_orphans(list, &self.devices);
        let n = before - state.orphans.len();
        if n > 0 {
            self.records.states.put(&sp, state)?;
        }
        Ok(n)
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

    /// 为一个阅读模式生成一本书的产物并送到设备上，做完才返回（没变化就跳过，`force` 强制重建）。放在哪见模块注释。
    /// 设备没接上返回错误（`sync` 先用 [`Library::device_status`] 看过，没接上的模式整个跳过）。
    /// `sync` 不用它：先比较、生成（[`Library::prepare`]），传设备交给传输线程（[`crate::Pipeline`]），传完再记（[`Library::complete`]）。
    pub fn build(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Built, String> {
        self.build_typed(meta, device, force)
            .map_err(|e| e.to_string())
    }

    fn build_typed(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Built, Error> {
        match self.prepare_typed(meta, device, force)? {
            Step::Done(b) => Ok(b),
            Step::Transfer(t) => {
                let r = t.run();
                self.complete_typed(*t, r)
            }
        }
    }

    /// 先比较、再生成：没变化的、设备上已有一样的直接给出结果（[`Step::Done`]）；要传的生成好、交回一件 [`Transfer`]
    /// （放上设备是别的线程的事，传完调 [`Library::complete`] 记下来）。
    pub fn prepare(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Step, String> {
        self.prepare_typed(meta, device, force)
            .map_err(|e| e.to_string())
    }

    /// [`Library::prepare`] 的内部版：错误带着种类，`build_run` 按种类决定掉线还是失败。
    pub(crate) fn prepare_typed(
        &self,
        meta: &Meta,
        device: &Profile,
        force: bool,
    ) -> Result<Step, Error> {
        // 原件的大小或修改时间变了：先核对内容（改过了就停下来），不能因为指纹没变就说"已是最新"。
        // 原件不在了的不在这里报：产物还是那本书的，照旧算最新（`sync`/`list` 会报原件不在）
        if self.original_state(meta) == crate::OriginalState::Touched {
            self.verified_original(meta)?;
        }
        let plan = self.plan(meta, device)?;
        self.rebase_state_case(device)?;
        // 指纹写法改了（2026-10-08 起文字书、漫画分开算）、规则没变的：记录改成新写法，不重新生成
        let sp = self.state_path(&device.id);
        let prev = self.records.states.get(&sp).books.get(&meta.id).cloned();
        if let Some(mut p) = prev.filter(|p| {
            !p.fingerprint.is_empty()
                && p.fingerprint != plan.fingerprint
                && p.fingerprint == plan.legacy
        }) {
            p.fingerprint = plan.fingerprint.clone();
            self.put_entry(&sp, &meta.id, p)?;
        }
        let t = self.target(device);
        let target = t
            .as_ref()
            .as_ref()
            .map_err(|e| format!("{} {e}", device.id))?;
        self.prepare_file(meta, device, target, plan, force)
    }

    /// 生成产物，放在书库临时目录 `.tmp-<id>-<模式>/` 里。失败时临时目录删掉。
    fn product(&self, meta: &Meta, device: &Profile, format: Format) -> Result<Made, String> {
        let tmp = self
            .root
            .join(format!("{TMP_PREFIX}{}-{}", meta.id, device.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let src = tmp.join(format!("out.{}", format.ext()));
        // KFX 在内存里就算好了哈希；EPUB 是优化器流式写的，写完再读一遍（刚写的，多半还在页缓存里）
        let made = self
            .produce(meta, device, format, &src, &tmp)
            .and_then(|(w, sha, a)| {
                Ok((
                    sha.map_or_else(|| crate::fsutil::sha256_file(&src), Ok)?,
                    w,
                    a,
                ))
            });
        match made {
            Ok((sha, warnings, audit)) => Ok(Made {
                tmp: Some(tmp),
                src,
                sha,
                warnings,
                audit,
            }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                Err(e)
            }
        }
    }

    /// 要放上设备的那份：`reuse` 是内容没变、只是位置变了（或以前放在电脑上）的已有产物，直接用它、不重新生成
    /// （记录里有哈希就用，没有就算）；没有就新生成（[`Library::product`]）。
    fn made(
        &self,
        meta: &Meta,
        device: &Profile,
        format: Format,
        reuse: Option<&StateEntry>,
    ) -> Result<Made, String> {
        match reuse {
            Some(p) => Ok(Made {
                tmp: None,
                src: p.path.clone(),
                sha: if p.sha.is_empty() {
                    crate::fsutil::sha256_file(&p.path)?
                } else {
                    p.sha.clone()
                },
                warnings: Vec::new(),
                audit: p.audit.clone(),
            }),
            None => self.product(meta, device, format),
        }
    }

    /// 产物是文件（电脑上、MTP 设备上）。
    fn prepare_file(
        &self,
        meta: &Meta,
        device: &Profile,
        target: &Target,
        plan: Plan,
        force: bool,
    ) -> Result<Step, Error> {
        let Plan {
            fingerprint,
            format,
            ..
        } = plan;
        let (root, dir) = self.output_dir(meta, device, target)?;
        let sp = self.state_path(&device.id);
        let state = self.records.states.get(&sp);
        let prev = state.books.get(&meta.id).cloned();
        // 记录里这本书的产物以前放在电脑上、用户自己拷上设备的（2026-10-07 以前的做法）：设备上同一子目录里叫那个名字的文件
        // 就是那份拷贝，当成本书的（覆盖它），不另起名字、不留重复
        let copied = prev
            .as_ref()
            .filter(|p| !p.root.as_os_str().is_empty() && p.root != root)
            .and_then(|p| {
                let rel = p.path.strip_prefix(&p.root).ok()?;
                (dir.strip_prefix(&root).ok()? == rel.parent()?)
                    .then(|| rel.file_name()?.to_str().map(str::to_string))
                    .flatten()
            });
        let taken = self.build.reserved_names(&dir, &meta.id);
        let out = dir.join(state.file_name_for(
            meta,
            &dir,
            format.ext(),
            None,
            copied.as_deref(),
            &taken,
        )?);
        drop(state);
        let unchanged = |p: &&StateEntry| !force && p.fingerprint == fingerprint;
        let done = prev.as_ref().filter(unchanged).filter(|p| p.path.is_file());
        let staged = matches!(target, Target::Dir { staged: true, .. });
        if let Some(p) = done.filter(|p| p.path == out) {
            if !p.old.is_empty() {
                self.finish(&sp, &meta.id, p.clone(), staged)?; // 上次没删掉的旧位置
            }
            return Ok(Step::Done(Built::UpToDate(out)));
        }
        // 先比较：指纹没变、记录里的位置空了（以前放电脑上、用户自己挪上了设备），设备上对应的位置就是那份 → 认领，不重新生成
        if let Some(p) = prev
            .as_ref()
            .filter(unchanged)
            .filter(|_| done.is_none() && copied.is_some() && out.is_file())
            // 记着哈希的要核对：同名的不一定是那份拷贝（别的书、拷了一半的）；对不上就当普通的旧文件，重新生成覆盖
            .filter(|p| {
                p.sha.is_empty() || crate::fsutil::sha256_file(&out).is_ok_and(|h| h == p.sha)
            })
        {
            let old = p
                .placements()
                .into_iter()
                .filter(|x| x.path != out)
                .collect();
            let mut entry = StateEntry::file(out.clone(), root, fingerprint, old);
            entry.sha = p.sha.clone();
            entry.audit = p.audit.clone();
            self.finish(&sp, &meta.id, entry, staged)?;
            return Ok(Step::Done(Built::Adopted(out)));
        }
        // 设备中途断开了：不生成、不改记录（不然下面会把完好的指纹、哈希清掉，接上后整本重传），报给 `build_all` 记成掉线
        if let Target::Dir { mount: Some(m), .. } = target {
            if !deliver::mounted(m) {
                return Err(Error::new(
                    Kind::Unmounted,
                    format!("{UNMOUNTED}：{}", m.display()),
                ));
            }
        }
        // 来源：内容没变、只是位置变了的已有产物（挪过去，不重新生成），或者新生成的
        let made = self.made(meta, device, format, done)?;
        let tmp = made.tmp.clone();
        // MTP 设备上直接写正式文件名（不经临时文件，[`direct_write`]）：写到一半被打断，正式文件名上会留下半截，所以要先登记一条指纹留空的记录，
        // 下次认得出这个位置没传完、重新传（见 `Work`）
        let direct = direct_write(target);
        let mount = match target {
            Target::Dir { mount, .. } => mount.clone(),
            _ => None,
        };
        let r = (|| {
            // 目录里已有一个不认识的同名文件、和这份逐字节相同（以前手工拷上去的）：认领它，不另起名字
            let out = dir.join(self.records.states.get(&sp).file_name_for(
                meta,
                &dir,
                format.ext(),
                Some(&made.src),
                copied.as_deref(),
                &taken,
            )?);
            let mut entry = StateEntry::file(out.clone(), root, String::new(), Vec::new());
            // 设备上这个位置放的就是我们上次传的、和这次逐字节相同（看记录的哈希和文件大小，不把设备上的文件读回来）
            let mut same = false;
            match &prev {
                Some(p) => {
                    entry.old = p
                        .placements()
                        .into_iter()
                        .filter(|x| x.path != out)
                        .collect();
                    if p.path == out {
                        entry.fingerprint = p.fingerprint.clone(); // 同一位置重建：记录不用预先改
                        entry.sha = p.sha.clone();
                        same = p.sha == made.sha
                            && std::fs::metadata(&made.src).ok().map(|m| m.len())
                                == std::fs::metadata(&out).ok().map(|m| m.len());
                        if direct && !same {
                            entry.fingerprint.clear(); // 指纹没变（强制重建）时，写到一半被打断的半截会被当成传好了
                        }
                    }
                    // 换位置：先把记录改成新位置（指纹留空＝没完成），旧位置记进待删——中途被打断的话，下次还认得新位置上的
                    // 文件是这本书的，旧位置也还会删
                    if entry.fingerprint.is_empty() || entry.old != p.old {
                        // 没完成的：哈希也留空（原地覆写到一半，设备上的已不是记着的那份，下次不能凭哈希当成一样）
                        let mut pre = entry.clone();
                        if pre.fingerprint.is_empty() {
                            pre.sha.clear();
                        }
                        self.put_entry(&sp, &meta.id, pre)?;
                    }
                }
                // 新书：传的过程中这个文件名先在内存里占住，同一轮里同名的别的书不会选它。电脑上不预登记（省一次整份记录的写），
                // 中途被打断的话，下次重新生成出逐字节相同的产物时认领它（`file_name_for` 的 `same_as`）；MTP 设备上直接写正式
                // 文件名，半截文件认领不了，要预登记
                None => {
                    self.build.reserve(&out, &meta.id);
                    if direct {
                        self.put_entry(&sp, &meta.id, entry.clone())?;
                    }
                }
            }
            // 记录里没有哈希（不是我们传的、或旧记录）时才把设备上的读回来比
            let compare = entry.sha.is_empty();
            entry.fingerprint = fingerprint.clone();
            let t = self.transfer(
                meta,
                device,
                sp.clone(),
                made,
                done.map(|p| p.path.clone()),
                entry,
                Work {
                    out,
                    compare,
                    direct,
                    mount,
                },
                staged,
            );
            if same {
                // 和设备上的一样：不用传
                return self.complete_typed(t, Ok(false)).map(Step::Done);
            }
            Ok(Step::Transfer(Box::new(t)))
        })();
        if r.is_err() {
            if let Some(t) = &tmp {
                let _ = std::fs::remove_dir_all(t);
            }
        }
        r
    }

    /// 交给传输线程的一件。
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &self,
        meta: &Meta,
        device: &Profile,
        sp: PathBuf,
        made: Made,
        from: Option<PathBuf>,
        entry: StateEntry,
        work: Work,
        staged: bool,
    ) -> Transfer {
        let Made {
            tmp,
            src,
            sha,
            warnings,
            audit,
        } = made;
        let mut entry = entry;
        entry.audit = audit;
        Transfer {
            device: device.id.clone(),
            title: meta.title.clone(),
            book: meta.id.clone(),
            sp,
            tmp,
            src,
            sha,
            warnings,
            from,
            entry,
            work,
            staged,
        }
    }

    /// 传完了（`r` 是传输线程的结果：真的写了没有）：删掉临时目录，写上完成的记录、删掉旧位置。传失败的：记录不改
    /// （先登记了新位置、指纹留空，下次重做），返回错误。
    pub fn complete(&self, t: Transfer, r: Result<bool, Error>) -> Result<Built, String> {
        self.complete_typed(t, r).map_err(|e| e.to_string())
    }

    /// [`Library::complete`] 的内部版：传设备的错误带着种类，`build_run` 按种类决定停不停这台设备。
    pub(crate) fn complete_typed(
        &self,
        t: Transfer,
        r: Result<bool, Error>,
    ) -> Result<Built, Error> {
        if let Some(tmp) = &t.tmp {
            let _ = std::fs::remove_dir_all(tmp);
        }
        // 传完（或失败）：占着的文件名放开（成功的接着就记进生成记录）
        self.build.unreserve(&t.work.out);
        let wrote = r?;
        let meta_id = t.book.clone();
        let staged = t.staged;
        let mut entry = t.entry;
        entry.sha = t.sha;
        let path = entry.path.clone();
        self.finish(&t.sp, &meta_id, entry, staged)?;
        Ok(match t.from {
            Some(from) => Built::Moved { from, to: path },
            None if wrote => Built::Written {
                path,
                warnings: t.warnings,
            },
            None => Built::Same {
                path,
                warnings: t.warnings,
            },
        })
    }

    /// 生成产物写到 `part`（书库临时目录里）：EPUB 直接是优化结果；KFX、KEPUB 是同一份优化结果再转一次（中间文件放 `tmp`）。
    /// 返回警告（质量门、体检摘要）、产物的 SHA-256（整份在内存里的时候顺带算好；EPUB 是 `None`，由调用方读文件算）和体检清单。
    fn produce(
        &self,
        meta: &Meta,
        device: &Profile,
        format: Format,
        part: &Path,
        tmp: &Path,
    ) -> Result<Produced, String> {
        let epub = self.prepared_input(meta)?;
        let mut warnings = Vec::new();
        let comic = self.comic_of(meta)?;
        let mut opts = bookconv::optimize::OptimizeOpts::for_profile(device);
        // 按书指定翻页方向（Bangumi 判成日本漫画的 rtl）；其余留给原书、profile
        opts.page_direction = crate::metadata::comic_direction(meta, device, comic);
        // 竖排（书自己的 layout.vertical → 目录清单 → 原书本来是否竖排；默认横排，漫画、固定版式不竖排）
        opts.set_vertical(self.vertical_for(meta, device)?);
        // 要转 KFX 的，优化结果先放临时目录
        let optimized = if format == Format::Epub {
            part.to_path_buf()
        } else {
            tmp.join("optimized.epub")
        };
        let optimized_rep =
            bookconv::optimize::optimize_epub_file_streaming(&epub, &optimized, &opts, |_, _| {})?;
        // 体检（怪癖表，只报告）：sync 打摘要，清单记进生成记录
        let findings = optimized_rep.wash.map(|w| w.audit).unwrap_or_default();
        let audit: Vec<String> = findings
            .iter()
            .map(bookconv::audit::Finding::line)
            .collect();
        let rep = bookconv::check::check_epub_file(&optimized)?;
        if !rep.ok {
            warnings.extend(rep.errors.iter().map(|e| format!("质量门未过：{e}")));
        }
        if format == Format::Kepub {
            // 只插 koboSpan 和包装（docs/kepub.md）；产物再过一次质量门（插坏了能看出来）
            let kp =
                bookconv::kepub::epub_to_kepub(&optimized, part, &bookconv::kepub::Opts::default())
                    .map_err(|e| e.to_string())?;
            warnings.extend(kp.warnings);
            let rep = bookconv::check::check_epub_file(part)?;
            if !rep.ok {
                warnings.extend(
                    rep.errors
                        .iter()
                        .map(|e| format!("质量门未过（KEPUB）：{e}")),
                );
            }
        } else if format == Format::Kfx {
            // 按文件读（不先整本读进内存）；BufReader：zip 按条目小块读
            let open = || {
                std::fs::File::open(&optimized)
                    .map(std::io::BufReader::new)
                    .map_err(|e| format!("读 {}: {e}", optimized.display()))
            };
            // 唯一 ID 取自书的 id：重建出来还是"同一本书"，Kindle 上的阅读进度不丢
            let (bytes, w) = kfx::write::epub_to_kfx_from(
                open()?,
                &kfx::write::Opts {
                    fixed_id: kfx_id(&meta.id),
                    media: Some(kfx::css::MediaEnv::for_profile(device)),
                },
            )?;
            warnings.extend(w);
            std::fs::write(part, &bytes).map_err(|e| format!("写 {}: {e}", part.display()))?;
            return Ok((warnings, Some(crate::fsutil::sha256_hex(&bytes)), audit));
        }
        warnings.extend(bookconv::audit::summary(&findings, &device.id));
        Ok((warnings, None, audit))
    }
    /// 要优化的 EPUB：原件（EPUB）或当场转换的（CBZ），再补上书里没有、书库里有找来的封面、简介、标签。
    /// 和上一次是同一本书、同样的补充时，直接用上次的中间文件。
    fn prepared_input(&self, meta: &Meta) -> Result<PathBuf, String> {
        let input = self.content_path(meta)?;
        let cover = meta.cover.as_ref().map_or("", |c| c.sha256.as_str());
        let info = self.injected_info_sig(meta).unwrap_or_default();
        let key = format!("{}|{cover}|{info}", meta.content_sha());
        if let Some(epub) = self.build.prepared(&meta.id, &key) {
            return Ok(epub);
        }
        // 这本书以前做的（补的东西变了）先丢掉：目录是同一个
        self.release_prepared_of(&meta.id);
        let dir = self.root.join(format!("{TMP_PREFIX}{}-src", meta.id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut prepared = PreparedInput {
            book: meta.id.clone(),
            key,
            dir,
            epub: PathBuf::new(),
            bytes: 0,
        };
        let own = self.own_dc_of(meta);
        prepared.epub = self.epub_input(meta, &input, &prepared.dir).and_then(|e| {
            crate::metadata::with_additions(self, meta, own.as_ref(), &e, &prepared.dir)
        })?;
        if prepared.epub == input {
            return Ok(input); // 原件直接用：`prepared` 丢掉时删空目录
        }
        prepared.bytes = std::fs::metadata(&prepared.epub).map_or(0, |m| m.len());
        let epub = prepared.epub.clone();
        self.build.keep_prepared(prepared);
        Ok(epub)
    }

    /// 删掉这本书留着给别的模式用的中间文件（这本书所有模式都做完了时调）。
    pub fn release_prepared_of(&self, book: &str) {
        self.build.release_prepared_of(book);
    }

    /// 删掉所有留着的中间文件（一轮生成结束时调）。
    pub fn release_prepared(&self) {
        self.build.release_prepared();
    }

    /// 改写这个模式的生成记录（原子写）。
    fn put_state(&self, sp: &Path, state: State) -> Result<(), String> {
        std::fs::create_dir_all(self.state_dir())
            .map_err(|e| format!("{}: {e}", self.state_dir().display()))?;
        self.records.states.put(sp, state)
    }

    /// 写一条记录（其余不动）。
    fn put_entry(&self, sp: &Path, id: &str, entry: StateEntry) -> Result<(), String> {
        let mut state = (*self.records.states.get(sp)).clone();
        state.books.insert(id.to_string(), entry);
        self.put_state(sp, state)
    }

    /// 产物已经到位：删掉记着的旧位置（删不掉的留着下次再删），写上完成的记录。记录和原来一样（旧位置还是删不掉，
    /// `sync --watch` 每轮都会走到这里）时不写。
    fn finish(
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

    /// 要优化的 EPUB：EPUB 直接用；CBZ 当场转换，写进 `tmp`。
    pub(crate) fn epub_input(
        &self,
        meta: &Meta,
        input: &Path,
        tmp: &Path,
    ) -> Result<PathBuf, String> {
        match meta.content_format() {
            "epub" => Ok(input.to_path_buf()),
            "cbz" => {
                // 按文件读（不先把整个 CBZ 读进内存）
                let file = std::fs::File::open(input)
                    .map(std::io::BufReader::new)
                    .map_err(|e| format!("读 {}: {e}", input.display()))?;
                let p = tmp.join("master.epub");
                crate::convert_to_epub("cbz", file, &meta.title, &p)?;
                Ok(p)
            }
            other => Err(crate::unsupported(other)),
        }
    }
}

/// 产物根目录和跟踪目录互相包含时的提示（生成和 `track` 共用）。
pub(crate) fn output_overlap(root: &Path, tracked: &Path) -> String {
    format!(
        "产物目录 {} 和跟踪的目录 {} 互相包含，生成出来的书会被当成新书入库、层层嵌套（跟踪的目录不要用模式 id 命名，也不要放在别的跟踪目录的产物目录里）",
        root.display(),
        tracked.display()
    )
}

/// 某模式的生成记录（`output-state/<模式 id>.json`）：书 id → 产物在哪、指纹。
#[derive(Default, Clone, Serialize, Deserialize)]
pub(crate) struct State {
    books: BTreeMap<String, StateEntry>,
    /// 从书库删了的书留在设备上的产物：删的时候设备没接上，接上了再删（[`Library::flush_removed`]）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    orphans: Vec<Placement>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub(crate) struct StateEntry {
    /// 产物的绝对路径。
    path: PathBuf,
    /// 产物根目录（`D/../<模式>` 或 `<书库>/output/<模式>`）：删旧文件后，只在它以内删变空的目录。
    root: PathBuf,
    /// 空 = 这个位置的产物还没生成完（换位置时先登记，见模块注释）。
    fingerprint: String,
    /// 放上去的产物的 SHA-256：下次重新生成出一样的就不再传（不用把设备上的文件读回来比）。旧记录没有。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sha: String,
    /// 以前的位置、还没删掉的产物。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    old: Vec<Placement>,
    /// 生成时的体检清单（[`bookconv::audit`]）：`booklib list` 显示。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    audit: Vec<String>,
}

/// 一个产物文件和它的根目录。
#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Placement {
    path: PathBuf,
    root: PathBuf,
}

impl StateEntry {
    /// 文件产物（电脑上、MTP 设备上）的记录。
    fn file(path: PathBuf, root: PathBuf, fingerprint: String, old: Vec<Placement>) -> StateEntry {
        StateEntry {
            path,
            root,
            fingerprint,
            sha: String::new(),
            old,
            audit: Vec::new(),
        }
    }

    /// 这条记录名下的所有产物：现在的位置和待删的旧位置。
    fn placements(&self) -> Vec<Placement> {
        let mut v = vec![Placement {
            path: self.path.clone(),
            root: self.root.clone(),
        }];
        v.extend(self.old.iter().cloned());
        v
    }
}

impl State {
    /// 设备接上了，把记录里旧前缀开头的绝对路径改写成新的 `devices_base/<链接名>` 前缀（`path`、`root`、`old`、`orphans`）。
    /// 只改记录：不碰文件、不重传、不删。改写后指向同一个文件的旧位置（路径和当前位置一样）从 `old` / `orphans` 里去掉，
    /// 免得后面当成"换位置"去删现在这份。返回改没改。
    fn migrate_prefixes(&mut self, mappings: &[(PathBuf, PathBuf)]) -> bool {
        if mappings.is_empty() {
            return false;
        }
        let mut changed = false;
        for e in self.books.values_mut() {
            changed |= migrate_placement_prefix(&mut e.path, &mut e.root, mappings);
            for old in &mut e.old {
                changed |= migrate_placement_prefix(&mut old.path, &mut old.root, mappings);
            }
        }
        for o in &mut self.orphans {
            changed |= migrate_placement_prefix(&mut o.path, &mut o.root, mappings);
        }
        // 旧位置经过链接就是当前位置：不是两份文件，不该走"换位置删除"
        let mut current = BTreeSet::new();
        for e in self.books.values_mut() {
            current.insert(e.path.clone());
            let before = e.old.len();
            let mut seen = BTreeSet::new();
            e.old
                .retain(|o| o.path != e.path && seen.insert(o.path.clone()));
            changed |= e.old.len() != before;
        }
        let before = self.orphans.len();
        self.orphans.retain(|o| !current.contains(&o.path));
        changed |= self.orphans.len() != before;
        changed
    }

    /// 产物根目录只差大小写、新路径上的文件也在（不分大小写的设备）时，把记录原样挪到新根目录：
    /// 只改记录，不碰设备上的文件。返回改没改。
    ///
    /// 新路径上文件不在（大小写敏感的设备上是两个真目录）就不换，交给原来的"挪位置"流程。
    fn rebase_case_root(&mut self, new_root: &Path) -> bool {
        let mut changed = false;
        for e in self.books.values_mut() {
            changed |= rebase_case_placement(&mut e.path, &mut e.root, new_root);
            for old in &mut e.old {
                changed |= rebase_case_placement(&mut old.path, &mut old.root, new_root);
            }
        }
        for o in &mut self.orphans {
            changed |= rebase_case_placement(&mut o.path, &mut o.root, new_root);
        }
        changed
    }

    /// 这个产物现在（或待删的旧位置里）是不是记在别的书名下（按路径认）。
    fn claimed(&self, p: &Placement) -> bool {
        self.books
            .values()
            .any(|e| e.path == p.path || e.old.iter().any(|o| o.path == p.path))
    }

    /// 删掉这些产物（已经不在记录里的书名下），文件在各自的根目录以内删掉变空的目录。返回删不掉的（下次再删）：
    /// 可拔下的设备（MTP、USB 大容量存储的挂载目录下）上的文件，设备没挂上（[`Devices::unplugged`]）时不当成已删；相对路径（2026-10-31 以前 Move 上的旧记录）不动文件、记录直接丢掉。
    /// 调用前要先把本书的记录从 `self` 里拿掉，别的书还记着的不删。
    /// 删掉这些产物（[`State::delete_placements`]），删不掉的记进 `orphans`（下次接上设备再删）。
    fn delete_into_orphans(&mut self, list: Vec<Placement>, devices: &Devices) {
        let left = self.delete_placements(list, None, devices);
        self.orphans.extend(left);
    }

    /// `current`：调用方手上、还没放回 `self` 的本书当前位置（`finish`），和别的书的一样不删。
    fn delete_placements(
        &self,
        list: Vec<Placement>,
        current: Option<&Path>,
        devices: &Devices,
    ) -> Vec<Placement> {
        let mut left = Vec::new();
        for p in list {
            if self.claimed(&p) || current == Some(p.path.as_path()) {
                continue;
            }
            // 逐段只差大小写的另一个位置（汉王 documents/Documents、跟踪目录 Manga/manga）：不分大小写的设备上就是同一个文件，
            // 删它等于删现在这份。不删文件、不删目录，记录丢掉；大小写敏感的设备上会留下一个旧文件，宁可多留一份。
            if current.is_some_and(|c| case_alias(c, &p.path) && !case_twin(c, &p.path))
                || self.books.values().any(|e| {
                    e.placements()
                        .iter()
                        .any(|q| case_alias(&q.path, &p.path) && !case_twin(&q.path, &p.path))
                })
            {
                continue;
            }
            // 只差大小写的另一个名字正被用着（书名只改了大小写）：阅读器的文件系统多半不分大小写，删旧名字就是删现在这份
            let twins: Vec<&Path> = self
                .books
                .values()
                .flat_map(|e| {
                    std::iter::once(e.path.as_path()).chain(e.old.iter().map(|o| o.path.as_path()))
                })
                .chain(current)
                .filter(|q| case_twin(q, &p.path))
                .collect();
            // 相对路径：2026-10-31 以前 Move 上的旧记录（`文件夹/文件名`，Move 已经删了），不动、也不再记着
            if !p.path.is_absolute() {
                continue;
            }
            // 设备没挂上（[`Devices::unplugged`]：要真的挂着，不只看根目录在不在）
            if devices.unplugged(&p.path, &p.root) {
                left.push(p);
                continue;
            }
            if !twins.is_empty() {
                match distinct_on_disk(&p.path, &twins) {
                    Some(true) => {}
                    Some(false) => continue, // 同一个文件：不删，旧名字的记录丢掉
                    None => {
                        left.push(p); // 读不了目录：下次再看
                        continue;
                    }
                }
            }
            if !remove_product(&p.path) {
                left.push(p);
                continue;
            }
            // Kindle 书旁的 `.sdr`（进度）：书删了就没用了（删书再拷回 Kindle 也当新书），留着的话这个目录永远删不空。
            // 这本书只是换了目录或改了大小写（`carry_sidecar` 没挪过去时）不删，进度还留在旧位置。只看这本书的新位置：
            // 别的书同名不算
            let moved = current.is_some_and(|c| {
                let n = |x: &Path| x.file_name().map(|n| n.to_string_lossy().to_lowercase());
                n(c) == n(&p.path)
            });
            if !moved && p.path.extension().is_some_and(|e| e == "kfx") {
                let sdr = p.path.with_extension("sdr");
                if sdr.is_dir() {
                    let _ = std::fs::remove_dir_all(&sdr);
                }
            }
            // 传输线程可能正往这些目录里放别的书（刚建好目录、还没写进文件）：它那边碰到目录没了会重建（`deliver::put_file`）
            let mut dir = p.path.parent();
            while let Some(d) = dir.filter(|d| *d != p.root && d.starts_with(&p.root)) {
                if std::fs::remove_dir(d).is_err() {
                    break; // 不空（或删不了）：到此为止
                }
                dir = d.parent();
            }
        }
        left
    }

    /// 产物文件名：`书名.<ext>`。同一目录里和别的书撞名（不分大小写：阅读器的文件系统多半不分），或者目录里已有
    /// 一个不是本书产物的同名文件时，用 `书名 [id 前 6 位].<ext>`，不覆盖别人的文件。
    /// 本书在这个目录里已经用着其中一个名字的，一直用下去（名字稳定，重新生成后覆盖设备上的旧文件就行）。
    /// 给了 `same_as`（这次要放上去的产物）时，目录里不认识的同名文件和它逐字节相同的（以前手工拷上设备的）也可以用：认领它。
    /// `copied` 是本书以前放在电脑上的产物的文件名：设备上叫这个名字的文件是用户拷上去的那份，也算本书的。
    /// `taken` 是同一轮里别的书正在传、还没记进来的文件名（小写），也算别人的。
    fn file_name_for(
        &self,
        meta: &Meta,
        dir: &Path,
        ext: &str,
        same_as: Option<&Path>,
        copied: Option<&str>,
        taken: &BTreeSet<String>,
    ) -> Result<String, String> {
        // 文件名跟修正后的书名（网站书名 + 卷次，2026-10-12 定）
        let base =
            bookconv::util::sanitize_filename(&crate::metadata::corrected_title(meta), &meta.id);
        let plain = format!("{base}.{ext}");
        let suffixed = format!("{base} [{}].{ext}", meta.id.get(..6).unwrap_or(&meta.id));
        if let Some(e) = self
            .books
            .get(&meta.id)
            .filter(|e| e.path.parent() == Some(dir))
        {
            if let Some(name) = e
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| *n == plain || *n == suffixed)
            {
                return Ok(name.to_string());
            }
        }
        let fold = |n: &str| n.to_lowercase();
        let existing: BTreeSet<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| fold(&e.file_name().to_string_lossy()))
            .collect();
        let others: BTreeSet<String> = self
            .books
            .iter()
            .filter(|(id, _)| *id != &meta.id)
            .flat_map(|(_, e)| e.placements())
            .filter(|p| p.path.parent() == Some(dir))
            .filter_map(|p| p.path.file_name().map(|n| fold(&n.to_string_lossy())))
            .collect();
        // 本书待删的旧文件正好叫这个名字（书名改回去了）：算本书的，可以用
        // 待删的孤儿（删书时设备没接上、这轮末尾才删的）也算：是本工具的旧产物，覆盖它就行，不用另起 `[id]` 名字
        let mine: BTreeSet<String> = self
            .books
            .get(&meta.id)
            .into_iter()
            .flat_map(StateEntry::placements)
            .chain(self.orphans.iter().cloned())
            .filter(|p| p.path.parent() == Some(dir))
            .filter_map(|p| p.path.file_name().map(|n| fold(&n.to_string_lossy())))
            .collect();
        let adopt = |n: &str| {
            copied == Some(n) || same_as.is_some_and(|s| deliver::same_bytes(s, &dir.join(n)))
        };
        let free = |n: &str| {
            !others.contains(&fold(n))
                && !taken.contains(&fold(n))
                && (!existing.contains(&fold(n)) || mine.contains(&fold(n)) || adopt(n))
        };
        if free(&plain) {
            Ok(plain)
        } else if free(&suffixed) {
            Ok(suffixed)
        } else {
            Err(format!(
                "{} 里已经有 {plain} 和 {suffixed}，都不是这本书的产物，不覆盖",
                dir.display()
            ))
        }
    }
}

/// 相对目录逐段换成设备上能建的名字（见 [`Library::output_dir`]）：只换有不能用的字符、或以空格或点结尾的那段。
fn device_safe_rel(rel: &Path) -> PathBuf {
    rel.components()
        .map(|c| {
            let s = c.as_os_str().to_string_lossy();
            let bad = s
                .chars()
                .any(|ch| ch.is_control() || "\\:*?\"<>|".contains(ch))
                || s.ends_with([' ', '.']);
            if bad {
                bookconv::util::sanitize_filename(s.trim_end_matches([' ', '.']), "_")
            } else {
                s.into_owned()
            }
        })
        .collect()
}

/// 产物换了目录（文件名没变）：阅读器放在书旁边的同名 `.sdr` 文件夹（Kindle 的阅读进度、书签，`docs/kfx.md#阅读进度2026-10-06-真机`）
/// 跟着挪过去，免得进度留在旧目录（2026-10-14 定：原件目录改名后《哆啦A梦》的进度丢在旧目录）。文件名变了的不挪
/// （`.sdr` 里的文件按书名起名，改了名阅读器认不认没验证过），新位置已经有 `.sdr` 的不动，挪不了的不管（旧目录里留着）。
/// KEPUB（`x.kepub.epub`）不挪：Kobo 的进度在设备数据库里，没有 `.sdr`，`with_extension` 还会算成 `x.kepub.sdr`。
fn carry_sidecar(old: &Path, new: &Path) {
    if old.to_string_lossy().ends_with(".kepub.epub") {
        return;
    }
    if old == new || !old.is_absolute() || old.file_name() != new.file_name() {
        return;
    }
    let (from, to) = (old.with_extension("sdr"), new.with_extension("sdr"));
    if !from.is_dir() || to.exists() {
        return;
    }
    if std::fs::rename(&from, &to).is_ok() {
        let _ = crate::fsutil::sync_parent(&to);
        return;
    }
    // 改不了名（跨文件系统：旧位置在电脑上；或者 MTP 不支持）：拷过去再删旧的；拷不成的删掉半截，旧的留着
    if copy_dir(&from, &to).is_ok() {
        let _ = std::fs::remove_dir_all(&from);
        let _ = crate::fsutil::sync_parent(&to);
    } else {
        let _ = std::fs::remove_dir_all(&to);
    }
}

/// 整个目录拷一份（只拷普通文件和子目录）。
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// 生成时发现设备的挂载点不在了（中途拔线）：`build_all` 认这个开头，把设备记成掉线。
/// [`Library::space_check`] 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceCheck {
    /// 不用查（产物放电脑上、设备没接上）。
    Skip,
    Enough,
    /// 不够：`count` 本要传，连余量要 `need` 字节，剩 `free`，差 `short`。
    Short {
        count: usize,
        need: u64,
        free: u64,
        short: u64,
    },
    /// 读不到剩余空间（只警告、照旧传）。
    Unknown(String),
}

pub(crate) const UNMOUNTED: &str = "设备中途断开（挂载点不在了）";

/// 设备上直接写正式文件名（不经临时文件）：只在 MTP 上（jmtpfs 改名等于下载再上传一遍）。USB 大容量存储（Kobo）
/// 同目录改名是原子的，经临时文件：直写的话更新已有的书要先删旧的，写到一半拔线就丢了旧书、留下 Nickel 会去导入的半截。
fn direct_write(target: &Target) -> bool {
    matches!(target, Target::Dir { staged: false, .. })
}

/// 一个产物记录的 (path, root) 跟着设备前缀换：旧前缀开头的改成新前缀，只改字符串。
fn migrate_placement_prefix(
    path: &mut PathBuf,
    root: &mut PathBuf,
    mappings: &[(PathBuf, PathBuf)],
) -> bool {
    let mut changed = false;
    if let Some(new) = rebase_prefix(path, mappings) {
        *path = new;
        changed = true;
    }
    if let Some(new) = rebase_prefix(root, mappings) {
        *root = new;
        changed = true;
    }
    changed
}

fn rebase_prefix(path: &Path, mappings: &[(PathBuf, PathBuf)]) -> Option<PathBuf> {
    mappings
        .iter()
        .find_map(|(old, new)| path.strip_prefix(old).ok().map(|rest| new.join(rest)))
}

/// 一个产物记录的 (path, root) 跟着根目录的大小写改：根目录只差大小写、`new_root + 相对路径` 的文件在才换。
fn rebase_case_placement(path: &mut PathBuf, root: &mut PathBuf, new_root: &Path) -> bool {
    if !case_alias(root, new_root) {
        return false;
    }
    let Ok(rel) = path.strip_prefix(&*root) else {
        return false;
    };
    let new_path = new_root.join(rel);
    if !new_path.is_file() {
        return false;
    }
    *root = new_root.to_path_buf();
    *path = new_path;
    true
}

/// 同一目录里、文件名只差大小写的两个不同路径：是 [`case_alias`] 的特例（父目录相同、只有文件名差大小写）。
pub(crate) fn case_twin(a: &Path, b: &Path) -> bool {
    a != b
        && a.parent() == b.parent()
        && match (a.file_name(), b.file_name()) {
            (Some(x), Some(y)) => {
                x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase()
            }
            _ => false,
        }
}

/// 两个路径逐段只差大小写（不是同一个路径）：例如 `…/documents/x.epub` 和 `…/Documents/x.epub`。不要求在同一目录，
/// 可以跨目录；同一目录、只差文件名时是 [`case_twin`]。
/// 不分大小写的设备（jmtpfs、安卓存储）上它们是同一个文件；分大小写的设备上不是，所以调用方还要看文件在不在。
pub(crate) fn case_alias(a: &Path, b: &Path) -> bool {
    if a == b {
        return false;
    }
    let (mut ca, mut cb) = (a.components(), b.components());
    loop {
        match (ca.next(), cb.next()) {
            (None, None) => return true,
            (Some(x), Some(y))
                if x.as_os_str().to_string_lossy().to_lowercase()
                    == y.as_os_str().to_string_lossy().to_lowercase() => {}
            _ => return false,
        }
    }
}

/// `path` 和只差大小写的 `twins` 在磁盘上是不是各是各的文件：列目录，`path` 的原名和某个 twin 的原名都列得出来才算
/// （分大小写的文件系统）。不分大小写的文件系统上目录里只会有其中一个名字，算同一个文件，不能删。读不了目录返回 `None`（记着、下次再看）。
/// 只列 `path` 所在的一个目录，判断不了跨目录的别名（那要用 [`case_alias`]），所以调用方只拿同一目录的 twins（[`case_twin`]）来调它。
fn distinct_on_disk(path: &Path, twins: &[&Path]) -> Option<bool> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Some(false);
    };
    let rd = std::fs::read_dir(dir).ok()?;
    let names: Vec<std::ffi::OsString> = rd.flatten().map(|e| e.file_name()).collect();
    Some(
        names.iter().any(|n| n == name)
            && twins.iter().any(|t| {
                t.file_name()
                    .is_some_and(|tn| names.iter().any(|n| n == tn))
            }),
    )
}

/// 删掉一个产物文件：删掉了或本来就不在返回 `true`。删的时候报"不在"不立即当真：jmtpfs 刚挪完文件偶尔 stat 不到，
/// 重新列一次目录，里面还有这个名字就再删一次（删不掉的算没删，下次再删）。
fn remove_product(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => {
            let _ = crate::fsutil::sync_parent(path);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
                return true;
            };
            let listed = match std::fs::read_dir(dir) {
                Ok(rd) => rd.flatten().any(|e| e.file_name() == name),
                // 目录都不在了：文件当然也不在
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(_) => return false,
            };
            !listed || std::fs::remove_file(path).is_ok()
        }
        Err(_) => false,
    }
}

/// KFX 的唯一 ID（容器 id `CR!…`、`content_id`、`book_id` 都由它派生）：书库的书 id（SHA-256 前 12 位十六进制）按十六进制读成整数。
/// 同一本书重建不变；OPF 唯一标识符相同的两本书（模板生成的书常见）也各是各的。2026-10-06 以前取 `get(..16)`，
/// 12 位的书 id 永远取不到，退回 OPF 标识符的哈希。读不出来（书 id 不是不超过 16 位的十六进制，只有手改过的书库会这样）时
/// 返回 `None`，写出器退回 OPF 标识符。
fn kfx_id(book_id: &str) -> Option<u64> {
    if book_id.is_empty() || book_id.len() > 16 {
        return None;
    }
    u64::from_str_radix(book_id, 16).ok()
}

#[cfg(test)]
mod remove_tests {
    /// 开着挂载检查、挂载点只是普通目录（和上级同一个文件系统，拔掉后留下的空挂载点）而里面正好有同名目录：
    /// 不当成接上，产物不删、记录留进 orphans。
    #[test]
    fn unmounted_point_with_same_dir_keeps_orphan() {
        use super::{Placement, State};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let usbms = d.path().join("media");
        let root = usbms.join("KOBOeReader/书");
        std::fs::create_dir_all(&root).unwrap();
        let f = root.join("书.kepub.epub");
        std::fs::write(&f, b"x").unwrap();
        let env = DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: true,
            usbms_base: usbms.clone(),
        };
        let mut state = State::default();
        let p = Placement {
            path: f.clone(),
            root: root.clone(),
        };
        state.delete_into_orphans(vec![p.clone()], &Devices::new(env.clone()));
        assert!(f.exists(), "没真挂上：不删");
        assert!(state.orphans == vec![p.clone()], "记录留进 orphans");
        // 关了挂载检查（测试用普通目录当设备）：根目录在就当接上，删掉
        let mut state = State::default();
        let off = DeviceEnv {
            mount_check: false,
            ..env
        };
        state.delete_into_orphans(vec![p], &Devices::new(off));
        assert!(!f.exists() && state.orphans.is_empty());
    }

    /// 旧记录 `…/documents`、设备上（不分大小写）叫 `Documents`：记录原样挪过去，不碰文件；再调一次不变。
    /// 新路径不在（大小写敏感）或根目录不是只差大小写：不换。
    #[test]
    fn rebase_case_root_only_when_new_path_exists() {
        use super::{Placement, State, StateEntry};
        let d = tempfile::tempdir().unwrap();
        let old_root = d.path().join("documents");
        std::fs::create_dir_all(old_root.join("子目录")).unwrap();
        let old_file = old_root.join("子目录/书.epub");
        std::fs::write(&old_file, b"x").unwrap();
        let new_root = d.path().join("Documents");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&old_root, &new_root).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir_all(&new_root).unwrap();

        let mut state = State::default();
        state.books.insert(
            "b".into(),
            StateEntry {
                path: old_file.clone(),
                root: old_root.clone(),
                fingerprint: "f".into(),
                sha: String::new(),
                old: vec![Placement {
                    path: old_root.join("旧.epub"),
                    root: old_root.clone(),
                }],
                audit: Vec::new(),
            },
        );
        state.orphans.push(Placement {
            path: old_root.join("删了.epub"),
            root: old_root.clone(),
        });
        // 旧位置的文件都在（模拟不分大小写：新路径读得到）
        std::fs::write(old_root.join("旧.epub"), b"y").unwrap();
        std::fs::write(old_root.join("删了.epub"), b"z").unwrap();
        assert!(state.rebase_case_root(&new_root));
        let e = &state.books["b"];
        assert_eq!(e.root, new_root);
        assert_eq!(e.path, new_root.join("子目录/书.epub"));
        assert_eq!(e.old[0].path, new_root.join("旧.epub"));
        assert_eq!(state.orphans[0].path, new_root.join("删了.epub"));
        assert!(old_file.exists(), "只改记录，不碰文件");
        assert!(!state.rebase_case_root(&new_root), "幂等");

        // 根目录不是只差大小写：不换
        let mut state2 = State::default();
        state2.books.insert(
            "b".into(),
            StateEntry {
                path: old_file.clone(),
                root: old_root.clone(),
                fingerprint: String::new(),
                sha: String::new(),
                old: Vec::new(),
                audit: Vec::new(),
            },
        );
        assert!(!state2.rebase_case_root(&d.path().join("Books2")));

        // 新路径上的文件不在（两个真目录）：不换
        let other = d.path().join("DOCUMENTS");
        std::fs::create_dir_all(&other).unwrap();
        assert!(!state2.rebase_case_root(&other));
        assert_eq!(state2.books["b"].path, old_file);
    }

    #[test]
    fn case_alias_paths_are_not_deleted() {
        use super::{Placement, State};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let old_root = d.path().join("documents");
        let new_root = d.path().join("Documents");
        std::fs::create_dir_all(&old_root).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&old_root, &new_root).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir_all(&new_root).unwrap();
        let file = old_root.join("书.epub");
        std::fs::write(&file, b"x").unwrap();
        let dev = Devices::new(DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: false,
            usbms_base: d.path().join("media"),
        });
        let state = State::default();
        let old = Placement {
            path: file.clone(),
            root: old_root,
        };
        let cur = new_root.join("书.epub");
        let left = state.delete_placements(vec![old], Some(&cur), &dev);
        assert!(left.is_empty(), "记录丢掉");
        assert!(file.exists(), "只差大小写的位置不删文件");
        assert!(cur.exists());
    }

    /// 只有 MTP 直写；USB 大容量存储（Kobo）和电脑上都经临时文件。
    #[test]
    fn only_mtp_writes_direct() {
        use super::{direct_write, Target};
        use std::path::PathBuf;
        let dir = |r: &str, staged| Target::Dir {
            root: PathBuf::from(r),
            mount: None,
            staged,
        };
        assert!(direct_write(&dir(
            "/run/user/1/mtp/k/内部存储/documents",
            false
        )));
        assert!(!direct_write(&dir("/run/media/u/KOBOeReader/书", true)));
        assert!(!direct_write(&Target::Local));
    }

    /// 删书：Kindle 书旁的 `.sdr` 一起删（不然目录删不空）；同名的书换了目录、`.sdr` 没挪过去的不删。
    /// `carry_sidecar` 改不了名时拷过去。
    #[test]
    fn sidecar_removed_with_book_and_copied_when_rename_fails() {
        use super::{Placement, State};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("out");
        let sub = root.join("漫画");
        std::fs::create_dir_all(sub.join("a.sdr")).unwrap();
        std::fs::write(sub.join("a.sdr/a.yjf"), b"p").unwrap();
        std::fs::write(sub.join("a.kfx"), b"k").unwrap();
        let dev = Devices::new(DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: false,
            usbms_base: d.path().join("media"),
        });
        let p = Placement {
            path: sub.join("a.kfx"),
            root: root.clone(),
        };
        // 换目录、同名：不删 .sdr
        let other = root.join("新/a.kfx");
        std::fs::create_dir_all(sub.join("x")).unwrap(); // 让旧目录删不空，看得见 .sdr 还在
        State::default().delete_placements(vec![p.clone()], Some(&other), &dev);
        assert!(sub.join("a.sdr/a.yjf").exists());
        // 删书：一起删，目录删空；别的目录里有同名的另一本书不碍事
        std::fs::write(sub.join("a.kfx"), b"k").unwrap();
        std::fs::remove_dir(sub.join("x")).unwrap();
        let mut st = State::default();
        st.books.insert(
            "别的".into(),
            super::StateEntry::file(
                root.join("名著/a.kfx"),
                root.clone(),
                "f".into(),
                Vec::new(),
            ),
        );
        st.delete_placements(vec![p], None, &dev);
        assert!(!sub.exists(), "书和 .sdr 都删了，目录删空");
        // 拷目录
        let (a, b) = (d.path().join("s.sdr"), d.path().join("t.sdr"));
        std::fs::create_dir_all(a.join("子")).unwrap();
        std::fs::write(a.join("子/f"), b"1").unwrap();
        super::copy_dir(&a, &b).unwrap();
        assert_eq!(std::fs::read(b.join("子/f")).unwrap(), b"1");
    }

    /// 书名只改了大小写（`Abc.epub` → `abc.epub`）：不分大小写的文件系统上目录里只剩新名字，旧位置不能删（删的就是现在这份）；
    /// 分大小写的文件系统上两个都在，旧的照删。
    #[test]
    fn case_only_rename_keeps_current() {
        use super::{Placement, State};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("out");
        std::fs::create_dir_all(&root).unwrap();
        let (old, cur) = (root.join("Abc.epub"), root.join("abc.epub"));
        let dev = Devices::new(DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: false,
            usbms_base: d.path().join("media"),
        });
        let p = Placement {
            path: old.clone(),
            root: root.clone(),
        };
        // 模拟不分大小写：改名后目录里只有 abc.epub，删 Abc.epub 会删到它
        std::fs::write(&cur, b"new").unwrap();
        let state = State::default();
        let left = state.delete_placements(vec![p.clone()], Some(&cur), &dev);
        assert!(left.is_empty(), "旧名字的记录丢掉");
        assert!(
            super::case_twin(&old, &cur) && super::distinct_on_disk(&old, &[&cur]) == Some(false)
        );
        assert!(cur.exists());
        // 列不了目录（目录不在）：记着下次再看，不当成同一个文件丢掉
        let gone = Placement {
            path: root.join("没有的目录/Abc.epub"),
            root: p.root.clone(),
        };
        let twin = root.join("没有的目录/abc.epub");
        assert_eq!(
            state.delete_placements(vec![gone], Some(&twin), &dev).len(),
            1
        );
        // 分大小写：两个都在，旧的删掉
        std::fs::write(&old, b"old").unwrap();
        let left = state.delete_placements(vec![p], Some(&cur), &dev);
        assert!(left.is_empty() && !old.exists() && cur.exists());
    }

    /// 缺省的 usbms_base 两次运行之间在 `/run/media/$USER` 和 `/media/$USER` 之间换了：另一处下的旧产物也认得是设备上的，
    /// 没挂上就不当成已删（以前认不出，删不到就当删了，记录丢掉、书留在设备上）。
    #[test]
    fn unplugged_knows_both_media_dirs() {
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let u = "booklib-test-no-such-user";
        let dev = Devices::new(DeviceEnv {
            devices_base: std::path::PathBuf::from("/nonexistent/devices"),
            mtp_base: std::path::PathBuf::from("/nonexistent/mtp"),
            mount_check: true,
            usbms_base: std::path::Path::new("/run/media").join(u),
        });
        let root = std::path::Path::new("/media").join(u).join("KOBOeReader");
        assert!(dev.unplugged(&root.join("a.epub"), &root));
        let dev = Devices::new(DeviceEnv {
            devices_base: std::path::PathBuf::from("/nonexistent/devices"),
            mtp_base: std::path::PathBuf::from("/nonexistent/mtp"),
            mount_check: true,
            usbms_base: std::path::Path::new("/media").join(u),
        });
        let root = std::path::Path::new("/run/media")
            .join(u)
            .join("KOBOeReader");
        assert!(dev.unplugged(&root.join("a.epub"), &root));
    }

    /// usbms_base 在 mtp_base 下面（关了挂载检查时的缺省）：挂载点按 usbms_base 算（`<usbms>/KOBOeReader`），不是 `<mtp>/usbms`。
    /// MTP 下的也要真挂着才删。
    #[test]
    fn unplugged_uses_the_longer_base() {
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let mtp = d.path().join("mtp");
        let root = mtp.join("usbms/KOBOeReader/书");
        std::fs::create_dir_all(&root).unwrap();
        let dev = Devices::new(DeviceEnv {
            devices_base: mtp.join("devices"),
            mtp_base: mtp.clone(),
            mount_check: true,
            usbms_base: mtp.join("usbms"),
        });
        // 普通目录：两个 base 下都算没挂上（mtp 下的挂载点也只是普通目录）
        assert!(dev.unplugged(&root.join("a.epub"), &root));
        let mroot = mtp.join("iReader/内部存储/documents");
        std::fs::create_dir_all(&mroot).unwrap();
        assert!(dev.unplugged(&mroot.join("a.epub"), &mroot));
        // 不在设备上的不管
        assert!(!dev.unplugged(&d.path().join("out/a.epub"), d.path()));
        // 关了挂载检查：根目录在就算接上
        let off = Devices::new(DeviceEnv {
            devices_base: mtp.join("devices"),
            mtp_base: mtp.clone(),
            mount_check: false,
            usbms_base: mtp.join("usbms"),
        });
        assert!(!off.unplugged(&root.join("a.epub"), &root));
        assert!(off.unplugged(
            &mtp.join("usbms/没有/书/a.epub"),
            &mtp.join("usbms/没有/书")
        ));
    }

    #[test]
    fn remove_product_reports_gone_and_missing() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("书.epub");
        std::fs::write(&f, b"x").unwrap();
        assert!(super::remove_product(&f));
        assert!(!f.exists());
        assert!(super::remove_product(&f), "本来就不在：算删掉了");
        assert!(
            super::remove_product(&d.path().join("没有的目录/书.epub")),
            "目录都不在：算删掉了"
        );
    }

    #[test]
    fn sidecar_follows_a_moved_product() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("漫画"), d.path().join("优秀漫画"));
        std::fs::create_dir_all(a.join("书 第01卷.sdr")).unwrap();
        std::fs::write(a.join("书 第01卷.sdr/进度.yjf"), b"p").unwrap();
        std::fs::create_dir_all(&b).unwrap();
        super::carry_sidecar(&a.join("书 第01卷.kfx"), &b.join("书 第01卷.kfx"));
        assert_eq!(
            std::fs::read(b.join("书 第01卷.sdr/进度.yjf")).unwrap(),
            b"p"
        );
        assert!(!a.join("书 第01卷.sdr").exists());
        // 改了名的不挪；新位置已有的不动
        std::fs::create_dir_all(a.join("甲.sdr")).unwrap();
        super::carry_sidecar(&a.join("甲.kfx"), &b.join("乙.kfx"));
        assert!(a.join("甲.sdr").is_dir() && !b.join("乙.sdr").exists());
        std::fs::create_dir_all(b.join("甲.sdr")).unwrap();
        std::fs::write(a.join("甲.sdr/x"), b"old").unwrap();
        super::carry_sidecar(&a.join("甲.kfx"), &b.join("甲.kfx"));
        assert!(a.join("甲.sdr/x").exists() && !b.join("甲.sdr/x").exists());
    }

    #[test]
    fn device_dir_names_made_safe_only_where_needed() {
        use std::path::{Path, PathBuf};
        assert_eq!(
            super::device_safe_rel(Path::new("侦探悬疑/亚洲侦探")),
            PathBuf::from("侦探悬疑/亚洲侦探"),
            "能用的原样"
        );
        assert_eq!(
            super::device_safe_rel(Path::new("科幻: 合集/上册.")),
            PathBuf::from("科幻_ 合集/上册")
        );
        assert_eq!(super::device_safe_rel(Path::new("")), PathBuf::from(""));
    }
}

#[cfg(test)]
mod kfx_id_tests {
    #[test]
    fn book_id_is_parsed_whole() {
        assert_eq!(super::kfx_id("0123456789ab"), Some(0x0123_4567_89ab));
        assert_eq!(super::kfx_id(""), None);
        assert_eq!(super::kfx_id("xyz"), None);
        assert_eq!(super::kfx_id("0123456789abcdef0"), None);
    }
}

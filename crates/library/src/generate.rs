//! 按阅读模式（设备 profile）生成产物并送到设备上：生成计划与指纹、产物放在哪、跳过没变的、生成记录。
//!
//! 产物位置（每个模式一份，`<名>` 见 [`State::file_name_for`]；2026-10-07 起直接送设备，电脑上不留，见 [`crate::deliver`]）：
//! - Kindle、掌阅（profile `[deliver] kind = "mtp"`）：设备存储根目录的 `documents/<原件所在目录相对跟踪目录的路径>/<名>.<扩展名>`；
//!   `add` 进来的单个文件（不在跟踪目录里）、网址书放 `documents/` 顶层。先在书库的临时目录里生成，再拷上去
//!   （设备上已有逐字节相同的就不拷：Kindle 覆盖成不同字节会清掉阅读进度）。
//! - Move（`kind = "xochitl"`）：经书架服务加入 xochitl，文件夹名是原件所在目录相对跟踪目录的路径；同一文件夹同一书名的
//!   原地替换（保留 uuid、进度）；文件夹或书名变了加入新的、旧的进回收站。
//! - 没写 `[deliver]` 的自定义模式：电脑上，跟踪目录 `D` 旁的 `D/../<模式 id>/<子目录>/`；add 进来的、网址书在 `<书库>/output/<模式 id>/`。
//!
//! 设备没接上的模式不生成（`sync` 跳过，下次接上再传）。
//!
//! 产物格式按模式：EPUB 直接是优化结果；KFX、AZW3（Kindle）是同一份优化结果再转一次（`kfx`、`azw3` crate）。
//! profile 可以给漫画另配格式（`comic_format`，见 [`Library::output_format`]）；内置模式都不用（kindle 文字书、漫画都出 KFX）。
//!
//! 生成记录 `<书库>/output-state/<模式 id>.json`：书 id → 产物绝对路径（Move 上的是 uuid 和 `文件夹/文件名`）、产物根目录、指纹。
//! 产物位置变了（原件移动、改名换了目录，书名改了）时删掉旧位置的——**只删记录里记着的**，不认识的文件一概不动；删完顺带删掉
//! 产物根目录以内变空的目录。内容没变、只是位置变了的，把已有产物挪过去，不重新生成（改成送设备以前留在电脑上的产物也这样挪上设备）。
//! 删书时设备没接上、删不掉的记在 `orphans`，接上后删。
//!
//! 换位置时先把记录改成新位置（指纹留空＝没完成，旧位置记进 `old` 待删），产物放好再补上指纹、删旧的：
//! 中途被打断的话，下次生成还认得新位置上的文件是这本书的（不会因为"有个不认识的同名文件"而改名），旧文件也还会删。

use crate::deliver::{self, Target};
use crate::fsutil::TMP_PREFIX;
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
    Written { path: PathBuf, warnings: Vec<String> },
    UpToDate(PathBuf),
    /// 内容没变、只是位置变了（原件移动改名、书名改了）：已有产物挪了过去，没有重新生成。
    Moved { from: PathBuf, to: PathBuf },
    /// 重新生成了（规则版本变了、`--force`），但和设备上那份逐字节相同：没有再传（Kindle 进度不受影响、Move 不重排）。
    Same { path: PathBuf, warnings: Vec<String> },
    /// 没变化、记录里的位置空了（以前放电脑上、用户自己挪上了设备），设备上对应位置的那份认领下来，没有重新生成。
    Adopted(PathBuf),
}

/// [`Library::prepare`] 的结果：已经有结论了，或者生成好、要交给传输线程放上设备的一件。
pub enum Step {
    Done(Built),
    Transfer(Box<Transfer>),
}

/// 生成好、要放上设备的一本（[`Library::prepare`] 交出，传输线程 [`Transfer::run`]，传完 [`Library::complete`] 记下来）。
/// 只含普通数据，能送进别的线程。
pub struct Transfer {
    /// 模式 id、书名（报告用）。
    pub device: String,
    pub title: String,
    book: String,
    /// 生成记录文件。
    sp: PathBuf,
    /// 产物在书库临时目录里：传完删掉这个目录。
    tmp: Option<PathBuf>,
    src: PathBuf,
    sha: String,
    warnings: Vec<String>,
    /// 内容没变、只是挪位置：原来在哪。
    from: Option<PathBuf>,
    /// 传完写的记录（指纹已填；Move 的 uuid、显示名传完才知道）。
    entry: StateEntry,
    pub(crate) work: Work,
}

/// 传输线程要做的。
pub(crate) enum Work {
    /// 放到 `out`（MTP 设备、电脑上）；`compare` 时 `out` 已是逐字节相同的就不动（要把它读回来比）。
    File { out: PathBuf, compare: bool },
    /// 交给 Move 上的书架服务：`replace` 有就原地替换这个 uuid（设备上已经没了就改成新加），否则新加进 `folder`。
    Move { client: deliver::MoveClient, name: String, folder: String, replace: Option<String> },
}

/// 传输线程做完了。
pub enum Done {
    /// 放好了文件（`false`：设备上已是一样的，没写）。
    File(bool),
    /// Move 上加入（或替换）好了。
    Move(deliver::Doc),
    /// Move 上那份已经一样，没再传。
    Same(deliver::Doc),
}

impl Transfer {
    /// 放上设备，做完才返回（单本生成用；`sync` 里 Move 的那件由传输线程交了以后轮流查，见 [`crate::Pipeline`]）。
    pub fn run(&self) -> Result<Done, String> {
        match &self.work {
            Work::File { out, compare } => deliver::put_file(&self.src, out, false, *compare).map(Done::File),
            Work::Move { client, .. } => {
                let s = self.submit()?;
                client.wait(s).map(Done::Move)
            }
        }
    }

    /// 交给 Move 以后不再需要产物文件（书架服务已经收下）：书库临时目录里的先删掉，省得排版排队时大漫画堆在电脑上。
    pub(crate) fn release_src(&self) {
        if self.tmp.is_some() {
            let _ = std::fs::remove_file(&self.src);
        }
    }

    /// 书 id。
    pub fn book_id(&self) -> &str {
        &self.book
    }

    pub(crate) fn is_move(&self) -> bool {
        matches!(self.work, Work::Move { .. })
    }

    /// Move：交出去（替换的那本设备上已经没了就改成新加）。
    pub(crate) fn submit(&self) -> Result<deliver::Submitted, String> {
        let Work::Move { client, name, folder, replace } = &self.work else { unreachable!("只给 Move 调") };
        if let Some(uuid) = replace {
            if let Some(s) = client.submit_replace(uuid, &self.src, name)? {
                return Ok(s);
            }
        }
        client.submit_import(&self.src, name, folder)
    }
}

/// 一本书在某模式下的产物状态（`list` 用）。
pub struct OutputStatus {
    pub device: String,
    pub path: PathBuf,
    /// `Some(true)` 最新；`Some(false)` 过期（原件或处理规则变了，或上次生成没完成）；`None` 判断不了（模式 profile 已删、文件不在）。
    pub fresh: Option<bool>,
}

/// 与模式无关的中间文件：CBZ 转出来的 EPUB、补了元数据的 EPUB（在书库的 `.tmp-<id>-src/` 里）。
/// 一本书要给几个模式生成时只做一次（一本 135MB 的漫画以前每个模式都整本转一遍）。丢掉时删目录。
pub(crate) struct PreparedInput {
    /// 内容哈希 + 补的封面 + 补的元数据：任何一样变了就重做。
    key: String,
    dir: PathBuf,
    epub: PathBuf,
}

impl Drop for PreparedInput {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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
        // AZW3、KFX 再带上写出器的版本（写出器改了也要重建）
        let format_seg = match format {
            Format::Epub => format.ext().to_string(),
            Format::Azw3 => format!("{}{}", format.ext(), azw3::WRITER_VERSION),
            Format::Kfx => format!("{}{}", format.ext(), kfx::write::WRITER_VERSION),
        };
        let cover = meta.cover.as_ref().map_or("-", |c| c.sha256.get(..12).unwrap_or(&c.sha256));
        let info = self.injected_info_sig(meta).unwrap_or_else(|| "-".into());
        // 补元数据那一步（`metadata::inject`）改了会影响产物时 `bookconv::opfmeta::VERSION` 加一：只让补过东西的书过期（`i5`），
        // 没补过东西的书指纹不变
        let info = if cover != "-" || info != "-" { format!("{info}i{}", bookconv::opfmeta::VERSION) } else { info };
        // 要当场转换的来源（CBZ）再带上格式转换的版本；写在流程版本后面，EPUB 来源的指纹保持原样（不白重建）
        let pipeline = if meta.content_format() == "epub" {
            PIPELINE_VERSION.to_string()
        } else {
            format!("{PIPELINE_VERSION}c{}", bookconv::convert::CONVERT_VERSION)
        };
        // 注释呈现方式（profile 的 notes：弹窗/跳转；书库 profiles/ 里的自定义模式改了它也要重建）
        let notes = match device.notes {
            profile::Notes::Popup => "popup",
            profile::Notes::Jump => "jump",
        };
        // 图标注释号换数字（profile 的 note_icons = "number"）时再带个 `#`
        let notes = if device.note_icons == profile::NoteIcons::Number { format!("{notes}#") } else { notes.to_string() };
        // 保留注释回链（profile 的 note_backlinks，缺省保留）时再带个 `<`
        let notes = if device.note_backlinks { format!("{notes}<") } else { notes };
        // 文字书、漫画分开算（2026-10-08）：只影响另一路的版本号、profile 字段变了，这一路不过期。
        // 文字书：EPUB 阅读范围 + 只管文字书的字段；漫画：EPUB 阅读范围 + 漫画画布 + 白边 + 只管漫画的字段。
        // 两路都过清洗层：背景图、rgba 两路都带。
        let comic = self.comic_of(meta)?;
        let shared = match (device.background_images, device.background_sizing) {
            (true, true) => "b",
            (true, false) => "bn",
            _ => "",
        };
        // 阅读器不认 rgba() 颜色（profile 的 css_rgba = false）
        let shared = if device.css_rgba { shared.to_string() } else { format!("{shared}r") };
        let (version, area_seg) = if comic {
            // 漫画画布和阅读器页边距（comic_readable、comic_reader_margins）、白边（comic_margin）、翻页方向改写（如 `dltr`）、固定版式（`f`）
            // EPUB 阅读范围也带上：漫画书里别的图（网上的图）按它缩
            let c = device.comic_readable();
            let mut seg = format!("{}x{}c{}x{}+{}{}", area.width, area.height, c.width, c.height, device.comic_margin, device.comic_reader_margins.map(|m| format!("m{m}")).unwrap_or_default());
            if let Some(d) = &device.comic_page_direction {
                seg.push_str(&format!("d{d}"));
            }
            if device.comic_fixed_layout {
                seg.push('f');
            }
            (format!("c{}", bookconv::optimize::COMIC_VERSION), format!("{seg}{shared}"))
        } else {
            // 带图注的竖长图写宽度（`k`，caption_fit）、正文图片透明处合成白底（`a`，image_alpha = false）、只修复（`t`，text_repair_only）
            let mut seg = format!("{}x{}{shared}", area.width, area.height);
            for (on, c) in [(device.caption_fit, 'k'), (!device.image_alpha, 'a'), (device.text_repair_only, 't'), (device.text_repair_only && device.repair_note_links, 'n')] {
                if on {
                    seg.push(c);
                }
            }
            (bookconv::optimize::OPTIMIZE_VERSION.to_string(), seg)
        };
        let fingerprint = format!(
            "{}|{cover}|{info}|{pipeline}|{version}|{notes}|{}|{area_seg}|{}|{}",
            meta.content_sha(),
            device.id,
            if device.color { "color" } else { "gray" },
            format_seg,
        );
        // 以前的写法：一个版本号、profile 的字段全带上。版本号用这一路现在的：这一路的版本变过，就对不上，照常重建
        let legacy = {
            let c = device.comic_readable();
            let mut seg = match (c != area, device.comic_reader_margins) {
                (false, None) => String::new(),
                (_, m) => format!("c{}x{}{}", c.width, c.height, m.map(|m| format!("m{m}")).unwrap_or_default()),
            };
            if let Some(d) = &device.comic_page_direction {
                seg.push_str(&format!("d{d}"));
            }
            for (on, x) in [(device.comic_fixed_layout, "f"), (device.background_images && device.background_sizing, "b"), (device.background_images && !device.background_sizing, "bn"), (!device.css_rgba, "r"), (device.caption_fit, "k"), (!device.image_alpha, "a"), (device.text_repair_only, "t")] {
                if on {
                    seg.push_str(x);
                }
            }
            let v = if comic { bookconv::optimize::COMIC_VERSION } else { bookconv::optimize::OPTIMIZE_VERSION };
            let gray = if device.color { "color" } else { "gray" };
            format!("{}|{cover}|{info}|{pipeline}|{v}|{notes}|{}|{}x{}+{}{seg}|{gray}|{format_seg}", meta.content_sha(), device.id, area.width, area.height, device.comic_margin)
        };
        Ok(Plan { fingerprint, legacy, format })
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
        if let Some(c) = self.comic.borrow().get(&sha).copied() {
            return Ok(c);
        }
        let c = self.is_comic(meta)?;
        self.comic.borrow_mut().insert(sha, c);
        if self.locked.get() {
            if let Some(mut m) = self.read_meta(&meta.id).filter(|m| m.content_sha() == meta.content_sha()) {
                m.comic = Some(c);
                self.save_meta(&m)?;
            }
        }
        Ok(c)
    }

    /// 生成时真正会补进书里的简介、标签的指纹（书里已有的那项不补、不进指纹：不然书里有简介的书，找来的简介变了也白重建）。
    /// 要看书里有没有：EPUB 读一下 OPF（按内容哈希缓存）；读不出来（原件不在了）、CBZ（当场转换，补进的是转换结果）时
    /// 按两项都补算（与以前的指纹相同）。
    fn injected_info_sig(&self, meta: &Meta) -> Option<String> {
        let info = meta.info.as_ref()?;
        info.injected_sig()?;
        let sha = meta.content_sha();
        let cached = self.own_dc.borrow().get(sha).copied();
        let own = match cached {
            Some(own) => Some(own),
            None if meta.content_format() == "epub" => {
                let path = match meta.source() {
                    crate::Source::Stored => self.entry_dir(&meta.id).join(&meta.master),
                    crate::Source::Original => PathBuf::from(&meta.source_path),
                };
                let own = crate::metadata::own_description_subjects(&path).ok();
                // 原件动过（可能不是这个内容了）的不记：生成前会核对，核对过再算
                if let Some(o) = own.filter(|_| self.original_state(meta) != crate::OriginalState::Touched) {
                    self.own_dc.borrow_mut().insert(sha.to_string(), o);
                }
                own
            }
            None => None,
        };
        match own {
            Some((d, s)) => info.injected_sig_for(d, s),
            None => info.injected_sig(),
        }
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

    /// `output-state/` 下各模式的生成记录（包括 profile 已经删掉的模式）：(模式 id, 记录文件)。
    pub(crate) fn state_files(&self) -> Vec<(String, PathBuf)> {
        let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(self.state_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let id = p.file_stem()?.to_str()?.to_string();
                (!id.starts_with('.') && p.extension().is_some_and(|x| x == "json")).then_some((id, p))
            })
            .collect();
        v.sort();
        v
    }

    /// 生成记录里产物所在的所有目录（拿到锁时清理残留临时文件用）：电脑上的和接着的 MTP 设备上的（没挂上的读不到，跳过）。
    pub(crate) fn output_dirs(&self) -> BTreeSet<PathBuf> {
        let mut dirs = BTreeSet::new();
        for (_, sp) in self.state_files() {
            let state = self.states.get(&sp);
            let files = state.books.values().flat_map(StateEntry::placements).chain(state.orphans.iter().cloned());
            dirs.extend(files.filter(|p| p.uuid.is_empty() && p.path.is_absolute()).filter_map(|p| p.path.parent().map(Path::to_path_buf)));
        }
        dirs
    }

    /// 原件在跟踪目录里的话：(跟踪目录, 原件所在目录相对它的路径)。
    fn tracked_rel(&self, meta: &Meta) -> Option<(PathBuf, PathBuf)> {
        if meta.source() == crate::Source::Stored || meta.source_path.is_empty() {
            return None;
        }
        let src = Path::new(&meta.source_path);
        let sources = self.sources_json.get(&self.sources_path());
        let d = sources.dirs().iter().find(|d| src.starts_with(d))?.clone();
        let rel = src.parent().and_then(|p| p.strip_prefix(&d).ok()).unwrap_or(Path::new("")).to_path_buf();
        Some((d, rel))
    }

    /// 这本书在该模式下的产物根目录和产物所在目录（绝对路径），见模块注释。`target` 是 MTP 设备时根目录是设备上的
    /// `<存储>/<dir>`，否则是电脑上跟踪目录旁的 `<模式 id>/`（add 进来的书、网址书在书库 `output/<模式 id>/`）。
    fn output_dir(&self, meta: &Meta, device: &Profile, target: &Target) -> Result<(PathBuf, PathBuf), String> {
        let device_root = match target {
            Target::Dir { root } => Some(root.clone()),
            _ => None,
        };
        let lib_root = device_root.clone().unwrap_or_else(|| std::path::absolute(&self.root).unwrap_or_else(|_| self.root.clone()).join("output").join(&device.id));
        let Some((d, rel)) = self.tracked_rel(meta) else {
            return Ok((lib_root.clone(), lib_root));
        };
        let root = match device_root {
            Some(r) => r,
            None => {
                let Some(parent) = d.parent() else { return Ok((lib_root.clone(), lib_root)) };
                let root = parent.join(&device.id);
                let sources = self.sources_json.get(&self.sources_path());
                if let Some(t) = sources.dirs().iter().find(|t| crate::sources::overlaps(&root, t)) {
                    return Err(output_overlap(&root, t));
                }
                root
            }
        };
        let dir = root.join(rel);
        Ok((root, dir))
    }

    /// Move 上放进哪个文件夹：原件所在目录相对跟踪目录的路径（`a/b`：书架服务按层建，先「a」、再它里面的「b」，2026-10-07 用户定）；
    /// 跟踪目录顶层的书、add 进来的书、网址书放书库根（空）。
    fn xochitl_folder(&self, meta: &Meta) -> String {
        let Some((_, rel)) = self.tracked_rel(meta) else { return String::new() };
        rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
    }

    /// 删掉一本书在所有模式下的产物（生成记录里记着的，包括还没删掉的旧位置），并从记录里去掉。
    /// 设备没接上、删不掉的记进 `orphans`，下次接上时删（[`Library::flush_removed`]）。
    pub(crate) fn remove_outputs(&self, id: &str) -> Result<(), String> {
        for (dev_id, sp) in self.state_files() {
            let state = self.states.get(&sp);
            let Some(entry) = state.books.get(id).cloned() else { continue };
            let mut state = (*state).clone();
            state.books.remove(id);
            let t = self.registry.get(&dev_id).map(|p| self.target(p));
            let target = t.as_deref().and_then(|r| r.as_ref().ok());
            let left = state.delete_placements(entry.placements(), target, &self.device_env.mtp_base);
            state.orphans.extend(left);
            self.states.put(&sp, state)?;
        }
        Ok(())
    }

    /// 删了的书留在设备上的产物（当时设备没接上）：设备接上了就删。返回删掉几个。
    pub fn flush_removed(&self, device: &Profile) -> Result<usize, String> {
        let sp = self.state_path(&device.id);
        let state = self.states.get(&sp);
        if state.orphans.is_empty() {
            return Ok(0);
        }
        let t = self.target(device);
        let Ok(target) = &*t else { return Ok(0) };
        let mut state = (*state).clone();
        let before = state.orphans.len();
        let list = std::mem::take(&mut state.orphans);
        state.orphans = state.delete_placements(list, Some(target), &self.device_env.mtp_base);
        let n = before - state.orphans.len();
        if n > 0 {
            self.states.put(&sp, state)?;
        }
        Ok(n)
    }

    /// 这本书在各模式下的产物，以及是否最新。Move 上的书只看记录（不联网查），路径显示成 `xochitl:文件夹/文件名`。
    pub fn outputs(&self, meta: &Meta) -> Vec<OutputStatus> {
        let mut out = Vec::new();
        for (dev_id, sp) in self.state_files() {
            let Some(entry) = self.states.get(&sp).books.get(&meta.id).cloned() else { continue };
            let fresh = if entry.uuid.is_empty() && !entry.path.is_file() {
                None
            } else {
                self.registry.get(&dev_id).and_then(|p| self.plan(meta, p).ok()).map(|plan| plan.fingerprint == entry.fingerprint || plan.legacy == entry.fingerprint)
            };
            out.push(OutputStatus { device: dev_id, path: entry.shown(), fresh });
        }
        out
    }

    /// 为一个阅读模式生成一本书的产物并送到设备上，做完才返回（没变化就跳过，`force` 强制重建）。放在哪见模块注释。
    /// 设备没接上返回错误（`sync` 先用 [`Library::device_status`] 看过，没接上的模式整个跳过）。
    /// `sync` 不用它：先比较、生成（[`Library::prepare`]），传设备交给传输线程（[`crate::Pipeline`]），传完再记（[`Library::complete`]）。
    pub fn build(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Built, String> {
        match self.prepare(meta, device, force)? {
            Step::Done(b) => Ok(b),
            Step::Transfer(t) => {
                let r = t.run();
                self.complete(*t, r)
            }
        }
    }

    /// 先比较、再生成：没变化的、设备上已有一样的直接给出结果（[`Step::Done`]）；要传的生成好、交回一件 [`Transfer`]
    /// （放上设备是别的线程的事，传完调 [`Library::complete`] 记下来）。
    pub fn prepare(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Step, String> {
        // 原件的大小或修改时间变了：先核对内容（改过了就停下来），不能因为指纹没变就说"已是最新"。
        // 原件不在了的不在这里报：产物还是那本书的，照旧算最新（`sync`/`list` 会报原件不在）
        if self.original_state(meta) == crate::OriginalState::Touched {
            self.verified_original(meta)?;
        }
        let plan = self.plan(meta, device)?;
        // 指纹写法改了（2026-10-08 起文字书、漫画分开算）、规则没变的：记录改成新写法，不重新生成
        let sp = self.state_path(&device.id);
        let prev = self.states.get(&sp).books.get(&meta.id).cloned();
        if let Some(mut p) = prev.filter(|p| !p.fingerprint.is_empty() && p.fingerprint != plan.fingerprint && p.fingerprint == plan.legacy) {
            p.fingerprint = plan.fingerprint.clone();
            self.put_entry(&sp, meta, p)?;
        }
        let t = self.target(device);
        let target = t.as_ref().as_ref().map_err(|e| format!("{} {e}", device.id))?;
        match target {
            Target::Xochitl(_) => self.prepare_xochitl(meta, device, target, plan, force),
            _ => self.prepare_file(meta, device, target, plan, force),
        }
    }

    /// 生成产物，放在书库临时目录 `.tmp-<id>-<模式>/` 里：(临时目录, 产物, 它的 SHA-256, 质量门警告)。失败时临时目录删掉。
    fn product(&self, meta: &Meta, device: &Profile, format: Format) -> Result<(PathBuf, PathBuf, String, Vec<String>), String> {
        let tmp = self.root.join(format!("{TMP_PREFIX}{}-{}", meta.id, device.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let src = tmp.join(format!("out.{}", format.ext()));
        let made = self.produce(meta, device, format, &src, &tmp).and_then(|w| Ok((crate::fsutil::sha256_file(&src)?, w)));
        match made {
            Ok((sha, w)) => Ok((tmp, src, sha, w)),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                Err(e)
            }
        }
    }

    /// 产物是文件（电脑上、MTP 设备上）。
    fn prepare_file(&self, meta: &Meta, device: &Profile, target: &Target, plan: Plan, force: bool) -> Result<Step, String> {
        let Plan { fingerprint, format, .. } = plan;
        let (root, dir) = self.output_dir(meta, device, target)?;
        let sp = self.state_path(&device.id);
        let state = self.states.get(&sp);
        let prev = state.books.get(&meta.id).cloned();
        // 记录里这本书的产物以前放在电脑上、用户自己拷上设备的（2026-10-07 以前的做法）：设备上同一子目录里叫那个名字的文件
        // 就是那份拷贝，当成本书的（覆盖它），不另起名字、不留重复
        let copied = prev.as_ref().filter(|p| p.uuid.is_empty() && !p.root.as_os_str().is_empty() && p.root != root).and_then(|p| {
            let rel = p.path.strip_prefix(&p.root).ok()?;
            (dir.strip_prefix(&root).ok()? == rel.parent()?).then(|| rel.file_name()?.to_str().map(str::to_string)).flatten()
        });
        let out = dir.join(state.file_name_for(meta, &dir, format.ext(), None, copied.as_deref())?);
        drop(state);
        let unchanged = |p: &&StateEntry| !force && p.fingerprint == fingerprint && p.uuid.is_empty();
        let done = prev.as_ref().filter(unchanged).filter(|p| p.path.is_file());
        if let Some(p) = done.filter(|p| p.path == out) {
            if !p.old.is_empty() {
                self.finish(&sp, meta, p.clone(), Some(target))?; // 上次没删掉的旧位置
            }
            return Ok(Step::Done(Built::UpToDate(out)));
        }
        // 先比较：指纹没变、记录里的位置空了（以前放电脑上、用户自己挪上了设备），设备上对应的位置就是那份 → 认领，不重新生成
        if let Some(p) = prev.as_ref().filter(unchanged).filter(|_| done.is_none() && copied.is_some() && out.is_file()) {
            let old = p.placements().into_iter().filter(|x| x.path != out).collect();
            let entry = StateEntry { path: out.clone(), root, fingerprint, sha: String::new(), uuid: String::new(), name: String::new(), old };
            self.finish(&sp, meta, entry, Some(target))?;
            return Ok(Step::Done(Built::Adopted(out)));
        }
        // 来源：内容没变、只是位置变了的已有产物（挪过去，不重新生成），或者新生成的
        let (tmp, src, sha, warnings) = match done {
            Some(p) => (None, p.path.clone(), if p.sha.is_empty() { crate::fsutil::sha256_file(&p.path)? } else { p.sha.clone() }, Vec::new()),
            None => self.product(meta, device, format).map(|(t, s, h, w)| (Some(t), s, h, w))?,
        };
        let cleanup = |r: Result<Step, String>| {
            if r.is_err() {
                if let Some(t) = &tmp {
                    let _ = std::fs::remove_dir_all(t);
                }
            }
            r
        };
        cleanup((|| {
            // 目录里已有一个不认识的同名文件、和这份逐字节相同（以前手工拷上去的）：认领它，不另起名字
            let out = dir.join(self.states.get(&sp).file_name_for(meta, &dir, format.ext(), Some(&src), copied.as_deref())?);
            // 先把记录改成新位置（指纹留空＝没完成），旧位置记进待删：中途被打断的话，下次还认得新位置上的文件是这本书的
            let mut entry = StateEntry { path: out.clone(), root, fingerprint: String::new(), sha: String::new(), uuid: String::new(), name: String::new(), old: Vec::new() };
            // 设备上这个位置放的就是我们上次传的、和这次逐字节相同（看记录的哈希和文件大小，不把设备上的文件读回来）
            let mut same = false;
            if let Some(p) = &prev {
                entry.old = p.placements().into_iter().filter(|x| x.path != out || !x.uuid.is_empty()).collect();
                if p.path == out && p.uuid.is_empty() {
                    entry.fingerprint = p.fingerprint.clone(); // 同一位置重建：记录不用预先改
                    entry.sha = p.sha.clone();
                    same = p.sha == sha && std::fs::metadata(&src).ok().map(|m| m.len()) == std::fs::metadata(&out).ok().map(|m| m.len());
                }
            }
            if entry.fingerprint.is_empty() || entry.old != prev.as_ref().map_or(Vec::new(), |p| p.old.clone()) {
                self.put_entry(&sp, meta, entry.clone())?;
            }
            // 记录里没有哈希（不是我们传的、或旧记录）时才把设备上的读回来比
            let compare = entry.sha.is_empty();
            entry.fingerprint = fingerprint.clone();
            let t = Transfer { device: device.id.clone(), title: meta.title.clone(), book: meta.id.clone(), sp: sp.clone(), tmp: tmp.clone(), src: src.clone(), sha: sha.clone(), warnings, from: done.map(|p| p.path.clone()), entry, work: Work::File { out, compare } };
            if same {
                // 和设备上的一样：不用传
                return self.complete(t, Ok(Done::File(false))).map(Step::Done);
            }
            Ok(Step::Transfer(Box::new(t)))
        })())
    }

    /// Move（xochitl）：经书架服务的导入接口加入，同一文件夹里的同一本原地替换（保留 uuid 和阅读进度）。
    /// 文件夹或书名变了：加入一本新的，旧的进回收站。重新生成出来和上次传的逐字节相同就不再传（不让 xochitl 白重排）。
    fn prepare_xochitl(&self, meta: &Meta, device: &Profile, target: &Target, plan: Plan, force: bool) -> Result<Step, String> {
        let Target::Xochitl(x) = target else { unreachable!("只给 xochitl 调") };
        let Plan { fingerprint, format, .. } = plan;
        if format != Format::Epub {
            return Err(format!("xochitl 只收 EPUB，这个模式出的是 {}", format.ext()));
        }
        let folder = self.xochitl_folder(meta);
        let name = format!("{}.{}", bookconv::util::sanitize_filename(&meta.title, &meta.id), format.ext());
        let loc = if folder.is_empty() { PathBuf::from(&name) } else { Path::new(&folder).join(&name) };
        let sp = self.state_path(&device.id);
        let prev = self.states.get(&sp).books.get(&meta.id).cloned();
        // 同一位置、设备上还在的上一份
        let same_loc = prev.as_ref().filter(|p| !p.uuid.is_empty() && p.path == loc);
        let alive = match same_loc {
            Some(p) => x.present(&p.uuid)?,
            None => false,
        };
        let live = same_loc.filter(|_| alive);
        if let Some(p) = live.filter(|p| !force && p.fingerprint == fingerprint) {
            if !p.old.is_empty() {
                self.finish(&sp, meta, p.clone(), Some(target))?;
            }
            return Ok(Step::Done(Built::UpToDate(p.shown())));
        }
        // 电脑上以前生成的同一份（改成直接送设备以前的产物）：直接传它，不重新生成
        let reusable = prev.as_ref().filter(|p| !force && p.fingerprint == fingerprint && p.uuid.is_empty() && p.path.is_file());
        let (tmp, src, sha, warnings) = match reusable {
            Some(p) => (None, p.path.clone(), if p.sha.is_empty() { crate::fsutil::sha256_file(&p.path)? } else { p.sha.clone() }, Vec::new()),
            None => self.product(meta, device, format).map(|(t, s, h, w)| (Some(t), s, h, w))?,
        };
        let old = prev.as_ref().map_or(Vec::new(), StateEntry::placements);
        let entry = StateEntry { path: loc, root: PathBuf::new(), fingerprint, sha: String::new(), uuid: String::new(), name: String::new(), old };
        let t = Transfer {
            device: device.id.clone(),
            title: meta.title.clone(),
            book: meta.id.clone(),
            sp,
            tmp,
            src,
            sha: sha.clone(),
            warnings,
            from: reusable.map(|p| p.path.clone()),
            entry,
            work: Work::Move { client: x.client(), name, folder, replace: live.map(|p| p.uuid.clone()) },
        };
        // 和上次传上去、设备上还在的那份一样：不用再传
        if let Some(p) = live.filter(|p| !p.sha.is_empty() && p.sha == sha) {
            let doc = deliver::Doc { uuid: p.uuid.clone(), name: p.name.clone() };
            return self.complete(t, Ok(Done::Same(doc))).map(Step::Done);
        }
        Ok(Step::Transfer(Box::new(t)))
    }

    /// 传完了（`r` 是传输线程的结果）：删掉临时目录，写上完成的记录、删掉旧位置（Move 上的进回收站）。传失败的：记录不改
    /// （Kindle、掌阅那边先登记了新位置、指纹留空，下次重做），返回错误。
    pub fn complete(&self, t: Transfer, r: Result<Done, String>) -> Result<Built, String> {
        if let Some(tmp) = &t.tmp {
            let _ = std::fs::remove_dir_all(tmp);
        }
        let done = r?;
        let meta_id = t.book.clone();
        let mut entry = t.entry;
        entry.sha = t.sha;
        let (wrote, doc) = match done {
            Done::File(w) => (w, None),
            Done::Move(d) => (true, Some(d)),
            Done::Same(d) => (false, Some(d)),
        };
        if let Some(d) = doc {
            entry.old.retain(|o| o.uuid != d.uuid);
            entry.uuid = d.uuid;
            entry.name = d.name;
        }
        let path = entry.shown();
        let tg = self.registry.get(&t.device).map(|p| self.target(p));
        let target = tg.as_deref().and_then(|r| r.as_ref().ok());
        self.finish_id(&t.sp, &meta_id, entry, target)?;
        Ok(match t.from {
            Some(from) => Built::Moved { from, to: path },
            None if wrote => Built::Written { path, warnings: t.warnings },
            None => Built::Same { path, warnings: t.warnings },
        })
    }

    /// 生成产物写到 `part`（书库临时目录里）：EPUB 直接是优化结果；KFX、AZW3 是同一份优化结果再转一次（中间文件放 `tmp`）。
    fn produce(&self, meta: &Meta, device: &Profile, format: Format, part: &Path, tmp: &Path) -> Result<Vec<String>, String> {
        let epub = self.prepared_input(meta)?;
        let mut warnings = Vec::new();
        let opts = bookconv::optimize::OptimizeOpts::for_profile(device);
        // 要转 AZW3、KFX 的，优化结果先放临时目录
        let optimized = if format == Format::Epub { part.to_path_buf() } else { tmp.join("optimized.epub") };
        bookconv::optimize::optimize_epub_file_streaming(&epub, &optimized, &opts, |_, _| {})?;
        let rep = bookconv::check::check_epub_file(&optimized)?;
        if !rep.ok {
            warnings.extend(rep.errors.iter().map(|e| format!("质量门未过：{e}")));
        }
        if format != Format::Epub {
            // 按文件读（不先整本读进内存）；BufReader：zip 按条目小块读
            let open = || std::fs::File::open(&optimized).map(std::io::BufReader::new).map_err(|e| format!("读 {}: {e}", optimized.display()));
            // 唯一 ID 取自书的 id（AZW3 再加入库时间）：重建出来还是"同一本书"，Kindle 上的阅读进度不丢
            let (bytes, w) = if format == Format::Kfx {
                kfx::write::epub_to_kfx_from(open()?, &kfx::write::Opts { fixed_id: kfx_id(&meta.id), media: Some(kfx::css::MediaEnv::for_profile(device)) })?
            } else {
                let uid = meta.id.get(..8).and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or(0);
                let aopts = azw3::Opts { fixed_id: Some((uid, meta.added as u32)), ..Default::default() };
                azw3::epub_to_azw3_from(open()?, &aopts)?
            };
            warnings.extend(w);
            std::fs::write(part, &bytes).map_err(|e| format!("写 {}: {e}", part.display()))?;
        }
        Ok(warnings)
    }
    /// 要优化的 EPUB：原件（EPUB）或当场转换的（CBZ），再补上书里没有、书库里有找来的封面、简介、标签。
    /// 和上一次是同一本书、同样的补充时，直接用上次的中间文件。
    fn prepared_input(&self, meta: &Meta) -> Result<PathBuf, String> {
        let input = self.content_path(meta)?;
        let cover = meta.cover.as_ref().map_or("", |c| c.sha256.as_str());
        let info = meta.info.as_ref().and_then(|i| i.injected_sig()).unwrap_or_default();
        let key = format!("{}|{cover}|{info}", meta.content_sha());
        if let Some(p) = self.prepared.borrow().as_ref().filter(|p| p.key == key) {
            return Ok(p.epub.clone());
        }
        self.prepared.replace(None);
        let dir = self.root.join(format!("{TMP_PREFIX}{}-src", meta.id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut prepared = PreparedInput { key, dir, epub: PathBuf::new() };
        prepared.epub = self.epub_input(meta, &input, &prepared.dir).and_then(|e| crate::metadata::with_additions(self, meta, &e, &prepared.dir))?;
        if prepared.epub == input {
            return Ok(input); // 原件直接用：`prepared` 丢掉时删空目录
        }
        let epub = prepared.epub.clone();
        self.prepared.replace(Some(prepared));
        Ok(epub)
    }

    /// 删掉留着给下一个模式用的中间文件（一轮生成结束时调）。
    pub fn release_prepared(&self) {
        self.prepared.replace(None);
    }

    /// 写一条记录（其余不动）。
    fn put_entry(&self, sp: &Path, meta: &Meta, entry: StateEntry) -> Result<(), String> {
        let mut state = (*self.states.get(sp)).clone();
        state.books.insert(meta.id.clone(), entry);
        std::fs::create_dir_all(self.state_dir()).map_err(|e| format!("{}: {e}", self.state_dir().display()))?;
        self.states.put(sp, state)
    }

    /// 产物已经到位：删掉记着的旧位置（删不掉的留着下次再删），写上完成的记录。记录和原来一样（旧位置还是删不掉，
    /// `sync --watch` 每轮都会走到这里）时不写。
    fn finish(&self, sp: &Path, meta: &Meta, entry: StateEntry, target: Option<&Target>) -> Result<(), String> {
        self.finish_id(sp, &meta.id, entry, target)
    }

    fn finish_id(&self, sp: &Path, id: &str, mut entry: StateEntry, target: Option<&Target>) -> Result<(), String> {
        let mut state = (*self.states.get(sp)).clone();
        let prev = state.books.remove(id);
        entry.old = state.delete_placements(std::mem::take(&mut entry.old), target, &self.device_env.mtp_base);
        if prev.as_ref() == Some(&entry) {
            return Ok(());
        }
        state.books.insert(id.to_string(), entry);
        std::fs::create_dir_all(self.state_dir()).map_err(|e| format!("{}: {e}", self.state_dir().display()))?;
        self.states.put(sp, state)
    }

    /// 要优化的 EPUB：EPUB 直接用；CBZ 当场转换，写进 `tmp`。
    pub(crate) fn epub_input(&self, meta: &Meta, input: &Path, tmp: &Path) -> Result<PathBuf, String> {
        match meta.content_format() {
            "epub" => Ok(input.to_path_buf()),
            "cbz" => {
                // 按文件读（不先把整个 CBZ 读进内存）
                let file = std::fs::File::open(input).map(std::io::BufReader::new).map_err(|e| format!("读 {}: {e}", input.display()))?;
                let bytes = crate::convert_to_epub("cbz", file, &meta.title)?;
                let p = tmp.join("master.epub");
                std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
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
struct StateEntry {
    /// 产物的绝对路径。
    path: PathBuf,
    /// 产物根目录（`D/../<模式>` 或 `<书库>/output/<模式>`）：删旧文件后，只在它以内删变空的目录。
    root: PathBuf,
    /// 空 = 这个位置的产物还没生成完（换位置时先登记，见模块注释）。
    fingerprint: String,
    /// 放上去的产物的 SHA-256：下次重新生成出一样的就不再传（不用把设备上的文件读回来比）。旧记录没有。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sha: String,
    /// 在 Move 的 xochitl 书库里：这本的 uuid 和显示名（`.metadata` 的 `visibleName`）。这时 `path` 是相对的
    /// `文件夹/文件名`（只用来认位置变没变），`root` 为空。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    uuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    /// 以前的位置、还没删掉的产物。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    old: Vec<Placement>,
}

/// 一个产物文件和它的根目录；或者 Move 上的一本（`uuid`、`name`）。
#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Placement {
    path: PathBuf,
    root: PathBuf,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    uuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
}

impl StateEntry {
    /// 这条记录名下的所有产物：现在的位置和待删的旧位置。
    fn placements(&self) -> Vec<Placement> {
        let mut v = vec![Placement { path: self.path.clone(), root: self.root.clone(), uuid: self.uuid.clone(), name: self.name.clone() }];
        v.extend(self.old.iter().cloned());
        v
    }

    /// 给人看的位置：文件的绝对路径；Move 上的是 `xochitl:文件夹/文件名`。
    fn shown(&self) -> PathBuf {
        if self.uuid.is_empty() { self.path.clone() } else { PathBuf::from(format!("xochitl:{}", self.path.display())) }
    }
}

impl State {
    /// 这个产物现在（或待删的旧位置里）是不是记在别的书名下。Move 上的按 uuid 认，文件按路径认。
    fn claimed(&self, p: &Placement) -> bool {
        let same = |path: &Path, uuid: &str| if p.uuid.is_empty() { uuid.is_empty() && path == p.path } else { uuid == p.uuid };
        self.books.values().any(|e| same(&e.path, &e.uuid) || e.old.iter().any(|o| same(&o.path, &o.uuid)))
    }

    /// 删掉这些产物（已经不在记录里的书名下），文件在各自的根目录以内删掉变空的目录。返回删不掉的（下次再删）：
    /// Move 上的要连着 Move（`target`）才删得了（进 xochitl 回收站）；MTP 设备（`mtp_base` 下）上的文件，设备没挂上时不当成已删。
    /// 调用前要先把本书的记录从 `self` 里拿掉，别的书还记着的不删。
    fn delete_placements(&self, list: Vec<Placement>, target: Option<&Target>, mtp_base: &Path) -> Vec<Placement> {
        let mut left = Vec::new();
        for p in list {
            if self.claimed(&p) {
                continue;
            }
            if !p.uuid.is_empty() {
                match target {
                    Some(Target::Xochitl(x)) if x.trash(&p.uuid, &p.name).is_ok() => {}
                    _ => left.push(p),
                }
                continue;
            }
            if !p.path.is_absolute() || (p.path.starts_with(mtp_base) && !p.root.is_dir()) {
                left.push(p);
                continue;
            }
            match std::fs::remove_file(&p.path) {
                Ok(()) => {
                    let _ = crate::fsutil::sync_parent(&p.path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    left.push(p);
                    continue;
                }
            }
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
    fn file_name_for(&self, meta: &Meta, dir: &Path, ext: &str, same_as: Option<&Path>, copied: Option<&str>) -> Result<String, String> {
        let base = bookconv::util::sanitize_filename(&meta.title, &meta.id);
        let plain = format!("{base}.{ext}");
        let suffixed = format!("{base} [{}].{ext}", meta.id.get(..6).unwrap_or(&meta.id));
        if let Some(e) = self.books.get(&meta.id).filter(|e| e.path.parent() == Some(dir)) {
            if let Some(name) = e.path.file_name().and_then(|n| n.to_str()).filter(|n| *n == plain || *n == suffixed) {
                return Ok(name.to_string());
            }
        }
        let fold = |n: &str| n.to_lowercase();
        let existing: BTreeSet<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| fold(&e.file_name().to_string_lossy())).collect();
        let others: BTreeSet<String> = self
            .books
            .iter()
            .filter(|(id, _)| *id != &meta.id)
            .flat_map(|(_, e)| e.placements())
            .filter(|p| p.path.parent() == Some(dir))
            .filter_map(|p| p.path.file_name().map(|n| fold(&n.to_string_lossy())))
            .collect();
        // 本书待删的旧文件正好叫这个名字（书名改回去了）：算本书的，可以用
        let mine: BTreeSet<String> = self
            .books
            .get(&meta.id)
            .into_iter()
            .flat_map(StateEntry::placements)
            .filter(|p| p.path.parent() == Some(dir))
            .filter_map(|p| p.path.file_name().map(|n| fold(&n.to_string_lossy())))
            .collect();
        let adopt = |n: &str| copied == Some(n) || same_as.is_some_and(|s| deliver::same_bytes(s, &dir.join(n)));
        let free = |n: &str| !others.contains(&fold(n)) && (!existing.contains(&fold(n)) || mine.contains(&fold(n)) || adopt(n));
        if free(&plain) {
            Ok(plain)
        } else if free(&suffixed) {
            Ok(suffixed)
        } else {
            Err(format!("{} 里已经有 {plain} 和 {suffixed}，都不是这本书的产物，不覆盖", dir.display()))
        }
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
mod kfx_id_tests {
    #[test]
    fn book_id_is_parsed_whole() {
        assert_eq!(super::kfx_id("0123456789ab"), Some(0x0123_4567_89ab));
        assert_eq!(super::kfx_id(""), None);
        assert_eq!(super::kfx_id("xyz"), None);
        assert_eq!(super::kfx_id("0123456789abcdef0"), None);
    }
}

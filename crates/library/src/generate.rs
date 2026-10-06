//! 按阅读模式（设备 profile）生成产物：生成计划与指纹、产物放在哪、跳过没变的、生成记录。
//!
//! 产物位置（每个模式一份，`<名>` 见 [`State::file_name_for`]）：
//! - 原件在跟踪目录 `D` 里：`D/../<模式 id>/<原件所在目录相对 D 的路径>/<名>.<扩展名>`，和 `D` 并列、镜像子目录。
//!   例：跟踪 `~/Documents/ereader/books`，原件 `books/haodoo/x.epub` → `~/Documents/ereader/kindle/haodoo/<书名>.kfx`；
//! - `add` 进来的单个文件（不在跟踪目录里）、网址书：`<书库>/output/<模式 id>/<名>.<扩展名>`。
//!
//! 产物格式按模式：EPUB 直接是优化结果；KFX、AZW3（Kindle）是同一份优化结果再转一次（`kfx`、`azw3` crate）。
//! profile 可以给漫画另配格式（`comic_format`，见 [`Library::output_format`]）；内置模式都不用（kindle 文字书、漫画都出 KFX）。
//!
//! 生成记录 `<书库>/output-state/<模式 id>.json`：书 id → 产物绝对路径、产物根目录、指纹。产物位置变了（原件移动、
//! 改名换了目录，书名改了）时删掉旧位置的文件——**只删记录里记着的文件**，不认识的文件一概不动；删完顺带删掉
//! 产物根目录以内变空的目录。内容没变、只是位置变了的，把已有产物挪过去，不重新生成。
//!
//! 换位置时先把记录改成新位置（指纹留空＝没完成，旧位置记进 `old` 待删），产物写好再补上指纹、删旧文件：
//! 中途被打断的话，下次生成还认得新位置上的文件是这本书的（不会因为"有个不认识的同名文件"而改名），旧文件也还会删。

use crate::fsutil::{tmp_sibling, TMP_PREFIX};
use crate::{Library, Meta};
use profile::{Format, Profile};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 生成流程本身（本 crate 的步骤、参数，以及生成时当场做的格式转换）的版本：改了会影响产物的地方要加一，
/// 旧产物随之判为过期。优化器、格式转换各有自己的版本号，也都进指纹。产物放在哪不影响产物内容，不进指纹。
const PIPELINE_VERSION: &str = "5";

#[derive(Debug)]
pub enum Built {
    Written { path: PathBuf, warnings: Vec<String> },
    UpToDate(PathBuf),
    /// 内容没变、只是位置变了（原件移动改名、书名改了）：已有产物挪了过去，没有重新生成。
    Moved { from: PathBuf, to: PathBuf },
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
        // 补元数据那一步（`metadata::inject`）改了会影响产物时 `bookconv::opfmeta::VERSION` 加一：只让补过东西的书过期（`i4`），
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
        // 阅读范围后面带上漫画白边（`+1`，profile 的 comic_margin，2026-09-29 起），改了白边的书都要重新生成；
        // 漫画阅读范围和阅读器页边距（comic_readable、comic_reader_margins）
        // 跟在后面（`c952x1457m1`），和 EPUB 阅读范围一样时不写
        let comic = device.comic_readable();
        let comic_seg = match (comic != area, device.comic_reader_margins) {
            (false, None) => String::new(),
            (_, m) => format!("c{}x{}{}", comic.width, comic.height, m.map(|m| format!("m{m}")).unwrap_or_default()),
        };
        // 漫画翻页方向改写（profile 的 comic_page_direction，如 `dltr`）
        let comic_seg = match &device.comic_page_direction {
            Some(d) => format!("{comic_seg}d{d}"),
            None => comic_seg,
        };
        // 漫画写成固定版式（profile 的 comic_fixed_layout）
        let comic_seg = if device.comic_fixed_layout { format!("{comic_seg}f") } else { comic_seg };
        // 保留背景图（profile 的 background_images）
        let comic_seg = match (device.background_images, device.background_sizing) {
            (true, true) => format!("{comic_seg}b"),
            (true, false) => format!("{comic_seg}bn"),
            _ => comic_seg,
        };
        let fingerprint = format!(
            "{}|{cover}|{info}|{pipeline}|{}|{notes}|{}|{}x{}+{}{comic_seg}|{}|{}",
            meta.content_sha(),
            bookconv::optimize::OPTIMIZE_VERSION,
            device.id,
            area.width,
            area.height,
            device.comic_margin,
            if device.color { "color" } else { "gray" },
            format_seg,
        );
        Ok(Plan { fingerprint, format })
    }

    /// 这本书在这个模式下的产物格式：profile 给漫画另配了格式（`comic_format`）时要先判断是不是漫画
    /// （同优化器的判定，按内容哈希缓存）。
    pub(crate) fn output_format(&self, meta: &Meta, device: &Profile) -> Result<Format, String> {
        if device.format_for(true) == device.format_for(false) {
            return Ok(device.format());
        }
        let sha = meta.content_sha().to_string();
        let cached = self.comic.borrow().get(&sha).copied();
        let comic = match cached {
            Some(c) => c,
            None => {
                let c = self.is_comic(meta)?;
                self.comic.borrow_mut().insert(sha, c);
                c
            }
        };
        Ok(device.format_for(comic))
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

    /// 生成记录里产物所在的所有目录（拿到锁时清理残留临时文件用）。
    pub(crate) fn output_dirs(&self) -> BTreeSet<PathBuf> {
        let mut dirs = BTreeSet::new();
        for (_, sp) in self.state_files() {
            for e in self.states.get(&sp).books.values() {
                dirs.extend(e.placements().into_iter().filter_map(|p| p.path.parent().map(Path::to_path_buf)));
            }
        }
        dirs
    }

    /// 这本书在该模式下的产物根目录和产物所在目录（绝对路径），见模块注释。
    fn output_dir(&self, meta: &Meta, device: &Profile) -> Result<(PathBuf, PathBuf), String> {
        let lib_root = std::path::absolute(&self.root).unwrap_or_else(|_| self.root.clone()).join("output").join(&device.id);
        if meta.source() == crate::Source::Stored || meta.source_path.is_empty() {
            return Ok((lib_root.clone(), lib_root));
        }
        let src = Path::new(&meta.source_path);
        let sources = self.sources_json.get(&self.sources_path());
        let Some((d, parent)) = sources.dirs().iter().find(|d| src.starts_with(d)).and_then(|d| Some((d, d.parent()?))) else {
            return Ok((lib_root.clone(), lib_root));
        };
        let root = parent.join(&device.id);
        if let Some(t) = sources.dirs().iter().find(|t| crate::sources::overlaps(&root, t)) {
            return Err(output_overlap(&root, t));
        }
        let rel = src.parent().and_then(|p| p.strip_prefix(d).ok()).unwrap_or(Path::new(""));
        let dir = root.join(rel);
        Ok((root, dir))
    }

    /// 删掉一本书在所有模式下的产物（生成记录里记着的文件，包括还没删掉的旧位置），并从记录里去掉。
    pub(crate) fn remove_outputs(&self, id: &str) -> Result<(), String> {
        for (_, sp) in self.state_files() {
            let state = self.states.get(&sp);
            let Some(entry) = state.books.get(id).cloned() else { continue };
            let mut state = (*state).clone();
            state.books.remove(id);
            let left = state.delete_placements(entry.placements());
            if !left.is_empty() {
                return Err(format!("删产物失败：{}", left.iter().map(|p| p.path.display().to_string()).collect::<Vec<_>>().join("、")));
            }
            self.states.put(&sp, state)?;
        }
        Ok(())
    }

    /// 这本书在各模式下的产物，以及是否最新。
    pub fn outputs(&self, meta: &Meta) -> Vec<OutputStatus> {
        let mut out = Vec::new();
        for (dev_id, sp) in self.state_files() {
            let Some(entry) = self.states.get(&sp).books.get(&meta.id).cloned() else { continue };
            let fresh = if !entry.path.is_file() {
                None
            } else {
                self.registry.get(&dev_id).and_then(|p| self.plan(meta, p).ok()).map(|plan| plan.fingerprint == entry.fingerprint)
            };
            out.push(OutputStatus { device: dev_id, path: entry.path, fresh });
        }
        out
    }

    /// 为一个阅读模式生成一本书的产物（没变化就跳过，`force` 强制重建）。放在哪见模块注释。
    pub fn build(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Built, String> {
        // 原件的大小或修改时间变了：先核对内容（改过了就停下来），不能因为指纹没变就说"已是最新"。
        // 原件不在了的不在这里报：产物还是那本书的，照旧算最新（`sync`/`list` 会报原件不在）
        if self.original_state(meta) == crate::OriginalState::Touched {
            self.verified_original(meta)?;
        }
        let Plan { fingerprint, format } = self.plan(meta, device)?;
        let (root, dir) = self.output_dir(meta, device)?;
        let sp = self.state_path(&device.id);
        let state = self.states.get(&sp);
        let out = dir.join(state.file_name_for(meta, &dir, format.ext())?);
        let prev = state.books.get(&meta.id).cloned();
        drop(state);
        let done = prev.as_ref().filter(|p| !force && p.fingerprint == fingerprint && p.path.is_file());
        if let Some(p) = done.filter(|p| p.path == out) {
            if !p.old.is_empty() {
                self.finish(&sp, meta, p.clone())?; // 上次没删掉的旧位置
            }
            return Ok(Built::UpToDate(out));
        }
        // 先把记录改成新位置（指纹留空＝没完成），旧位置记进待删
        let mut entry = StateEntry { path: out.clone(), root, fingerprint: String::new(), old: Vec::new() };
        if let Some(p) = &prev {
            entry.old = p.placements().into_iter().filter(|x| x.path != out).collect();
            if p.path == out {
                entry.fingerprint = p.fingerprint.clone(); // 同一位置重建：记录不用预先改
            }
        }
        if entry.fingerprint.is_empty() || entry.old != prev.as_ref().map_or(Vec::new(), |p| p.old.clone()) {
            self.put_entry(&sp, meta, entry.clone())?;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        // 内容没变、只是位置变了：挪过去（跨文件系统挪不了就重新生成）
        if let Some(p) = done {
            if !out.exists() && std::fs::rename(&p.path, &out).is_ok() {
                let _ = crate::fsutil::sync_parent(&out);
                entry.fingerprint = fingerprint;
                self.finish(&sp, meta, entry)?;
                return Ok(Built::Moved { from: p.path.clone(), to: out });
            }
        }

        // 要优化的 EPUB（与模式无关，见 [`PreparedInput`]）；AZW3 的中间产物放在书库的临时目录里；
        // 产物直接写成目标旁边的临时文件，过了质量门、落盘后改名到位（中途失败不会留下半个产物，也不会覆盖掉上一版）
        let epub = self.prepared_input(meta)?;
        let tmp = self.root.join(format!("{TMP_PREFIX}{}-{}", meta.id, device.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let result = bookconv::util::produce_then_replace(&tmp_sibling(&out), &out, |part| {
            let part = part.to_path_buf();
            let mut warnings = Vec::new();
            let opts = bookconv::optimize::OptimizeOpts::for_profile(device);
            // 要转 AZW3、KFX 的，优化结果先放临时目录
            let optimized = if format == Format::Epub { part.clone() } else { tmp.join("optimized.epub") };
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
                std::fs::write(&part, &bytes).map_err(|e| format!("写 {}: {e}", part.display()))?;
            }
            Ok(warnings)
        });
        let _ = std::fs::remove_dir_all(&tmp);
        let warnings = result?;
        entry.fingerprint = fingerprint;
        self.finish(&sp, meta, entry)?;
        Ok(Built::Written { path: out, warnings })
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
    fn finish(&self, sp: &Path, meta: &Meta, mut entry: StateEntry) -> Result<(), String> {
        let mut state = (*self.states.get(sp)).clone();
        let prev = state.books.remove(&meta.id);
        entry.old = state.delete_placements(std::mem::take(&mut entry.old));
        if prev.as_ref() == Some(&entry) {
            return Ok(());
        }
        state.books.insert(meta.id.clone(), entry);
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

/// 产物根目录和跟踪目录互相包含时的提示（`build` 和 `track` 共用）。
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
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct StateEntry {
    /// 产物的绝对路径。
    path: PathBuf,
    /// 产物根目录（`D/../<模式>` 或 `<书库>/output/<模式>`）：删旧文件后，只在它以内删变空的目录。
    root: PathBuf,
    /// 空 = 这个位置的产物还没生成完（换位置时先登记，见模块注释）。
    fingerprint: String,
    /// 以前的位置、还没删掉的产物。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    old: Vec<Placement>,
}

/// 一个产物文件和它的根目录。
#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Placement {
    path: PathBuf,
    root: PathBuf,
}

impl StateEntry {
    /// 这条记录名下的所有文件：现在的位置和待删的旧位置。
    fn placements(&self) -> Vec<Placement> {
        let mut v = vec![Placement { path: self.path.clone(), root: self.root.clone() }];
        v.extend(self.old.iter().cloned());
        v
    }
}

impl State {
    /// 这个路径现在（或待删的旧位置里）是不是记在别的书名下。
    fn claimed(&self, path: &Path) -> bool {
        self.books.values().any(|e| e.path == path || e.old.iter().any(|o| o.path == path))
    }

    /// 删掉这些文件（已经不在记录里的书名下），并在各自的根目录以内删掉变空的目录。返回删不掉的（下次再删）。
    /// 调用前要先把本书的记录从 `self` 里拿掉，别的书还记着的路径不删。
    fn delete_placements(&self, list: Vec<Placement>) -> Vec<Placement> {
        let mut left = Vec::new();
        for p in list {
            if self.claimed(&p.path) {
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
    fn file_name_for(&self, meta: &Meta, dir: &Path, ext: &str) -> Result<String, String> {
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
        let free = |n: &str| !others.contains(&fold(n)) && (!existing.contains(&fold(n)) || mine.contains(&fold(n)));
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

//! 按设备生成产物：生成计划与指纹、跳过没变的、产物目录里的生成记录。

use crate::fsutil::write_atomic;
use crate::{Library, Meta, Source};
use profile::{Format, Profile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 生成流程本身（本 crate 的步骤、参数，以及生成时当场做的格式转换）的版本：改了会影响产物的地方要加一，
/// 旧产物随之判为过期。优化器、AZW3 写出器各有自己的版本号，也都进指纹。
const PIPELINE_VERSION: &str = "5";

#[derive(Debug)]
pub enum Built {
    Written { path: PathBuf, warnings: Vec<String> },
    UpToDate(PathBuf),
}

/// 一本书在某设备下的产物状态（`list` 用）。
pub struct OutputStatus {
    pub device: String,
    pub path: PathBuf,
    /// `Some(true)` 最新；`Some(false)` 过期（原件或处理规则变了）；`None` 判断不了（设备 profile 已删、文件丢了）。
    pub fresh: Option<bool>,
}

/// 生成计划：产物格式、阅读范围、指纹。
struct Plan {
    format: Format,
    area: profile::Screen,
    fingerprint: String,
}

impl Library {
    fn plan(&self, meta: &Meta, device: &Profile) -> Result<Plan, String> {
        if meta.content_sha().is_empty() {
            return Err("条目缺内容哈希（早期版本入库），先运行 booklib dedupe 迁移".into());
        }
        let is_pdf = match meta.source() {
            Source::Original => meta.source_format == "pdf",
            Source::Stored => meta.master.ends_with(".pdf"),
        };
        let format = if is_pdf && meta.pdf_text_layer != Some(true) {
            if !device.formats.contains(&Format::Pdf) {
                return Err(format!("图片型 PDF（扫描件/漫画）暂时只能生成给支持 PDF 的设备，{} 不支持", device.name));
            }
            Format::Pdf
        } else {
            device.reflow_format().ok_or_else(|| format!("设备 {} 没有流式格式（EPUB/AZW3）", device.id))?
        };
        let area = device.readable(format);
        let writer = if format == Format::Azw3 { azw3::WRITER_VERSION } else { "-" };
        let cover = meta.cover.as_ref().map_or("-", |c| &c.sha256[..12]);
        let info = meta.info.as_ref().and_then(|i| i.injected_sig()).unwrap_or_else(|| "-".into());
        let fingerprint = format!(
            "{}|{cover}|{info}|{PIPELINE_VERSION}|{}|{writer}|{}|{}x{}|{}|{}",
            meta.content_sha(),
            bookconv::optimize::OPTIMIZE_VERSION,
            device.id,
            area.width,
            area.height,
            if device.color { "color" } else { "gray" },
            format.ext(),
        );
        Ok(Plan { format, area, fingerprint })
    }

    /// 产物目录：`output/`，加上早期版本 `build --out` 直接生成过的目录（记在 `outputs.json`，只读，清理用）。
    fn output_roots(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read(self.root.join("outputs.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let default = self.root.join("output");
        if !v.contains(&default) {
            v.insert(0, default);
        }
        v
    }

    /// 删掉一本书在所有产物目录、所有设备下的产物。
    pub(crate) fn remove_outputs(&self, id: &str) -> Result<(), String> {
        for dev in self.output_roots().iter().filter_map(|r| std::fs::read_dir(r).ok()).flatten().flatten() {
            let mut state = State::load(&dev.path());
            if let Some(entry) = state.books.remove(id) {
                let _ = std::fs::remove_file(dev.path().join(entry.file));
                state.save(&dev.path())?;
            }
        }
        Ok(())
    }

    /// 这本书在所有用过的产物目录、所有设备下的产物，以及是否最新。
    pub fn outputs(&self, meta: &Meta) -> Vec<OutputStatus> {
        let mut out = Vec::new();
        for root in self.output_roots() {
            for dev in std::fs::read_dir(&root).into_iter().flatten().flatten() {
                let Some(dev_id) = dev.file_name().to_str().map(str::to_string) else { continue };
                let Some(entry) = State::load(&dev.path()).books.get(&meta.id).cloned() else { continue };
                let path = dev.path().join(&entry.file);
                let fresh = if !path.exists() {
                    None
                } else {
                    self.registry.get(&dev_id).and_then(|p| self.plan(meta, p).ok()).map(|plan| plan.fingerprint == entry.fingerprint)
                };
                out.push(OutputStatus { device: dev_id, path, fresh });
            }
        }
        out.sort_by(|a, b| a.device.cmp(&b.device).then(a.path.cmp(&b.path)));
        out
    }

    /// 这本书在该设备下已生成的产物和它的指纹（没生成过、文件不在时 `None`）。
    pub(crate) fn built_output(&self, meta: &Meta, device: &Profile) -> Option<(PathBuf, String)> {
        let dir = self.root.join("output").join(&device.id);
        let e = State::load(&dir).books.get(&meta.id).cloned()?;
        let p = dir.join(&e.file);
        p.exists().then_some((p, e.fingerprint))
    }

    /// 为设备生成一本书的产物（没变化就跳过，`force` 强制重建）。产物放在书库的 `output/<设备 id>/`；
    /// 要放到别处（U 盘、阅读器）用 [`Library::deliver`] 拷过去。
    pub fn build(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Built, String> {
        let Plan { format, area, fingerprint } = self.plan(meta, device)?;
        let dir = self.root.join("output").join(&device.id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut state = State::load(&dir);
        let file = state.file_name_for(meta, format.ext(), &dir);
        let out = dir.join(&file);
        if !force && out.exists() && state.books.get(&meta.id).is_some_and(|e| e.fingerprint == fingerprint && e.file == file) {
            return Ok(Built::UpToDate(out));
        }
        // 内容从哪读：书库里存着的，或核对过的原件
        let input = match meta.source() {
            Source::Stored => self.entry_dir(&meta.id).join(&meta.master),
            Source::Original => self.verified_original(meta)?,
        };

        // 所有中间文件都在产物目录下的临时目录里，成品最后一步改名到位（中途失败不会留下半个产物）
        let tmp = dir.join(format!(".tmp-{}", meta.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let result = (|| -> Result<Vec<String>, String> {
            let mut warnings = Vec::new();
            let done = tmp.join("out");
            if format == Format::Pdf {
                bookconv::pdf_ingest::optimize_pdf_trim_only(&input, &done, area, !device.color, |_, _| {})?;
            } else {
                let epub = self.epub_input(meta, &input, &tmp)?;
                // 书里没有的封面、简介、标签，书库里有找来的：补进去
                let epub = crate::metadata::with_additions(self, meta, &epub, &tmp)?;
                let optimized = tmp.join("optimized.epub");
                let opts = bookconv::optimize::OptimizeOpts { wash: Some(Default::default()), grayscale: !device.color, ..bookconv::optimize::OptimizeOpts::new(area) };
                bookconv::optimize::optimize_epub_file_streaming(&epub, &optimized, &opts, |_, _| {})?;
                let rep = bookconv::check::check_epub_file(&optimized)?;
                if !rep.ok {
                    warnings.extend(rep.errors.iter().map(|e| format!("质量门未过：{e}")));
                }
                if format == Format::Azw3 {
                    let epub = std::fs::read(&optimized).map_err(|e| e.to_string())?;
                    // 唯一 ID 取自书的 id、时间取入库时间：重建出来还是"同一本书"，Kindle 上的阅读进度不丢
                    let uid = u32::from_str_radix(&meta.id[..8], 16).unwrap_or(0);
                    let opts = azw3::Opts { fixed_id: Some((uid, meta.added as u32)), ..Default::default() };
                    let (azw3, w) = azw3::epub_to_azw3_with_warnings(&epub, &opts)?;
                    warnings.extend(w);
                    std::fs::write(&done, azw3).map_err(|e| e.to_string())?;
                } else {
                    std::fs::rename(&optimized, &done).map_err(|e| e.to_string())?;
                }
            }
            std::fs::rename(&done, &out).map_err(|e| format!("写 {}: {e}", out.display()))?;
            Ok(warnings)
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        let warnings = result?;
        // 书名变了时，旧文件名的产物删掉
        if let Some(old) = state.books.get(&meta.id) {
            if old.file != file {
                let _ = std::fs::remove_file(dir.join(&old.file));
            }
        }
        state.books.insert(meta.id.clone(), StateEntry { file, fingerprint });
        state.save(&dir)?;
        Ok(Built::Written { path: out, warnings })
    }

    /// 要优化的 EPUB：EPUB 直接用；有文字层的 PDF、MOBI/FB2/CBZ 等当场转换，写进 `tmp`。
    pub(crate) fn epub_input(&self, meta: &Meta, input: &Path, tmp: &Path) -> Result<PathBuf, String> {
        let is_epub = match meta.source() {
            Source::Stored => meta.master.ends_with(".epub"),
            Source::Original => meta.source_format == "epub",
        };
        if is_epub {
            return Ok(input.to_path_buf());
        }
        let bytes = if meta.pdf_text_layer == Some(true) {
            let (mut book, _, css) = bookconv::pdf_ingest::optimize_pdf_to_epub(input, |_, _| {})?;
            bookconv::epub::assemble_pdf_derived(&mut book, &css)?
        } else {
            let data = std::fs::read(input).map_err(|e| format!("读 {}: {e}", input.display()))?;
            crate::convert_to_epub(&meta.source_format, &meta.source, &data, &meta.title)?
        };
        let p = tmp.join("master.epub");
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        Ok(p)
    }
}

/// 某设备产物目录的生成记录：id → (文件名, 指纹)。
#[derive(Default, Serialize, Deserialize)]
struct State {
    books: BTreeMap<String, StateEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StateEntry {
    file: String,
    fingerprint: String,
}

impl State {
    fn load(dir: &Path) -> State {
        std::fs::read(dir.join(".state.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self, dir: &Path) -> Result<(), String> {
        write_atomic(&dir.join(".state.json"), serde_json::to_string_pretty(self).unwrap().as_bytes())
    }

    /// 产物文件名：`书名.ext`。和别的书撞名（不分大小写：U 盘、Kindle 的文件系统不分），或者目录里已有
    /// 一个不是本书产物的同名文件时，加 id 后缀，不覆盖别人的文件。
    fn file_name_for(&self, meta: &Meta, ext: &str, dir: &Path) -> String {
        let base = bookconv::util::sanitize_filename(&meta.title, &meta.id);
        let plain = format!("{base}.{ext}");
        let folded = plain.to_lowercase();
        let ours = self.books.get(&meta.id).is_some_and(|e| e.file == plain);
        let taken = self.books.iter().any(|(id, e)| id != &meta.id && e.file.to_lowercase() == folded) || (!ours && dir.join(&plain).exists());
        if taken { format!("{base} [{}].{ext}", &meta.id[..6]) } else { plain }
    }
}

//! 母版库：书先原样入库成**与设备无关的母版**，再按设备 profile 生成产物（可随时重建）。
//!
//! 目录结构（`root` 缺省 `~/.local/share/booklib`）：
//! ```text
//! masters/<id>/master.epub | master.pdf   母版，入库后不再改动
//! masters/<id>/source.<原扩展名>           原始文件留底（网址入库时是 source.url）
//! masters/<id>/meta.json                   书名、作者、来源、指纹
//! output/<设备 id>/<书名>.<epub|azw3|pdf>  产物；output/<设备 id>/.state.json 记每本书的生成指纹，没变就跳过
//! ```
//! `<id>` 是原始内容 SHA-256 的前 12 位十六进制：同一本书重复入库会认出来。
//!
//! 母版：EPUB 原样；MOBI/AZW/AZW3/PRC/FB2 与网页转成 EPUB；CBZ 转成每页一张图的 EPUB；PDF 保留原件，生成时再处理
//! （有文字层 → 转 EPUB；图片型 → 只给支持 PDF 的设备裁白边）。带 DRM 的书现在拒收（解 DRM 还没做）。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub use profile::{Format, Profile};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    /// 原始文件名（网址入库时是网址）。
    pub source: String,
    /// 原始格式（小写扩展名，网址是 "url"）。
    pub source_format: String,
    /// 母版文件名：`master.epub` 或 `master.pdf`。
    pub master: String,
    /// 入库时间（Unix 秒）。
    pub added: u64,
    /// 母版文件的 SHA-256（判断产物是否过期用）；早期条目没有，第一次用到时补算并写回。
    #[serde(default)]
    pub master_sha256: String,
    /// 入库时原文件的绝对路径（网址入库为空）。只作记录，书库不依赖它。
    #[serde(default)]
    pub source_path: String,
    /// 原文件内容在书库里怎么存的（EPUB/PDF 是 master 本身，要转换的格式是 source.*）。
    #[serde(default)]
    pub storage: Storage,
}

/// 书库里保存原文件内容的方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    /// 独立一份（跨文件系统或不支持共享时）。
    #[default]
    Copy,
    /// 写时复制克隆（btrfs/xfs 等）：和原文件共用磁盘数据，但两边互不影响。
    Reflink,
    /// 硬链接：和原文件是同一个文件（原文件被就地改写时母版也会变，靠 `master_sha256` 能查出来）。
    Hardlink,
}

/// 把 `src` 放到 `dst`：能克隆就克隆，不行就硬链接，再不行才复制。
fn share_file(src: &Path, dst: &Path) -> Result<Storage, String> {
    if reflink_copy::reflink(src, dst).is_ok() {
        return Ok(Storage::Reflink);
    }
    let _ = std::fs::remove_file(dst);
    if std::fs::hard_link(src, dst).is_ok() {
        return Ok(Storage::Hardlink);
    }
    std::fs::copy(src, dst).map_err(|e| format!("复制 {}: {e}", src.display()))?;
    Ok(Storage::Copy)
}

/// `dedupe` 的结果。
#[derive(Default, Debug)]
pub struct DedupeReport {
    /// 改成与外部原件共享存储的文件数与大小。
    pub shared_files: usize,
    pub shared_bytes: u64,
    /// 删掉的多余 source 副本（和 master 内容相同）数与大小。
    pub removed_sources: usize,
    pub removed_bytes: u64,
}

/// 一本书在某设备下的产物状态（`list` 用）。
pub struct OutputStatus {
    pub device: String,
    pub path: PathBuf,
    /// `Some(true)` 最新；`Some(false)` 过期（母版或处理规则变了）；`None` 判断不了（设备 profile 已删、文件丢了）。
    pub fresh: Option<bool>,
}

/// 生成计划：走哪条路、产物格式、指纹。
struct Plan {
    master_path: PathBuf,
    pdf_kind: Option<bookconv::pdf_ingest::PdfKind>,
    format: Format,
    area: profile::Screen,
    ext: &'static str,
    fingerprint: String,
}

pub struct Library {
    root: PathBuf,
}

pub enum Added {
    New(Meta),
    Existing(Meta),
}

pub enum Built {
    Written { path: PathBuf, warnings: Vec<String> },
    UpToDate(PathBuf),
}

fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// EPUB 的书名与作者（OPF `dc:title` / `dc:creator`）。
fn epub_title_authors(epub: &[u8]) -> (String, Vec<String>) {
    let Ok(entries) = bookconv::epubzip::read_entries(epub) else { return (String::new(), Vec::new()) };
    let Some(opf) = bookconv::wash::parse_opf(&entries) else { return (String::new(), Vec::new()) };
    let text = String::from_utf8_lossy(&entries[opf.index].data);
    let grab = |tag: &str| -> Vec<String> {
        regex::Regex::new(&format!(r#"(?s)<dc:{tag}\b[^>]*>(.*?)</dc:{tag}>"#))
            .unwrap()
            .captures_iter(&text)
            .map(|c| bookconv::wash::plain_text(&c[1]))
            .filter(|s| !s.is_empty())
            .collect()
    };
    (grab("title").into_iter().next().unwrap_or_default(), grab("creator"))
}

/// EPUB 带不带真 DRM（正文被加密，或有 Adobe `rights.xml`）。只加密字体的"伪 DRM"不算，优化时会剥掉。
fn epub_drm(epub: &[u8]) -> Option<String> {
    let entries = bookconv::epubzip::read_entries(epub).ok()?;
    if entries.iter().any(|e| e.name == "META-INF/rights.xml") {
        return Some("Adobe DRM（META-INF/rights.xml）".into());
    }
    let targets = bookconv::wash::encrypted_targets(&entries)?;
    let bad = bookconv::wash::real_drm_items(&targets);
    (!bad.is_empty()).then(|| format!("正文被加密（{} 等 {} 项）", bad[0], bad.len()))
}

impl Library {
    /// 缺省位置：`$BOOKLIB_DIR`，否则 `$XDG_DATA_HOME/booklib`，否则 `~/.local/share/booklib`。
    pub fn default_root() -> PathBuf {
        if let Some(d) = std::env::var_os("BOOKLIB_DIR") {
            return PathBuf::from(d);
        }
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
        data.join("booklib")
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Library, String> {
        let root = root.into();
        std::fs::create_dir_all(root.join("masters")).map_err(|e| format!("{}: {e}", root.display()))?;
        Ok(Library { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn master_dir(&self, id: &str) -> PathBuf {
        self.root.join("masters").join(id)
    }

    fn load_meta(&self, id: &str) -> Option<Meta> {
        serde_json::from_slice(&std::fs::read(self.master_dir(id).join("meta.json")).ok()?).ok()
    }

    /// 写母版：先写进临时目录再改名，半路失败不留残缺条目。
    /// 写母版。`master = None` 表示母版就是原文件本身（EPUB/PDF），从 `source` 共享过来，不另存 source 副本；
    /// 否则母版写字节，原文件（有的话）以 `source.<扩展名>` 共享过来。`extra` 是额外的小文件（网址的 source.url）。
    fn store(&self, meta: &mut Meta, master: Option<&[u8]>, source: Option<(&Path, &str)>, extra: &[(&str, &[u8])]) -> Result<(), String> {
        let dir = self.master_dir(&meta.id);
        let tmp = self.root.join("masters").join(format!(".tmp-{}", meta.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let w = |name: &str, data: &[u8]| std::fs::write(tmp.join(name), data).map_err(|e| format!("写 {name}: {e}"));
        match (master, source) {
            (Some(bytes), src) => {
                w(&meta.master, bytes)?;
                if let Some((path, name)) = src {
                    meta.storage = share_file(path, &tmp.join(name))?;
                }
            }
            (None, Some((path, _))) => meta.storage = share_file(path, &tmp.join(&meta.master))?,
            (None, None) => return Err("没有母版内容".into()),
        }
        for (name, data) in extra {
            w(name, data)?;
        }
        w("meta.json", serde_json::to_string_pretty(meta).unwrap().as_bytes())?;
        std::fs::rename(&tmp, &dir).map_err(|e| format!("落库失败: {e}"))
    }

    /// 入库一个文件。
    pub fn add_file(&self, path: &Path) -> Result<Added, String> {
        let data = std::fs::read(path).map_err(|e| format!("读 {}: {e}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("book").to_string();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("book");
        let id = sha256_hex(&data)[..12].to_string();
        if let Some(m) = self.load_meta(&id) {
            return Ok(Added::Existing(m));
        }
        let fallback_title = bookconv::naming::canonical_book_name(stem);
        // `None` = 母版就是原文件（不转换）
        let (master, master_name): (Option<Vec<u8>>, &str) = match ext.as_str() {
            "epub" => {
                if let Some(d) = epub_drm(&data) {
                    return Err(format!("有 DRM：{d}。解 DRM 还没做，暂时不能入库"));
                }
                (None, "master.epub")
            }
            "pdf" => (None, "master.pdf"),
            "cbz" => (Some(bookconv::convert::cbz::cbz_to_epub(&data, &fallback_title)?), "master.epub"),
            "mobi" | "azw" | "azw3" | "prc" | "fb2" => {
                bookconv::convert::precheck(&name, &data)?;
                // 这几种格式转出来的 EPUB 与设备无关（屏幕参数只有 CBZ→PDF 才用），随便给一个。
                let screen = profile::Screen { width: 1, height: 1 };
                let c = bookconv::convert::convert_file(&name, &data, Default::default(), screen).ok_or("不支持的格式")??;
                (Some(c.data), "master.epub")
            }
            _ => return Err(format!("不支持的格式 .{ext}（支持 epub / pdf / cbz / mobi / azw / azw3 / prc / fb2，或网址）")),
        };
        let master_bytes: &[u8] = master.as_deref().unwrap_or(&data);
        let (title, authors) = if master_name == "master.epub" { epub_title_authors(master_bytes) } else { (String::new(), Vec::new()) };
        let mut meta = Meta {
            master_sha256: sha256_hex(master_bytes),
            source_path: std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).display().to_string(),
            storage: Storage::Copy,
            id,
            title: if title.is_empty() { fallback_title } else { title },
            authors,
            source: name,
            source_format: ext.clone(),
            master: master_name.to_string(),
            added: now(),
        };
        let source_name = format!("source.{ext}");
        self.store(&mut meta, master.as_deref(), Some((path, &source_name)), &[])?;
        Ok(Added::New(meta))
    }

    /// 入库一个网址：抓正文组成 EPUB（图片保留原图）。
    pub fn add_url(&self, url: &str) -> Result<Added, String> {
        let id = sha256_hex(url.as_bytes())[..12].to_string();
        if let Some(m) = self.load_meta(&id) {
            return Ok(Added::Existing(m));
        }
        let (epub, title) = bookconv::article::build_article_epub(url)?;
        let mut meta = Meta {
            master_sha256: sha256_hex(&epub),
            source_path: String::new(),
            storage: Storage::Copy,
            id,
            title,
            authors: Vec::new(),
            source: url.to_string(),
            source_format: "url".into(),
            master: "master.epub".into(),
            added: now(),
        };
        self.store(&mut meta, Some(&epub), None, &[("source.url", url.as_bytes())])?;
        Ok(Added::New(meta))
    }

    /// 全部母版，按入库时间排序。
    pub fn list(&self) -> Vec<Meta> {
        let mut v: Vec<Meta> = std::fs::read_dir(self.root.join("masters"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str().filter(|n| !n.starts_with('.')).and_then(|n| self.load_meta(n)))
            .collect();
        v.sort_by(|a, b| a.added.cmp(&b.added).then(a.title.cmp(&b.title)));
        v
    }

    /// 按 id 前缀或书名片段挑书；`selectors` 为空＝全部。
    pub fn select(&self, selectors: &[String]) -> Vec<Meta> {
        let all = self.list();
        if selectors.is_empty() {
            return all;
        }
        all.into_iter().filter(|m| selectors.iter().any(|s| m.id.starts_with(s.as_str()) || m.title.contains(s.as_str()))).collect()
    }

    /// 用过的产物目录（缺省的 `output/` 加上 `build` 时指定过的其它目录），记在 `outputs.json`。
    fn output_roots(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read(self.root.join("outputs.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let default = self.root.join("output");
        if !v.contains(&default) {
            v.insert(0, default);
        }
        v
    }

    fn remember_output_root(&self, out_root: &Path) -> Result<(), String> {
        let mut v = self.output_roots();
        let p = std::fs::canonicalize(out_root).unwrap_or_else(|_| out_root.to_path_buf());
        if v.contains(&p) {
            return Ok(());
        }
        v.push(p);
        std::fs::write(self.root.join("outputs.json"), serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())
    }

    fn save_meta(&self, meta: &Meta) -> Result<(), String> {
        std::fs::write(self.master_dir(&meta.id).join("meta.json"), serde_json::to_string_pretty(meta).unwrap()).map_err(|e| e.to_string())
    }

    /// 去重：①删掉和 master 内容相同的多余 `source.*` 副本（早期入库 EPUB/PDF 时多存的）；②在 `dirs` 里（递归）找和书库里
    /// 原文件内容相同的文件，把书库那份换成与它共享存储（克隆，不行就硬链接）。已经共享的不再处理。先比大小再比哈希。
    pub fn dedupe(&self, dirs: &[PathBuf]) -> Result<DedupeReport, String> {
        let mut rep = DedupeReport::default();
        let mut metas = self.list();
        // ① 多余的 source 副本
        for m in &metas {
            let dir = self.master_dir(&m.id);
            let src = dir.join(format!("source.{}", m.source_format));
            let master = dir.join(&m.master);
            if format!("master.{}", m.source_format) != m.master || !src.exists() {
                continue;
            }
            let (Ok(a), Ok(b)) = (std::fs::read(&src), std::fs::read(&master)) else { continue };
            if a == b {
                std::fs::remove_file(&src).map_err(|e| e.to_string())?;
                rep.removed_sources += 1;
                rep.removed_bytes += a.len() as u64;
            }
        }
        // ② 与外部原件共享：候选 = 还是独立一份的原文件内容（EPUB/PDF 是 master，其余是 source.*）
        let mut by_size: HashMap<u64, Vec<(PathBuf, usize)>> = HashMap::new();
        for (i, m) in metas.iter().enumerate() {
            if m.storage != Storage::Copy || m.source_format == "url" {
                continue;
            }
            let dir = self.master_dir(&m.id);
            let file = if format!("master.{}", m.source_format) == m.master { dir.join(&m.master) } else { dir.join(format!("source.{}", m.source_format)) };
            if let Ok(md) = std::fs::metadata(&file) {
                by_size.entry(md.len()).or_default().push((file, i));
            }
        }
        let mut lib_sha: HashMap<PathBuf, String> = HashMap::new();
        let mut stack: Vec<PathBuf> = dirs.to_vec();
        while let Some(d) = stack.pop() {
            if d.starts_with(&self.root) {
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let Ok(ft) = e.file_type() else { continue };
                let path = e.path();
                if ft.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !ft.is_file() {
                    continue;
                }
                let Ok(md) = e.metadata() else { continue };
                let Some(cands) = by_size.get_mut(&md.len()) else { continue };
                let Ok(bytes) = std::fs::read(&path) else { continue };
                let sha = sha256_hex(&bytes);
                let Some(pos) = cands.iter().position(|(lib_file, _)| {
                    let h = lib_sha.entry(lib_file.clone()).or_insert_with(|| std::fs::read(lib_file).map(|b| sha256_hex(&b)).unwrap_or_default());
                    *h == sha
                }) else {
                    continue;
                };
                let (lib_file, mi) = cands.remove(pos);
                let tmp = lib_file.with_extension("dedupe-tmp");
                let _ = std::fs::remove_file(&tmp);
                let storage = share_file(&path, &tmp)?;
                if storage == Storage::Copy {
                    let _ = std::fs::remove_file(&tmp); // 共享不了（跨文件系统），不省空间，保持原样
                    continue;
                }
                std::fs::rename(&tmp, &lib_file).map_err(|e| e.to_string())?;
                let m = &mut metas[mi];
                m.storage = storage;
                m.source_path = std::fs::canonicalize(&path).unwrap_or(path.clone()).display().to_string();
                self.save_meta(m)?;
                rep.shared_files += 1;
                rep.shared_bytes += md.len();
            }
        }
        Ok(rep)
    }

    /// 删掉母版和它在各设备下的产物（所有用过的产物目录）。
    pub fn remove(&self, id: &str) -> Result<Meta, String> {
        let meta = self.load_meta(id).ok_or_else(|| format!("没有 id 为 {id} 的书"))?;
        for dev in self.output_roots().iter().filter_map(|r| std::fs::read_dir(r).ok()).flatten().flatten() {
            let mut state = State::load(&dev.path());
            if let Some(entry) = state.books.remove(id) {
                let _ = std::fs::remove_file(dev.path().join(entry.file));
                state.save(&dev.path())?;
            }
        }
        std::fs::remove_dir_all(self.master_dir(id)).map_err(|e| e.to_string())?;
        Ok(meta)
    }

    /// 母版哈希：meta 里有就用，没有就算出来并写回 meta.json。
    fn master_sha(&self, meta: &Meta) -> Result<String, String> {
        if !meta.master_sha256.is_empty() {
            return Ok(meta.master_sha256.clone());
        }
        let bytes = std::fs::read(self.master_dir(&meta.id).join(&meta.master)).map_err(|e| format!("读母版: {e}"))?;
        let sha = sha256_hex(&bytes);
        let mut m = meta.clone();
        m.master_sha256 = sha.clone();
        let _ = std::fs::write(self.master_dir(&meta.id).join("meta.json"), serde_json::to_string_pretty(&m).unwrap());
        Ok(sha)
    }

    fn plan(&self, meta: &Meta, device: &Profile) -> Result<Plan, String> {
        let master_path = self.master_dir(&meta.id).join(&meta.master);
        let reflow = device.reflow_format().ok_or_else(|| format!("设备 {} 没有流式格式", device.id))?;
        let pdf_kind = (meta.master == "master.pdf").then(|| bookconv::pdf_ingest::classify_pdf(&master_path));
        let format = match pdf_kind {
            Some(bookconv::pdf_ingest::PdfKind::TextLayer) | None => reflow,
            Some(_) if device.formats.contains(&Format::Pdf) => Format::Pdf,
            Some(_) => return Err(format!("图片型 PDF（扫描件/漫画）暂时只能生成给支持 PDF 的设备，{} 不支持", device.name)),
        };
        let area = device.readable(format);
        let ext = match format {
            Format::Epub => "epub",
            Format::Azw3 => "azw3",
            Format::Pdf => "pdf",
        };
        let fingerprint = format!(
            "{}|{}|{}|{}x{}|{ext}|{}",
            self.master_sha(meta)?,
            bookconv::optimize::OPTIMIZE_VERSION,
            device.id,
            area.width,
            area.height,
            env!("CARGO_PKG_VERSION")
        );
        Ok(Plan { master_path, pdf_kind, format, area, ext, fingerprint })
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
                    profile::get(&dev_id).and_then(|p| self.plan(meta, p).ok()).map(|plan| plan.fingerprint == entry.fingerprint)
                };
                out.push(OutputStatus { device: dev_id, path, fresh });
            }
        }
        out.sort_by(|a, b| a.device.cmp(&b.device).then(a.path.cmp(&b.path)));
        out
    }

    /// 为设备生成一本书的产物（没变化就跳过，`force` 强制重建）。产物放在 `out_root/<设备 id>/`。
    pub fn build(&self, meta: &Meta, device: &Profile, out_root: &Path, force: bool) -> Result<Built, String> {
        let dir = out_root.join(&device.id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        self.remember_output_root(out_root)?;
        let Plan { master_path, pdf_kind, format, area, ext, fingerprint } = self.plan(meta, device)?;
        let mut state = State::load(&dir);
        let file = state.file_name_for(meta, ext);
        let out = dir.join(&file);
        if !force && out.exists() && state.books.get(&meta.id).is_some_and(|e| e.fingerprint == fingerprint && e.file == file) {
            return Ok(Built::UpToDate(out));
        }

        let tmp = dir.join(format!(".tmp-{}", meta.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let result = (|| -> Result<Vec<String>, String> {
            let mut warnings = Vec::new();
            if format == Format::Pdf {
                bookconv::pdf_ingest::optimize_pdf_trim_only(&master_path, &tmp.join("out.pdf"), area, |_, _| {})?;
                std::fs::rename(tmp.join("out.pdf"), &out).map_err(|e| e.to_string())?;
                return Ok(warnings);
            }
            // EPUB 母版（PDF 有文字层的先转 EPUB）
            let epub_master = if pdf_kind.is_some() {
                let (mut book, _, css) = bookconv::pdf_ingest::optimize_pdf_to_epub(&master_path, |_, _| {})?;
                let bytes = bookconv::epub::assemble_pdf_derived(&mut book, &css)?;
                let p = tmp.join("from-pdf.epub");
                std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
                p
            } else {
                master_path.clone()
            };
            let optimized = tmp.join("optimized.epub");
            let opts = bookconv::optimize::OptimizeOpts { wash: Some(Default::default()), grayscale: !device.color, ..bookconv::optimize::OptimizeOpts::new(area) };
            bookconv::optimize::optimize_epub_file_streaming(&epub_master, &optimized, &opts, |_, _| {})?;
            let rep = bookconv::check::check_epub_file(&optimized)?;
            if !rep.ok {
                warnings.extend(rep.errors.iter().map(|e| format!("质量门未过：{e}")));
            }
            match format {
                Format::Epub => std::fs::rename(&optimized, &out).map_err(|e| e.to_string())?,
                Format::Azw3 => {
                    let epub = std::fs::read(&optimized).map_err(|e| e.to_string())?;
                    let azw3 = azw3::epub_to_azw3(&epub, &azw3::Opts::default())?;
                    std::fs::write(&out, azw3).map_err(|e| e.to_string())?;
                }
                Format::Pdf => unreachable!(),
            }
            Ok(warnings)
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        let warnings = result?;
        // 书名变了（改了母版）时，旧文件名的产物删掉
        if let Some(old) = state.books.get(&meta.id) {
            if old.file != file {
                let _ = std::fs::remove_file(dir.join(&old.file));
            }
        }
        state.books.insert(meta.id.clone(), StateEntry { file, fingerprint });
        state.save(&dir)?;
        Ok(Built::Written { path: out, warnings })
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
        let tmp = dir.join(".state.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join(".state.json")).map_err(|e| e.to_string())
    }
    /// 产物文件名：`书名.ext`；和别的书撞名时加 id 后缀。
    fn file_name_for(&self, meta: &Meta, ext: &str) -> String {
        let base = bookconv::util::sanitize_filename(&meta.title, &meta.id);
        let plain = format!("{base}.{ext}");
        let taken = self.books.iter().any(|(id, e)| id != &meta.id && e.file == plain);
        if taken { format!("{base} [{}].{ext}", &meta.id[..6]) } else { plain }
    }
}

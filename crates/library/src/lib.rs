//! 母版库：书先原样入库成**与设备无关的母版**，再按设备 profile 生成产物（可随时重建）。
//!
//! 目录结构（`root` 缺省 `~/.local/share/booklib`）：
//! ```text
//! masters/<id>/master.epub | master.pdf   母版，入库后不再改动
//! masters/<id>/source.<原扩展名>           要转换的格式（MOBI、CBZ…）的原文件；网址入库时是 source.url
//! masters/<id>/meta.json                   书名、作者、来源、母版哈希、存储方式
//! output/<设备 id>/<书名>.<epub|azw3|pdf>  产物；output/<设备 id>/.state.json 记每本书的生成指纹，没变就跳过
//! outputs.json                             build 用过的其它产物目录（remove 时一起清理）
//! profiles/*.toml                          可选：自定义设备 profile，同 id 覆盖内置
//! .lock                                    进程锁
//! ```
//! `<id>` 是原始内容 SHA-256 的前 12 位十六进制：同一本书重复入库会认出来。
//!
//! 母版：EPUB 原样；MOBI/AZW/AZW3/PRC/FB2 与网页转成 EPUB；CBZ 转成每页一张图的 EPUB；PDF 保留原件，生成时再处理
//! （有文字层 → 转 EPUB；图片型 → 只给支持 PDF 的设备裁白边）。带 DRM 的书现在拒收（解 DRM 还没做）。
//!
//! 存储不重复：原文件内容进书库时先试写时复制克隆（reflink），不行才复制（见 [`Storage`]）。

mod fsutil;

use fsutil::{sha256_file, sha256_hex, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub use fsutil::Lock;
pub use profile::{Format, Profile, Registry};

/// 生成流程本身（本 crate 的步骤、参数）的版本：改了会影响产物的地方要加一，旧产物随之判为过期。
/// 优化器、AZW3 写出器各有自己的版本号，也都进指纹。
const PIPELINE_VERSION: &str = "2";

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
    /// 母版文件的 SHA-256（判断产物是否过期用）；早期条目没有，读条目时补算并写回。
    #[serde(default)]
    pub master_sha256: String,
    /// 入库时原文件的绝对路径（网址入库为空）。只作记录，书库不依赖它。
    #[serde(default)]
    pub source_path: String,
    /// 原文件内容在书库里怎么存的（EPUB/PDF 是 master 本身，要转换的格式是 source.*）。
    #[serde(default)]
    pub storage: Storage,
    /// PDF 母版有没有文字层（有 → 转 EPUB；没有 → 图片型）。母版不变，入库时判定一次存下来。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_text_layer: Option<bool>,
}

/// 书库里保存原文件内容的方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    /// 独立一份（跨文件系统或不支持克隆时）。
    #[default]
    Copy,
    /// 写时复制克隆（btrfs/xfs 等）：和原文件共用磁盘数据，但两边互不影响。
    Reflink,
    /// 硬链接：只有早期版本会产生。和原文件是同一个文件，原文件被就地改写时母版也会变，
    /// 所以生成前会重新核对哈希；`dedupe` 会把它换成克隆。
    Hardlink,
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
    /// 早期的硬链接母版换成克隆的个数。
    pub unlinked: usize,
}

/// 一本书在某设备下的产物状态（`list` 用）。
pub struct OutputStatus {
    pub device: String,
    pub path: PathBuf,
    /// `Some(true)` 最新；`Some(false)` 过期（母版或处理规则变了）；`None` 判断不了（设备 profile 已删、文件丢了）。
    pub fresh: Option<bool>,
}

/// 生成计划：产物格式、阅读范围、指纹。
struct Plan {
    master_path: PathBuf,
    /// PDF 母版有文字层（先转 EPUB）。
    from_pdf_text: bool,
    format: Format,
    area: profile::Screen,
    fingerprint: String,
}

pub struct Library {
    root: PathBuf,
    registry: Registry,
}

pub enum Added {
    New(Meta),
    Existing(Meta),
}

pub enum Built {
    Written { path: PathBuf, warnings: Vec<String> },
    UpToDate(PathBuf),
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 入库时要从 EPUB 里看的东西：书名、作者、有没有真 DRM。只读非图片条目（大漫画不整本解压）。
struct EpubInfo {
    title: String,
    authors: Vec<String>,
    drm: Option<String>,
}

impl EpubInfo {
    fn read(epub: &[u8]) -> EpubInfo {
        let mut info = EpubInfo { title: String::new(), authors: Vec::new(), drm: None };
        let Ok(mut zip) = zip_archive(epub) else { return info };
        let Ok(sk) = bookconv::epubzip::read_skeleton(&mut zip) else { return info };
        let entries = &sk.entries;
        if entries.iter().any(|e| e.name == "META-INF/rights.xml") {
            info.drm = Some("Adobe DRM（META-INF/rights.xml）".into());
        } else if let Some(targets) = bookconv::wash::encrypted_targets(entries) {
            // 只加密字体的"伪 DRM"不算，优化时会剥掉
            let bad = bookconv::wash::real_drm_items(&targets);
            if !bad.is_empty() {
                info.drm = Some(format!("正文被加密（{} 等 {} 项）", bad[0], bad.len()));
            }
        }
        if let Some(opf) = bookconv::wash::parse_opf(entries) {
            let dc = bookconv::wash::opf_dc(&String::from_utf8_lossy(&entries[opf.index].data));
            info.title = dc.title;
            info.authors = dc.creators;
        }
        info
    }
}

fn zip_archive(bytes: &[u8]) -> Result<bookconv::zip::ZipArchive<std::io::Cursor<&[u8]>>, String> {
    bookconv::zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())
}

fn is_pdf_text_layer(path: &Path) -> bool {
    bookconv::pdf_ingest::classify_pdf(path) == bookconv::pdf_ingest::PdfKind::TextLayer
}

impl Library {
    /// 缺省位置：`$BOOKLIB_DIR`，否则 `$XDG_DATA_HOME/booklib`，否则 `~/.local/share/booklib`。
    pub fn default_root() -> PathBuf {
        if let Some(d) = std::env::var_os("BOOKLIB_DIR").filter(|d| !d.is_empty()) {
            return PathBuf::from(d);
        }
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
        data.join("booklib")
    }

    /// 打开（没有就建）书库。设备 profile = 内置的，加上书库 `profiles/` 目录里的（同 id 覆盖内置）。
    pub fn open(root: impl Into<PathBuf>) -> Result<Library, String> {
        let root = root.into();
        std::fs::create_dir_all(root.join("masters")).map_err(|e| format!("{}: {e}", root.display()))?;
        let custom = root.join("profiles");
        let registry = if custom.is_dir() { Registry::with_dir(&custom)? } else { Registry::builtin().clone() };
        Ok(Library { root, registry })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 本书库可用的设备 profile。
    pub fn devices(&self) -> &Registry {
        &self.registry
    }

    /// 加进程锁。会改动书库的操作（add/build/remove/dedupe）之前调用，持有到操作结束。
    pub fn lock(&self) -> Result<Lock, String> {
        fsutil::lock(&self.root)
    }

    fn master_dir(&self, id: &str) -> PathBuf {
        self.root.join("masters").join(id)
    }

    fn read_meta(&self, id: &str) -> Option<Meta> {
        serde_json::from_slice(&std::fs::read(self.master_dir(id).join("meta.json")).ok()?).ok()
    }

    fn save_meta(&self, meta: &Meta) -> Result<(), String> {
        write_atomic(&self.master_dir(&meta.id).join("meta.json"), serde_json::to_string_pretty(meta).unwrap().as_bytes())
    }

    /// 读一个条目；早期条目缺的字段（母版哈希、PDF 类型）补算并写回，之后不用再算。
    fn load(&self, id: &str) -> Option<Meta> {
        let mut m = self.read_meta(id)?;
        let master = self.master_dir(id).join(&m.master);
        let mut changed = false;
        if m.master_sha256.is_empty() {
            if let Ok(sha) = sha256_file(&master) {
                m.master_sha256 = sha;
                changed = true;
            }
        }
        if m.master == "master.pdf" && m.pdf_text_layer.is_none() {
            m.pdf_text_layer = Some(is_pdf_text_layer(&master));
            changed = true;
        }
        if changed {
            let _ = self.save_meta(&m); // 写不回也不影响这次使用，下次再补
        }
        Some(m)
    }

    /// 写母版：先写进临时目录再改名，半路失败不留残缺条目。
    /// `master = None` 表示母版就是原文件本身（EPUB/PDF），从 `source` 共享过来，不另存 source 副本；
    /// 否则母版写字节，原文件（有的话）以 `source.<扩展名>` 共享过来。`extra` 是额外的小文件（网址的 source.url）。
    fn store(&self, meta: &mut Meta, master: Option<&[u8]>, source: Option<(&Path, &str)>, extra: &[(&str, &[u8])]) -> Result<(), String> {
        let dir = self.master_dir(&meta.id);
        let tmp = self.root.join("masters").join(format!(".tmp-{}", meta.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let result = (|| {
            let w = |name: &str, data: &[u8]| std::fs::write(tmp.join(name), data).map_err(|e| format!("写 {name}: {e}"));
            match (master, source) {
                (Some(bytes), src) => {
                    w(&meta.master, bytes)?;
                    if let Some((path, name)) = src {
                        meta.storage = fsutil::share_or_copy(path, &tmp.join(name))?;
                    }
                }
                (None, Some((path, _))) => meta.storage = fsutil::share_or_copy(path, &tmp.join(&meta.master))?,
                (None, None) => return Err("没有母版内容".into()),
            }
            for (name, data) in extra {
                w(name, data)?;
            }
            w("meta.json", serde_json::to_string_pretty(meta).unwrap().as_bytes())?;
            // 同 id 的目录还在但 meta 读不出来（写坏了）：内容由 id 决定，用这次的新条目替换
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| format!("替换损坏条目 {}: {e}", dir.display()))?;
            }
            std::fs::rename(&tmp, &dir).map_err(|e| format!("落库失败: {e}"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        result
    }

    /// 入库一个文件。
    pub fn add_file(&self, path: &Path) -> Result<Added, String> {
        let data = std::fs::read(path).map_err(|e| format!("读 {}: {e}", path.display()))?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "book".into());
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "book".into());
        let sha = sha256_hex(&data);
        let id = sha[..12].to_string();
        if let Some(m) = self.load(&id) {
            return Ok(Added::Existing(m));
        }
        let fallback_title = bookconv::naming::canonical_book_name(&stem);
        // `None` = 母版就是原文件（不转换）
        let (master, master_name): (Option<Vec<u8>>, &str) = match ext.as_str() {
            "epub" => (None, "master.epub"),
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
        let (title, authors) = if master_name == "master.epub" {
            let info = EpubInfo::read(master_bytes);
            if let Some(d) = info.drm {
                return Err(format!("有 DRM：{d}。解 DRM 还没做，暂时不能入库"));
            }
            (info.title, info.authors)
        } else {
            (String::new(), Vec::new())
        };
        let mut meta = Meta {
            master_sha256: if master.is_some() { sha256_hex(master_bytes) } else { sha },
            source_path: std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).display().to_string(),
            storage: Storage::Copy,
            pdf_text_layer: (master_name == "master.pdf").then(|| is_pdf_text_layer(path)),
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
        if let Some(m) = self.load(&id) {
            return Ok(Added::Existing(m));
        }
        let (epub, title) = bookconv::article::build_article_epub(url)?;
        let mut meta = Meta {
            master_sha256: sha256_hex(&epub),
            source_path: String::new(),
            storage: Storage::Copy,
            pdf_text_layer: None,
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

    /// `masters/` 下的条目目录名（跳过临时目录）。
    fn entry_ids(&self) -> Vec<String> {
        std::fs::read_dir(self.root.join("masters"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().to_str().filter(|n| !n.starts_with('.')).map(str::to_string))
            .collect()
    }

    /// 全部母版，按入库时间排序。
    pub fn list(&self) -> Vec<Meta> {
        let mut v: Vec<Meta> = self.entry_ids().iter().filter_map(|id| self.load(id)).collect();
        v.sort_by(|a, b| a.added.cmp(&b.added).then(a.title.cmp(&b.title)));
        v
    }

    /// `meta.json` 读不出来的条目 id（写坏了）。可以 `remove` 掉，或者重新入库同一个文件覆盖。
    pub fn broken(&self) -> Vec<String> {
        self.entry_ids().into_iter().filter(|id| self.read_meta(id).is_none()).collect()
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
        write_atomic(&self.root.join("outputs.json"), serde_json::to_string_pretty(&v).unwrap().as_bytes())
    }

    /// 书库里保存"原文件内容"的那个文件：EPUB/PDF 是 master，要转换的格式是 `source.*`。
    fn original_file(&self, m: &Meta) -> PathBuf {
        let dir = self.master_dir(&m.id);
        if format!("master.{}", m.source_format) == m.master {
            dir.join(&m.master)
        } else {
            dir.join(format!("source.{}", m.source_format))
        }
    }

    /// 去重：
    /// ① 删掉和 master 内容相同的多余 `source.*` 副本（早期入库 EPUB/PDF 时多存的）；
    /// ② 早期的硬链接母版换成克隆（断开和原文件的联系，原文件再改也不影响母版）；
    /// ③ 在 `dirs` 里（递归，不跟符号链接）找和书库里原文件内容相同的文件，把书库那份换成与它共享存储的克隆。
    ///    文件系统不支持克隆时保持原样（不省空间也不白复制）。先比大小再比哈希。
    pub fn dedupe(&self, dirs: &[PathBuf]) -> Result<DedupeReport, String> {
        let mut rep = DedupeReport::default();
        let mut metas = self.list();
        for m in &mut metas {
            let dir = self.master_dir(&m.id);
            // ①
            let src = dir.join(format!("source.{}", m.source_format));
            let master = dir.join(&m.master);
            if format!("master.{}", m.source_format) == m.master {
                if let (Ok(a), Ok(b)) = (std::fs::metadata(&src), std::fs::metadata(&master)) {
                    if a.len() == b.len() && sha256_file(&src)? == sha256_file(&master)? {
                        std::fs::remove_file(&src).map_err(|e| e.to_string())?;
                        rep.removed_sources += 1;
                        rep.removed_bytes += a.len();
                    }
                }
            }
            // ②
            if m.storage == Storage::Hardlink {
                let file = self.original_file(m);
                let tmp = file.with_extension("dedupe-tmp");
                let storage = fsutil::share_or_copy(&file, &tmp)?;
                std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
                m.storage = storage;
                self.save_meta(m)?;
                rep.unlinked += 1;
            }
        }
        // ③ 候选 = 还是独立一份的原文件内容
        let mut by_size: HashMap<u64, Vec<(PathBuf, usize)>> = HashMap::new();
        for (i, m) in metas.iter().enumerate() {
            if m.storage != Storage::Copy || m.source_format == "url" {
                continue;
            }
            let file = self.original_file(m);
            if let Ok(md) = std::fs::metadata(&file) {
                by_size.entry(md.len()).or_default().push((file, i));
            }
        }
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        let mut lib_sha: HashMap<PathBuf, String> = HashMap::new();
        let mut stack: Vec<PathBuf> = dirs.iter().map(|d| std::fs::canonicalize(d).unwrap_or_else(|_| d.clone())).collect();
        while let Some(d) = stack.pop() {
            if d.starts_with(&root) {
                continue; // 书库自己不算（否则母版会和自己"去重"）
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
                let Ok(sha) = sha256_file(&path) else { continue };
                let Some(pos) = cands.iter().position(|(lib_file, mi)| {
                    let m = &metas[*mi];
                    let h = lib_sha.entry(lib_file.clone()).or_insert_with(|| {
                        if lib_file.ends_with(&m.master) && !m.master_sha256.is_empty() {
                            m.master_sha256.clone()
                        } else {
                            sha256_file(lib_file).unwrap_or_default()
                        }
                    });
                    *h == sha
                }) else {
                    continue;
                };
                let (lib_file, mi) = cands.remove(pos);
                let tmp = lib_file.with_extension("dedupe-tmp");
                let Some(storage) = fsutil::try_reflink(&path, &tmp) else { continue };
                std::fs::rename(&tmp, &lib_file).map_err(|e| e.to_string())?;
                let m = &mut metas[mi];
                m.storage = storage;
                m.source_path = path.display().to_string();
                self.save_meta(m)?;
                rep.shared_files += 1;
                rep.shared_bytes += md.len();
            }
        }
        Ok(rep)
    }

    /// 删掉母版和它在各设备下的产物（所有用过的产物目录）。`meta.json` 损坏的条目也能删。返回书名。
    pub fn remove(&self, id: &str) -> Result<String, String> {
        let dir = self.master_dir(id);
        if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\']) || !dir.is_dir() {
            return Err(format!("没有 id 为 {id} 的书"));
        }
        let title = self.read_meta(id).map(|m| m.title).unwrap_or_else(|| "（条目已损坏）".into());
        for dev in self.output_roots().iter().filter_map(|r| std::fs::read_dir(r).ok()).flatten().flatten() {
            let mut state = State::load(&dev.path());
            if let Some(entry) = state.books.remove(id) {
                let _ = std::fs::remove_file(dev.path().join(entry.file));
                state.save(&dev.path())?;
            }
        }
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(title)
    }

    fn plan(&self, meta: &Meta, device: &Profile) -> Result<Plan, String> {
        let master_path = self.master_dir(&meta.id).join(&meta.master);
        if meta.master_sha256.is_empty() {
            return Err("读不到母版".into());
        }
        if meta.storage == Storage::Hardlink && sha256_file(&master_path)? != meta.master_sha256 {
            return Err("母版和入库时不一样了（早期版本用硬链接存母版，原文件被改过）。先 remove 再重新入库".into());
        }
        let is_pdf = meta.master == "master.pdf";
        let from_pdf_text = is_pdf && meta.pdf_text_layer == Some(true);
        let format = if is_pdf && !from_pdf_text {
            if !device.formats.contains(&Format::Pdf) {
                return Err(format!("图片型 PDF（扫描件/漫画）暂时只能生成给支持 PDF 的设备，{} 不支持", device.name));
            }
            Format::Pdf
        } else {
            device.reflow_format().ok_or_else(|| format!("设备 {} 没有流式格式（EPUB/AZW3）", device.id))?
        };
        let area = device.readable(format);
        let writer = if format == Format::Azw3 { azw3::WRITER_VERSION } else { "-" };
        let fingerprint = format!(
            "{}|{PIPELINE_VERSION}|{}|{writer}|{}|{}x{}|{}|{}",
            meta.master_sha256,
            bookconv::optimize::OPTIMIZE_VERSION,
            device.id,
            area.width,
            area.height,
            if device.color { "color" } else { "gray" },
            format.ext(),
        );
        Ok(Plan { master_path, from_pdf_text, format, area, fingerprint })
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

    /// 为设备生成一本书的产物（没变化就跳过，`force` 强制重建）。产物放在 `out_root/<设备 id>/`。
    pub fn build(&self, meta: &Meta, device: &Profile, out_root: &Path, force: bool) -> Result<Built, String> {
        let Plan { master_path, from_pdf_text, format, area, fingerprint } = self.plan(meta, device)?;
        let dir = out_root.join(&device.id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        self.remember_output_root(out_root)?;
        let mut state = State::load(&dir);
        let file = state.file_name_for(meta, format.ext(), &dir);
        let out = dir.join(&file);
        if !force && out.exists() && state.books.get(&meta.id).is_some_and(|e| e.fingerprint == fingerprint && e.file == file) {
            return Ok(Built::UpToDate(out));
        }

        // 所有中间文件都在产物目录下的临时目录里，成品最后一步改名到位（中途失败不会留下半个产物）
        let tmp = dir.join(format!(".tmp-{}", meta.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let result = (|| -> Result<Vec<String>, String> {
            let mut warnings = Vec::new();
            let done = tmp.join("out");
            if format == Format::Pdf {
                bookconv::pdf_ingest::optimize_pdf_trim_only(&master_path, &done, area, |_, _| {})?;
            } else {
                // EPUB 母版（PDF 有文字层的先转 EPUB）
                let epub_master = if from_pdf_text {
                    let (mut book, _, css) = bookconv::pdf_ingest::optimize_pdf_to_epub(&master_path, |_, _| {})?;
                    let p = tmp.join("from-pdf.epub");
                    std::fs::write(&p, bookconv::epub::assemble_pdf_derived(&mut book, &css)?).map_err(|e| e.to_string())?;
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

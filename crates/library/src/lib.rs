//! 书库：**只存索引**（每本书一个 `meta.json`：原件在哪、内容哈希、书名作者），按设备 profile 从原件生成产物。
//!
//! 目录结构（`root` 缺省 `~/.local/share/booklib`）：
//! ```text
//! masters/<id>/meta.json                   一本书的索引：原件路径、SHA-256、大小与修改时间、书名、作者
//! masters/<id>/master.epub                 只有网址入库的书有（没有原件，抓下来的正文存这里）；早期版本入库的条目也可能有
//! output/<设备 id>/<书名>.<epub|azw3|pdf>  产物；output/<设备 id>/.state.json 记每本书的生成指纹，没变就跳过
//! outputs.json                             build 用过的其它产物目录（remove 时一起清理）
//! sources.json                             跟踪的原件目录（track），以及其中每个文件上次看到时的大小、修改时间、id
//! profiles/*.toml                          可选：自定义设备 profile，同 id 覆盖内置
//! .lock                                    进程锁
//! ```
//! `<id>` 是原件内容 SHA-256 的前 12 位十六进制：同一本书重复入库会认出来，改名移动了也认得出。
//!
//! 生成时读原件：EPUB、PDF 直接用；MOBI/AZW/AZW3/PRC/FB2、CBZ 当场转成 EPUB（与设备无关的转换，结果不落书库）。
//! 原件不在了或者内容变了（大小、修改时间变了就重算哈希核对），生成会停下来提示先 `sync` 或重新入库，
//! 不会拿改过的内容冒充原来那本书。带 DRM 的书现在拒收（解 DRM 还没做）。

mod fsutil;
mod generate;
mod sources;

use fsutil::{sha256_file, sha256_hex, write_atomic};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use fsutil::Lock;
pub use generate::{Built, OutputStatus};
pub use profile::{Format, Profile, Registry};
pub use sources::{book_files, SyncEvent, SyncReport, SUPPORTED_EXTS};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    /// 原件文件名（网址入库时是网址）。
    pub source: String,
    /// 原件格式（小写扩展名，网址是 "url"）。
    pub source_format: String,
    /// 入库时间（Unix 秒）。
    pub added: u64,
    /// 原件的绝对路径（网址入库为空）。原件移动后 `sync` 或重新 `add` 会更新它。
    #[serde(default)]
    pub source_path: String,
    /// 原件内容的 SHA-256（`id` 是它的前 12 位）。早期条目没有，迁移（`dedupe`）时补上。
    #[serde(default)]
    pub source_sha256: String,
    /// 原件上次核对时的大小和修改时间：都没变就不重算哈希。
    #[serde(default)]
    pub source_size: u64,
    #[serde(default)]
    pub source_mtime_ns: u64,
    /// 书库里存着的母版文件名：网址入库的 `master.epub`，或早期版本入库、还没迁移的条目。空 = 只存索引。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub master: String,
    /// 存着的母版文件的 SHA-256。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub master_sha256: String,
    /// PDF 有没有文字层（有 → 转 EPUB；没有 → 图片型）。入库时判定一次存下来。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_text_layer: Option<bool>,
}

impl Meta {
    /// 生成时读哪里：存着的母版，还是原件。
    pub fn source(&self) -> Source {
        if self.master.is_empty() { Source::Original } else { Source::Stored }
    }

    /// 判断产物是否过期用的内容哈希。
    fn content_sha(&self) -> &str {
        if self.master.is_empty() { &self.source_sha256 } else { &self.master_sha256 }
    }
}

/// 一本书的内容从哪来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 原件（只存索引）。
    Original,
    /// 书库里存着的 `master.*`（网址入库、早期条目）。
    Stored,
}

/// 原件现在的状态（`list` 用，只看文件在不在，不读内容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalState {
    Present,
    Missing,
    /// 大小或修改时间变了（内容可能改过，生成前会核对哈希）。
    Touched,
    /// 书库里存着内容，不依赖原件。
    NotNeeded,
}

/// `dedupe`（迁移早期条目）的结果。
#[derive(Default, Debug)]
pub struct DedupeReport {
    /// 找到原件、删掉书库里副本的条目数与释放的字节数。
    pub migrated: usize,
    pub freed_bytes: u64,
    /// 找不到原件、副本保留的条目。
    pub kept: Vec<Meta>,
}

pub struct Library {
    root: PathBuf,
    registry: Registry,
}

pub enum Added {
    New(Meta),
    Existing(Meta),
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 文件的大小和修改时间（纳秒）。
pub(crate) fn file_stat(path: &Path) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos() as u64;
    Some((md.len(), mtime))
}

/// 入库时要从 EPUB 里看的东西：书名、作者、有没有真 DRM。只读非图片条目（大漫画不整本解压）。
/// 不是 zip、读不出条目、没有 OPF 的，不算 EPUB，拒收。
struct EpubInfo {
    title: String,
    authors: Vec<String>,
    drm: Option<String>,
}

impl EpubInfo {
    fn read(epub: &[u8]) -> Result<EpubInfo, String> {
        let mut info = EpubInfo { title: String::new(), authors: Vec::new(), drm: None };
        let bad = |e: String| format!("不是有效的 EPUB（{e}）");
        let mut zip = bookconv::zip::ZipArchive::new(std::io::Cursor::new(epub)).map_err(|e| bad(e.to_string()))?;
        let sk = bookconv::epubzip::read_skeleton(&mut zip).map_err(bad)?;
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
        if info.drm.is_some() {
            return Ok(info); // DRM 比"结构不对"更值得告诉用户
        }
        let opf = bookconv::wash::parse_opf(entries).ok_or_else(|| bad("找不到 OPF".into()))?;
        let dc = bookconv::wash::opf_dc(&String::from_utf8_lossy(&entries[opf.index].data));
        info.title = dc.title;
        info.authors = dc.creators;
        Ok(info)
    }
}

fn is_pdf_text_layer(path: &Path) -> bool {
    bookconv::pdf_ingest::classify_pdf(path) == bookconv::pdf_ingest::PdfKind::TextLayer
}

/// 要转换才能用的原件格式 → EPUB 字节（与设备无关）。EPUB、PDF 不走这里。
pub(crate) fn convert_to_epub(format: &str, name: &str, data: &[u8], title: &str) -> Result<Vec<u8>, String> {
    match format {
        "cbz" => bookconv::convert::cbz::cbz_to_epub(data, title),
        "mobi" | "azw" | "azw3" | "prc" | "fb2" => {
            bookconv::convert::precheck(name, data)?;
            // 这几种格式转出来的 EPUB 与设备无关（屏幕参数只有 CBZ→PDF 才用），随便给一个。
            let screen = profile::Screen { width: 1, height: 1 };
            Ok(bookconv::convert::convert_file(name, data, Default::default(), screen).ok_or("不支持的格式")??.data)
        }
        _ => Err(format!("不支持的格式 .{format}（支持 {}，或网址）", SUPPORTED_EXTS.join(" / "))),
    }
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

    /// 加进程锁。会改动书库的操作（add/build/remove/sync/dedupe）之前调用，持有到操作结束。
    pub fn lock(&self) -> Result<Lock, String> {
        fsutil::lock(&self.root)
    }

    fn entry_dir(&self, id: &str) -> PathBuf {
        self.root.join("masters").join(id)
    }

    fn read_meta(&self, id: &str) -> Option<Meta> {
        serde_json::from_slice(&std::fs::read(self.entry_dir(id).join("meta.json")).ok()?).ok()
    }

    fn save_meta(&self, meta: &Meta) -> Result<(), String> {
        write_atomic(&self.entry_dir(&meta.id).join("meta.json"), serde_json::to_string_pretty(meta).unwrap().as_bytes())
    }

    /// 读一个条目；早期条目缺的字段（母版哈希、PDF 类型）补算并写回，之后不用再算。
    fn load(&self, id: &str) -> Option<Meta> {
        let mut m = self.read_meta(id)?;
        let mut changed = false;
        let stored = self.entry_dir(id).join(&m.master);
        if !m.master.is_empty() && m.master_sha256.is_empty() {
            if let Ok(sha) = sha256_file(&stored) {
                m.master_sha256 = sha;
                changed = true;
            }
        }
        let is_pdf = if m.master.is_empty() { m.source_format == "pdf" } else { m.master.ends_with(".pdf") };
        if is_pdf && m.pdf_text_layer.is_none() {
            let p = if m.master.is_empty() { PathBuf::from(&m.source_path) } else { stored };
            if p.exists() {
                m.pdf_text_layer = Some(is_pdf_text_layer(&p));
                changed = true;
            }
        }
        if changed {
            let _ = self.save_meta(&m); // 写不回也不影响这次使用，下次再补
        }
        Some(m)
    }

    /// 新建条目：`masters/.tmp-<id>/` 里准备好再改名，半路失败不留残缺条目。`files` 是要存的母版（只有网址入库有）。
    fn store(&self, meta: &Meta, files: &[(&str, &[u8])]) -> Result<(), String> {
        let dir = self.entry_dir(&meta.id);
        let tmp = self.root.join("masters").join(format!(".tmp-{}", meta.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let result = (|| {
            for (name, data) in files {
                std::fs::write(tmp.join(name), data).map_err(|e| format!("写 {name}: {e}"))?;
            }
            std::fs::write(tmp.join("meta.json"), serde_json::to_string_pretty(meta).unwrap()).map_err(|e| format!("写 meta.json: {e}"))?;
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

    /// 入库一个文件：只记索引，不复制原件。已在库里时，如果记着的原件位置已经不在了，改成这个位置（移动过）。
    pub fn add_file(&self, path: &Path) -> Result<Added, String> {
        let path = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let data = std::fs::read(&path).map_err(|e| format!("读 {}: {e}", path.display()))?;
        let (size, mtime_ns) = file_stat(&path).unwrap_or((data.len() as u64, 0));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "book".into());
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "book".into());
        let sha = sha256_hex(&data);
        let id = sha[..12].to_string();
        if let Some(mut m) = self.load(&id) {
            if m.master.is_empty() && m.source_path != path.display().to_string() && !Path::new(&m.source_path).exists() {
                m.source_path = path.display().to_string();
                m.source = name;
                (m.source_size, m.source_mtime_ns) = (size, mtime_ns);
                self.save_meta(&m)?;
            }
            return Ok(Added::Existing(m));
        }
        let fallback_title = bookconv::naming::canonical_book_name(&stem);
        // 检查能不能用、取书名作者：要转换的格式先转一遍（结果不存，生成时再转）
        let (title, authors, pdf_text_layer) = match ext.as_str() {
            "pdf" => (String::new(), Vec::new(), Some(is_pdf_text_layer(&path))),
            _ => {
                let converted = if ext == "epub" { None } else { Some(convert_to_epub(&ext, &name, &data, &fallback_title)?) };
                let info = EpubInfo::read(converted.as_deref().unwrap_or(&data))?;
                if let Some(d) = info.drm {
                    return Err(format!("有 DRM：{d}。解 DRM 还没做，暂时不能入库"));
                }
                (info.title, info.authors, None)
            }
        };
        let meta = Meta {
            id,
            title: if title.is_empty() { fallback_title } else { title },
            authors,
            source: name,
            source_format: ext,
            added: now(),
            source_path: path.display().to_string(),
            source_sha256: sha,
            source_size: size,
            source_mtime_ns: mtime_ns,
            master: String::new(),
            master_sha256: String::new(),
            pdf_text_layer,
        };
        self.store(&meta, &[])?;
        Ok(Added::New(meta))
    }

    /// 入库一个网址：抓正文组成 EPUB（图片保留原图）。没有原件，这份 EPUB 存在书库里。
    pub fn add_url(&self, url: &str) -> Result<Added, String> {
        let id = sha256_hex(url.as_bytes())[..12].to_string();
        if let Some(m) = self.load(&id) {
            return Ok(Added::Existing(m));
        }
        let (epub, title) = bookconv::article::build_article_epub(url)?;
        let meta = Meta {
            id,
            title,
            authors: Vec::new(),
            source: url.to_string(),
            source_format: "url".into(),
            added: now(),
            source_path: String::new(),
            source_sha256: String::new(),
            source_size: 0,
            source_mtime_ns: 0,
            master: "master.epub".into(),
            master_sha256: sha256_hex(&epub),
            pdf_text_layer: None,
        };
        self.store(&meta, &[("master.epub", &epub), ("source.url", url.as_bytes())])?;
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

    /// 全部书，按入库时间排序。
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

    /// 原件现在的状态（只看文件属性，不读内容）。
    pub fn original_state(&self, m: &Meta) -> OriginalState {
        if m.source() == Source::Stored {
            return OriginalState::NotNeeded;
        }
        match file_stat(Path::new(&m.source_path)) {
            None => OriginalState::Missing,
            Some(st) if st == (m.source_size, m.source_mtime_ns) => OriginalState::Present,
            Some(_) => OriginalState::Touched,
        }
    }

    /// 核对原件还是入库时那本书，返回它的路径。大小和修改时间没变就不读；变了重算哈希，内容一样就更新记录。
    pub(crate) fn verified_original(&self, m: &Meta) -> Result<PathBuf, String> {
        let path = PathBuf::from(&m.source_path);
        let st = file_stat(&path).ok_or_else(|| format!("原件不在了：{}（移动过的话 booklib sync 或重新 add 新位置；不要了就 remove）", path.display()))?;
        if st == (m.source_size, m.source_mtime_ns) {
            return Ok(path);
        }
        if sha256_file(&path)? != m.source_sha256 {
            return Err(format!("原件改过了：{}（内容和入库时不同。booklib sync 或重新 add 入库新版本）", path.display()));
        }
        let mut m = m.clone();
        (m.source_size, m.source_mtime_ns) = st;
        let _ = self.save_meta(&m);
        Ok(path)
    }

    /// 迁移早期版本入库的条目（书库里存着母版副本、`source.*`）：找到原件就改成只存索引，删掉副本。
    ///
    /// 原件先看 `meta.json` 记着的路径，再在 `dirs` 里（递归）找内容相同的文件（SHA-256 前 12 位等于 id）。
    /// 找不到原件的保留副本（不然这本书就没了）。网址入库的书没有原件，不动。
    pub fn dedupe(&self, dirs: &[PathBuf]) -> Result<DedupeReport, String> {
        let mut rep = DedupeReport::default();
        let legacy: Vec<Meta> = self.list().into_iter().filter(|m| !m.master.is_empty() && m.source_format != "url").collect();
        if legacy.is_empty() {
            return Ok(rep);
        }
        // 候选文件按扩展名分组，只在需要时算哈希
        let mut candidates: Vec<PathBuf> = Vec::new();
        for d in dirs {
            candidates.extend(book_files(d));
        }
        let mut hashed: std::collections::HashMap<PathBuf, String> = std::collections::HashMap::new();
        let mut hash_of = |p: &Path| -> Option<String> {
            if let Some(h) = hashed.get(p) {
                return Some(h.clone());
            }
            let h = sha256_file(p).ok()?;
            hashed.insert(p.to_path_buf(), h.clone());
            Some(h)
        };
        for mut m in legacy {
            let dir = self.entry_dir(&m.id);
            let mut tries: Vec<PathBuf> = Vec::new();
            if !m.source_path.is_empty() {
                tries.push(PathBuf::from(&m.source_path));
            }
            tries.extend(candidates.iter().filter(|p| p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(&m.source_format))).cloned());
            let found = tries.into_iter().filter(|p| p.is_file()).find_map(|p| hash_of(&p).filter(|h| h.starts_with(&m.id)).map(|h| (p, h)));
            let Some((path, sha)) = found else {
                rep.kept.push(m);
                continue;
            };
            let path = std::fs::canonicalize(&path).unwrap_or(path);
            let (size, mtime_ns) = file_stat(&path).unwrap_or_default();
            m.source_path = path.display().to_string();
            m.source_sha256 = sha;
            (m.source_size, m.source_mtime_ns) = (size, mtime_ns);
            m.master.clear();
            m.master_sha256.clear();
            self.save_meta(&m)?;
            // meta 已改成读原件，再删副本（顺序反过来的话，中途失败会丢书）
            for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("master.") || n.starts_with("source.") {
                    rep.freed_bytes += e.metadata().map(|md| md.len()).unwrap_or(0);
                    std::fs::remove_file(e.path()).map_err(|e| e.to_string())?;
                }
            }
            rep.migrated += 1;
        }
        Ok(rep)
    }

    /// 删掉一本书的索引和它在各设备下的产物（所有用过的产物目录）。原件不动。`meta.json` 损坏的条目也能删。返回书名。
    pub fn remove(&self, id: &str) -> Result<String, String> {
        let dir = self.entry_dir(id);
        if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\']) || !dir.is_dir() {
            return Err(format!("没有 id 为 {id} 的书"));
        }
        let title = self.read_meta(id).map(|m| m.title).unwrap_or_else(|| "（条目已损坏）".into());
        self.remove_outputs(id)?;
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(title)
    }
}

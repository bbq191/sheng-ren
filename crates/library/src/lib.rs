//! 书库：**只存索引**（每本书一个 `meta.json`：原件在哪、内容哈希、书名作者），按阅读模式（设备 profile）从原件生成优化过的 EPUB。
//!
//! 目录结构（`root` 缺省 `~/.local/share/booklib`）：
//! ```text
//! masters/<id>/meta.json                   一本书的索引：原件路径、SHA-256、大小与修改时间、书名、作者
//! masters/<id>/cover.jpg                   可选：联网找来的封面（booklib meta --fetch），书里没封面时生成产物用
//! masters/<id>/master.epub                 只有网址入库的书有（没有原件，抓下来的正文存这里）；早期版本入库的条目也可能有
//! output/<模式>/<书名>.epub                 产物放电脑上的自定义模式：add 进来的书（不在跟踪目录里）和网址书的产物
//! output-state/<模式>.json                 每个模式的生成记录：书 id → 产物在设备上的位置、指纹（没变就跳过；只删这里记着的）
//! sources.json                             跟踪的原件目录（track），以及其中每个文件上次看到时的大小、修改时间、id
//! profiles/*.toml                          可选：自定义设备 profile，同 id 覆盖内置
//! .lock                                    进程锁
//! .tmp-*（各处）                           临时文件、临时目录；进程被杀时留下的，下次拿到锁时清掉
//! ```
//! `<id>` 是原件内容 SHA-256 的前 12 位十六进制：同一本书重复入库会认出来，改名移动了也认得出。
//!
//! 产物直接送到接着的设备上（2026-10-07 起，见 `deliver.rs`、`generate.rs`）：跟踪目录 `D` 里的书按原件所在子目录镜像
//! （`books/haodoo/x.epub` → Kindle 上 `documents/haodoo/<书名>.kfx`、Move 上文件夹 `haodoo`）。
//!
//! 生成时读原件：EPUB 直接用；CBZ 当场转成 EPUB（与设备无关的转换，结果不落书库）。
//! 早期版本收过的其它格式（MOBI/AZW3/FB2/PDF 等）2026-09-29 起不再支持：条目保留（`list` 标出来），生成时跳过。
//! 原件不在了或者内容变了（大小、修改时间变了就重算哈希核对），生成会停下来提示先 `sync` 或重新入库，
//! 不会拿改过的内容冒充原来那本书。带 DRM 的书现在拒收（解 DRM 还没做）。

mod cover;
mod covergen;
mod deliver;
mod douban;
mod fsutil;
mod generate;
mod matching;
mod metadata;
mod net;
mod qqread;
mod session;
mod sources;
mod transfer;
mod wikidata;

use fsutil::{sha256_file, sha256_hex};
use serde::{Deserialize, Serialize};
use session::{BuildCtx, ContentFacts, Devices, Records};
use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use cover::{CoverInfo, CoverResult};
pub use deliver::{DeviceEnv, Doc};
pub use fsutil::Lock;
pub use generate::{Built, OutputStatus, Step};
pub use metadata::{BookInfo, Edition, InfoResult};
pub use profile::{Format, Profile, Registry};
pub use sources::{book_files, Prune, SyncEvent, SyncMemo, SyncReport, SUPPORTED_EXTS};
pub use transfer::{Done, Event, Pipeline, Transfer};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
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
    /// 联网找来的封面（`booklib meta --fetch`）；书里没有封面时，生成产物时放进去。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<CoverInfo>,
    /// 联网找来的元数据（`booklib meta --fetch`）；简介、标签书里没有的，生成产物时补进去。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<BookInfo>,
    /// 是不是漫画（优化器同一套判定，CBZ 一律算）：入库时判一次存下，指纹按文字书、漫画分开算要用（原件暂时不在也算得出）。
    /// 早期条目没有，第一次用到时判、持锁的命令顺带存下。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comic: Option<bool>,
    /// 书里自己有没有简介、标签（EPUB 的 OPF）：书里已有的那项生成时不补、也不进指纹。入库时看一次存下，原件暂时不在也算得出指纹。
    /// 早期条目没有，第一次用到时看、持锁的命令顺带存下；看不了（原件不在）时指纹按两项都补算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_dc: Option<OwnDc>,
}

/// 书里自己有没有简介（`dc:description`）、标签（`dc:subject`），见 [`Meta::own_dc`]。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnDc {
    pub description: bool,
    pub subjects: bool,
}

impl Meta {
    /// 生成时读哪里：存着的母版，还是原件。
    pub fn source(&self) -> Source {
        if self.master.is_empty() { Source::Original } else { Source::Stored }
    }

    /// 内容的格式（小写扩展名）：存着的母版看母版文件名，否则是原件格式。
    pub fn content_format(&self) -> &str {
        if self.master.is_empty() { &self.source_format } else { self.master.rsplit_once('.').map_or("", |(_, e)| e) }
    }

    /// 内容格式现在还支持生成（EPUB、CBZ）。早期版本收过的 MOBI/AZW3/FB2/PDF 等不再支持：条目保留，生成时跳过。
    pub fn supported(&self) -> bool {
        SUPPORTED_EXTS.contains(&self.content_format())
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// 书库。缓存和运行状态按活多久分组（见 `session` 模块）：记录文件与锁、按内容记住的事实整个进程有效，
/// 设备连接跨轮保留，生成过程中的临时状态一轮生成（或一件传输）结束就清。
pub struct Library {
    root: PathBuf,
    registry: Registry,
    /// 记录文件的读缓存、锁的状态（整个进程）。
    records: Records,
    /// 按内容记住的事实：核对过的原件、书里有没有简介标签、是不是漫画（整个进程）。
    facts: ContentFacts,
    /// 设备在哪找、连上的设备（跨轮保留，[`Library::refresh_devices`] 每轮核一次）。
    devices: Devices,
    /// 生成过程中的临时状态：中间文件、传输中占着的文件名（一轮生成 / 一件传输）。
    build: BuildCtx,
    /// 联网查书目用的 HTTP（节流、离线状态跨书共用；整个进程，第一次用时建）。
    net: OnceCell<net::Net>,
}

pub enum Added {
    New(Meta),
    Existing(Meta),
    /// 同一路径的原件内容变了（`add` 改过的文件）：新版本入库，旧版本的条目连同产物删掉（和 `sync` 的"换成新版本"一样）。
    /// 第二项是删掉的旧版本的书名。
    Replaced(Meta, String),
}

impl Added {
    pub fn meta(&self) -> &Meta {
        match self {
            Added::New(m) | Added::Existing(m) | Added::Replaced(m, _) => m,
        }
    }
}

/// 路径不是 UTF-8 时的提示：书库的索引是 JSON，存不下这种路径。
pub(crate) const NOT_UTF8: &str = "文件名不是 UTF-8，请改名";
/// 入库时文件还在被写（读的前后大小或修改时间变了）。`sync` 认这个前缀：不记下来，下一轮再试。
pub(crate) const BUSY: &str = "文件正在写入";

pub(crate) fn now() -> u64 {
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
    comic: bool,
}

/// EPUB 的文字条目（图片条目只记名字、不读内容，大漫画不整本解压）：入库检查和漫画判定共用。
fn epub_text_entries(file: std::fs::File) -> Result<Vec<bookconv::epubzip::Entry>, String> {
    let mut zip = bookconv::zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    Ok(bookconv::epubzip::read_skeleton(&mut zip)?.entries)
}

impl EpubInfo {
    fn read(file: std::fs::File) -> Result<EpubInfo, String> {
        let mut info = EpubInfo { title: String::new(), authors: Vec::new(), drm: None, comic: false };
        let bad = |e: String| format!("不是有效的 EPUB（{e}）");
        let entries = &epub_text_entries(file).map_err(bad)?;
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
        info.comic = bookconv::comic_detect::is_comic(entries);
        Ok(info)
    }
}

/// 产物用到的各项规则的版本（`booklib --version` 显示）：(名称, 值)。哪一项变了，哪些书的产物就过期，见 docs/development.md#版本号。
pub fn rule_versions() -> Vec<(&'static str, String)> {
    vec![
        ("优化", bookconv::optimize::OPTIMIZE_VERSION.to_string()),
        ("漫画优化", bookconv::optimize::COMIC_VERSION.to_string()),
        ("掌阅/Move 统一规则", bookconv::wash::KINDLE_RULES_VERSION.to_string()),
        ("CBZ 转换", bookconv::convert::CONVERT_VERSION.to_string()),
        ("补元数据", bookconv::opfmeta::VERSION.to_string()),
        ("KFX 写出器", kfx::write::WRITER_VERSION.to_string()),
        ("生成流程", generate::PIPELINE_VERSION.to_string()),
    ]
}

/// 不再支持的格式的提示。
pub fn unsupported(format: &str) -> String {
    format!(".{format} 不再支持（只支持 EPUB 和 CBZ，或网址）")
}

/// 要转换才能用的原件格式（CBZ）→ EPUB 字节（与设备无关）。EPUB 不走这里。
pub(crate) fn convert_to_epub(format: &str, data: impl std::io::Read + std::io::Seek, title: &str) -> Result<Vec<u8>, String> {
    match format {
        "cbz" => bookconv::convert::cbz::cbz_to_epub_from(data, title),
        _ => Err(unsupported(format)),
    }
}

impl Library {
    /// 缺省位置：`$BOOKLIB_DIR`，否则 `$XDG_DATA_HOME/booklib`，否则 `~/.local/share/booklib`。
    pub fn default_root() -> PathBuf {
        if let Some(d) = std::env::var_os("BOOKLIB_DIR").filter(|d| !d.is_empty()) {
            return PathBuf::from(d);
        }
        let data = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
        data.join("booklib")
    }

    /// 打开（没有就建）书库。设备 profile = 内置的，加上书库 `profiles/` 目录里的（同 id 覆盖内置）。
    pub fn open(root: impl Into<PathBuf>) -> Result<Library, String> {
        let root = root.into();
        std::fs::create_dir_all(root.join("masters")).map_err(|e| format!("{}: {e}", root.display()))?;
        let custom = root.join("profiles");
        let registry = if custom.is_dir() { Registry::with_dir(&custom)? } else { Registry::builtin().clone() };
        Ok(Library {
            root,
            registry,
            records: Records::new(),
            facts: ContentFacts::default(),
            devices: Devices::new(DeviceEnv::from_env()),
            build: BuildCtx::default(),
            net: OnceCell::new(),
        })
    }

    /// 改设备在哪找（测试、特殊环境；缺省从环境变量读，见 [`DeviceEnv::from_env`]）。
    pub fn set_device_env(&mut self, env: DeviceEnv) {
        self.devices.set_env(env);
    }

    /// 重新看设备接没接上（`sync --watch` 每轮调一次）：MTP 设备、没接上的下次用到时重新看；到 Move 的 SSH 隧道还通就接着用
    /// （不每轮重连），断了才重连。返回连着、这次发现断了的 Move（模式 id）：上一轮跟它打交道出的错多半是掉线，可以重试。
    pub fn refresh_devices(&self) -> Vec<String> {
        self.devices.refresh()
    }

    /// 这个模式的产物送到哪：设备接上了（或产物放电脑上）`Ok`，没接上 `Err(原因)`。一轮里只连一次。
    pub(crate) fn target(&self, p: &Profile) -> session::Connection {
        self.devices.target(p)
    }

    /// 设备接没接上（`sync` 开头报一次）：`Ok(Some(说明))` 接上了，`Ok(None)` 产物放电脑上，`Err(原因)` 没接上。
    pub fn device_status(&self, p: &Profile) -> Result<Option<String>, String> {
        match &*self.target(p) {
            Ok(t) => Ok(t.describe()),
            Err(e) => Err(e.clone()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 书库的"有没有变化"戳（`sync --watch` 用）：`masters/` 与各条目目录、`output-state/` 的修改时间，以及 `sources.json`。
    /// 条目的增删改（原件移动改名后改记位置、`meta` 找来封面）、生成记录的改动都会改它们所在目录的修改时间（原子写是改名）；
    /// 别的进程 track/untrack 会换掉 `sources.json`（产物根目录跟着变）。
    pub fn change_stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut dir = |p: &Path, deep: bool| {
            let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            (p, mtime(p)).hash(&mut h);
            if deep {
                for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                    (e.path(), mtime(&e.path())).hash(&mut h);
                }
            }
        };
        dir(&self.root.join("masters"), true);
        dir(&self.root.join("output-state"), false);
        dir(&self.root.join("sources.json"), false);
        h.finish()
    }

    /// 本书库可用的设备 profile。
    pub fn devices(&self) -> &Registry {
        &self.registry
    }

    /// 加进程锁。会改动书库的操作（add/sync/remove/dedupe/meta）之前调用，持有到操作结束。
    /// 本进程第一次拿到锁时，顺带清理进程被杀时留下的临时文件和目录（`.tmp-*`）。
    /// 书库的记录文件读不出来时拒绝加锁（见 [`fsutil::check_json`]）。
    pub fn lock(&self) -> Result<Lock<'_>, String> {
        let l = fsutil::lock(&self.root, &self.records.locked)?;
        self.check_records()?;
        if !self.records.cleaned.replace(true) {
            self.clean_leftovers();
        }
        Ok(l)
    }

    /// 核对 `sources.json` 和各模式的生成记录都读得出来。
    fn check_records(&self) -> Result<(), String> {
        fsutil::check_json::<sources::Sources>(&self.sources_path())?;
        for (_, p) in self.state_files() {
            fsutil::check_json::<generate::State>(&p)?;
        }
        Ok(())
    }

    /// 书库里会放临时文件、临时目录的地方：书库根、`masters/` 和各条目、`output-state/`、`output/<模式>/`，
    /// 以及生成记录里产物所在的目录（那里只清临时文件，不动目录）。
    fn clean_leftovers(&self) {
        fsutil::clean_tmp(&self.root);
        let masters = self.root.join("masters");
        fsutil::clean_tmp(&masters);
        for id in self.entry_ids() {
            fsutil::clean_tmp(&masters.join(id));
        }
        fsutil::clean_tmp(&self.root.join("output-state"));
        for dev in std::fs::read_dir(self.root.join("output")).into_iter().flatten().flatten() {
            if dev.file_type().is_ok_and(|t| t.is_dir()) {
                fsutil::clean_tmp(&dev.path());
            }
        }
        for dir in self.output_dirs() {
            fsutil::clean_tmp_files(&dir);
        }
    }

    /// 联网用的 HTTP 客户端（第一次用时建，之后各本书共用：节流间隔、离线状态都延续）。
    pub(crate) fn net(&self) -> &net::Net {
        self.net.get_or_init(net::Net::new)
    }

    /// 联网时看起来整个断网了（接连两个网站连不上，见 `Net::offline`）。`booklib meta --fetch` 看到它就中止整轮。
    pub fn offline(&self) -> bool {
        self.net.get().is_some_and(net::Net::offline)
    }

    pub(crate) fn entry_dir(&self, id: &str) -> PathBuf {
        self.root.join("masters").join(id)
    }

    pub(crate) fn read_meta(&self, id: &str) -> Option<Meta> {
        fsutil::read_json(&self.entry_dir(id).join("meta.json"))
    }

    pub(crate) fn save_meta(&self, meta: &Meta) -> Result<(), String> {
        fsutil::write_json(&self.entry_dir(&meta.id).join("meta.json"), meta)
    }

    /// 读一个条目；早期条目缺的字段（母版哈希）补算，持锁时写回（之后不用再算）。
    /// 不持锁（`list`）时只在内存里补，不写书库。
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
        if changed && self.records.locked.get() {
            let _ = self.save_meta(&m); // 写不回也不影响这次使用，下次再补
        }
        Some(m)
    }

    /// 新建条目：`masters/.tmp-<id>/` 里准备好再改名，半路失败不留残缺条目。`files` 是要存的母版（只有网址入库有）。
    fn store(&self, meta: &Meta, files: &[(&str, &[u8])]) -> Result<(), String> {
        let dir = self.entry_dir(&meta.id);
        let tmp = self.root.join("masters").join(format!("{}{}", fsutil::TMP_PREFIX, meta.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let result = (|| {
            // 各文件落盘后再改名（断电后不会出现 0 字节的 meta.json）
            for (name, data) in files {
                fsutil::write_atomic_cleanable(&tmp.join(name), data)?;
            }
            fsutil::write_json(&tmp.join("meta.json"), meta)?;
            // 同 id 的目录还在但 meta 读不出来（写坏了）：内容由 id 决定，用这次的新条目替换
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| format!("替换损坏条目 {}: {e}", dir.display()))?;
            }
            std::fs::rename(&tmp, &dir).and_then(|_| fsutil::sync_parent(&dir)).map_err(|e| format!("落库失败: {e}"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        result
    }

    /// 入库一个文件（`booklib add`）：见 [`Library::add_file_only`]。书库里还有记着**同一路径**、内容不同的条目
    /// （入库以后原件被改过）时，按 `sync` 的"换成新版本"处理：旧版本的条目连同产物删掉（跟踪目录里还有一份旧内容的不删），
    /// `sources.json` 里这个文件也记成新版本。
    pub fn add_file(&self, path: &Path) -> Result<Added, String> {
        let added = self.add_file_only(path)?;
        let Ok(path) = std::fs::canonicalize(path) else { return Ok(added) };
        let m = added.meta().clone();
        let olds: Vec<Meta> = self.list().into_iter().filter(|o| o.id != m.id && o.master.is_empty() && Path::new(&o.source_path) == path).collect();
        if olds.is_empty() {
            return Ok(added);
        }
        let mut sources = self.load_sources();
        let mut retired = Vec::new();
        for o in &olds {
            match self.retire_old_version(&o.id, &m.id, &path, &sources.files) {
                Ok(true) => retired.push(o),
                Ok(false) => {}
                Err(e) => return Err(format!("新版本已入库（{} {}），旧版本 {} 删不掉：{e}", m.id, m.title, o.id)),
            }
        }
        if retired.is_empty() {
            return Ok(added);
        }
        if sources.record_new_version(&path, &m.id, retired.iter().map(|o| o.id.as_str())) {
            self.save_sources(&sources)?;
        }
        Ok(Added::Replaced(m, retired.iter().map(|o| o.title.as_str()).collect::<Vec<_>>().join("、")))
    }

    /// 入库一个文件：只记索引，不复制原件。已在库里时：记着的原件位置已经不在了，改成这个位置（移动过）；
    /// 就是这个位置的，刷新记着的大小和修改时间。不管同一路径的旧版本（`sync` 自己管，见 [`Library::add_file`]）。
    ///
    /// 边读边算哈希（不整本读进内存）。读之前和读完各看一次大小和修改时间，变了说明文件正在写入，报错不入库。
    pub(crate) fn add_file_only(&self, path: &Path) -> Result<Added, String> {
        let path = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let Some(path_str) = path.to_str().map(str::to_string) else { return Err(NOT_UTF8.into()) };
        let stat = || file_stat(&path).ok_or_else(|| format!("读 {path_str}: 文件不见了"));
        let before = stat()?;
        let unchanged = || -> Result<(), String> {
            if stat()? == before { Ok(()) } else { Err(format!("{BUSY}（读的过程中大小或修改时间变了），等写完再试")) }
        };
        let sha = sha256_file(&path)?;
        unchanged()?;
        let (size, mtime_ns) = before;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("book").to_string();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("book").to_string();
        let id = sha[..12].to_string();
        if let Some(mut m) = self.load(&id) {
            if m.master.is_empty() {
                if m.source_path == path_str {
                    if m.source_sha256 == sha && (m.source_size, m.source_mtime_ns) != before {
                        (m.source_size, m.source_mtime_ns) = before;
                        self.save_meta(&m)?;
                    }
                } else if !Path::new(&m.source_path).exists() {
                    m.source_path = path_str;
                    m.source = name;
                    (m.source_size, m.source_mtime_ns) = before;
                    self.save_meta(&m)?;
                }
            }
            return Ok(Added::Existing(m));
        }
        if !SUPPORTED_EXTS.contains(&ext.as_str()) {
            return Err("只支持 EPUB 和 CBZ".into());
        }
        let fallback_title = bookconv::naming::canonical_book_name(&stem);
        // 检查能不能用、取书名作者。CBZ 只看目录里有没有页面图片（不整本转换；书名取文件名，生成时再转）
        let file = std::fs::File::open(&path).map_err(|e| format!("读 {path_str}: {e}"))?;
        let info = if ext == "epub" {
            EpubInfo::read(file)?
        } else {
            bookconv::convert::cbz::check_cbz(std::io::BufReader::new(file))?;
            EpubInfo { title: String::new(), authors: Vec::new(), drm: None, comic: true }
        };
        if let Some(d) = info.drm {
            return Err(format!("有 DRM：{d}。解 DRM 还没做，暂时不能入库"));
        }
        let (title, authors, comic) = (info.title, info.authors, info.comic);
        // 书里有没有简介、标签（指纹要用，见 `Meta::own_dc`）：只读 OPF
        let own_dc = if ext == "epub" { metadata::own_description_subjects(&path).ok().map(|(description, subjects)| OwnDc { description, subjects }) } else { None };
        unchanged()?;
        let meta = Meta {
            id,
            title: if title.is_empty() { fallback_title } else { title },
            authors,
            source: name,
            source_format: ext,
            added: now(),
            source_path: path_str,
            source_sha256: sha,
            source_size: size,
            source_mtime_ns: mtime_ns,
            comic: Some(comic),
            own_dc,
            ..Default::default()
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
            source: url.to_string(),
            source_format: "url".into(),
            added: now(),
            master: "master.epub".into(),
            master_sha256: sha256_hex(&epub),
            ..Default::default()
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

    /// 按 id 前缀、书名片段或原件路径挑书；`selectors` 为空＝全部。存在的文件＝原件就是它的那本，存在的目录＝原件在它下面（递归）的所有书
    /// （2026-09-30：用户直接给原件路径 `booklib meta --fetch --clear ~/…/书.epub` 时报"没有匹配的书"）。
    pub fn select(&self, selectors: &[String]) -> Vec<Meta> {
        let all = self.list();
        if selectors.is_empty() {
            return all;
        }
        // 路径形式的选择词：规范化成和 `source_path` 一样的绝对路径（入库时就是 canonicalize 过的）
        let paths: Vec<(String, bool)> = selectors
            .iter()
            .filter_map(|s| std::fs::canonicalize(s).ok())
            .filter_map(|p| Some((p.to_str()?.to_string(), p.is_dir())))
            .collect();
        let by_path = |m: &Meta| {
            !m.source_path.is_empty()
                && paths.iter().any(|(p, dir)| if *dir { Path::new(&m.source_path).starts_with(p) } else { m.source_path == *p })
        };
        all.into_iter().filter(|m| by_path(m) || selectors.iter().any(|s| m.id.starts_with(s.as_str()) || m.title.contains(s.as_str()))).collect()
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

    /// 核对原件还是入库时那本书，返回它的路径。大小和修改时间没变就不读；变了重算哈希，内容一样就更新记录
    /// （本进程里也记住，同一本书给下一台设备生成时不再重算）。
    pub(crate) fn verified_original(&self, m: &Meta) -> Result<PathBuf, String> {
        let path = PathBuf::from(&m.source_path);
        let st = file_stat(&path).ok_or_else(|| format!("原件不在了：{}（移动过的话 booklib sync 或重新 add 新位置；不要了就 remove）", path.display()))?;
        if st == (m.source_size, m.source_mtime_ns) || self.facts.verified(&m.id, &m.source_path, st) {
            return Ok(path);
        }
        if sha256_file(&path)? != m.source_sha256 {
            return Err(format!("原件改过了：{}（内容和入库时不同。booklib sync 或重新 add 入库新版本）", path.display()));
        }
        self.facts.note_verified(&m.id, &m.source_path, st);
        // 不持锁（`list` 经 `is_comic` 走到这里）时只记在本进程里，不写书库
        if !self.records.locked.get() {
            return Ok(path);
        }
        // 从书库重读再改：调用方手里的 `m` 可能是旧的（比如之后 meta 找来了封面）
        let mut cur = self.read_meta(&m.id).unwrap_or_else(|| m.clone());
        if cur.source_path == m.source_path {
            (cur.source_size, cur.source_mtime_ns) = st;
            let _ = self.save_meta(&cur);
        }
        Ok(path)
    }

    /// 是不是漫画：CBZ 一律算；EPUB 按优化器同一套判定（`bookconv::comic_detect::is_comic`：图 ≥ 20 张、平均每张图配的字少于 40），
    /// 只读文字部分（图片条目不读内容）。
    pub fn is_comic(&self, m: &Meta) -> Result<bool, String> {
        if m.content_format() == "cbz" {
            return Ok(true);
        }
        let path = self.content_path(m)?;
        let file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let entries = epub_text_entries(file).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(bookconv::comic_detect::is_comic(&entries))
    }

    /// 生成时读的内容：书库里存着的母版，或核对过的原件。
    pub(crate) fn content_path(&self, m: &Meta) -> Result<PathBuf, String> {
        match m.source() {
            Source::Stored => Ok(self.content_location(m)),
            Source::Original => self.verified_original(m),
        }
    }

    /// 内容在哪（不核对原件）：书库里存着的母版，或原件记着的路径。
    pub(crate) fn content_location(&self, m: &Meta) -> PathBuf {
        match m.source() {
            Source::Stored => self.entry_dir(&m.id).join(&m.master),
            Source::Original => PathBuf::from(&m.source_path),
        }
    }

    /// 迁移早期版本入库的条目（书库里存着母版副本、`source.*`）：找到原件就改成只存索引，删掉副本。
    ///
    /// 原件先看 `meta.json` 记着的路径，再在 `dirs` 里（递归）找内容相同的文件（SHA-256 前 12 位等于 id）。
    /// 找不到原件的保留副本（不然这本书就没了）。网址入库的书没有原件，不动。路径不是 UTF-8 的文件不算。
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
        let mut hashed: HashMap<PathBuf, String> = HashMap::new();
        let mut hash_of = |p: &Path| -> Option<String> {
            if let Some(h) = hashed.get(p) {
                return Some(h.clone());
            }
            let h = sha256_file(p).ok()?;
            hashed.insert(p.to_path_buf(), h.clone());
            Some(h)
        };
        // 书库里的文件（各条目存着的副本）不算原件：`dedupe` 给了书库所在的目录时，副本和原件内容相同、会被当成"找到了原件"，
        // 接着把它自己删掉——这本书就没了
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        for mut m in legacy {
            let dir = self.entry_dir(&m.id);
            let mut tries: Vec<PathBuf> = Vec::new();
            if !m.source_path.is_empty() {
                tries.push(PathBuf::from(&m.source_path));
            }
            tries.extend(candidates.iter().filter(|p| p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(&m.source_format))).cloned());
            let found = tries.into_iter().filter(|p| p.is_file()).find_map(|p| {
                let p = std::fs::canonicalize(&p).unwrap_or(p);
                if p.starts_with(&root) {
                    return None;
                }
                let s = p.to_str()?.to_string();
                hash_of(&p).filter(|h| h.starts_with(&m.id)).map(|h| (p, s, h))
            });
            let Some((path, path_str, sha)) = found else {
                rep.kept.push(m);
                continue;
            };
            let (size, mtime_ns) = file_stat(&path).unwrap_or_default();
            m.source_path = path_str;
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

    /// 书库里有没有这个 id 的条目（目录在就算，`meta.json` 坏了也算）。
    pub fn entry_exists(&self, id: &str) -> bool {
        !id.is_empty() && !id.starts_with('.') && !id.contains(['/', '\\']) && self.entry_dir(id).is_dir()
    }

    /// 删掉一本书的索引和它在各模式下的产物（只删生成记录里记着的文件）。原件不动。`meta.json` 损坏的条目也能删。返回书名。
    pub fn remove(&self, id: &str) -> Result<String, String> {
        let dir = self.entry_dir(id);
        if !self.entry_exists(id) {
            return Err(format!("没有 id 为 {id} 的书"));
        }
        let title = self.read_meta(id).map(|m| m.title).unwrap_or_else(|| "（条目已损坏）".into());
        self.remove_outputs(id)?;
        // 先改名成临时目录再删：`remove_dir_all` 中途断电的话，条目目录里可能只剩一半文件（比如 meta.json 删了、
        // master.epub 还在，成了"损坏的条目"）；改名是原子的，留下的临时目录下次拿到锁时清掉
        let doomed = fsutil::tmp_sibling(&dir);
        std::fs::rename(&dir, &doomed).map_err(|e| format!("删 {}: {e}", dir.display()))?;
        let _ = fsutil::sync_parent(&dir);
        let _ = std::fs::remove_dir_all(&doomed);
        self.facts.forget(id);
        Ok(title)
    }
}

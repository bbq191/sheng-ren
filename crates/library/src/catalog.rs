//! 书库条目：一本书一个 `masters/<id>/`（`meta.json` 索引），入库（文件、网址）、列出、按选择器挑、核对原件、去重迁移、删除。
//! 都是 [`Library`] 的方法，和 `sources.rs`（跟踪目录）、`generate.rs`（生成）分开放。

use super::*;

impl Library {
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
    pub(crate) fn load(&self, id: &str) -> Option<Meta> {
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
    pub(crate) fn store(&self, meta: &Meta, files: &[(&str, &[u8])]) -> Result<(), String> {
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
        let own_dc = if ext == "epub" { metadata::read_own(&path).ok() } else { None };
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
    pub(crate) fn entry_ids(&self) -> Vec<String> {
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
    /// （2026-09-30：维护者直接给原件路径 `booklib meta --fetch --clear ~/…/书.epub` 时报"没有匹配的书"）。
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

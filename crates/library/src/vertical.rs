//! 竖排（第一期）的书库侧：`vertical.toml` 目录清单、按书的 `layout.vertical`、自动判定缓存，以及最终决定
//! 「这本书在这台设备上竖不竖排」。

use crate::{epub_text_entries, fsutil, Library, Meta};
use profile::Profile;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 书库根目录的 `vertical.toml`：按目录覆盖竖排（相对书根目录，前缀匹配，最长前缀胜）。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dirs {
    /// 强制竖排的目录。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<String>,
    /// 不竖排的目录。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,
}

impl Dirs {
    /// 这个相对目录匹配哪一条：最长前缀胜；同长时 `off` 胜（保守不竖排）。返回 `Some(true)` 强开、`Some(false)` 强关。
    fn lookup(&self, dir: &str) -> Option<bool> {
        let dir = normalize(dir);
        let best = |list: &[String]| {
            list.iter()
                .map(|p| normalize(p))
                .filter(|p| !p.is_empty() && (dir == *p || dir.starts_with(&format!("{p}/"))))
                .map(|p| p.len())
                .max()
        };
        let (on, off) = (best(&self.on), best(&self.off));
        match (on, off) {
            (Some(a), Some(b)) if a > b => Some(true),
            (Some(_), Some(_)) => Some(false),
            (Some(_), None) => Some(true),
            (None, Some(_)) => Some(false),
            (None, None) => None,
        }
    }

    fn add(&mut self, dir: &str, value: Option<bool>) {
        let d = normalize(dir);
        self.on.retain(|p| normalize(p) != d);
        self.off.retain(|p| normalize(p) != d);
        match value {
            Some(true) => self.on.push(d),
            Some(false) => self.off.push(d),
            None => {}
        }
        self.on.sort();
        self.off.sort();
    }
}

fn normalize(s: &str) -> String {
    s.trim()
        .replace('\\', "/")
        .trim_matches('/')
        .trim_start_matches("./")
        .to_string()
}

impl Library {
    fn vertical_path(&self) -> PathBuf {
        self.root.join("vertical.toml")
    }

    /// 读 `vertical.toml`；没有算空的，坏了报错（不能当成空的再写回去）。
    pub(crate) fn vertical_dirs(&self) -> Result<Dirs, String> {
        let p = self.vertical_path();
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                toml::from_str(&s).map_err(|e| format!("{} 坏了（{e}）：修好它再试", p.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Dirs::default()),
            Err(e) => Err(format!("读 {}: {e}", p.display())),
        }
    }

    pub(crate) fn save_vertical_dirs(&self, d: &Dirs) -> Result<(), String> {
        let s = toml::to_string_pretty(d).map_err(|e| format!("写 vertical.toml: {e}"))?;
        fsutil::write_atomic_cleanable(&self.vertical_path(), s.as_bytes())
    }

    /// 自动判定缓存（同 `comic_of`）：早期条目第一次用到时判、持锁时存进 meta.json。
    pub fn zh_text_of(&self, meta: &Meta) -> Result<bool, String> {
        if let Some(z) = meta.zh_text {
            return Ok(z);
        }
        let sha = meta.content_sha().to_string();
        if let Some(z) = self.facts.zh_text(&sha) {
            return Ok(z);
        }
        let z = self.is_zh_text(meta).unwrap_or(false);
        self.facts.note_zh_text(&sha, z);
        if self.records.locked.get() {
            if let Some(mut m) = self
                .read_meta(&meta.id)
                .filter(|m| m.content_sha() == meta.content_sha())
            {
                m.zh_text = Some(z);
                self.save_meta(&m)?;
            }
        }
        Ok(z)
    }

    /// 这本书是不是中文文字书（`bookconv::vertical::is_zh_text`）：CBZ 一律不是，读不出来按不是。
    pub fn is_zh_text(&self, m: &Meta) -> Result<bool, String> {
        if m.content_format() == "cbz" {
            return Ok(false);
        }
        let path = self.content_path(m)?;
        let file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let entries = epub_text_entries(file).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(bookconv::vertical::is_zh_text(&entries))
    }

    /// 固定版式（强制竖排时要排除）。
    pub fn is_fixed_layout(&self, m: &Meta) -> Result<bool, String> {
        if m.content_format() == "cbz" {
            return Ok(false);
        }
        let path = self.content_path(m)?;
        let file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let entries = epub_text_entries(file).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(bookconv::vertical::is_fixed_layout(&entries))
    }

    /// 这本书在这台设备上竖不竖排：设备没开 `vertical_text`、漫画、固定版式一律不竖排；
    /// 否则按「书自己的 `layout.vertical` → `vertical.toml` 目录清单（最长前缀）→ 自动判定」。
    pub fn vertical_for(&self, meta: &Meta, device: &Profile) -> Result<bool, String> {
        if !device.vertical_text {
            return Ok(false);
        }
        if self.comic_of(meta)? {
            return Ok(false);
        }
        if let Some(v) = meta.layout.as_ref().and_then(|l| l.vertical) {
            if !v {
                return Ok(false);
            }
            return Ok(!self.is_fixed_layout(meta)?);
        }
        if let Some((_, rel)) = self.tracked_rel(meta, &device.id) {
            let dir = rel.to_string_lossy().replace('\\', "/");
            if let Some(v) = self.vertical_dirs()?.lookup(&dir) {
                return Ok(v && !self.is_fixed_layout(meta)?);
            }
        }
        self.zh_text_of(meta)
    }

    /// 按书设置竖排覆盖（`booklib vertical`）：`Some(true/false)` 写进 `layout.vertical`，`None` 删掉。
    pub fn set_book_vertical(&self, id: &str, value: Option<bool>) -> Result<(), String> {
        let Some(mut m) = self.read_meta(id) else {
            return Err(format!("书库里没有 {id}"));
        };
        let mut l = m.layout.take().unwrap_or_default();
        l.vertical = value;
        m.layout = (l.vertical.is_some()).then_some(l);
        self.save_meta(&m)
    }

    /// 给目录清单写 / 删一条，并把目录下现有的书逐本写上（`--auto` 删书自己的值）。
    pub fn set_dir_vertical(&self, dir: &str, value: Option<bool>) -> Result<usize, String> {
        let mut dirs = self.vertical_dirs()?;
        dirs.add(dir, value);
        self.save_vertical_dirs(&dirs)?;
        let rel = normalize(dir);
        let mut n = 0;
        for m in self.list() {
            let Some((_, r)) = self.tracked_rel(&m, "kobo").or_else(|| self.tracked_rel(&m, "hanvon"))
            else {
                continue;
            };
            let d = r.to_string_lossy().replace('\\', "/");
            if rel.is_empty() || d == rel || d.starts_with(&format!("{rel}/")) {
                self.set_book_vertical(&m.id, value)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 找到原件路径 `path` 对应的书（写 `booklib vertical` 用）。
    pub fn book_by_path(&self, path: &Path) -> Option<Meta> {
        let want = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.list().into_iter().find(|m| {
            !m.source_path.is_empty()
                && std::path::absolute(&m.source_path)
                    .map(|p| p == want)
                    .unwrap_or(false)
        })
    }

    /// 把用户给的目录换成相对书根目录的字符串；本来就是这个形式就原样。
    pub fn dir_arg(&self, path: &Path) -> String {
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(root) = self.root_dir() {
            if let Ok(rel) = abs.strip_prefix(&root) {
                return normalize(&rel.to_string_lossy());
            }
        }
        normalize(&path.to_string_lossy())
    }
}

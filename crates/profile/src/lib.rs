//! 设备 profile：目标阅读器的屏幕与投递格式参数。
//!
//! 算法里不写死屏幕数字，一律从 profile 读。`[screen]` 是设备**标称**分辨率；优化时用的是
//! [`Profile::readable`]：阅读器实际能用来显示内容的范围（按产物格式分，阅读器页边距、状态栏等都已扣掉），
//! 有实测值就内置在 `[readable.<格式>]`，没有就退回标称尺寸。每台设备一份 `profiles/<id>.toml`（文件名即 id），
//! 构建时全部嵌入（见 `build.rs`）——**增删设备 = 增删文件**，不改代码。运行期还可以用
//! [`Registry::with_dir`] 从外部目录加载，同 id 覆盖内置项。

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/builtin.rs"));

/// 产物格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Epub,
    Pdf,
    /// KF8。
    Azw3,
}

impl Format {
    /// 流式排版（文字随字号重排）的格式；PDF 是固定版式。
    pub fn is_reflowable(self) -> bool {
        matches!(self, Format::Epub | Format::Azw3)
    }

    /// 文件扩展名（也是 TOML 里的写法）。
    pub fn ext(self) -> &'static str {
        match self {
            Format::Epub => "epub",
            Format::Pdf => "pdf",
            Format::Azw3 => "azw3",
        }
    }
}

/// 竖屏像素尺寸（`width <= height`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

impl Screen {
    pub const fn long_edge(self) -> u32 {
        self.height
    }
    pub const fn short_edge(self) -> u32 {
        self.width
    }
    /// 宽/高。
    pub fn aspect(self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// 文件名（不含 `.toml`），命令行与配置里用的标识。
    pub id: String,
    pub name: String,
    pub screen: Screen,
    pub ppi: u32,
    pub color: bool,
    /// 首选产物格式在前。
    pub formats: Vec<Format>,
    /// 各格式在阅读器里的真实可阅读范围（像素，竖屏）；没有的格式用 `screen`。
    readable: BTreeMap<Format, Screen>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    name: String,
    screen: Screen,
    ppi: u32,
    color: bool,
    formats: Vec<Format>,
    #[serde(default)]
    readable: BTreeMap<Format, Screen>,
}

impl Profile {
    /// 解析一份 profile TOML；`id` 由调用方给（通常是文件名）。
    pub fn parse(id: &str, toml_text: &str) -> Result<Profile, String> {
        let f: ProfileFile = toml::from_str(toml_text).map_err(|e| format!("profile {id}: {e}"))?;
        let p = Profile { id: id.to_string(), name: f.name, screen: f.screen, ppi: f.ppi, color: f.color, formats: f.formats, readable: f.readable };
        p.validate()?;
        Ok(p)
    }

    fn validate(&self) -> Result<(), String> {
        let Screen { width, height } = self.screen;
        if self.id.is_empty() {
            return Err("profile id 为空".into());
        }
        if width == 0 || height == 0 || width > height {
            return Err(format!("profile {}: screen 须为竖屏且非零（width <= height），实际 {width}x{height}", self.id));
        }
        if self.ppi == 0 {
            return Err(format!("profile {}: ppi 为 0", self.id));
        }
        if self.formats.is_empty() {
            return Err(format!("profile {}: formats 为空", self.id));
        }
        for (fmt, r) in &self.readable {
            if !self.formats.contains(fmt) {
                return Err(format!("profile {}: readable 里的 {fmt:?} 不在 formats 里", self.id));
            }
            if r.width == 0 || r.height == 0 || r.width > width || r.height > height {
                return Err(format!("profile {}: readable {fmt:?} 须非零且不超过屏幕 {width}x{height}，实际 {}x{}", self.id, r.width, r.height));
            }
        }
        Ok(())
    }

    /// 产物为 `format` 时的真实可阅读范围：有内置实测值用实测值，否则用标称屏幕。
    pub fn readable(&self, format: Format) -> Screen {
        self.readable.get(&format).copied().unwrap_or(self.screen)
    }

    /// 这台设备首选的流式格式（EPUB 优化产物最终落成的格式，Kindle 是 AZW3）；没有流式格式时 `None`。
    pub fn reflow_format(&self) -> Option<Format> {
        self.formats.iter().copied().find(|f| f.is_reflowable())
    }

    /// `format` 的阅读范围是不是实测内置的（`false` = 退回了标称屏幕）。
    pub fn has_measured_readable(&self, format: Format) -> bool {
        self.readable.contains_key(&format)
    }
}

/// 一组 profile，按 id 排序。
#[derive(Debug, Clone, Default)]
pub struct Registry {
    profiles: BTreeMap<String, Profile>,
}

impl Registry {
    /// 内置 profile（`profiles/*.toml`）。
    pub fn builtin() -> &'static Registry {
        static REG: OnceLock<Registry> = OnceLock::new();
        REG.get_or_init(|| {
            let mut r = Registry::default();
            for (id, text) in BUILTIN {
                let p = Profile::parse(id, text).unwrap_or_else(|e| panic!("内置 {e}"));
                r.profiles.insert(p.id.clone(), p);
            }
            r
        })
    }

    /// 内置 profile 加上 `dir` 下的 `*.toml`，同 id 以 `dir` 里的为准。
    pub fn with_dir(dir: &Path) -> Result<Registry, String> {
        let mut r = Registry::builtin().clone();
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for e in entries {
            let path = e.map_err(|e| e.to_string())?.path();
            if path.extension().is_none_or(|x| x != "toml") {
                continue;
            }
            let id = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| format!("{}: 文件名不是 UTF-8", path.display()))?;
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let p = Profile::parse(id, &text)?;
            r.profiles.insert(p.id.clone(), p);
        }
        Ok(r)
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Profile> {
        self.profiles.values()
    }
}

/// 内置 profile 按 id 取。
pub fn get(id: &str) -> Option<&'static Profile> {
    Registry::builtin().get(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_parse() {
        let ids: Vec<_> = Registry::builtin().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["ireader-ocean5-pro", "kindle-pw12-sig", "rmpp-move", "rmpp-move-koreader"]);
        let mk = get("rmpp-move-koreader").unwrap();
        assert_eq!((mk.screen, mk.readable(Format::Epub)), (Screen { width: 954, height: 1696 }, Screen { width: 954, height: 1696 }), "KOReader 漫画方案铺满整屏");
        assert!(mk.color);
        let m = get("rmpp-move").unwrap();
        assert_eq!(m.screen, Screen { width: 954, height: 1696 });
        assert_eq!(m.readable(Format::Epub), Screen { width: 842, height: 1455 }, "EPUB 用实测阅读范围");
        assert_eq!(m.readable(Format::Pdf), m.screen, "没有实测值的格式退回标称屏幕");
        let k = get("kindle-pw12-sig").unwrap();
        assert_eq!(k.readable(Format::Azw3), Screen { width: 1104, height: 1546 }, "Kindle AZW3 已实测");
        let i = get("ireader-ocean5-pro").unwrap();
        assert!(i.has_measured_readable(Format::Epub), "掌阅 EPUB 已实测（整页图铺满整屏）");
        assert_eq!(i.readable(Format::Epub), i.screen);
        assert!(m.color);
        assert_eq!(get("kindle-pw12-sig").unwrap().formats, [Format::Azw3]);
        assert!(get("nope").is_none());
    }

    #[test]
    fn rejects_landscape_and_unknown_keys() {
        let base = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\"]\n";
        assert!(Profile::parse("x", &format!("{base}[screen]\nwidth = 1680\nheight = 1264\n")).is_err());
        assert!(Profile::parse("x", &format!("{base}extra = 1\n[screen]\nwidth = 1\nheight = 2\n")).is_err());
        assert!(Profile::parse("x", &format!("{base}[screen]\nwidth = 1\nheight = 2\n")).is_ok());
        let scr = "[screen]\nwidth = 100\nheight = 200\n";
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.epub]\nwidth = 90\nheight = 180\n")).is_ok());
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.epub]\nwidth = 101\nheight = 180\n")).is_err(), "阅读范围不能超过屏幕");
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.pdf]\nwidth = 90\nheight = 180\n")).is_err(), "formats 里没有的格式不能写阅读范围");
    }

    #[test]
    fn dir_overrides_and_adds() {
        let dir = std::env::temp_dir().join(format!("profile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let body = |w: u32| format!("name = \"t\"\nppi = 212\ncolor = false\nformats = [\"pdf\"]\n[screen]\nwidth = {w}\nheight = 1448\n");
        std::fs::write(dir.join("rmpp-move.toml"), body(1072)).unwrap();
        std::fs::write(dir.join("new-dev.toml"), body(1000)).unwrap();
        std::fs::write(dir.join("README.md"), "ignored").unwrap();
        let r = Registry::with_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(r.get("rmpp-move").unwrap().screen.width, 1072);
        assert_eq!(r.get("new-dev").unwrap().screen.width, 1000);
        assert_eq!(r.iter().count(), Registry::builtin().iter().count() + 1, "覆盖同 id、新增一台");
    }
}

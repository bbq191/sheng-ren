//! 设备 profile（阅读模式）：目标阅读器的屏幕、真实可阅读范围、黑白彩色。现在有三份，都是设备自带的阅读器（2026-09-30 用户撤了 KOReader）：
//! `kindle`（Kindle PW12 签名版，AZW3）、`ireader`（掌阅 Ocean 5 Pro，EPUB）、`xochitl`（reMarkable Move，EPUB）。
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

/// profile 没写 `comic_margin` 时漫画页的白边（像素）。
pub const DEFAULT_COMIC_MARGIN: u32 = 1;

/// 产物格式。AZW3 是先按同一套规则优化出 EPUB、再转成 AZW3（2026-09-30 恢复，给 Kindle 自带阅读器）；PDF 已删，写了按未知值报错。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Epub,
    Azw3,
}

impl Format {
    /// 文件扩展名（也是 TOML 里的写法）。
    pub fn ext(self) -> &'static str {
        match self {
            Format::Epub => "epub",
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

/// 注释在这个阅读器里怎么看（用户 2026-09-29 定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Notes {
    /// 点标号弹窗显示：标号标 `epub:type="noteref"`、注释正文是 `<aside epub:type="footnote">`，阅读器据此认出注释。
    /// 内置的三个模式都不用（自带阅读器上没验证过弹窗），留给书库 `profiles/` 里的自定义模式。
    Popup,
    /// 点标号跳到注释、再返回（xochitl：没有弹窗，只认同文件 `#锚点`，不认 `epub:type`）。
    Jump,
}

/// 注释标号只有一个小图标（多看等书）时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteIcons {
    /// 保留原图标，限成一个字高（要阅读器认 CSS 限高、图标链接也能点）。内置的三个模式都换数字（见 `Number`），
    /// 留给自定义模式。
    #[default]
    Keep,
    /// 换成上标数字（xochitl：只有图的链接点了没反应，CSS 也限不住图标大小）。
    Number,
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
    /// 注释的呈现方式。
    pub notes: Notes,
    /// 只有图标的注释标号怎么办（TOML 里不写是 `keep`）。
    pub note_icons: NoteIcons,
    /// 保留原书注释里"跳回正文"的回链（TOML 里不写是 `true`）。xochitl 写 `false`：它遇到标号↔注释互相链接的一对，
    /// 两条链接都丢掉（正向也点不动），只能去掉回链、返回靠阅读器自己的"返回"。Kindle 自带阅读器没有可靠的"返回"，要靠回链。
    pub note_backlinks: bool,
    /// 各格式在阅读器里的真实可阅读范围（像素，竖屏）；没有的格式用 `screen`。
    readable: BTreeMap<Format, Screen>,
    /// 漫画页图到可阅读范围四边的白边（像素）：图保比缩放进"阅读范围 − 2×白边"的框，受限的那条边两侧正好是这么宽。
    /// TOML 里不写是 [`DEFAULT_COMIC_MARGIN`]；须小于阅读范围短边的 1/4。
    pub comic_margin: u32,
    /// 漫画在阅读器里要设成的页边距（xochitl 设置里的"页边距"，单位同 `.content` 的 `margins`）。`Some` 时：优化器给漫画写
    /// `META-INF/eink-reader-margins`（值就是它），`xochitl/comic-margins.sh` 凭它登记、Move 上的页边距代理在第一次开书时设好；
    /// 漫画的文字页、混排页的字补回默认留白，有图的页去掉 `<body>` 的类（见 `bookconv::comicpad`）。
    pub comic_reader_margins: Option<u32>,
    /// 漫画的真实可阅读范围（设成 `comic_reader_margins` 后实测）；没写就和 EPUB 的一样。
    comic_readable: Option<Screen>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    name: String,
    screen: Screen,
    ppi: u32,
    color: bool,
    formats: Vec<Format>,
    notes: Notes,
    #[serde(default)]
    note_icons: NoteIcons,
    #[serde(default = "yes")]
    note_backlinks: bool,
    #[serde(default)]
    readable: BTreeMap<Format, Screen>,
    comic_margin: Option<u32>,
    comic_reader_margins: Option<u32>,
    comic_readable: Option<Screen>,
}

fn yes() -> bool {
    true
}

impl Profile {
    /// 解析一份 profile TOML；`id` 由调用方给（通常是文件名）。
    pub fn parse(id: &str, toml_text: &str) -> Result<Profile, String> {
        let f: ProfileFile = toml::from_str(toml_text).map_err(|e| format!("profile {id}: {e}"))?;
        let p = Profile { id: id.to_string(), name: f.name, screen: f.screen, ppi: f.ppi, color: f.color, formats: f.formats, notes: f.notes, note_icons: f.note_icons, note_backlinks: f.note_backlinks, readable: f.readable, comic_margin: f.comic_margin.unwrap_or(DEFAULT_COMIC_MARGIN), comic_reader_margins: f.comic_reader_margins, comic_readable: f.comic_readable };
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
        if let Some((i, f)) = self.formats.iter().enumerate().find(|(i, f)| self.formats[..*i].contains(f)) {
            return Err(format!("profile {}: formats 第 {} 项 {f:?} 重复", self.id, i + 1));
        }
        for (fmt, r) in &self.readable {
            if !self.formats.contains(fmt) {
                return Err(format!("profile {}: readable 里的 {fmt:?} 不在 formats 里", self.id));
            }
            if r.width == 0 || r.height == 0 || r.width > width || r.height > height {
                return Err(format!("profile {}: readable {fmt:?} 须非零且不超过屏幕 {width}x{height}，实际 {}x{}", self.id, r.width, r.height));
            }
        }
        if let Some(r) = self.comic_readable {
            if r.width == 0 || r.height == 0 || r.width > width || r.height > height {
                return Err(format!("profile {}: comic_readable 须非零且不超过屏幕 {width}x{height}，实际 {}x{}", self.id, r.width, r.height));
            }
            if self.comic_margin >= r.width.min(r.height) / 4 {
                return Err(format!("profile {}: comic_margin {} 须小于漫画阅读范围短边的 1/4（{}x{}）", self.id, self.comic_margin, r.width, r.height));
            }
        }
        // 漫画白边按每种格式的阅读范围（没有实测值的格式用屏幕）都要够小：留给图的框至少是阅读范围的一半
        for fmt in &self.formats {
            let r = self.readable(*fmt);
            if self.comic_margin >= r.width.min(r.height) / 4 {
                return Err(format!("profile {}: comic_margin {} 须小于 {fmt:?} 阅读范围短边的 1/4（{}x{}）", self.id, self.comic_margin, r.width, r.height));
            }
        }
        Ok(())
    }

    /// 产物为 `format` 时的真实可阅读范围：有内置实测值用实测值，否则用标称屏幕。
    pub fn readable(&self, format: Format) -> Screen {
        self.readable.get(&format).copied().unwrap_or(self.screen)
    }

    /// 产物格式：`formats` 的第一个。
    pub fn format(&self) -> Format {
        self.formats[0]
    }

    /// 产物格式的阅读范围（[`Profile::readable`]`(self.format())`）。
    pub fn output_readable(&self) -> Screen {
        self.readable(self.format())
    }

    /// 漫画页排版用的阅读范围：`comic_readable`，没写就是产物格式的阅读范围。
    pub fn comic_readable(&self) -> Screen {
        self.comic_readable.unwrap_or_else(|| self.output_readable())
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

/// 命令行参数里的 `--device=<id>`（必填）→ 内置 profile。缺失或未知 id 时 `Err` 带上可选 id 列表，供调用方报用法错。
pub fn device_from_args<S: AsRef<str>>(args: &[S]) -> Result<&'static Profile, String> {
    let id = args.iter().find_map(|a| a.as_ref().strip_prefix("--device="));
    id.and_then(get).ok_or_else(|| {
        let ids: Vec<_> = Registry::builtin().iter().map(|p| p.id.as_str()).collect();
        format!("需要 --device=<设备>，可选: {}", ids.join(" / "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_parse() {
        let ids: Vec<_> = Registry::builtin().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["ireader", "kindle", "xochitl"]);
        let k = get("kindle").unwrap();
        assert_eq!((k.format(), k.output_readable()), (Format::Azw3, Screen { width: 1104, height: 1546 }), "Kindle 自带阅读器真机实测");
        let i = get("ireader").unwrap();
        assert_eq!((i.format(), i.output_readable()), (Format::Epub, Screen { width: 1264, height: 1680 }), "掌阅整页图铺满整屏");
        assert!(!k.color && !i.color);
        let x = get("xochitl").unwrap();
        assert_eq!(x.screen, Screen { width: 954, height: 1696 });
        assert_eq!(x.readable(Format::Epub), Screen { width: 842, height: 1455 }, "EPUB 用实测阅读范围");
        assert!(x.color);
        assert_eq!(x.formats, [Format::Epub]);
        assert_eq!((k.comic_margin, i.comic_margin, x.comic_margin), (1, 1, 0));
        assert_eq!(x.note_icons, NoteIcons::Number);
        assert_eq!((k.comic_reader_margins, x.comic_reader_margins), (None, Some(1)));
        assert_eq!((k.comic_readable(), x.comic_readable()), (Screen { width: 1104, height: 1546 }, Screen { width: 952, height: 1457 }));
        assert!(get("nope").is_none());
    }

    #[test]
    fn device_from_args_finds_id_or_lists_choices() {
        assert_eq!(device_from_args(&["a.epub", "--device=xochitl"]).unwrap().id, "xochitl");
        let err = device_from_args(&["--device=nope"]).unwrap_err();
        assert!(err.contains("kindle") && err.contains("--device="), "{err}");
        assert!(device_from_args::<&str>(&[]).is_err());
    }

    #[test]
    fn rejects_landscape_and_unknown_keys() {
        let base = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n";
        assert!(Profile::parse("x", &format!("{base}[screen]\nwidth = 1680\nheight = 1264\n")).is_err());
        assert!(Profile::parse("x", &format!("{base}extra = 1\n[screen]\nwidth = 10\nheight = 20\n")).is_err());
        assert!(Profile::parse("x", &format!("{base}[screen]\nwidth = 10\nheight = 20\n")).is_ok(), "comic_margin 缺省 1，要小于短边的 1/4");
        let scr = "[screen]\nwidth = 100\nheight = 200\n";
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.epub]\nwidth = 90\nheight = 180\n")).is_ok());
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.epub]\nwidth = 101\nheight = 180\n")).is_err(), "阅读范围不能超过屏幕");
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.pdf]\nwidth = 90\nheight = 180\n")).is_err(), "不再支持的格式（pdf）报错");
        assert!(Profile::parse("x", &format!("name = \"x\"\nppi = 300\ncolor = false\nformats = [\"azw3\"]\nnotes = \"jump\"\n{scr}")).is_ok(), "azw3 可以");
        for fmt in ["pdf", "mobi"] {
            let old = format!("name = \"x\"\nppi = 300\ncolor = false\nformats = [\"{fmt}\"]\nnotes = \"jump\"\n{scr}");
            assert!(Profile::parse("x", &old).is_err(), "formats 里写 {fmt} 报错");
        }
        let dup = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\", \"epub\"]\nnotes = \"jump\"\n";
        assert!(Profile::parse("x", &format!("{dup}{scr}")).is_err(), "formats 不能重复");
    }

    #[test]
    fn comic_margin_defaults_to_one_and_is_validated() {
        let base = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n";
        let scr = "[screen]\nwidth = 100\nheight = 200\n";
        assert_eq!(Profile::parse("x", &format!("{base}{scr}")).unwrap().comic_margin, DEFAULT_COMIC_MARGIN);
        assert_eq!(Profile::parse("x", &format!("{base}comic_margin = 0\n{scr}")).unwrap().comic_margin, 0);
        assert_eq!(Profile::parse("x", &format!("{base}comic_margin = 24\n{scr}")).unwrap().comic_margin, 24);
        assert!(Profile::parse("x", &format!("{base}comic_margin = 25\n{scr}")).is_err(), "短边 100 的 1/4 = 25，不能等于");
        // 按阅读范围算，不按屏幕：阅读范围 80 宽时上限是 20
        let rd = "[readable.epub]\nwidth = 80\nheight = 180\n";
        assert!(Profile::parse("x", &format!("{base}comic_margin = 19\n{scr}{rd}")).is_ok());
        assert!(Profile::parse("x", &format!("{base}comic_margin = 20\n{scr}{rd}")).is_err());
        assert!(Profile::parse("x", &format!("{base}comic_margin = -1\n{scr}")).is_err(), "负数报错");
    }

    #[test]
    fn dir_overrides_and_adds() {
        let dir = std::env::temp_dir().join(format!("profile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let body = |w: u32| format!("name = \"t\"\nppi = 212\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n[screen]\nwidth = {w}\nheight = 1448\n");
        std::fs::write(dir.join("xochitl.toml"), body(1072)).unwrap();
        std::fs::write(dir.join("new-dev.toml"), body(1000)).unwrap();
        std::fs::write(dir.join("README.md"), "ignored").unwrap();
        let r = Registry::with_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(r.get("xochitl").unwrap().screen.width, 1072);
        assert_eq!(r.get("new-dev").unwrap().screen.width, 1000);
        assert_eq!(r.iter().count(), Registry::builtin().iter().count() + 1, "覆盖同 id、新增一台");
    }
}

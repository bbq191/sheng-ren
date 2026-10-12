//! 图片资源（[`ResourceStore`]）：按书里的路径登记，图片字节从书里搬进来，取不到的只警告一次。

use super::*;

/// 图片在书里怎么用。Kindle 把正文图片的透明处画成黑色（《绍宋》章标题图、《飘》章头图整块黑，2026-10-05、10-12 真机），
/// 所以正文图片带透明的 PNG 写进 KFX 时合成白底——透明处本来就该露出白纸，合成以后看到的就是原意；书里的 EPUB 不动
/// （文字书只修复，2026-10-08 起优化器不再合成白底，`image_alpha` 只剩这里一处生效）。背景图不合成：Kindle 的 PNG 背景
/// 透明处照常露出背景色（`$284` 真机 ✓）。同一张图第一次按哪种用法登记就按哪种处理（同一个文件又当正文图又当背景的书没见过）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImageUse {
    /// `<img>`、SVG `<image>`、整页图、封面。
    Content,
    /// CSS 背景图。
    Background,
}

pub(super) struct Res {
    /// 资源实体（`$164`）的名字。
    pub(super) name: String,
    /// 资源路径（`$165`）。
    pub(super) location: String,
    pub(super) format: u32,
    pub(super) mime: &'static str,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) bytes: Vec<u8>,
}

/// 用到的图片资源，按第一次用到的顺序编号（下标就是 [`ResourceStore::register`] 返回的编号）。
pub(super) struct ResourceStore {
    list: Vec<Res>,
    by_path: HashMap<String, usize>,
    /// 取不到的图片（找不到或格式不支持）：只警告一次，以后直接跳过（以前每引用一次警告一次）。
    missing: HashSet<String>,
    /// 书里还没用到的图片字节：路径 → (字节, MIME)。
    images: HashMap<String, (Vec<u8>, &'static str)>,
}

impl ResourceStore {
    pub(super) fn new(images: HashMap<String, (Vec<u8>, &'static str)>) -> ResourceStore {
        ResourceStore { list: Vec::new(), by_path: HashMap::new(), missing: HashSet::new(), images }
    }

    /// 登记一张图片，返回资源编号；取不到（找不到、格式不支持）的往 `warnings` 记一条、返回 `None`。
    /// `use_` 是第一次用到它的地方：正文图片（`<img>`、整页图、封面）带透明的 PNG 合成白底，背景图原样（见 [`ImageUse`]）。
    pub(super) fn register(&mut self, path: &str, use_: ImageUse, warnings: &mut Vec<String>) -> Option<usize> {
        if let Some(&i) = self.by_path.get(path) {
            return Some(i);
        }
        if self.missing.contains(path) {
            return None;
        }
        let (format, mime) = match self.images.get(path).map(|(_, m)| *m) {
            Some("image/jpeg") => (FORMAT_JPG, "image/jpg"),
            Some("image/png") => (FORMAT_PNG, "image/png"),
            Some("image/gif") => (FORMAT_GIF, "image/gif"),
            other => {
                warnings.push(match other {
                    Some(other) => format!("图片格式 {other} 不支持：{path}"),
                    None => format!("图片找不到或格式不支持：{path}"),
                });
                self.missing.insert(path.to_string());
                return None;
            }
        };
        // 字节搬进资源，不复制（大漫画几百 MB）；同一路径以后走上面的 `by_path`，不会再来取。
        let (mut bytes, _) = self.images.remove(path)?;
        if use_ == ImageUse::Content && format == FORMAT_PNG {
            if let Some(flat) = bookconv::imgopt::flatten_transparent_png(&bytes, bookconv::imgopt::MAX_COMIC_DECODE_PIXELS) {
                bytes = flat;
            }
        }
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok())
            .unwrap_or((0, 0));
        let i = self.list.len();
        self.list.push(Res { name: format!("img{i}"), location: format!("resource/img{i}"), format, mime, width, height, bytes });
        // 图片字节的实体名另起（样本里是 `…-ad`，和 `$165` 的资源路径不同名；用 `resource/…` 当实体名时
        // Kindle 不显示图片，2026-10-05 真机）。
        self.by_path.insert(path.to_string(), i);
        Some(i)
    }

    /// 已登记的路径的资源编号（不登记新的）。
    pub(super) fn index_of(&self, path: &str) -> Option<usize> {
        self.by_path.get(path).copied()
    }

    pub(super) fn iter(&self) -> std::slice::Iter<'_, Res> {
        self.list.iter()
    }

    pub(super) fn len(&self) -> usize {
        self.list.len()
    }

    /// 拿走资源的图片字节（写字节实体时，不复制）。
    pub(super) fn take_bytes(&mut self, i: usize) -> Vec<u8> {
        std::mem::take(&mut self.list[i].bytes)
    }
}

/// 资源编号都是 [`ResourceStore::register`] 返回的，下标必然有效。
impl std::ops::Index<usize> for ResourceStore {
    type Output = Res;
    fn index(&self, i: usize) -> &Res {
        &self.list[i]
    }
}

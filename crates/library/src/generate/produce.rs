//! 产出 EPUB/KFX/KEPUB（拆分自 `generate.rs`）：优化、质量门、格式转换，以及要优化的输入准备。

use super::types::Produced;
use crate::fsutil::TMP_PREFIX;
use crate::session::PreparedInput;
use crate::{Library, Meta};
use profile::{Format, Profile};
use std::path::{Path, PathBuf};

impl Library {
    /// 生成产物写到 `part`（书库临时目录里）：EPUB 直接是优化结果；KFX、KEPUB 是同一份优化结果再转一次（中间文件放 `tmp`）。
    /// 返回警告（质量门、体检摘要）、产物的 SHA-256（整份在内存里的时候顺带算好；EPUB 是 `None`，由调用方读文件算）和体检清单。
    pub(super) fn produce(
        &self,
        meta: &Meta,
        device: &Profile,
        format: Format,
        part: &Path,
        tmp: &Path,
    ) -> Result<Produced, String> {
        let epub = self.prepared_input(meta)?;
        let mut warnings = Vec::new();
        let comic = self.comic_of(meta)?;
        let mut opts = bookconv::optimize::OptimizeOpts::for_profile(device);
        // 按书指定翻页方向（Bangumi 判成日本漫画的 rtl）；其余留给原书、profile
        opts.page_direction = crate::metadata::comic_direction(meta, device, comic);
        // 竖排（书自己的 layout.vertical → 目录清单 → 原书本来是否竖排；默认横排，漫画、固定版式不竖排）
        opts.set_vertical(self.vertical_for(meta, device)?);
        // 要转 KFX 的，优化结果先放临时目录
        let optimized = if format == Format::Epub {
            part.to_path_buf()
        } else {
            tmp.join("optimized.epub")
        };
        let optimized_rep =
            bookconv::optimize::optimize_epub_file_streaming(&epub, &optimized, &opts, |_, _| {})?;
        // 体检（怪癖表，只报告）：sync 打摘要，清单记进生成记录
        let findings = optimized_rep.wash.map(|w| w.audit).unwrap_or_default();
        let audit: Vec<String> = findings
            .iter()
            .map(bookconv::audit::Finding::line)
            .collect();
        let rep = bookconv::check::check_epub_file(&optimized)?;
        if !rep.ok {
            warnings.extend(rep.errors.iter().map(|e| format!("质量门未过：{e}")));
        }
        if format == Format::Kepub {
            // 只插 koboSpan 和包装（docs/kepub.md）；产物再过一次质量门（插坏了能看出来）
            let kp =
                bookconv::kepub::epub_to_kepub(&optimized, part, &bookconv::kepub::Opts::default())
                    .map_err(|e| e.to_string())?;
            warnings.extend(kp.warnings);
            let rep = bookconv::check::check_epub_file(part)?;
            if !rep.ok {
                warnings.extend(
                    rep.errors
                        .iter()
                        .map(|e| format!("质量门未过（KEPUB）：{e}")),
                );
            }
        } else if format == Format::Kfx {
            // 按文件读（不先整本读进内存）；BufReader：zip 按条目小块读
            let open = || {
                std::fs::File::open(&optimized)
                    .map(std::io::BufReader::new)
                    .map_err(|e| format!("读 {}: {e}", optimized.display()))
            };
            // 唯一 ID 取自书的 id：重建出来还是"同一本书"，Kindle 上的阅读进度不丢
            let (bytes, w) = kfx::write::epub_to_kfx_from(
                open()?,
                &kfx::write::Opts {
                    fixed_id: kfx_id(&meta.id),
                    media: Some(kfx::css::MediaEnv::for_profile(device)),
                },
            )?;
            warnings.extend(w);
            std::fs::write(part, &bytes).map_err(|e| format!("写 {}: {e}", part.display()))?;
            return Ok((warnings, Some(crate::fsutil::sha256_hex(&bytes)), audit));
        }
        warnings.extend(bookconv::audit::summary(&findings, &device.id));
        Ok((warnings, None, audit))
    }
    /// 要优化的 EPUB：原件（EPUB）或当场转换的（CBZ），再补上书里没有、书库里有找来的封面、简介、标签。
    /// 和上一次是同一本书、同样的补充时，直接用上次的中间文件。
    fn prepared_input(&self, meta: &Meta) -> Result<PathBuf, String> {
        let input = self.content_path(meta)?;
        let cover = meta.cover.as_ref().map_or("", |c| c.sha256.as_str());
        let info = self.injected_info_sig(meta).unwrap_or_default();
        let key = format!("{}|{cover}|{info}", meta.content_sha());
        if let Some(epub) = self.build.prepared(&meta.id, &key) {
            return Ok(epub);
        }
        // 这本书以前做的（补的东西变了）先丢掉：目录是同一个
        self.release_prepared_of(&meta.id);
        let dir = self.root.join(format!("{TMP_PREFIX}{}-src", meta.id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut prepared = PreparedInput {
            book: meta.id.clone(),
            key,
            dir,
            epub: PathBuf::new(),
            bytes: 0,
        };
        let own = self.own_dc_of(meta);
        prepared.epub = self.epub_input(meta, &input, &prepared.dir).and_then(|e| {
            crate::metadata::with_additions(self, meta, own.as_ref(), &e, &prepared.dir)
        })?;
        if prepared.epub == input {
            return Ok(input); // 原件直接用：`prepared` 丢掉时删空目录
        }
        prepared.bytes = std::fs::metadata(&prepared.epub).map_or(0, |m| m.len());
        let epub = prepared.epub.clone();
        self.build.keep_prepared(prepared);
        Ok(epub)
    }
    /// 要优化的 EPUB：EPUB 直接用；CBZ 当场转换，写进 `tmp`。
    pub(crate) fn epub_input(
        &self,
        meta: &Meta,
        input: &Path,
        tmp: &Path,
    ) -> Result<PathBuf, String> {
        match meta.content_format() {
            "epub" => Ok(input.to_path_buf()),
            "cbz" => {
                // 按文件读（不先把整个 CBZ 读进内存）
                let file = std::fs::File::open(input)
                    .map(std::io::BufReader::new)
                    .map_err(|e| format!("读 {}: {e}", input.display()))?;
                let p = tmp.join("master.epub");
                crate::convert_to_epub("cbz", file, &meta.title, &p)?;
                Ok(p)
            }
            other => Err(crate::unsupported(other)),
        }
    }
}
/// KFX 的唯一 ID（容器 id `CR!…`、`content_id`、`book_id` 都由它派生）：书库的书 id（SHA-256 前 12 位十六进制）按十六进制读成整数。
/// 同一本书重建不变；OPF 唯一标识符相同的两本书（模板生成的书常见）也各是各的。2026-10-06 以前取 `get(..16)`，
/// 12 位的书 id 永远取不到，退回 OPF 标识符的哈希。读不出来（书 id 不是不超过 16 位的十六进制，只有手改过的书库会这样）时
/// 返回 `None`，写出器退回 OPF 标识符。
pub(super) fn kfx_id(book_id: &str) -> Option<u64> {
    if book_id.is_empty() || book_id.len() > 16 {
        return None;
    }
    u64::from_str_radix(book_id, 16).ok()
}

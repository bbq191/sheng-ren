//! 产出 EPUB/KFX/KEPUB（拆分自 `generate.rs`）：优化、质量门、格式转换，以及要优化的输入准备。

use super::types::Produced;
use crate::fsutil::TMP_PREFIX;
use crate::session::PreparedInput;
use crate::{Library, Meta};
use profile::{Format, Profile};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// `warnings` 里给 `booklib sync` 按轮汇总的类别标记：逐本结果行不再打，一轮结束各汇总成一行；
/// 明细记在生成记录的 `audit` 里（`booklib list` 看）。改了要一起改 `booklib` 里认这些标记的地方。
pub const AUDIT_WARNING: &str = "体检：有和 Kindle 不一样、没处理或没验证过的写法";
pub const GATE_WARNING: &str = "质量门未过";
pub const KFX_WARNING: &str = "KFX：转换时有问题";

/// 优化产物的质量门错误记进生成记录：每条一行。
fn gate_audit_lines(errors: &[String]) -> Vec<String> {
    errors
        .iter()
        .map(|e| format!("⚠ 质量门未过：{e}"))
        .collect()
}

/// KEPUB 产物的质量门错误记进生成记录：和优化产物文字完全相同的（KEPUB 只多插了包装和 koboSpan，
/// 优化产物有的错几乎一定原样再报一遍）不再记，只记新出现的。
fn kepub_gate_audit_lines(errors: &[String], gate: &[String]) -> Vec<String> {
    errors
        .iter()
        .filter(|e| !gate.contains(e))
        .map(|e| format!("⚠ 质量门未过（KEPUB）：{e}"))
        .collect()
}

/// KFX 写出器的警告记进生成记录：同样的去重，最多记 20 条，多出来的再加一行汇总
/// （链接目标找不到的书可能有上百条，生成记录不能被撑大）。
fn kfx_audit_lines(warnings: &[String]) -> Vec<String> {
    const MAX: usize = 20;
    let mut seen = HashSet::new();
    let unique: Vec<&String> = warnings
        .iter()
        .filter(|w| seen.insert(w.as_str()))
        .collect();
    let mut out: Vec<String> = unique
        .iter()
        .take(MAX)
        .map(|w| format!("⚠ KFX：{w}"))
        .collect();
    if unique.len() > MAX {
        out.push(format!(
            "⚠ KFX：另有 {} 条（同类，未逐条记）",
            unique.len() - MAX
        ));
    }
    out
}

impl Library {
    /// 生成产物写到 `part`（书库临时目录里）：EPUB 直接是优化结果；KFX、KEPUB 是同一份优化结果再转一次（中间文件放 `tmp`）。
    /// 返回警告（给 sync 按轮汇总的类别标记）、产物的 SHA-256（整份在内存里的时候顺带算好；EPUB 是 `None`，由调用方读文件算）和体检清单。
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
        // 体检（怪癖表，只报告）：sync 只认标记（一轮汇总一行），清单记进生成记录
        let findings = optimized_rep.wash.map(|w| w.audit).unwrap_or_default();
        let mut audit: Vec<String> = findings
            .iter()
            .map(bookconv::audit::Finding::line)
            .collect();
        let gate = bookconv::check::check_epub_file(&optimized)?;
        if !gate.ok {
            audit.extend(gate_audit_lines(&gate.errors));
            warnings.push(GATE_WARNING.to_string());
        }
        if format == Format::Kepub {
            // 只插 koboSpan 和包装（docs/kepub.md）；产物再过一次质量门（插坏了能看出来）
            let kp =
                bookconv::kepub::epub_to_kepub(&optimized, part, &bookconv::kepub::Opts::default())
                    .map_err(|e| e.to_string())?;
            if !kp.risky_css.is_empty() {
                // 逐本记进生成记录（`booklib list` 能查到哪几本）；`sync` 认这个固定串，一轮汇总成一行
                audit.push(format!(
                    "⚠ KEPUB：CSS 里 {} 可能作用到新加的包装或 koboSpan（只提醒，没改 CSS）",
                    kp.risky_css.join("、")
                ));
                warnings.push(bookconv::kepub::RISKY_CSS_WARNING.to_string());
            }
            warnings.extend(kp.warnings);
            let rep = bookconv::check::check_epub_file(part)?;
            if !rep.ok {
                // KEPUB 只在优化产物上插包装和 koboSpan：和优化产物文字完全相同的错误不再记、
                // 也不再报，只有 KEPUB 新出现的才记（判定本身不变，两次检查照做）。
                let new = kepub_gate_audit_lines(&rep.errors, &gate.errors);
                if !new.is_empty() {
                    audit.extend(new);
                    if !warnings.iter().any(|w| w == GATE_WARNING) {
                        warnings.push(GATE_WARNING.to_string());
                    }
                }
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
            audit.extend(kfx_audit_lines(&w));
            if !w.is_empty() {
                warnings.push(KFX_WARNING.to_string());
            }
            std::fs::write(part, &bytes).map_err(|e| format!("写 {}: {e}", part.display()))?;
            return Ok((warnings, Some(crate::fsutil::sha256_hex(&bytes)), audit));
        }
        // 体检清单逐条在 `audit` 里；`sync` 只认这个标记，一轮汇总一行
        if !findings.is_empty() {
            warnings.push(AUDIT_WARNING.to_string());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// KEPUB 质量门和优化产物文字完全相同的错误只记一次；KEPUB 新出现的照记。两边判定都照做。
    #[test]
    fn kepub_gate_errors_are_deduplicated() {
        let gate = vec![
            "目录 href 文件命中率过低 0/10".to_string(),
            "单标签双 id：p".to_string(),
        ];
        let kepub = vec![
            "目录 href 文件命中率过低 0/10".to_string(),
            "单标签双 id：p".to_string(),
            "KEPUB 才有的错".to_string(),
        ];
        assert_eq!(
            gate_audit_lines(&gate),
            [
                "⚠ 质量门未过：目录 href 文件命中率过低 0/10",
                "⚠ 质量门未过：单标签双 id：p"
            ]
        );
        assert_eq!(
            kepub_gate_audit_lines(&kepub, &gate),
            ["⚠ 质量门未过（KEPUB）：KEPUB 才有的错"]
        );
    }

    /// KFX 警告同样的去重，超过 20 条只记前 20 条，再记一行"另有 N 条"。
    #[test]
    fn kfx_warnings_are_deduplicated_and_capped() {
        let mut warnings: Vec<String> = (0..25)
            .map(|i| format!("链接目标找不到：c.xhtml#a{i}"))
            .collect();
        warnings.push(warnings[0].clone());
        let lines = kfx_audit_lines(&warnings);
        assert_eq!(lines.len(), 21, "{lines:?}");
        assert_eq!(lines[0], "⚠ KFX：链接目标找不到：c.xhtml#a0");
        assert_eq!(lines[19], "⚠ KFX：链接目标找不到：c.xhtml#a19");
        assert_eq!(lines[20], "⚠ KFX：另有 5 条（同类，未逐条记）");
        assert_eq!(
            lines.iter().filter(|l| l.contains("#a0")).count(),
            1,
            "重复的不再记：{lines:?}"
        );
    }
}

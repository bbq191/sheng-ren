//! 命令行：按阅读模式（设备 profile）优化一本 EPUB（`optimize::optimize_epub_file_streaming`，与书库 `booklib build` 同一函数）。
//! 缺省 = 清洗层（伪 DRM 剥离 / CSS 锁剥离 / 边距段距归零+首行缩进 / 空页清理 / 缺目录时自动目录 / 双 id 折叠 / 章节分页）
//! 加优化器（脚注按阅读模式弹窗或跳转 / duokan 标记 / 远程图内联 / 双 id 去重 / 图片按阅读范围缩放），产物自带
//! `META-INF/eink-optimized` 标记。
//!
//! 用法: epub-optimize --device=<设备> [选项] 输入.epub 输出.epub
//!   （流式处理，大漫画也不整本读进内存；产物先写到 `输出.epub.optimizing.tmp`，成功后改名，失败不留半成品）
//!   --device=<id>    阅读模式 profile（必填：koreader / xochitl，见 profile crate 的 profiles/*.toml）
//!   --no-wash        只跑优化器不清洗
//!   --keep-spacing   清洗但保留原书段间距（诗集/剧本）
//!   --auto-toc       强制从 h1–h6 重建目录（缺省仅在无目录时生成）
//!   --no-paginate    不做章节分页（缺省：章标题独立一页、节与节之间分页）
//!   --check          产物过质量门，打印 JSON 报告；不过则退出码 3（产物仍写出）
//!   --require-toc    质量门把"无目录"升为失败
//! 退出码: 0 成功；1 用法错；2 优化失败（输入原样不动）；3 质量门未过。

use bookconv::optimize::{self, OptimizeOpts};
use bookconv::util::cli::{self, die};
use bookconv::wash::{AutoToc, WashOpts};

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args.iter().filter(|a| a.starts_with("--") && !a.starts_with("--device=")).map(|s| s.as_str()).collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.len() != 2 || flags.iter().any(|f| !["--no-wash", "--keep-spacing", "--auto-toc", "--no-paginate", "--check", "--require-toc"].contains(f)) {
        die(cli::USAGE, "用法: epub-optimize --device=<设备> [--no-wash] [--keep-spacing] [--auto-toc] [--no-paginate] [--check] [--require-toc] 输入.epub 输出.epub");
    }
    // 阅读模式定阅读范围、黑白屏转灰度、注释弹窗还是跳转（和书库生成同一个起点）
    let device = profile::device_from_args(&args).unwrap_or_else(|e| die(cli::USAGE, e));
    let wash = if flags.contains(&"--no-wash") {
        None
    } else {
        Some(WashOpts {
            keep_para_spacing: flags.contains(&"--keep-spacing"),
            auto_toc: if flags.contains(&"--auto-toc") { AutoToc::Always } else { AutoToc::IfMissing },
            paginate: !flags.contains(&"--no-paginate"),
            ..Default::default()
        })
    };
    // 先写临时文件，成功后再改名：输入输出同路径（就地覆盖）时不会边读边写同一个文件，失败也不留半成品。
    let target = std::path::Path::new(files[1]);
    let opts = OptimizeOpts { wash, ..OptimizeOpts::for_profile(device) };
    let rep = bookconv::util::produce_then_replace(&bookconv::util::tmp_beside(target, "optimizing"), target, |t| optimize::optimize_epub_file_streaming(std::path::Path::new(files[0]), t, &opts, |_, _| {}))
        .unwrap_or_else(|e| die(cli::FAILED, format!("优化失败: {e}")));
    println!("epub-optimize v{}: {} 文件/{} 章, {} → {} 字节", optimize::OPTIMIZE_VERSION, rep.total_files, rep.html_files, rep.bytes_before, rep.bytes_after);
    if let Some(w) = &rep.wash {
        println!(
            "清洗: css {} / html {} / 伪DRM剥离 {:?} / 空页 {:?} / 自动目录 {} 条 / 双id折叠 {} / 分部重建 {} 条 / ncx uid 修复 {} / ncx doctype 剥离 {} / ncx manifest id 修复 {} / 分页新增 {} 份 / 注释随节搬移 {} / 目录补节 {} / 目录改指 {}",
            w.css_files, w.html_files, w.pseudo_drm_stripped, w.empty_pages_removed, w.toc_generated, w.dup_id_tags_collapsed, w.toc_parts_restructured, w.ncx_uid_fixed, w.ncx_doctype_stripped, w.ncx_manifest_id_fixed, w.sections_paginated, w.paginate_notes_moved, w.toc_sections_added, w.ncx_targets_repaired
        );
    }
    if flags.contains(&"--check") {
        // 按路径查（图片条目不读进内存），跟书库生成时的质量门同一个实现。
        let r = bookconv::check::check_epub_file_with(target, flags.contains(&"--require-toc")).unwrap_or_else(|e| die(3, format!("质量门读产物失败: {e}")));
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
        if !r.ok {
            std::process::exit(3);
        }
    }
}

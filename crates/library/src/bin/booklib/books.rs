//! 书库本身的命令：竖排覆盖、设备列表、入库、列出、去重、删除。

use super::{usage_error, Ctx};
use crate::report::added_line;
use library::OriginalState;
use std::path::{Path, PathBuf};

pub(crate) fn cmd_vertical(ctx: &Ctx) {
    let lib = ctx.lib;
    let value = if ctx.args.has("on") {
        Some(true)
    } else if ctx.args.has("off") {
        Some(false)
    } else {
        None
    };
    let root = lib.books_root();
    for arg in ctx.rest() {
        let raw = PathBuf::from(arg);
        let path = if raw.exists() {
            raw
        } else if let Some(r) = root.as_ref().filter(|_| !raw.is_absolute()) {
            let p = r.join(&raw);
            if p.exists() {
                p
            } else {
                raw
            }
        } else {
            raw
        };
        if path.is_dir() {
            let dir = lib.dir_arg(&path);
            match lib.set_dir_vertical(&dir, value) {
                Ok(n) => println!("✓ 目录 {dir}/ 下的 {n} 本书已更新（以后入库的继承）"),
                Err(e) => ctx.fail_line(&e),
            }
        } else if let Some(m) = lib.book_by_path(&path) {
            match lib.set_book_vertical(&m.id, value) {
                Ok(()) => println!(
                    "✓ {}  {}  {}",
                    m.id,
                    m.title,
                    match value {
                        Some(true) => "强制竖排",
                        Some(false) => "强制横排",
                        None => "回到默认（横排；原书已竖排的仍竖排）",
                    }
                ),
                Err(e) => ctx.fail_line(&e),
            }
        } else {
            ctx.fail_line(&format!("{} 不是已跟踪的书或目录", path.display()));
        }
    }
}

pub(crate) fn cmd_devices(ctx: &Ctx) {
    let lib = ctx.lib;
    for p in lib.devices().iter() {
        let r = p.output_readable();
        println!(
            "{:<12} {}  {}  屏幕 {}×{}  阅读范围 {}×{}  {}",
            p.id,
            p.name,
            match p.comic.format.filter(|f| *f != p.format()) {
                Some(c) => format!(
                    "{}（漫画 {}）",
                    p.format().name().to_uppercase(),
                    c.name().to_uppercase()
                ),
                None => p.format().name().to_uppercase(),
            },
            p.screen.width,
            p.screen.height,
            r.width,
            r.height,
            if p.color { "彩色" } else { "黑白" }
        );
        if p.vertical_text {
            let ascii = match p.vertical_ascii {
                library::VerticalAscii::Fullwidth => "，ASCII 全角",
                library::VerticalAscii::Keep => "，ASCII 原样",
                library::VerticalAscii::Tcy => "",
            };
            println!("{:<12} 中文文字书竖排{ascii}", "");
        }
        match lib.device_status(p) {
            Ok(Some(at)) => println!("{:<12} ✓ 接上了：{at}", ""),
            Ok(None) => println!("{:<12} 产物放电脑上", ""),
            Err(e) => println!("{:<12} - {e}", ""),
        }
    }
}

pub(crate) fn cmd_add(ctx: &Ctx) {
    let lib = ctx.lib;
    if ctx.rest().is_empty() {
        usage_error("add 要给文件或网址（目录用 track + sync）");
    }
    for item in ctx.rest() {
        let text = item.to_string_lossy().into_owned();
        let path = Path::new(item);
        ctx.report(if text.starts_with("http://") || text.starts_with("https://") {
            lib.add_url(&text).map(|a| added_line(&a)).map_err(|e| format!("✗ {text}: {e}"))
        } else if path.is_dir() {
            Err(format!("✗ {text} 是目录：目录用 booklib track {text} 登记跟踪，再 booklib sync 入库（之后增删改都会同步）"))
        } else {
            lib.add_file(path).map(|a| added_line(&a)).map_err(|e| format!("✗ {text}: {e}"))
        });
    }
}

/// `list` 里一份产物的生成记录（`audit`）显示成几行：`✗` 逐条原文、`⚠ `（各类提醒：KEPUB CSS、质量门、KFX）逐条原文，
/// 没验证过的 `? ` 合成一行。缩进由调用方加。
pub(crate) fn audit_lines(audit: &[String]) -> Vec<String> {
    let mut out: Vec<String> = audit
        .iter()
        .filter(|l| l.starts_with('✗'))
        .map(|l| format!("体检 {l}"))
        .collect();
    for l in audit.iter().filter(|l| l.starts_with("⚠ ")) {
        out.push(format!("提醒 {l}"));
    }
    let unknown: Vec<&str> = audit.iter().filter_map(|l| l.strip_prefix("? ")).collect();
    if !unknown.is_empty() {
        out.push(format!(
            "体检 ? 没验证过 {} 种：{}",
            unknown.len(),
            unknown.join("；")
        ));
    }
    out
}

pub(crate) fn cmd_list(ctx: &Ctx) {
    let lib = ctx.lib;
    for m in lib.select(&ctx.args.texts()) {
        println!(
            "{}  {:<6} {}{}",
            m.id,
            m.source_format,
            m.title,
            if m.authors.is_empty() {
                String::new()
            } else {
                format!(" — {}", m.authors.join("、"))
            }
        );
        if !m.supported() {
            println!(
                "      - 不再支持的格式（.{}）：不再生成，已有的产物不动；不要了就 remove",
                m.content_format()
            );
        }
        match lib.original_state(&m) {
            OriginalState::Missing => println!(
                "      ✗ 原件不在了：{}（移动过就 sync 或重新 add；不要了就 remove）",
                m.source_path
            ),
            OriginalState::Touched => {
                println!("      ⚠ 原件可能改过：{}（生成前会核对）", m.source_path)
            }
            OriginalState::Present | OriginalState::NotNeeded => {}
        }
        for o in lib.outputs(&m) {
            let state = match (o.fresh, o.device_unchecked) {
                (Some(true), false) => "✓ 最新",
                (Some(true), true) => "✓ 最新 未核对设备",
                (Some(false), false) => "⚠ 过期",
                (Some(false), true) => "⚠ 过期 未核对设备",
                (None, _) => "? 未知",
            };
            let shown = o
                .path
                .strip_prefix(lib.root())
                .map(|p| p.to_path_buf())
                .unwrap_or(o.path.clone());
            // 模式已经删了的（生成记录还在）照旧 `? 未知`：sync 不会再管它，别说"下次从设备上删"
            let state = if lib.devices().get(&o.device).is_none() || lib.wants(&m, &o.device) {
                state
            } else {
                "✗ 不在这台的跟踪范围（下次 sync 从设备上删）"
            };
            let vertical = lib
                .devices()
                .get(&o.device)
                .filter(|p| p.vertical_text)
                .map(|p| {
                    if lib.vertical_for(&m, p).unwrap_or(false) {
                        "竖排"
                    } else {
                        "横排"
                    }
                });
            println!(
                "      {:<20} {state}  {}{}",
                o.device,
                shown.display(),
                vertical.map_or(String::new(), |v| format!("  {v}"))
            );
            // 体检（生成时记下的）和各类提醒（KEPUB CSS、质量门、KFX）：逐条列出；没验证过的合成一行
            for line in audit_lines(&o.audit) {
                println!("        {line}");
            }
        }
    }
    for id in lib.broken() {
        eprintln!("⚠ 条目 {id} 的 meta.json 读不出来（写坏了）：booklib remove {id} 删掉，或重新 add 原文件覆盖");
    }
}

pub(crate) fn cmd_dedupe(ctx: &Ctx) {
    let dirs: Vec<PathBuf> = ctx.rest().iter().map(PathBuf::from).collect();
    let mb = |b: u64| b as f64 / 1048576.0;
    ctx.report(ctx.lib.dedupe(&dirs).map(|r| {
        let mut lines = vec![format!(
            "✓ {} 本书改成只存索引，删掉书库里的副本 {:.1} MB",
            r.migrated,
            mb(r.freed_bytes)
        )];
        for m in &r.kept {
            lines.push(format!(
                "  ? 没找到原件，副本保留：{}  {}（原来在 {}）",
                m.id,
                m.title,
                if m.source_path.is_empty() {
                    &m.source
                } else {
                    &m.source_path
                }
            ));
        }
        lines.join("\n")
    }));
}

pub(crate) fn cmd_remove(ctx: &Ctx) {
    let lib = ctx.lib;
    if ctx.rest().is_empty() {
        usage_error("remove 要给 id");
    }
    for id in ctx.args.texts() {
        let tracked = lib.tracked_original(&id);
        ctx.report(
            lib.remove(&id)
                .map(|title| format!("✓ 删除 {id}  {title}"))
                .map_err(|e| format!("✗ {e}")),
        );
        if let Some(p) = tracked.filter(|_| !lib.entry_exists(&id)) {
            println!("  ⚠ 这本的原件在跟踪的目录里：{}\n    下次 sync 会再入库；要彻底不要，就从书目录里删掉原件", p.display());
        }
    }
}

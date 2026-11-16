//! `meta` 的联网补元数据与查看（`meta --edit` 改文件的那条在 `meta_edit` 里）。

use super::{fail, Ctx};
use library::{CoverResult, InfoResult};
use std::path::Path;

/// 元数据一行摘要：来源、简介字数、标签、参考版本。
pub(crate) fn info_summary(i: &library::BookInfo) -> String {
    let mut parts = vec![i.source.clone()];
    if !i.original_title.is_empty() {
        parts.push(format!("原作名 {}", i.original_title));
    }
    if !i.first_published.is_empty() {
        parts.push(format!("{} 年首次出版", i.first_published));
    }
    if !i.description.is_empty() {
        parts.push(format!("简介 {} 字", i.description.chars().count()));
    }
    if !i.subjects.is_empty() {
        parts.push(format!("标签 {}", i.subjects.join("、")));
    }
    if let Some(e) = &i.edition {
        let v: Vec<&str> = [e.publisher.as_str(), e.pubdate.as_str()]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect();
        let tr = if e.translators.is_empty() {
            String::new()
        } else {
            format!(" {} 译", e.translators.join("、"))
        };
        parts.push(format!("参考版本 {}{tr}", v.join(" ")));
    }
    parts.join("；")
}

/// `meta --show` 的一本：书里写的元数据（读原件的 OPF），和联网找来的。
fn show_meta(m: &library::Meta, fail_line: &mut dyn FnMut(String)) {
    println!(
        "{}  {}{}",
        m.id,
        m.title,
        if m.authors.is_empty() {
            String::new()
        } else {
            format!(" — {}", m.authors.join("、"))
        }
    );
    println!("  原件：{}", m.source_path);
    let path = Path::new(&m.source_path);
    if m.content_format() != "epub" {
        println!(
            "  书里：（{} 来源，没有 OPF 元数据）",
            m.content_format().to_uppercase()
        );
    } else if !path.is_file() {
        fail_line(format!(
            "  ✗ 原件不在了：{}（改名或移动过就 booklib sync）",
            m.source_path
        ));
    } else {
        match bookconv::opfmeta::read_epub(path) {
            Ok(fields) => {
                for (field, values) in fields.iter().filter(|(_, v)| !v.is_empty()) {
                    let v = values.join("; ");
                    let v = if v.chars().count() > 80 {
                        format!(
                            "{}…（{} 字）",
                            v.chars().take(80).collect::<String>(),
                            v.chars().count()
                        )
                    } else {
                        v
                    };
                    println!("  书里 {}：{}", field.label(), v.replace('\n', " "));
                }
                match bookconv::epubzip::cover_image_of(path) {
                    Some((ext, bytes)) => {
                        let dims = image::load_from_memory(&bytes)
                            .map(|i| format!("，{}×{}", i.width(), i.height()))
                            .unwrap_or_default();
                        println!("  书里 封面：有（{ext}{dims}，{} KB）", bytes.len() / 1024);
                    }
                    None => println!("  书里 封面：无"),
                }
            }
            Err(e) => fail_line(format!("  ✗ 读不出元数据：{e}")),
        }
    }
    match &m.info {
        Some(i) => println!("  找来：{}", info_summary(i)),
        None => println!("  找来：没找过（booklib meta --fetch）"),
    }
    if let Some(c) = &m.cover {
        println!("  找来 封面：{}（{}）", c.work, c.source_url);
    }
}

pub(crate) fn cmd_meta(ctx: &Ctx) {
    if ctx.args.has("show") {
        meta_show(ctx)
    } else {
        meta_fetch(ctx)
    }
}

/// `meta --show`：跟踪目录里的书的元数据。
fn meta_show(ctx: &Ctx) {
    let lib = ctx.lib;
    let tracked = lib.tracked();
    let books: Vec<_> = lib
        .select(&ctx.args.texts())
        .into_iter()
        .filter(|m| {
            !m.source_path.is_empty()
                && tracked
                    .iter()
                    .any(|d| Path::new(&m.source_path).starts_with(d))
        })
        .collect();
    if books.is_empty() {
        fail(if tracked.is_empty() {
            "没有跟踪的目录（booklib track 目录）"
        } else {
            "跟踪目录里没有匹配的书（booklib list 查看书库）"
        });
    }
    for (i, m) in books.iter().enumerate() {
        if i > 0 {
            println!();
        }
        show_meta(m, &mut |e| ctx.fail_line(&e));
    }
    println!(
        "\n共 {} 本（跟踪目录：{}）",
        books.len(),
        tracked
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join("、")
    );
}

/// `meta --fetch [--force] [--clear]`：联网补元数据（或去掉找来的）。
fn meta_fetch(ctx: &Ctx) {
    let lib = ctx.lib;
    let books = lib.select(&ctx.args.texts());
    if books.is_empty() {
        fail("没有匹配的书（booklib list 查看书库）");
    }
    let (force, clear) = (ctx.args.has("force"), ctx.args.has("clear"));
    for m in &books {
        if !m.supported() && !clear {
            println!(
                "- 跳过 {}：{}",
                m.title,
                library::unsupported(m.content_format())
            );
            continue;
        }
        if lib.offline() {
            ctx.fail_line("✗ 连不上网，这一轮中止（没查的书下次再查）");
            break;
        }
        if clear {
            ctx.report(lib.clear_metadata(m).map(|had| {
                if had {
                    format!("✓ 去掉找来的元数据和封面  {}", m.title)
                } else {
                    format!("= 本来就没有找来的元数据  {}", m.title)
                }
            }));
            continue;
        }
        // 漫画也找（2026-10-12 定，推翻 2026-09-30 的「漫画不找」）：书名的卷次去掉再搜（`matching::base_title`）
        match lib.fetch_metadata(m, force) {
            Ok((info, cover)) => {
                // 一半有结果、一半网络出错没查成：有结果的那半已存下，整本算失败（退出码 2），下次再查没查成的
                let partial = matches!(info, InfoResult::Failed(_))
                    || matches!(cover, CoverResult::Failed(_));
                let info = match info {
                    InfoResult::Found(i) => format!("元数据 ← {}", info_summary(&i)),
                    InfoResult::Shared(i, from) => {
                        format!("元数据 ← 同一套的《{from}》找过的（{}）", info_summary(&i))
                    }
                    InfoResult::Existing(i) => format!("元数据 = 已有 ← {}", info_summary(&i)),
                    InfoResult::NotFound(why) => format!("元数据 ? {why}"),
                    InfoResult::Failed(why) => format!("元数据 ✗ {why}"),
                };
                let cover = match cover {
                    CoverResult::Found(c) => format!("封面 ← {}（{}）", c.work, c.source_url),
                    CoverResult::Existing(c) => format!("封面 = 已有 ← {}", c.work),
                    CoverResult::HasCover => "封面 = 书里有".to_string(),
                    CoverResult::Generated(c, why) => format!("封面 ◇ {why}，{}", c.work),
                    CoverResult::Failed(why) => format!("封面 ✗ {why}"),
                };
                let mark = if partial { "⚠" } else { "✓" };
                ctx.report(if partial { Err } else { Ok }(format!(
                    "{mark} {}\n      {info}\n      {cover}",
                    m.title
                )));
            }
            Err(e) => ctx.report(Err(format!("✗ {}: {e}", m.title))),
        }
    }
    if !clear {
        println!("  简介、标签、作者国籍、译者、出版社、ISBN、封面在生成产物时补进书里（书里已有的不动）：booklib sync 会把这些书判为过期并重建");
    }
}

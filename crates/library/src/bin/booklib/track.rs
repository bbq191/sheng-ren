//! `track` / `untrack`：登记、取消跟踪范围，以及按设备列出跟踪范围。

use super::{usage_error, Args, Ctx};
use crate::sync::parse_devices;
use library::{Library, TrackChange, UntrackChange};
use std::path::{Path, PathBuf};

pub(crate) fn cmd_track(ctx: &Ctx) {
    track_or_untrack(ctx, true);
}

pub(crate) fn cmd_untrack(ctx: &Ctx) {
    track_or_untrack(ctx, false);
}

/// `track`/`untrack` 的 `--device=`：模式 id（不写、写了 all 是空＝全部设备）。写了不存在的模式报用法错。
fn device_ids(args: &Args, lib: &Library) -> Vec<String> {
    let all = args
        .opts
        .iter()
        .filter(|(k, _)| k == "device")
        .flat_map(|(_, v)| v.split(','))
        .any(|v| v.trim() == "all");
    if all || !args.opts.iter().any(|(k, _)| k == "device") {
        return Vec::new();
    }
    parse_devices(args, lib)
        .into_iter()
        .map(|p| p.id.clone())
        .collect()
}

fn track_or_untrack(ctx: &Ctx, track: bool) {
    let lib = ctx.lib;
    if let Some(r) = ctx.args.opt("root").filter(|_| track) {
        if !ctx.rest().is_empty()
            || ctx.args.opts.iter().any(|(k, _)| k == "device")
            || ctx.args.has("top")
        {
            usage_error("--root= 单独用：booklib track --root=目录");
        }
        ctx.report(
            lib.set_books_root(Path::new(r))
                .map(|changed| {
                    if changed {
                        format!("✓ 书根目录设为 {r}（已传的书下次 sync 挪到按它算的位置）")
                    } else {
                        format!("= 书根目录本来就是 {r}")
                    }
                })
                .map_err(|e| format!("✗ {e}")),
        );
        print_tracked(lib, &[]);
        return;
    }
    if ctx.rest().is_empty() {
        if track {
            let devices = device_ids(ctx.args, lib);
            print_tracked(lib, &devices);
            return;
        }
        usage_error(&format!("{} 要给目录或书", ctx.cmd));
    }
    let devices = device_ids(ctx.args, lib);
    let top = ctx.args.has("top");
    let to = if devices.is_empty() {
        "全部设备".to_string()
    } else {
        devices.join("、")
    };
    for d in ctx.rest() {
        let d = match resolve_track_path(lib, Path::new(d), track) {
            Ok(p) => p,
            Err(e) => {
                ctx.report(Err(e));
                continue;
            }
        };
        let d = d.as_path();
        let kind = if d.is_file() { "单本" } else { "目录" };
        ctx.report(if track {
            lib.track_with(d, &devices, top).map(|c| {
                // 书库里记的是规范化的绝对路径：相对路径、经软链接的路径要先换成它，否则对不上书根目录
                let placed = placement_shown(
                    &lib.entry_placement(&library::Tracked {
                        path: std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf()),
                        file: d.is_file(),
                        devices: devices.clone(),
                        top,
                    }),
                    d.is_file(),
                );
                match c {
                    TrackChange::New => format!("✓ 开始跟踪{kind} {}，给 {to} → 设备上 {placed}（运行 booklib sync 入库、传）", d.display()),
                    TrackChange::Changed => format!("✓ 已在跟踪{kind} {}，加上 {to} → 设备上 {placed}（运行 booklib sync 生效）", d.display()),
                    TrackChange::Same => format!("= 已在跟踪{kind} {}", d.display()),
                }
            })
        } else {
            lib.untrack_with(d, &devices).map(|c| match c {
                UntrackChange::Removed(n) => format!("✓ 不再跟踪 {}：下次 sync 从书库删掉 {n} 本（按上次 sync 看到的算；连同设备上的，还被别的跟踪条目覆盖的照留）", d.display()),
                UntrackChange::Narrowed(n) => format!("✓ {} 不再给 {to}：下次 sync 从这几台上删掉 {n} 本（按上次 sync 看到的算）", d.display()),
                UntrackChange::NotTracked => format!("= 本来就没在跟踪 {}{}", d.display(), if devices.is_empty() { String::new() } else { format!("（给 {to} 的）") }),
            })
        }
        .map_err(|e| format!("✗ {e}")));
    }
    print_tracked(lib, &[]);
}

/// `track`/`untrack` 的路径：相对路径先按当前目录找，找不到再按书根目录找（2026-10-19 要：`track 优秀漫画/死亡筆記`
/// 不用每次写全路径）；绝对路径照旧。两处都先按原样找，没有再补 `.epub`、`.cbz`（同日维护者要：`track 欧美侦探/罗杰疑案`；
/// 两个都有时报错，要写上扩展名）。`track` 都找不到时报错；`untrack` 的（原件可能已经删了）在书根目录下跟踪着也算。
fn resolve_track_path(lib: &Library, p: &Path, track: bool) -> Result<PathBuf, String> {
    let tracked = if track { Vec::new() } else { lib.tracked() };
    let root = lib.books_root();
    let bases: Vec<PathBuf> = if p.is_absolute() {
        vec![p.to_path_buf()]
    } else {
        std::iter::once(p.to_path_buf())
            .chain(root.as_ref().map(|r| r.join(p)))
            .collect()
    };
    for q in &bases {
        if q.exists() || tracked.contains(q) {
            return Ok(q.clone());
        }
        let found: Vec<PathBuf> = with_book_ext(q)
            .into_iter()
            .filter(|c| c.is_file() || tracked.contains(c))
            .collect();
        match found.as_slice() {
            [] => {}
            [one] => return Ok(one.clone()),
            _ => {
                return Err(format!(
                    "✗ {} 有 .epub 和 .cbz 两本，写上扩展名",
                    q.display()
                ))
            }
        }
    }
    if !track {
        return Ok(p.to_path_buf());
    }
    Err(match root {
        Some(r) if !p.is_absolute() => format!(
            "✗ 找不到 {}：当前目录和书根目录 {} 里都没有（也试过补 .epub、.cbz）",
            p.display(),
            r.display()
        ),
        _ => format!("✗ 找不到 {}（也试过补 .epub、.cbz）", p.display()),
    })
}

/// 补上书的扩展名的两个候选（`罗杰疑案` → `罗杰疑案.epub`、`罗杰疑案.cbz`）；以 `/` 结尾的（写的是目录）不补。
fn with_book_ext(p: &Path) -> Vec<PathBuf> {
    let s = p.as_os_str();
    if s.as_encoded_bytes().ends_with(b"/") {
        return Vec::new();
    }
    ["epub", "cbz"]
        .iter()
        .map(|e| {
            let mut o = s.to_owned();
            o.push(".");
            o.push(e);
            PathBuf::from(o)
        })
        .collect()
}

/// 条目在设备上的位置文案：目录是它的文件夹（末尾带 `/`），单本是所在目录；空＝设备存储根目录。
fn placement_shown(rel: &Path, file: bool) -> String {
    if rel.as_os_str().is_empty() {
        return "根目录".into();
    }
    let s = rel.display().to_string();
    if file {
        s
    } else {
        format!("{s}/")
    }
}

/// 按设备列出跟踪范围：每台的目录、单本（给全部设备的标出来），以及这条在设备上放在哪。
/// `only` 是 `--device=` 给的模式 id（空＝全部设备）。
fn print_tracked(lib: &Library, only: &[String]) {
    let entries = lib.tracked_entries();
    if entries.is_empty() {
        println!("  还没有跟踪任何目录或书");
        return;
    }
    if let Some(r) = lib.books_root() {
        println!("  书根目录：{}（设备上按相对它的路径放）", r.display());
    }
    let shown = lib
        .devices()
        .iter()
        .filter(|p| only.is_empty() || only.iter().any(|id| id == &p.id));
    for p in shown {
        let mine: Vec<&library::Tracked> = entries.iter().filter(|e| e.for_device(&p.id)).collect();
        println!(
            "  {}：{}",
            p.id,
            if mine.is_empty() { "（没有）" } else { "" }
        );
        for e in &mine {
            let kind = if e.file { "单本" } else { "目录" };
            let scope = if e.devices.is_empty() {
                "（全部设备）"
            } else {
                ""
            };
            let placed = placement_shown(&lib.entry_placement(e), e.file);
            println!(
                "    {kind} {}{scope} → 设备上 {placed}",
                e.path.display()
            );
        }
        // --top 的目录在设备上同名：会并进同一个文件夹（书名也一样的，后来的那本带 id 后缀），只提示
        let mut seen: std::collections::BTreeMap<String, Vec<&Path>> =
            std::collections::BTreeMap::new();
        for e in mine.iter().filter(|e| e.top && !e.file) {
            if let Some(n) = e.path.file_name() {
                seen.entry(n.to_string_lossy().to_lowercase())
                    .or_default()
                    .push(&e.path);
            }
        }
        for (name, paths) in seen.iter().filter(|(_, v)| v.len() > 1) {
            println!(
                "    ⚠ 设备上会并进同一个文件夹「{name}」：{}",
                paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            );
        }
    }
}

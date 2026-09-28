//! ebook-meta：查看、改写 EPUB 的元数据（OPF 里的 Dublin Core）和封面。改的是文件本身（不是阅读器的旁路缓存）。
//!
//! 只改 OPF（改书名时连 NCX 里的书名）和封面图这几个条目，其余条目原样拷贝，正文一个字节都不变。
//! 写前缺省备份成 `<文件>.bak-<时间戳>`；写入先写临时文件再改名，中途失败原文件不动。

use bookconv::opfmeta::{self, DcField, Edits};
use bookconv::util::cli;
use std::path::{Path, PathBuf};

const USAGE: &str = "用法:
  ebook-meta 书.epub                                    查看
  ebook-meta 书.epub --title 书名 --author 作者甲 --author 作者乙
  ebook-meta 书.epub --language zh --publisher 出版社 --date 2026-09-28 --description 简介…
  ebook-meta 书.epub --tag 小说 --tag 科幻               标签整体替换
  ebook-meta 书.epub --cover 封面.jpg                    换封面（书里有封面图就原地换掉，没有就加上）
  ebook-meta 书.epub --get-cover 封面.jpg                取出封面
选项:
  --title --author --language --publisher --description --tag --date --identifier
      --author/--tag/--identifier 可重复，给出即整体替换（给几个就是最终的几个）；其余是单值；值给空字符串 = 删掉这个字段
      --identifier 不动 OPF 唯一标识（unique-identifier 指向的那个），只替换其余标识符
  --no-backup   不写 .bak 备份";

fn fail(msg: &str) -> ! {
    cli::die(cli::USAGE, msg)
}

fn show(path: &Path) {
    let meta = opfmeta::read_epub(path).unwrap_or_else(|e| fail(&e));
    for (field, values) in meta.iter().filter(|(_, v)| !v.is_empty()) {
        println!("{}: {}", field.label(), values.join("; "));
    }
    if let Some((ext, bytes)) = bookconv::epubzip::cover_image_of(path) {
        let dims = image::load_from_memory(&bytes).map(|i| format!("{}×{}", i.width(), i.height())).unwrap_or_default();
        println!("封面: 有（{ext}，{dims}，{} KB）", bytes.len() / 1024);
    } else {
        println!("封面: 无");
    }
}

/// 同目录下不重名的备份文件名：`<文件>.bak-<时间戳>`（1 秒内连跑多次加序号）。
fn backup_path(file: &Path) -> PathBuf {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut name = file.as_os_str().to_os_string();
    name.push(format!(".bak-{secs}"));
    let mut p = PathBuf::from(&name);
    let mut n = 1;
    while p.exists() {
        let mut alt = name.clone();
        alt.push(format!("-{n}"));
        p = PathBuf::from(alt);
        n += 1;
    }
    p
}

fn main() {
    cli::restore_sigpipe();
    let mut args = std::env::args_os().skip(1);
    let mut file: Option<PathBuf> = None;
    let mut singles: Vec<(DcField, String)> = Vec::new();
    let mut multis: Vec<(DcField, Vec<String>)> = Vec::new();
    let (mut cover, mut get_cover, mut backup) = (None::<PathBuf>, None::<PathBuf>, true);
    while let Some(a) = args.next() {
        let a = a.to_string_lossy().into_owned();
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = || inline.clone().or_else(|| args.next().map(|v| v.to_string_lossy().into_owned())).unwrap_or_else(|| fail(&format!("{key} 要给值\n{USAGE}")));
        match key.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "--title" => singles.push((DcField::Title, value())),
            "--language" => singles.push((DcField::Language, value())),
            "--publisher" => singles.push((DcField::Publisher, value())),
            "--description" => singles.push((DcField::Description, value())),
            "--date" => singles.push((DcField::Date, value())),
            "--author" | "--tag" | "--identifier" => {
                let field = match key.as_str() {
                    "--author" => DcField::Creator,
                    "--tag" => DcField::Subject,
                    _ => DcField::Identifier,
                };
                let v = value();
                match multis.iter_mut().find(|(f, _)| *f == field) {
                    Some((_, list)) => list.push(v),
                    None => multis.push((field, vec![v])),
                }
            }
            "--cover" => cover = Some(PathBuf::from(value())),
            "--get-cover" => get_cover = Some(PathBuf::from(value())),
            "--no-backup" => backup = false,
            k if k.starts_with('-') => fail(&format!("不认识的选项 {k}\n{USAGE}")),
            _ if file.is_none() => file = Some(PathBuf::from(a)),
            _ => fail(&format!("只能给一个文件\n{USAGE}")),
        }
    }
    let file = file.unwrap_or_else(|| fail(USAGE));
    if !file.is_file() {
        fail(&format!("文件不存在：{}", file.display()));
    }
    if let Some(out) = get_cover {
        let (ext, bytes) = bookconv::epubzip::cover_image_of(&file).unwrap_or_else(|| fail("书里没有封面"));
        bookconv::util::write_atomic(&out, &bytes).unwrap_or_else(|e| fail(&e));
        println!("已取出封面（{ext}）：{}", out.display());
        return;
    }

    let mut edits = Edits::default();
    for (field, v) in singles {
        edits.set.retain(|(f, _)| *f != field); // 同一选项给了多次，以最后一次为准
        edits.set.push((field, if v.trim().is_empty() { vec![] } else { vec![v] }));
    }
    for (field, list) in multis {
        let list: Vec<String> = list.into_iter().filter(|v| !v.trim().is_empty()).collect();
        edits.set.push((field, list));
    }
    if let Some(p) = &cover {
        edits.cover = Some(std::fs::read(p).unwrap_or_else(|e| fail(&format!("{}: {e}", p.display()))));
    }
    if edits.is_empty() {
        show(&file);
        return;
    }

    // 先写临时文件；备份（在改名之前）和改名任何一步失败，临时文件都清掉、原文件不动。
    let backup_then = || -> Result<(), String> {
        if backup {
            let b = backup_path(&file);
            std::fs::copy(&file, &b).map_err(|e| format!("备份失败，没改：{e}"))?;
            println!("已备份：{}", b.display());
        }
        Ok(())
    };
    let edited = bookconv::util::produce_then_replace_with(&bookconv::util::tmp_beside(&file, "ebook-meta"), &file, |tmp| opfmeta::edit_epub(&file, tmp, &edits).map_err(|e| format!("没改：{e}")), backup_then);
    let report = edited.unwrap_or_else(|e| fail(&e));
    let what: Vec<&str> = report.fields.iter().map(|f| f.label()).chain(report.cover_replaced.map(|r| if r { "封面（换掉原图）" } else { "封面（新加）" })).collect();
    println!("已写入：{}（{}）", file.display(), what.join("、"));
    show(&file);
}

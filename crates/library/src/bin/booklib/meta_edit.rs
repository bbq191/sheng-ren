//! `booklib meta --edit`：查看、改写一个 EPUB 文件的元数据（OPF 里的 Dublin Core）和封面。改的是文件本身（不是阅读器的旁路缓存），
//! 和书库无关（不打开书库、不加锁）。
//!
//! 改 OPF（改书名时连 NCX 里的书名）和封面；写出的书和 booklib 的产物一样过一遍 EPUB 3 规范整理（XHTML 修成合法 XML、
//! OPF 升到 3.0、补导航文档），`dcterms:modified` 写成现在的时间。可见文字一个不动。
//! 参数统一：任何选项给空字符串 = 删掉这一项（`--cover ""` 就是去掉封面）；`--名字 值` 和 `--名字=值` 两种写法都认。
//! 不备份（2026-10-01 用户定）；写入先写临时文件再改名，中途失败原文件不动。

use bookconv::opfmeta::{self, DcField, Edits};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const USAGE: &str = r#"用法:
  booklib meta --edit 书.epub                                    查看
  booklib meta --edit 书.epub --title 书名 --author 作者甲 --author 作者乙
  booklib meta --edit 书.epub --language zh --publisher 出版社 --date 2026-09-28 --description 简介…
  booklib meta --edit 书.epub --tag 小说 --tag 科幻               标签整体替换
  booklib meta --edit 书.epub --cover 封面.jpg                    换封面（书里有封面图就原地换掉，没有就加上）
  booklib meta --edit 书.epub --cover ""                         去掉封面（封面声明、只放封面的那一页、封面图；正文别处用着的图留着）
  booklib meta --edit 书.epub --get-cover 封面.jpg                取出封面
选项（--名字 值 或 --名字=值）:
  --title --author --language --publisher --description --tag --date --identifier --cover
      值给空字符串 = 删掉这一项（字段、封面都一样）
      --author/--tag/--identifier 可重复，给出即整体替换（给几个就是最终的几个）；其余是单值
      --identifier 不动 OPF 唯一标识（unique-identifier 指向的那个），只替换其余标识符
改的是任意一个 EPUB 文件，和书库无关；改了跟踪目录里的原件，下次 sync 会当成新版本重新入库"#;

fn usage(msg: &str) -> ! {
    eprintln!("{msg}\n\n{USAGE}");
    std::process::exit(1)
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2)
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

/// `args`：`meta` 之后、去掉 `--edit` 的参数（`--library=` 已去掉：改文件用不到书库）。
pub fn run(args: Vec<OsString>) {
    let mut args = args.into_iter();
    let mut file: Option<PathBuf> = None;
    let mut singles: Vec<(DcField, String)> = Vec::new();
    let mut multis: Vec<(DcField, Vec<String>)> = Vec::new();
    let (mut cover, mut get_cover) = (None::<String>, None::<PathBuf>);
    while let Some(a) = args.next() {
        let a = a.to_string_lossy().into_owned();
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = || inline.clone().or_else(|| args.next().map(|v| v.to_string_lossy().into_owned())).unwrap_or_else(|| usage(&format!("{key} 要给值")));
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
            "--cover" => cover = Some(value()),
            "--get-cover" => get_cover = Some(PathBuf::from(value())),
            "--fetch" | "--force" | "--clear" => usage(&format!("{key} 是 meta --fetch 的选项，不能和 --edit 一起用")),
            k if k.starts_with('-') => usage(&format!("meta --edit 不认识选项 {k}")),
            _ if file.is_none() => file = Some(PathBuf::from(a)),
            _ => usage("meta --edit 只能给一个文件（路径里有空格时要整个加引号）"),
        }
    }
    let file = file.unwrap_or_else(|| usage("meta --edit 要给一个 EPUB 文件"));
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
    edits.cover = cover.map(|p| {
        if p.trim().is_empty() {
            opfmeta::CoverEdit::Remove
        } else {
            opfmeta::CoverEdit::Set(std::fs::read(&p).unwrap_or_else(|e| fail(&format!("{p}: {e}"))))
        }
    });
    if edits.is_empty() {
        show(&file);
        return;
    }
    edits.modified = Some(bookconv::util::utc_now_w3c());
    edits.normalize = true;

    // 先写临时文件再改名（不备份：2026-10-01 用户定），中途失败原文件不动。
    let edited = bookconv::util::produce_then_replace(&bookconv::util::tmp_beside(&file, "meta-edit"), &file, |tmp| opfmeta::edit_epub(&file, tmp, &edits).map_err(|e| format!("没改：{e}")));
    let report = edited.unwrap_or_else(|e| fail(&e));
    let removed = report.cover_removed.as_ref().map(|gone| if gone.is_empty() { "封面（书里本来就没有）".to_string() } else { format!("封面（去掉 {}）", gone.join("、")) });
    let what: Vec<String> = report
        .fields
        .iter()
        .map(|f| f.label().to_string())
        .chain(report.cover_replaced.map(|r| if r { "封面（换掉原图）" } else { "封面（新加）" }.to_string()))
        .chain(removed)
        .collect();
    println!("已写入：{}（{}）", file.display(), what.join("、"));
    show(&file);
}

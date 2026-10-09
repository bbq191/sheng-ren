//! MOBI 词典 → StarDict，给 KOReader 查词（只转自己手上的词典，产物不进仓库）。
//!
//! 用法: mobi-dict-to-stardict 词典.mobi 输出目录 [--name=名称]
//!   在 输出目录/名称/ 下写 名称.ifo、名称.idx、名称.dict（名称缺省取 MOBI 文件名）。整个目录拷到 KOReader 的 data/dict/ 下。
//! 退出码: 0 成功；1 用法错；2 读取/转换/写出失败（先写临时目录再改名，失败不留半成品，已有的同名词典不动）。

use bookconv::util::cli::{self, die};
use bookconv::util::tmp_beside;
use std::path::Path;

fn main() {
    cli::restore_sigpipe();
    // `args_os`：路径不是 UTF-8 时 `std::env::args` 会 panic
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut name: Option<String> = None;
    let mut files: Vec<&Path> = Vec::new();
    for raw in &args {
        let a = raw.to_string_lossy();
        match a.strip_prefix("--name=") {
            Some(n) if !n.trim().is_empty() => name = Some(n.trim().to_string()),
            _ if a.starts_with("--") => die(cli::USAGE, format!("不认识的参数 {a}\n用法: mobi-dict-to-stardict 词典.mobi 输出目录 [--name=名称]")),
            _ => files.push(Path::new(raw)),
        }
    }
    if files.len() != 2 {
        die(cli::USAGE, "用法: mobi-dict-to-stardict 词典.mobi 输出目录 [--name=名称]");
    }
    let src = files[0];
    let name = name.unwrap_or_else(|| src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let name = bookconv::util::sanitize_filename(&name, "dict");

    let data = cli::read_or_die(src);
    let dict = mobidict::read(&data).unwrap_or_else(|e| die(cli::FAILED, format!("读 {}: {e}", src.display())));
    let (out, stats) = mobidict::to_stardict(&dict, &name).unwrap_or_else(|e| die(cli::FAILED, format!("转 {}: {e}", src.display())));
    let target = files[1].join(&name);
    write_dir(&target, &name, &out).unwrap_or_else(|e| die(cli::FAILED, e));
    println!(
        "mobi-dict-to-stardict: {} → {}/（{} 个词头，{} 段释义，.dict {} 字节）",
        src.display(),
        target.display(),
        stats.words,
        stats.articles,
        out.dict.len()
    );
}

/// 先写到同级临时目录，写完落盘再换上去；原来有同名目录的，换上之后才删旧的。
/// 换的是整个目录（`rename` 不能盖掉非空目录），所以不走 `util::produce_then_replace`，只共用临时名的规则。
fn write_dir(target: &Path, name: &str, f: &mobidict::stardict::Files) -> Result<(), String> {
    let parent = target.parent().ok_or("输出目录不对")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("建 {}: {e}", parent.display()))?;
    let tmp = tmp_beside(target, "writing");
    let old = tmp_beside(target, "old");
    let _ = std::fs::remove_dir_all(&tmp);
    let result = (|| {
        std::fs::create_dir(&tmp).map_err(|e| format!("建 {}: {e}", tmp.display()))?;
        for (ext, bytes) in [("ifo", f.ifo.as_bytes()), ("idx", &f.idx[..]), ("dict", &f.dict[..])] {
            let p = tmp.join(format!("{name}.{ext}"));
            std::fs::write(&p, bytes).map_err(|e| format!("写 {}: {e}", p.display()))?;
            std::fs::File::open(&p).and_then(|h| h.sync_all()).map_err(|e| format!("落盘 {}: {e}", p.display()))?;
        }
        let _ = std::fs::remove_dir_all(&old);
        if target.exists() {
            std::fs::rename(target, &old).map_err(|e| format!("挪开旧的 {}: {e}", target.display()))?;
        }
        std::fs::rename(&tmp, target).map_err(|e| format!("换上 {}: {e}", target.display()))?;
        bookconv::util::sync_parent(target).map_err(|e| format!("落盘 {}: {e}", parent.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
        if !target.exists() && old.exists() {
            let _ = std::fs::rename(&old, target);
        }
        return result;
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

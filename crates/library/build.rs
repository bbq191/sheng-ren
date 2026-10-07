//! 把编译时的 git 提交号、提交日期写进环境变量，`booklib --version` 显示（不在 git 仓库里编译时是"未知"）。

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    // 提交、切分支会改这几个文件：变了才重新取。不标"有没有没提交的改动"：改源码不会让这里重跑，标出来会过时
    if let (Some(dir), Some(common)) = (git(&["rev-parse", "--absolute-git-dir"]), git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])) {
        for p in [format!("{dir}/HEAD"), format!("{common}/refs/heads"), format!("{common}/packed-refs")] {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    let commit = git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "未知".into());
    let date = git(&["log", "-1", "--format=%cd", "--date=short"]).unwrap_or_default();
    println!("cargo:rustc-env=BOOKLIB_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=BOOKLIB_GIT_DATE={date}");
}

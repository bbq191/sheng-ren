//! EPUB → KFX（Kindle 自带阅读器的增强排版格式）。
//! 用法：epub-to-kfx [--id=数字] 优化过的.epub 输出.kfx
//! `--id` 固定唯一 ID（同一本书做几个测试版本时用，Kindle 按 ID 认书）；不给就按书的 OPF 唯一标识符派生。
//! 输入应先经 `epub-optimize --device=kindle` 优化；这里只做格式转换。
//! 退出码：0 成功；1 用法错；2 读写或转换失败（先写临时文件再改名，失败不留半成品）。

use bookconv::util::cli::{self, die};

const USAGE: &str = "用法：epub-to-kfx [--id=数字] 优化过的.epub 输出.kfx";

fn main() {
    cli::restore_sigpipe();
    let mut opts = kfx::Opts::default();
    let mut args = Vec::new();
    for a in std::env::args().skip(1) {
        if a == "-h" || a == "--help" {
            println!("{USAGE}");
            return;
        }
        match a.strip_prefix("--id=") {
            Some(n) => opts.fixed_id = Some(n.parse().unwrap_or_else(|_| die(cli::USAGE, "--id 要给数字"))),
            None if a.starts_with("--") => die(cli::USAGE, format!("不认识的参数 {a}\n{USAGE}")),
            None => args.push(a),
        }
    }
    let [input, output] = args.as_slice() else { die(cli::USAGE, USAGE) };
    let (kfx, warnings) = kfx::epub_to_kfx_from(cli::open_or_die(input), &opts).unwrap_or_else(|e| die(cli::FAILED, format!("{input}：{e}")));
    for w in &warnings {
        eprintln!("警告：{w}");
    }
    cli::write_or_die(output, &kfx);
    println!("{output}：{} 字节", kfx.len());
}

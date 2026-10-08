//! EPUB → KFX（Kindle 自带阅读器的增强排版格式）。
//! 用法：epub-to-kfx [--id=数字] [--device=阅读模式] 优化过的.epub 输出.kfx
//! `--id` 固定唯一 ID（同一本书做几个测试版本时用，Kindle 按 ID 认书）；不给就按书的 OPF 唯一标识符派生。
//! `--device` 给 `@media` 特性条件（`min-width` 等）求值用的阅读范围、屏幕，缺省 `kindle`（和书库一致）。
//! 输入应先经 `epub-optimize --device=kindle` 优化；这里只做格式转换。
//! `epub-to-kfx --styles 书.epub`：不写 KFX，按 CSS 原样把每个文字块的样式打到标准输出（`kfx::epub_text_styles`，`tools/kfx/s2kdev.py` 用）。
//! 退出码：0 成功；1 用法错；2 读写或转换失败（先写临时文件再改名，失败不留半成品）。

use bookconv::util::cli::{self, die};

const USAGE: &str = "用法：epub-to-kfx [--id=数字] [--device=阅读模式，缺省 kindle] 优化过的.epub 输出.kfx\n      epub-to-kfx --styles [--device=阅读模式] 书.epub";

fn main() {
    cli::restore_sigpipe();
    let mut opts = kfx::Opts::default();
    let mut device = "kindle".to_string();
    let mut styles = false;
    let mut args = Vec::new();
    for a in std::env::args().skip(1) {
        if a == "-h" || a == "--help" {
            println!("{USAGE}");
            return;
        }
        if a == "--styles" {
            styles = true;
        } else if let Some(n) = a.strip_prefix("--id=") {
            opts.fixed_id = Some(n.parse().unwrap_or_else(|_| die(cli::USAGE, "--id 要给数字")));
        } else if let Some(d) = a.strip_prefix("--device=") {
            device = d.to_string();
        } else if a.starts_with("--") {
            die(cli::USAGE, format!("不认识的参数 {a}\n{USAGE}"));
        } else {
            args.push(a);
        }
    }
    let p = profile::device_from_args(&[format!("--device={device}")]).unwrap_or_else(|e| die(cli::USAGE, e));
    opts.media = Some(kfx::css::MediaEnv::for_profile(p));
    if styles {
        let [input] = args.as_slice() else { die(cli::USAGE, USAGE) };
        let s = kfx::epub_text_styles(cli::open_or_die(input), &opts).unwrap_or_else(|e| die(cli::FAILED, format!("{input}：{e}")));
        print!("{s}");
        return;
    }
    let [input, output] = args.as_slice() else { die(cli::USAGE, USAGE) };
    let (kfx, warnings) = kfx::epub_to_kfx_from(cli::open_or_die(input), &opts).unwrap_or_else(|e| die(cli::FAILED, format!("{input}：{e}")));
    for w in &warnings {
        eprintln!("警告：{w}");
    }
    cli::write_or_die(output, &kfx);
    println!("{output}：{} 字节", kfx.len());
}

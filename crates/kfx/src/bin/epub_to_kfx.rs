//! EPUB → KFX（Kindle 自带阅读器的增强排版格式）。
//! 用法：epub-to-kfx [--id=数字] 优化过的.epub 输出.kfx
//! `--id` 固定唯一 ID（同一本书做几个测试版本时用，Kindle 按 ID 认书）；不给就按书的 OPF 唯一标识符派生。
//! 输入应先经 `epub-optimize --device=kindle` 优化；这里只做格式转换。

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut opts = kfx::Opts::default();
    let mut args = Vec::new();
    for a in std::env::args().skip(1) {
        match a.strip_prefix("--id=") {
            Some(n) => match n.parse() {
                Ok(n) => opts.fixed_id = Some(n),
                Err(_) => {
                    eprintln!("--id 要给数字");
                    return ExitCode::from(2);
                }
            },
            None => args.push(a),
        }
    }
    let [input, output] = args.as_slice() else {
        eprintln!("用法：epub-to-kfx [--id=数字] 优化过的.epub 输出.kfx");
        return ExitCode::from(2);
    };
    let data = match std::fs::read(input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("读 {input} 失败：{e}");
            return ExitCode::FAILURE;
        }
    };
    let (kfx, warnings) = match kfx::epub_to_kfx(&data, &opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{input}：{e}");
            return ExitCode::FAILURE;
        }
    };
    for w in &warnings {
        eprintln!("警告：{w}");
    }
    if let Err(e) = bookconv::util::write_atomic(std::path::Path::new(output), &kfx) {
        eprintln!("写 {output} 失败：{e}");
        return ExitCode::FAILURE;
    }
    println!("{output}：{} 字节", kfx.len());
    ExitCode::SUCCESS
}

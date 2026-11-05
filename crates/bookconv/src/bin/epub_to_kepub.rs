//! 命令行：优化过的 EPUB → KEPUB（`bookconv::kepub::epub_to_kepub`，与书库 `booklib sync` 给 kobo 生成时同一函数）。
//! 给真机 A/B 用（docs/kepub.md#真机-ab）：先 `epub-optimize --device=kobo`，再用这个转。
//!
//! 用法: epub-to-kepub [--no-wrap-image-pages] 输入.epub 输出.kepub.epub
//!   --no-wrap-image-pages   没有文字的页（封面、漫画页）不加 book-columns/book-inner 两层包装（缺省加）
//!   固定版式看输入 OPF 的 `rendition:layout = pre-paginated`（漫画），不靠输出文件名。
//! 产物先写到 `输出.kepub.tmp`，成功后改名。退出码: 0 成功；1 用法错；2 转换失败。

use bookconv::kepub::{self, Opts};
use bookconv::util::cli::{self, die};

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args
        .iter()
        .filter(|a| a.starts_with("--"))
        .map(|s| s.as_str())
        .collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.len() != 2 || flags.iter().any(|f| *f != "--no-wrap-image-pages") {
        die(
            cli::USAGE,
            "用法: epub-to-kepub [--no-wrap-image-pages] 输入.epub 输出.kepub.epub",
        );
    }
    let opts = Opts {
        wrap_image_pages: !flags.contains(&"--no-wrap-image-pages"),
    };
    let (input, output) = (
        std::path::Path::new(files[0]),
        std::path::Path::new(files[1]),
    );
    let tmp = bookconv::util::tmp_beside(output, "kepub");
    let rep = bookconv::util::produce_then_replace(&tmp, output, |t| {
        kepub::epub_to_kepub(input, t, &opts).map_err(|e| e.to_string())
    })
    .unwrap_or_else(|e| die(cli::FAILED, e));
    for w in &rep.warnings {
        eprintln!("⚠ {w}");
    }
    eprintln!(
        "改写 {} 个章节，加了 {} 个 koboSpan → {}",
        rep.files,
        rep.spans,
        output.display()
    );
}

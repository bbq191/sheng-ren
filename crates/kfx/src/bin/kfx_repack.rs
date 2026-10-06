//! 把 KFX 解开再原样打包：验证容器读写（没改动时输出应和输入逐字节相同）。
//! 用法：kfx-repack 输入.kfx 输出.kfx
//! 退出码：0 成功（相同与否看输出的那行字）；1 用法错；2 读写或解析失败。

use bookconv::util::cli::{self, die};

const USAGE: &str = "用法：kfx-repack 输入.kfx 输出.kfx";

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let [input, output] = args.as_slice() else { die(cli::USAGE, USAGE) };
    let data = cli::read_or_die(input);
    let c = kfx::Container::parse(&data).unwrap_or_else(|e| die(cli::FAILED, format!("{input}：{e}")));
    let out = c.to_bytes();
    cli::write_or_die(output, &out);
    if out == data {
        println!("{} 个实体，输出与输入逐字节相同", c.entities.len());
    } else {
        let first = out.iter().zip(&data).position(|(a, b)| a != b).unwrap_or(out.len().min(data.len()));
        println!("{} 个实体，输出与输入不同：{} → {} 字节，第一处不同在偏移 {first}", c.entities.len(), data.len(), out.len());
    }
}

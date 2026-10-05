//! 把 KFX 解开再原样打包：验证容器读写（没改动时输出应和输入逐字节相同）。
//! 用法：kfx-repack 输入.kfx 输出.kfx

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("用法：kfx-repack 输入.kfx 输出.kfx");
        return ExitCode::from(2);
    };
    let data = match std::fs::read(input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("读 {input} 失败：{e}");
            return ExitCode::FAILURE;
        }
    };
    let c = match kfx::Container::parse(&data) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{input}：{e}");
            return ExitCode::FAILURE;
        }
    };
    let out = c.to_bytes();
    let tmp = format!("{output}.tmp");
    if let Err(e) = std::fs::write(&tmp, &out).and_then(|_| std::fs::rename(&tmp, output)) {
        eprintln!("写 {output} 失败：{e}");
        return ExitCode::FAILURE;
    }
    if out == data {
        println!("{} 个实体，输出与输入逐字节相同", c.entities.len());
    } else {
        let first = out.iter().zip(&data).position(|(a, b)| a != b).unwrap_or(out.len().min(data.len()));
        println!("{} 个实体，输出与输入不同：{} → {} 字节，第一处不同在偏移 {first}", c.entities.len(), data.len(), out.len());
    }
    ExitCode::SUCCESS
}

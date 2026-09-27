//! 把 `profiles/*.toml` 全部嵌进二进制：增删设备只需增删文件，不用改代码。
use std::{env, fs, path::Path};

fn main() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("profiles");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("profiles/ 目录读不了")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    let mut out = String::from("pub(crate) const BUILTIN: &[(&str, &str)] = &[\n");
    for p in &files {
        println!("cargo:rerun-if-changed={}", p.display());
        let stem = p.file_stem().unwrap().to_str().unwrap();
        out += &format!("    ({stem:?}, include_str!({:?})),\n", p.display().to_string());
    }
    out += "];\n";
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("builtin.rs"), out).unwrap();
}

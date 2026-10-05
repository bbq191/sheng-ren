//! 打印 KFX 容器的结构（分析样本用）。
//! 用法：kfx-dump [--type=N]… [--full] 书.kfx
//!   不加 --type 时打印容器信息、片段类型统计和每类第一个片段；--type 打印该类型的全部片段；--full 不截断长列表和长字符串。

use kfx::ion::{SymbolTable, Value};
use kfx::{Body, Container};
use std::collections::BTreeMap;
use std::process::ExitCode;

struct Fmt<'a> {
    syms: &'a SymbolTable,
    full: bool,
}

impl Fmt<'_> {
    fn sym(&self, sid: u32) -> String {
        self.syms.display(sid)
    }

    fn value(&self, v: &Value, out: &mut String) {
        const MAX_ITEMS: usize = 30;
        const MAX_STR: usize = 120;
        match v {
            Value::Null(t) => out.push_str(&format!("null.{t:?}").to_lowercase()),
            Value::Bool(b) => out.push_str(&b.to_string()),
            Value::Int(n) => out.push_str(&n.to_string()),
            Value::F32(f) => out.push_str(&format!("{f}f")),
            Value::F64(f) => out.push_str(&format!("{f}e0")),
            Value::Decimal { neg, mag, exp, .. } => out.push_str(&format!("{}{mag}d{exp}", if *neg { "-" } else { "" })),
            Value::Timestamp(b) => out.push_str(&format!("<时间戳 {b:02x?}>")),
            Value::Symbol(s) => out.push_str(&format!("'{}'", self.sym(*s))),
            Value::String(s) => {
                let n = s.chars().count();
                if !self.full && n > MAX_STR {
                    out.push_str(&format!("{:?}…（共 {n} 字）", s.chars().take(MAX_STR).collect::<String>()));
                } else {
                    out.push_str(&format!("{s:?}"));
                }
            }
            Value::Clob(b) => out.push_str(&format!("<clob {} 字节>", b.len())),
            Value::Blob(b) => out.push_str(&format!("<blob {} 字节>", b.len())),
            Value::List(items) | Value::Sexp(items) => {
                let (open, close) = if matches!(v, Value::List(_)) { ("[", "]") } else { ("(", ")") };
                out.push_str(open);
                for (i, it) in items.iter().enumerate() {
                    if !self.full && i == MAX_ITEMS {
                        out.push_str(&format!(", …（共 {} 项）", items.len()));
                        break;
                    }
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.value(it, out);
                }
                out.push_str(close);
            }
            Value::Struct(fields) => {
                out.push('{');
                for (i, (k, fv)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&self.sym(*k));
                    out.push_str(": ");
                    self.value(fv, out);
                }
                out.push('}');
            }
            Value::Annotated(anns, inner) => {
                for a in anns {
                    out.push_str(&self.sym(*a));
                    out.push_str("::");
                }
                self.value(inner, out);
            }
        }
    }

    fn show(&self, v: &Value) -> String {
        let mut s = String::new();
        self.value(v, &mut s);
        s
    }
}

fn main() -> ExitCode {
    let mut types = Vec::new();
    let mut full = false;
    let mut path = None;
    for a in std::env::args().skip(1) {
        if let Some(t) = a.strip_prefix("--type=") {
            match t.trim_start_matches('$').parse::<u32>() {
                Ok(n) => types.push(n),
                Err(_) => {
                    eprintln!("--type 要给编号，比如 --type=145");
                    return ExitCode::from(2);
                }
            }
        } else if a == "--full" {
            full = true;
        } else {
            path = Some(a);
        }
    }
    let Some(path) = path else {
        eprintln!("用法：kfx-dump [--type=N]… [--full] 书.kfx");
        return ExitCode::from(2);
    };
    let c = match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|d| Container::parse(&d).map_err(|e| e.to_string())) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}：{e}");
            return ExitCode::FAILURE;
        }
    };
    let syms = c.symbols();
    let f = Fmt { syms: &syms, full };
    let show_entity = |e: &kfx::Entity| {
        let body = match &e.body {
            Body::Raw(b) => format!("<原始字节 {} 字节，开头 {:02x?}>", b.len(), &b[..b.len().min(8)]),
            Body::Ion(_) => e.values().map(|v| f.show(v)).collect::<Vec<_>>().join(" ; "),
        };
        println!("${} {}：{body}", e.ty, f.sym(e.id));
    };
    if !types.is_empty() {
        for e in c.entities.iter().filter(|e| types.contains(&e.ty)) {
            show_entity(e);
        }
        return ExitCode::SUCCESS;
    }
    println!("版本 {}，符号 {} 个，实体 {} 个", c.version, syms.len(), c.entities.len());
    println!("容器信息：{}", f.show(&c.info));
    for it in c.capabilities.iter().filter_map(|it| if let kfx::ion::Item::Value(v) = it { Some(v) } else { None }) {
        println!("格式能力：{}", f.show(it));
    }
    println!("kfxgen：{}", String::from_utf8_lossy(&c.kfxgen));
    let mut by: BTreeMap<u32, Vec<&kfx::Entity>> = BTreeMap::new();
    for e in &c.entities {
        by.entry(e.ty).or_default().push(e);
    }
    println!("\n片段类型（类型：个数）：{}", by.iter().map(|(t, v)| format!("${t}:{}", v.len())).collect::<Vec<_>>().join(" "));
    for es in by.values() {
        println!();
        show_entity(es[0]);
    }
    ExitCode::SUCCESS
}

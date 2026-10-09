//! 打印 KFX 容器的结构（分析样本用）。
//! 用法：kfx-dump [--type=N]… [--full] 书.kfx
//!        kfx-dump --json 书.kfx
//!   不加 --type 时打印容器信息、片段类型统计和每类第一个片段；--type 打印该类型的全部片段；--full 不截断长列表和长字符串。
//!   --json：整本按 JSON 打印给分析脚本用（`tools/kfx/kfx.py` 的 `load` 读它，Python 那边不再自己解 Ion）：
//!   `{"info": 容器信息, "entities": [[片段名, "$类型", [值…]], …]}`。结构体的键是符号名（没有名字的写 `$N`，重复的键第二个起
//!   加 `#2`、`#3`），符号、字符串都是字符串，十进制数、浮点都是数，null 是 null；其余写成对象：注解 `{"$annot": [...], "$value": 值}`、
//!   blob/clob `{"$blob"/"$clob": 字节数, "$head": 开头 8 字节的十六进制}`、时间戳 `{"$ts": 十六进制}`、资源（图片、字体）片段整个是
//!   `{"$raw": 字节数, "$head": …}`。

use kfx::ion::{SymbolTable, Value};
use kfx::{Body, Container};
use serde_json::{json, Map, Value as Json};
use std::collections::BTreeMap;
use bookconv::util::cli::{self, die};

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

const USAGE: &str = "用法：kfx-dump [--type=N]… [--full] 书.kfx | kfx-dump --json 书.kfx";

fn head_hex(b: &[u8]) -> String {
    b.iter().take(8).map(|x| format!("{x:02x}")).collect()
}

/// Ion 值 → JSON（写法见文件头）。
fn to_json(syms: &SymbolTable, v: &Value) -> Json {
    match v {
        Value::Null(_) => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Int(n) => json!(n),
        Value::F32(f) => json!(f64::from(*f)),
        Value::F64(f) => json!(f),
        Value::Decimal { neg, mag, exp, .. } => {
            let x = *mag as f64 * 10f64.powi(i32::try_from(*exp).unwrap_or(0));
            json!(if *neg { -x } else { x })
        }
        Value::Timestamp(b) => json!({ "$ts": b.iter().map(|x| format!("{x:02x}")).collect::<String>() }),
        Value::Symbol(s) => json!(syms.display(*s)),
        Value::String(s) => json!(s),
        Value::Clob(b) => json!({ "$clob": b.len(), "$head": head_hex(b) }),
        Value::Blob(b) => json!({ "$blob": b.len(), "$head": head_hex(b) }),
        Value::List(items) | Value::Sexp(items) => Json::Array(items.iter().map(|x| to_json(syms, x)).collect()),
        Value::Struct(fields) => {
            let mut m = Map::new();
            for (k, fv) in fields {
                let base = syms.display(*k);
                let mut key = base.clone();
                let mut n = 2;
                while m.contains_key(&key) {
                    key = format!("{base}#{n}");
                    n += 1;
                }
                m.insert(key, to_json(syms, fv));
            }
            Json::Object(m)
        }
        Value::Annotated(anns, inner) => json!({ "$annot": anns.iter().map(|a| syms.display(*a)).collect::<Vec<_>>(), "$value": to_json(syms, inner) }),
    }
}

fn main() {
    // 输出常接 `head`、`less`：管道提前关了就安静退出，不 panic。
    cli::restore_sigpipe();
    let mut types = Vec::new();
    let mut full = false;
    let mut as_json = false;
    let mut path = None;
    for a in std::env::args().skip(1) {
        if a == "-h" || a == "--help" {
            println!("{USAGE}");
            return;
        } else if let Some(t) = a.strip_prefix("--type=") {
            types.push(t.trim_start_matches('$').parse::<u32>().unwrap_or_else(|_| die(cli::USAGE, "--type 要给编号，比如 --type=145")));
        } else if a == "--full" {
            full = true;
        } else if a == "--json" {
            as_json = true;
        } else if a.starts_with("--") || path.is_some() {
            die(cli::USAGE, format!("不认识的参数 {a}\n{USAGE}"));
        } else {
            path = Some(a);
        }
    }
    let Some(path) = path else { die(cli::USAGE, USAGE) };
    let c = Container::parse(&cli::read_or_die(&path)).unwrap_or_else(|e| die(cli::FAILED, format!("{path}：{e}")));
    let syms = c.symbols();
    if as_json {
        let entities: Vec<Json> = c
            .entities
            .iter()
            .map(|e| {
                let body = match &e.body {
                    Body::Raw(b) => json!({ "$raw": b.len(), "$head": head_hex(b) }),
                    Body::Ion(_) => Json::Array(e.values().map(|v| to_json(&syms, v)).collect()),
                };
                json!([syms.display(e.id), format!("${}", e.ty), body])
            })
            .collect();
        let out = json!({ "info": to_json(&syms, &c.info), "entities": entities });
        let mut w = std::io::BufWriter::new(std::io::stdout().lock());
        if serde_json::to_writer(&mut w, &out).and_then(|()| std::io::Write::flush(&mut w).map_err(serde_json::Error::io)).is_err() {
            std::process::exit(cli::FAILED);
        }
        return;
    }
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
        return;
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
}

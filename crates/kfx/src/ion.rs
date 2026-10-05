//! Amazon Ion 1.0 二进制编解码，只照公开规范（amzn.github.io/ion-docs/docs/binary.html）写。
//!
//! 符号一律按编号（SID）保存，不在这里解析成名字：KFX 用的共享符号表 `YJ_symbols` 没有公开的名字，
//! 写出时也只需要编号。名字表见 [`SymbolTable`]。
//!
//! 编码取最短表示（长度 < 14 写进类型字节，否则 14 + VarUInt），和样本里 Amazon 的写法一致，
//! 所以"解码 → 编码"能逐字节还原（样本上验证过，见 `docs/kfx.md`）。
//! 输入是外来字节：越界、长度溢出、非法 UTF-8、嵌套过深都返回错误，不 panic。

use std::fmt;

/// Ion 版本标记（BVM）。
pub const BVM: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];

/// 系统符号表（SID 1–9）。
pub const SYSTEM_SYMBOLS: [&str; 9] =
    ["$ion", "$ion_1_0", "$ion_symbol_table", "name", "version", "imports", "symbols", "max_id", "$ion_shared_symbol_table"];

pub const SID_ION_SYMBOL_TABLE: u32 = 3;
pub const SID_NAME: u32 = 4;
pub const SID_VERSION: u32 = 5;
pub const SID_IMPORTS: u32 = 6;
pub const SID_SYMBOLS: u32 = 7;
pub const SID_MAX_ID: u32 = 8;

/// 嵌套上限：样本里最深不到 20 层，留足余量又不至于让恶意输入栈溢出。
const MAX_DEPTH: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IonType {
    Null,
    Bool,
    Int,
    Float,
    Decimal,
    Timestamp,
    Symbol,
    String,
    Clob,
    Blob,
    List,
    Sexp,
    Struct,
}

impl IonType {
    fn code(self) -> u8 {
        match self {
            IonType::Null => 0,
            IonType::Bool => 1,
            IonType::Int => 2,
            IonType::Float => 4,
            IonType::Decimal => 5,
            IonType::Timestamp => 6,
            IonType::Symbol => 7,
            IonType::String => 8,
            IonType::Clob => 9,
            IonType::Blob => 0xA,
            IonType::List => 0xB,
            IonType::Sexp => 0xC,
            IonType::Struct => 0xD,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// 带类型的 null（`null.int` 等）；`IonType::Null` 是 `null.null`。
    Null(IonType),
    Bool(bool),
    Int(i64),
    /// 4 字节浮点（保留宽度，才能逐字节还原）。
    F32(f32),
    F64(f64),
    /// 十进制数：值 = (neg ? -1 : 1) × mag × 10^exp。`neg` 单独存，才能表示 -0。
    /// `exp == 0 && mag == 0 && !neg` 且 `zero_len` 为真时编码成长度 0 的 `0d0`。
    Decimal { neg: bool, mag: u64, exp: i64, zero_len: bool },
    /// 时间戳原样保留字节（KFX 里没见过，不解析）。
    Timestamp(Vec<u8>),
    Symbol(u32),
    String(String),
    Clob(Vec<u8>),
    Blob(Vec<u8>),
    List(Vec<Value>),
    Sexp(Vec<Value>),
    /// 字段按出现顺序保存（Ion 结构体允许重复字段）。
    Struct(Vec<(u32, Value)>),
    Annotated(Vec<u32>, Box<Value>),
}

/// 顶层流里的一项：版本标记或一个值。
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Bvm,
    Value(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub msg: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ion 解码错误（偏移 {}）：{}", self.offset, self.msg)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn err<T>(offset: usize, msg: &'static str) -> Result<T> {
    Err(Error { offset, msg })
}

impl Value {
    pub fn as_struct(&self) -> Option<&[(u32, Value)]> {
        match self {
            Value::Struct(f) => Some(f),
            _ => None,
        }
    }

    /// 结构体里第一个编号为 `sid` 的字段。
    pub fn field(&self, sid: u32) -> Option<&Value> {
        self.as_struct()?.iter().find(|(k, _)| *k == sid).map(|(_, v)| v)
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_symbol(&self) -> Option<u32> {
        match self {
            Value::Symbol(s) => Some(*s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) | Value::Sexp(v) => Some(v),
            _ => None,
        }
    }

    /// 去掉注解后的值。
    pub fn unannotated(&self) -> &Value {
        match self {
            Value::Annotated(_, v) => v.unannotated(),
            v => v,
        }
    }
}

// ---------------------------------------------------------------- 解码

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn byte(&mut self) -> Result<u8> {
        let c = *self.b.get(self.i).ok_or(Error { offset: self.i, msg: "数据提前结束" })?;
        self.i += 1;
        Ok(c)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.i.checked_add(n).filter(|&e| e <= self.b.len()).ok_or(Error { offset: self.i, msg: "长度超出数据" })?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }

    fn varuint(&mut self) -> Result<u64> {
        let start = self.i;
        let mut v: u64 = 0;
        loop {
            let c = self.byte()?;
            if v >> 57 != 0 {
                return err(start, "VarUInt 溢出");
            }
            v = (v << 7) | u64::from(c & 0x7F);
            if c & 0x80 != 0 {
                return Ok(v);
            }
        }
    }

    fn varint(&mut self) -> Result<i64> {
        let start = self.i;
        let c = self.byte()?;
        let neg = c & 0x40 != 0;
        let mut v: u64 = u64::from(c & 0x3F);
        let mut last = c & 0x80 != 0;
        while !last {
            let c = self.byte()?;
            if v >> 56 != 0 {
                return err(start, "VarInt 溢出");
            }
            v = (v << 7) | u64::from(c & 0x7F);
            last = c & 0x80 != 0;
        }
        let v = i64::try_from(v).map_err(|_| Error { offset: start, msg: "VarInt 溢出" })?;
        Ok(if neg { -v } else { v })
    }

    fn len(&mut self, l: u8) -> Result<usize> {
        if l == 14 {
            usize::try_from(self.varuint()?).map_err(|_| Error { offset: self.i, msg: "长度溢出" })
        } else {
            Ok(usize::from(l))
        }
    }
}

fn uint_be(b: &[u8], at: usize) -> Result<u64> {
    if b.len() > 8 {
        // 允许前导 0（规范允许非最短 UInt）。
        let (hi, lo) = b.split_at(b.len() - 8);
        if hi.iter().any(|&x| x != 0) {
            return err(at, "整数超出 64 位");
        }
        return uint_be(lo, at);
    }
    Ok(b.iter().fold(0u64, |v, &x| (v << 8) | u64::from(x)))
}

/// 解码顶层流（可含多个 BVM 和值；NOP 填充跳过）。
pub fn decode_stream(b: &[u8]) -> Result<Vec<Item>> {
    let mut c = Cursor { b, i: 0 };
    let mut out = Vec::new();
    while c.i < b.len() {
        if b[c.i..].starts_with(&BVM) {
            c.i += 4;
            out.push(Item::Bvm);
            continue;
        }
        if let Some(v) = value(&mut c, 0)? {
            out.push(Item::Value(v));
        }
    }
    Ok(out)
}

/// 解码顶层流，只要值（丢掉 BVM）。
pub fn decode_values(b: &[u8]) -> Result<Vec<Value>> {
    Ok(decode_stream(b)?.into_iter().filter_map(|it| if let Item::Value(v) = it { Some(v) } else { None }).collect())
}

/// 读一个值；NOP 填充返回 `None`。
fn value(c: &mut Cursor, depth: usize) -> Result<Option<Value>> {
    if depth > MAX_DEPTH {
        return err(c.i, "嵌套过深");
    }
    let at = c.i;
    let td = c.byte()?;
    let (t, l) = (td >> 4, td & 0x0F);
    if t == 0 && l != 15 {
        let n = c.len(l)?;
        c.take(n)?;
        return Ok(None);
    }
    if l == 15 {
        let ty = match t {
            0 => IonType::Null,
            1 => IonType::Bool,
            2 | 3 => IonType::Int,
            4 => IonType::Float,
            5 => IonType::Decimal,
            6 => IonType::Timestamp,
            7 => IonType::Symbol,
            8 => IonType::String,
            9 => IonType::Clob,
            0xA => IonType::Blob,
            0xB => IonType::List,
            0xC => IonType::Sexp,
            0xD => IonType::Struct,
            _ => return err(at, "保留的类型码"),
        };
        return Ok(Some(Value::Null(ty)));
    }
    if t == 1 {
        return match l {
            0 => Ok(Some(Value::Bool(false))),
            1 => Ok(Some(Value::Bool(true))),
            _ => err(at, "非法布尔值"),
        };
    }
    // 结构体 L=1：字段按 SID 排过序，长度跟在后面。
    let n = if t == 0xD && l == 1 { c.len(14)? } else { c.len(l)? };
    let body_at = c.i;
    let d = c.take(n)?;
    let v = match t {
        2 | 3 => {
            let m = uint_be(d, body_at)?;
            let m = i64::try_from(m).map_err(|_| Error { offset: body_at, msg: "整数超出 i64" })?;
            if t == 3 && m == 0 {
                return err(at, "负零整数");
            }
            Value::Int(if t == 3 { -m } else { m })
        }
        4 => match n {
            0 => Value::F64(0.0),
            4 => Value::F32(f32::from_be_bytes([d[0], d[1], d[2], d[3]])),
            8 => Value::F64(f64::from_be_bytes(d.try_into().map_err(|_| Error { offset: body_at, msg: "浮点长度" })?)),
            _ => return err(at, "浮点长度只能是 0、4、8"),
        },
        5 => {
            if n == 0 {
                Value::Decimal { neg: false, mag: 0, exp: 0, zero_len: true }
            } else {
                let mut sub = Cursor { b: d, i: 0 };
                let exp = sub.varint()?;
                let co = &d[sub.i..];
                let (neg, mag) = match co.split_first() {
                    None => (false, 0),
                    Some((&first, rest)) => {
                        let mut bytes = Vec::with_capacity(co.len());
                        bytes.push(first & 0x7F);
                        bytes.extend_from_slice(rest);
                        (first & 0x80 != 0, uint_be(&bytes, body_at)?)
                    }
                };
                Value::Decimal { neg, mag, exp, zero_len: false }
            }
        }
        6 => Value::Timestamp(d.to_vec()),
        7 => {
            let s = uint_be(d, body_at)?;
            Value::Symbol(u32::try_from(s).map_err(|_| Error { offset: body_at, msg: "符号编号溢出" })?)
        }
        8 => Value::String(String::from_utf8(d.to_vec()).map_err(|_| Error { offset: body_at, msg: "字符串不是 UTF-8" })?),
        9 => Value::Clob(d.to_vec()),
        0xA => Value::Blob(d.to_vec()),
        0xB | 0xC => {
            let mut sub = Cursor { b: d, i: 0 };
            let mut items = Vec::new();
            while sub.i < d.len() {
                if let Some(v) = value(&mut sub, depth + 1).map_err(|e| shift(e, body_at))? {
                    items.push(v);
                }
            }
            if t == 0xB { Value::List(items) } else { Value::Sexp(items) }
        }
        0xD => {
            let mut sub = Cursor { b: d, i: 0 };
            let mut fields = Vec::new();
            while sub.i < d.len() {
                let f = sub.varuint().map_err(|e| shift(e, body_at))?;
                let f = u32::try_from(f).map_err(|_| Error { offset: body_at + sub.i, msg: "字段编号溢出" })?;
                if let Some(v) = value(&mut sub, depth + 1).map_err(|e| shift(e, body_at))? {
                    fields.push((f, v));
                }
            }
            Value::Struct(fields)
        }
        0xE => {
            if n == 0 {
                return err(at, "BVM 只能出现在顶层");
            }
            let mut sub = Cursor { b: d, i: 0 };
            let al = usize::try_from(sub.varuint()?).map_err(|_| Error { offset: body_at, msg: "注解长度溢出" })?;
            let ab = sub.take(al).map_err(|e| shift(e, body_at))?;
            let mut ac = Cursor { b: ab, i: 0 };
            let mut anns = Vec::new();
            while ac.i < ab.len() {
                let a = ac.varuint().map_err(|e| shift(e, body_at))?;
                anns.push(u32::try_from(a).map_err(|_| Error { offset: body_at, msg: "注解编号溢出" })?);
            }
            if anns.is_empty() {
                return err(at, "空注解");
            }
            let inner = value(&mut sub, depth + 1).map_err(|e| shift(e, body_at))?.ok_or(Error { offset: body_at, msg: "注解包着 NOP" })?;
            if sub.i != d.len() {
                return err(at, "注解包装长度不符");
            }
            Value::Annotated(anns, Box::new(inner))
        }
        _ => return err(at, "保留的类型码"),
    };
    Ok(Some(v))
}

fn shift(mut e: Error, by: usize) -> Error {
    e.offset = e.offset.saturating_add(by);
    e
}

// ---------------------------------------------------------------- 编码

fn put_varuint(out: &mut Vec<u8>, mut v: u64) {
    let mut tmp = [0u8; 10];
    let mut n = 0;
    loop {
        tmp[n] = (v & 0x7F) as u8;
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    tmp[0] |= 0x80;
    out.extend(tmp[..n].iter().rev());
}

fn put_varint(out: &mut Vec<u8>, v: i64) {
    let neg = v < 0;
    let mut m = v.unsigned_abs();
    // 最高字节只有 6 位数据（还有符号位）。
    let mut groups = vec![(m & 0x7F) as u8];
    m >>= 7;
    while m != 0 {
        groups.push((m & 0x7F) as u8);
        m >>= 7;
    }
    if groups.last().is_some_and(|&g| g & 0x40 != 0) {
        groups.push(0);
    }
    groups.reverse();
    if neg {
        groups[0] |= 0x40;
    }
    let last = groups.len() - 1;
    groups[last] |= 0x80;
    out.extend(groups);
}

fn uint_bytes(v: u64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let skip = b.iter().take_while(|&&x| x == 0).count();
    b[skip..].to_vec()
}

fn put_header(out: &mut Vec<u8>, t: u8, len: usize) {
    if len < 14 {
        out.push((t << 4) | len as u8);
    } else {
        out.push((t << 4) | 14);
        put_varuint(out, len as u64);
    }
}

/// 编码一个值，追加到 `out`。
pub fn encode_value(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Null(t) => out.push((t.code() << 4) | 0x0F),
        Value::Bool(b) => out.push(0x10 | u8::from(*b)),
        Value::Int(n) => {
            let m = uint_bytes(n.unsigned_abs());
            put_header(out, if *n < 0 { 3 } else { 2 }, m.len());
            out.extend(m);
        }
        Value::F32(f) => {
            out.push(0x44);
            out.extend(f.to_be_bytes());
        }
        Value::F64(f) => {
            if *f == 0.0 && f.is_sign_positive() {
                out.push(0x40);
            } else {
                out.push(0x48);
                out.extend(f.to_be_bytes());
            }
        }
        Value::Decimal { neg, mag, exp, zero_len } => {
            if *zero_len && !*neg && *mag == 0 && *exp == 0 {
                out.push(0x50);
                return;
            }
            let mut body = Vec::new();
            put_varint(&mut body, *exp);
            if *mag != 0 || *neg {
                let mut co = uint_bytes(*mag);
                if co.first().is_none_or(|&x| x & 0x80 != 0) {
                    co.insert(0, 0);
                }
                if *neg {
                    co[0] |= 0x80;
                }
                body.extend(co);
            }
            put_header(out, 5, body.len());
            out.extend(body);
        }
        Value::Timestamp(b) => {
            put_header(out, 6, b.len());
            out.extend(b);
        }
        Value::Symbol(s) => {
            let m = uint_bytes(u64::from(*s));
            put_header(out, 7, m.len());
            out.extend(m);
        }
        Value::String(s) => {
            put_header(out, 8, s.len());
            out.extend(s.as_bytes());
        }
        Value::Clob(b) => {
            put_header(out, 9, b.len());
            out.extend(b);
        }
        Value::Blob(b) => {
            put_header(out, 0xA, b.len());
            out.extend(b);
        }
        Value::List(items) | Value::Sexp(items) => {
            let mut body = Vec::new();
            for it in items {
                encode_value(&mut body, it);
            }
            put_header(out, if matches!(v, Value::List(_)) { 0xB } else { 0xC }, body.len());
            out.extend(body);
        }
        Value::Struct(fields) => {
            let mut body = Vec::new();
            for (k, fv) in fields {
                put_varuint(&mut body, u64::from(*k));
                encode_value(&mut body, fv);
            }
            // L=1 是"已排序"标记，长度 1 的结构体只能用 14 + VarUInt 写（不会出现：字段至少 2 字节）。
            put_header(out, 0xD, body.len());
            out.extend(body);
        }
        Value::Annotated(anns, inner) => {
            let mut ab = Vec::new();
            for a in anns {
                put_varuint(&mut ab, u64::from(*a));
            }
            let mut body = Vec::new();
            put_varuint(&mut body, ab.len() as u64);
            body.extend(ab);
            encode_value(&mut body, inner);
            put_header(out, 0xE, body.len());
            out.extend(body);
        }
    }
}

/// 编码顶层流。
pub fn encode_stream(items: &[Item]) -> Vec<u8> {
    let mut out = Vec::new();
    for it in items {
        match it {
            Item::Bvm => out.extend(BVM),
            Item::Value(v) => encode_value(&mut out, v),
        }
    }
    out
}

// ---------------------------------------------------------------- 符号表

/// SID → 名字。导入的共享表没有名字（`None`），本地符号有名字。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolTable {
    /// 下标 = SID；0 号永远是 `None`。
    names: Vec<Option<String>>,
}

/// 共享表导入项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub name: String,
    pub version: i64,
    pub max_id: u32,
}

impl SymbolTable {
    pub fn system() -> Self {
        let mut names = vec![None];
        names.extend(SYSTEM_SYMBOLS.iter().map(|s| Some(s.to_string())));
        SymbolTable { names }
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.len() <= 1
    }

    pub fn name(&self, sid: u32) -> Option<&str> {
        self.names.get(sid as usize)?.as_deref()
    }

    /// 按名字找 SID（只找有名字的）。
    pub fn sid(&self, name: &str) -> Option<u32> {
        self.names.iter().position(|n| n.as_deref() == Some(name)).map(|i| i as u32)
    }

    /// 显示用：有名字给名字，没有给 `$N`。
    pub fn display(&self, sid: u32) -> String {
        self.name(sid).map(str::to_string).unwrap_or_else(|| format!("${sid}"))
    }

    /// 追加一个本地符号，返回它的 SID。
    pub fn push(&mut self, name: &str) -> u32 {
        self.names.push(Some(name.to_string()));
        (self.names.len() - 1) as u32
    }

    /// 按 `$ion_symbol_table::{imports, symbols}` 更新：`imports` 是 `$ion_symbol_table` 符号时在现有表后追加，
    /// 否则从系统表重来，导入的共享表按 `max_id` 占位（没有名字）。返回读到的导入项。
    pub fn apply(&mut self, st: &Value) -> Vec<Import> {
        let mut imports = Vec::new();
        match st.field(SID_IMPORTS) {
            Some(Value::Symbol(SID_ION_SYMBOL_TABLE)) => {}
            other => {
                *self = SymbolTable::system();
                for imp in other.and_then(Value::as_list).unwrap_or(&[]) {
                    let max_id = imp.field(SID_MAX_ID).and_then(Value::as_int).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
                    self.names.extend(std::iter::repeat_n(None, max_id as usize));
                    imports.push(Import {
                        name: imp.field(SID_NAME).and_then(Value::as_str).unwrap_or_default().to_string(),
                        version: imp.field(SID_VERSION).and_then(Value::as_int).unwrap_or(1),
                        max_id,
                    });
                }
            }
        }
        for s in st.field(SID_SYMBOLS).and_then(Value::as_list).unwrap_or(&[]) {
            self.names.push(s.as_str().map(str::to_string));
        }
        imports
    }

    /// 从一段顶层流里找出符号表声明并依次应用。
    pub fn from_stream(items: &[Item]) -> (Self, Vec<Import>) {
        let mut t = SymbolTable::system();
        let mut imports = Vec::new();
        for it in items {
            match it {
                Item::Bvm => t = SymbolTable::system(),
                Item::Value(Value::Annotated(a, v)) if a.first() == Some(&SID_ION_SYMBOL_TABLE) => imports.extend(t.apply(v)),
                _ => {}
            }
        }
        (t, imports)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(v: Value) {
        let mut b = Vec::new();
        encode_value(&mut b, &v);
        let back = decode_values(&b).unwrap();
        assert_eq!(back, vec![v], "字节 {b:02x?}");
    }

    #[test]
    fn spec_vectors() {
        // 规范里的例子：0 是 0x20，-1 是 0x31 0x01，"hello" 是 0x85 …
        assert_eq!(decode_values(&[0x20]).unwrap(), vec![Value::Int(0)]);
        assert_eq!(decode_values(&[0x31, 0x01]).unwrap(), vec![Value::Int(-1)]);
        assert_eq!(decode_values(b"\x85hello").unwrap(), vec![Value::String("hello".into())]);
        assert_eq!(decode_values(&[0x0F, 0x1F, 0x2F, 0xDF]).unwrap(), vec![
            Value::Null(IonType::Null),
            Value::Null(IonType::Bool),
            Value::Null(IonType::Int),
            Value::Null(IonType::Struct)
        ]);
        // NOP 填充跳过。
        assert_eq!(decode_values(&[0x02, 0, 0, 0x11]).unwrap(), vec![Value::Bool(true)]);
        // 有序结构体（L=1）。
        assert_eq!(decode_values(&[0xD1, 0x82, 0x84, 0x20]).unwrap(), vec![Value::Struct(vec![(4, Value::Int(0))])]);
    }

    #[test]
    fn varint_edges() {
        for v in [0i64, 1, -1, 63, 64, -64, 8191, 8192, -8192, i64::from(i32::MAX), -(1 << 40)] {
            let mut b = Vec::new();
            put_varint(&mut b, v);
            assert_eq!(Cursor { b: &b, i: 0 }.varint().unwrap(), v, "{v}");
        }
        for v in [0u64, 127, 128, 16383, 16384, u64::from(u32::MAX)] {
            let mut b = Vec::new();
            put_varuint(&mut b, v);
            assert_eq!(Cursor { b: &b, i: 0 }.varuint().unwrap(), v);
        }
    }

    #[test]
    fn roundtrips() {
        rt(Value::Int(4278190080));
        rt(Value::Int(-300));
        rt(Value::F64(1.29167));
        rt(Value::F64(0.0));
        rt(Value::F32(1.5));
        rt(Value::Decimal { neg: false, mag: 129167, exp: -5, zero_len: false });
        rt(Value::Decimal { neg: true, mag: 0, exp: 0, zero_len: false });
        rt(Value::Decimal { neg: false, mag: 0, exp: 0, zero_len: true });
        rt(Value::Decimal { neg: false, mag: 128, exp: 3, zero_len: false });
        rt(Value::Symbol(930));
        rt(Value::String("書名：ABC謀殺案".repeat(5)));
        rt(Value::Blob(vec![0xFF; 300]));
        rt(Value::Annotated(vec![3], Box::new(Value::Struct(vec![(6, Value::List(vec![])), (7, Value::List(vec![Value::String("c0".into())]))]))));
        rt(Value::Sexp(vec![Value::Symbol(1), Value::Null(IonType::String)]));
    }

    #[test]
    fn bad_input_is_error_not_panic() {
        for b in [&[0x8E][..], &[0x8E, 0x7F], &[0x85, b'a'], &[0xF0], &[0xE0, 0x01], &[0xEE, 0x81], &[0x31, 0x00], &[0xD3, 0x80, 0x21]] {
            assert!(decode_values(b).is_err(), "{b:02x?}");
        }
        // 嵌套过深。
        let nested = (0..300).fold(vec![0xB0u8], |inner, _| {
            let mut o = Vec::new();
            put_header(&mut o, 0xB, inner.len());
            o.extend(inner);
            o
        });
        assert!(decode_values(&nested).is_err());
    }

    #[test]
    fn symbol_table_apply() {
        let st = Value::Struct(vec![
            (SID_IMPORTS, Value::List(vec![Value::Struct(vec![
                (SID_NAME, Value::String("YJ_symbols".into())),
                (SID_VERSION, Value::Int(10)),
                (SID_MAX_ID, Value::Int(859)),
            ])])),
            (SID_SYMBOLS, Value::List(vec![Value::String("c0".into())])),
        ]);
        let items = vec![Item::Bvm, Item::Value(Value::Annotated(vec![SID_ION_SYMBOL_TABLE], Box::new(st)))];
        let (t, imps) = SymbolTable::from_stream(&items);
        assert_eq!(imps, vec![Import { name: "YJ_symbols".into(), version: 10, max_id: 859 }]);
        assert_eq!(t.name(869), Some("c0"));
        assert_eq!(t.name(500), None);
        assert_eq!(t.display(500), "$500");
        assert_eq!(t.sid("c0"), Some(869));
        assert_eq!(t.name(4), Some("name"));
    }
}

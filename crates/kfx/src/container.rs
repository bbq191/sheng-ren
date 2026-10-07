//! KFX 容器（`CONT`）读写。布局是对样本的黑盒分析得来的，见 `docs/kfx.md#容器`。
//!
//! 读进来的东西尽量原样保留（容器信息里没认出的字段、实体头、kfxgen 信息），
//! 写出时只重算偏移、长度和实体区的 SHA-1；没改过的容器写出来和原文件逐字节相同。

use crate::ion::{self, Item, SymbolTable, Value};
use sha1::{Digest, Sha1};
use std::fmt;

pub const MAGIC: &[u8; 4] = b"CONT";
pub const ENTITY_MAGIC: &[u8; 4] = b"ENTY";
/// 固定头：魔数 4 + 版本 2 + 头长度 4 + 容器信息偏移 4 + 容器信息长度 4。
const FIXED_HEADER: usize = 18;
const INDEX_ENTRY: usize = 24;
/// 实体固定头：魔数 4 + 版本 2 + 头长度 4。
const ENTITY_FIXED: usize = 10;

// 容器信息里的字段（YJ_symbols 编号，含义是从样本推的）。
pub const SID_CONTAINER_ID: u32 = 409;
pub const SID_COMPRESSION: u32 = 410;
pub const SID_DRM_SCHEME: u32 = 411;
pub const SID_CHUNK_SIZE: u32 = 412;
pub const SID_INDEX_OFFSET: u32 = 413;
pub const SID_INDEX_LENGTH: u32 = 414;
pub const SID_SYMTAB_OFFSET: u32 = 415;
pub const SID_SYMTAB_LENGTH: u32 = 416;
pub const SID_CAPS_OFFSET: u32 = 594;
pub const SID_CAPS_LENGTH: u32 = 595;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KFX 容器：{}", self.0)
    }
}

impl std::error::Error for Error {}

impl From<ion::Error> for Error {
    fn from(e: ion::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    /// Ion 流（以 BVM 开头的实体）。
    Ion(Vec<Item>),
    /// 原始字节（图片、字体等资源）。
    Raw(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    /// 片段名的 SID（本地符号）；没有名字的片段是 `$348` 之类的共享符号。
    pub id: u32,
    /// 片段类型的 SID（`$145` 正文、`$157` 样式……）。
    pub ty: u32,
    pub version: u16,
    /// 实体头（样本里都是 `{$410: 0, $411: 0}`）。
    pub header: Vec<Item>,
    pub body: Body,
}

impl Entity {
    /// Ion 实体的值（去掉 BVM）。
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        let items: &[Item] = match &self.body {
            Body::Ion(items) => items,
            Body::Raw(_) => &[],
        };
        items.iter().filter_map(|it| if let Item::Value(v) = it { Some(v) } else { None })
    }

    /// Ion 实体的第一个值。
    pub fn value(&self) -> Option<&Value> {
        self.values().next()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Container {
    pub version: u16,
    /// 容器信息原样（写出时只改偏移、长度字段）。
    pub info: Value,
    /// 符号表声明（Ion 流）。
    pub symtab: Vec<Item>,
    /// 格式能力（`$593::[…]`）；没有就是空。
    pub capabilities: Vec<Item>,
    /// 容器信息后、实体区前的 kfxgen 信息（Ion 文本，原样保留；写出时更新 `kfxgen_payload_sha1`）。
    pub kfxgen: Vec<u8>,
    pub entities: Vec<Entity>,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| Error(format!("偏移 {at} 越界")))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| Error(format!("偏移 {at} 越界")))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64> {
    b.get(at..at + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap_or_default())).ok_or_else(|| Error(format!("偏移 {at} 越界")))
}

fn slice<'a>(b: &'a [u8], off: u64, len: u64, what: &str) -> Result<&'a [u8]> {
    let (Ok(off), Ok(len)) = (usize::try_from(off), usize::try_from(len)) else { return bad(format!("{what}：偏移或长度溢出")) };
    off.checked_add(len).and_then(|end| b.get(off..end)).ok_or_else(|| Error(format!("{what}：越界（偏移 {off}，长度 {len}，文件 {}）", b.len())))
}

fn info_u64(info: &Value, sid: u32) -> Result<Option<u64>> {
    match info.field(sid) {
        None => Ok(None),
        Some(v) => v.as_int().and_then(|n| u64::try_from(n).ok()).map(Some).ok_or_else(|| Error(format!("容器信息 ${sid} 不是非负整数"))),
    }
}

impl Container {
    pub fn parse(b: &[u8]) -> Result<Container> {
        if !b.starts_with(MAGIC) {
            if b.starts_with(b"\xeaDRMION\xee") {
                return bad("有 DRM（DRMION），不处理");
            }
            return bad("不是 KFX 容器（开头不是 CONT）");
        }
        let version = u16_at(b, 4)?;
        let header_len = u32_at(b, 6)? as usize;
        let (ci_off, ci_len) = (u32_at(b, 10)?, u32_at(b, 14)?);
        let info = ion::decode_values(slice(b, ci_off.into(), ci_len.into(), "容器信息")?)?
            .into_iter()
            .next()
            .ok_or_else(|| Error("容器信息是空的".into()))?;
        if info.as_struct().is_none() {
            return bad("容器信息不是结构体");
        }
        if info_u64(&info, SID_COMPRESSION)?.unwrap_or(0) != 0 || info_u64(&info, SID_DRM_SCHEME)?.unwrap_or(0) != 0 {
            return bad("压缩或 DRM 方案不是 0（没见过的变体，不处理）");
        }
        let need = |sid| info_u64(&info, sid)?.ok_or_else(|| Error(format!("容器信息缺 ${sid}")));
        let index = slice(b, need(SID_INDEX_OFFSET)?, need(SID_INDEX_LENGTH)?, "实体索引")?;
        let symtab = ion::decode_stream(slice(b, need(SID_SYMTAB_OFFSET)?, need(SID_SYMTAB_LENGTH)?, "符号表")?)?;
        let capabilities = match (info_u64(&info, SID_CAPS_OFFSET)?, info_u64(&info, SID_CAPS_LENGTH)?) {
            (Some(o), Some(l)) => ion::decode_stream(slice(b, o, l, "格式能力")?)?,
            _ => Vec::new(),
        };
        let ci_end = ci_off as usize + ci_len as usize;
        let kfxgen = b.get(ci_end..header_len).map(<[u8]>::to_vec).unwrap_or_default();
        if index.len() % INDEX_ENTRY != 0 {
            return bad("实体索引长度不是 24 的倍数");
        }
        let mut entities = Vec::with_capacity(index.len() / INDEX_ENTRY);
        for e in index.chunks_exact(INDEX_ENTRY) {
            let id = u32_at(e, 0)?;
            let ty = u32_at(e, 4)?;
            let off = u64_at(e, 8)?.checked_add(header_len as u64).ok_or_else(|| Error("实体偏移溢出".into()))?;
            let data = slice(b, off, u64_at(e, 16)?, "实体")?;
            entities.push(parse_entity(id, ty, data)?);
        }
        Ok(Container { version, info, symtab, capabilities, kfxgen, entities })
    }

    /// 符号表（共享表部分没有名字）。
    pub fn symbols(&self) -> SymbolTable {
        SymbolTable::from_stream(&self.symtab).0
    }

    /// 容器 id（`$409`，`CR!…`）。
    pub fn container_id(&self) -> Option<&str> {
        self.info.field(SID_CONTAINER_ID).and_then(Value::as_str)
    }

    /// 换容器 id：书里凡是等于旧 id 的字符串一起换（容器信息、kfxgen 的 `kfxgen_acr`、`$419` 内容清单、
    /// `$490` 元数据的 `asset_id`）。只换容器信息不换清单，Kindle 找不到图片资源，封面页空白（2026-10-05 真机）。
    pub fn set_container_id(&mut self, new: &str) {
        let Some(old) = self.container_id().map(str::to_string) else {
            set_string_field(&mut self.info, SID_CONTAINER_ID, new);
            return;
        };
        replace_strings(&mut self.info, &old, new);
        for e in &mut self.entities {
            if let Body::Ion(items) = &mut e.body {
                for it in items {
                    if let Item::Value(v) = it {
                        replace_strings(v, &old, new);
                    }
                }
            }
        }
        let quoted = |s: &str| format!("\"{s}\"").into_bytes();
        self.kfxgen = replace_bytes(&self.kfxgen, &quoted(&old), &quoted(new));
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        // 实体区：每个实体编码成「头 + Ion 正文」，原始字节（图片、字体）借用不复制，最后直接拼进输出——
        // 不先拼一份实体区再整体复制（大漫画几百 MB，省一份峰值内存和一遍拷贝）。
        let parts: Vec<(Vec<u8>, &[u8])> = self.entities.iter().map(entity_parts).collect();
        let (mut out, _) = self.header(&parts);
        for (head, raw) in &parts {
            out.extend_from_slice(head);
            out.extend_from_slice(raw);
        }
        out
    }

    /// 同 [`to_bytes`](Self::to_bytes)（字节相同），但消耗容器：每个实体的原始字节（图片、字体）拷进输出就释放，峰值内存是
    /// "输出一份"而不是"输出 + 全部图片"（写出器生成整本书时用；大漫画峰值约减半）。
    pub fn into_bytes(mut self) -> Vec<u8> {
        let (heads, mut out) = {
            let parts: Vec<(Vec<u8>, &[u8])> = self.entities.iter().map(entity_parts).collect();
            let (header, payload_len) = self.header(&parts);
            // 输出一次分配够（没写到的页不占物理内存），写的过程中不再扩容复制
            let mut out = Vec::with_capacity(header.len() + payload_len);
            out.extend_from_slice(&header);
            (parts.into_iter().map(|p| p.0).collect::<Vec<_>>(), out)
        };
        for (e, head) in std::mem::take(&mut self.entities).into_iter().zip(heads) {
            out.extend_from_slice(&head);
            if let Body::Raw(b) = &e.body {
                out.extend_from_slice(b);
            }
        }
        out
    }

    /// 实体区之前的全部字节（固定头、索引表、符号表、格式能力、容器信息、kfxgen 信息），和实体区的总长。`parts` 是各实体的
    /// （头 + Ion 正文, 原始字节），顺序同 `self.entities`。
    fn header(&self, parts: &[(Vec<u8>, &[u8])]) -> (Vec<u8>, usize) {
        let mut index = Vec::with_capacity(self.entities.len() * INDEX_ENTRY);
        let mut sha = Sha1::new();
        let mut payload_len = 0usize;
        for (e, (head, raw)) in self.entities.iter().zip(parts) {
            let len = head.len() + raw.len();
            index.extend(e.id.to_le_bytes());
            index.extend(e.ty.to_le_bytes());
            index.extend((payload_len as u64).to_le_bytes());
            index.extend((len as u64).to_le_bytes());
            sha.update(head);
            sha.update(raw);
            payload_len += len;
        }
        let symtab = ion::encode_stream(&self.symtab);
        let caps = ion::encode_stream(&self.capabilities);
        let index_off = FIXED_HEADER;
        let symtab_off = index_off + index.len();
        let caps_off = symtab_off + symtab.len();
        let info_off = caps_off + caps.len();

        let mut info = self.info.clone();
        set_field(&mut info, SID_INDEX_OFFSET, index_off);
        set_field(&mut info, SID_INDEX_LENGTH, index.len());
        set_field(&mut info, SID_SYMTAB_OFFSET, symtab_off);
        set_field(&mut info, SID_SYMTAB_LENGTH, symtab.len());
        if !self.capabilities.is_empty() {
            set_field(&mut info, SID_CAPS_OFFSET, caps_off);
            set_field(&mut info, SID_CAPS_LENGTH, caps.len());
        }
        let info_bytes = ion::encode_stream(&[Item::Bvm, Item::Value(info)]);
        let kfxgen = update_payload_sha1(&self.kfxgen, &sha.finalize());
        let header_len = info_off + info_bytes.len() + kfxgen.len();

        let mut out = Vec::with_capacity(header_len);
        out.extend(MAGIC);
        out.extend(self.version.to_le_bytes());
        out.extend((header_len as u32).to_le_bytes());
        out.extend((info_off as u32).to_le_bytes());
        out.extend((info_bytes.len() as u32).to_le_bytes());
        out.extend(index);
        out.extend(symtab);
        out.extend(caps);
        out.extend(info_bytes);
        out.extend(kfxgen);
        (out, payload_len)
    }
}

fn parse_entity(id: u32, ty: u32, d: &[u8]) -> Result<Entity> {
    if !d.starts_with(ENTITY_MAGIC) {
        return bad(format!("实体 {id} 开头不是 ENTY"));
    }
    let version = u16_at(d, 4)?;
    let hl = u32_at(d, 6)? as usize;
    if hl < ENTITY_FIXED || hl > d.len() {
        return bad(format!("实体 {id} 头长度 {hl} 不对"));
    }
    let header = ion::decode_stream(&d[ENTITY_FIXED..hl])?;
    let rest = &d[hl..];
    let body = if rest.starts_with(&ion::BVM) {
        Body::Ion(ion::decode_stream(rest).map_err(|e| Error(format!("实体 {id}（类型 ${ty}）：{e}")))?)
    } else {
        Body::Raw(rest.to_vec())
    };
    Ok(Entity { id, ty, version, header, body })
}

/// 一个实体的字节，分两段：实体头 + Ion 正文（编码出来的），原始字节正文（借用）。
fn entity_parts(e: &Entity) -> (Vec<u8>, &[u8]) {
    let header = ion::encode_stream(&e.header);
    let mut out = Vec::new();
    out.extend(ENTITY_MAGIC);
    out.extend(e.version.to_le_bytes());
    out.extend(((ENTITY_FIXED + header.len()) as u32).to_le_bytes());
    out.extend(header);
    match &e.body {
        Body::Ion(items) => {
            for it in items {
                match it {
                    Item::Bvm => out.extend(ion::BVM),
                    Item::Value(v) => ion::encode_value(&mut out, v),
                }
            }
            (out, &[])
        }
        Body::Raw(b) => (out, b),
    }
}

/// 改结构体里已有字段的值；没有就追加在末尾。
fn set_field(v: &mut Value, sid: u32, n: usize) {
    if let Value::Struct(fields) = v {
        let n = Value::Int(n as i64);
        match fields.iter_mut().find(|(k, _)| *k == sid) {
            Some((_, slot)) => *slot = n,
            None => fields.push((sid, n)),
        }
    }
}

fn set_string_field(v: &mut Value, sid: u32, s: &str) {
    if let Value::Struct(fields) = v {
        let s = Value::String(s.to_string());
        match fields.iter_mut().find(|(k, _)| *k == sid) {
            Some((_, slot)) => *slot = s,
            None => fields.push((sid, s)),
        }
    }
}

fn replace_strings(v: &mut Value, old: &str, new: &str) {
    match v {
        Value::String(s) if s == old => *s = new.to_string(),
        Value::List(x) | Value::Sexp(x) => x.iter_mut().for_each(|c| replace_strings(c, old, new)),
        Value::Struct(x) => x.iter_mut().for_each(|(_, c)| replace_strings(c, old, new)),
        Value::Annotated(_, c) => replace_strings(c, old, new),
        _ => {}
    }
}

fn replace_bytes(hay: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if !old.is_empty() && hay[i..].starts_with(old) {
            out.extend_from_slice(new);
            i += old.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// kfxgen 信息里 `kfxgen_payload_sha1` 的值是实体区的 SHA-1（十六进制，`digest` 是算好的摘要）。设备不校验它（2026-10-05 真机），照样更新。
fn update_payload_sha1(kfxgen: &[u8], digest: &[u8]) -> Vec<u8> {
    const KEY: &[u8] = b"key:\"kfxgen_payload_sha1\",value:\"";
    let Some(p) = kfxgen.windows(KEY.len()).position(|w| w == KEY) else { return kfxgen.to_vec() };
    let start = p + KEY.len();
    if kfxgen.len() < start + 40 {
        return kfxgen.to_vec();
    }
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let mut out = kfxgen.to_vec();
    out[start..start + 40].copy_from_slice(hex.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Container {
        let symtab = vec![
            Item::Bvm,
            Item::Value(Value::Annotated(
                vec![ion::SID_ION_SYMBOL_TABLE],
                Box::new(Value::Struct(vec![
                    (
                        ion::SID_IMPORTS,
                        Value::List(vec![Value::Struct(vec![
                            (ion::SID_NAME, Value::String("YJ_symbols".into())),
                            (ion::SID_VERSION, Value::Int(10)),
                            (ion::SID_MAX_ID, Value::Int(859)),
                        ])]),
                    ),
                    (ion::SID_SYMBOLS, Value::List(vec![Value::String("t1".into())])),
                ])),
            )),
        ];
        let header = vec![Item::Bvm, Item::Value(Value::Struct(vec![(410, Value::Int(0)), (411, Value::Int(0))]))];
        Container {
            version: 2,
            info: Value::Struct(vec![
                (SID_CONTAINER_ID, Value::String("CR!TEST".into())),
                (SID_COMPRESSION, Value::Int(0)),
                (SID_DRM_SCHEME, Value::Int(0)),
                (SID_INDEX_OFFSET, Value::Int(0)),
                (SID_INDEX_LENGTH, Value::Int(0)),
                (SID_SYMTAB_OFFSET, Value::Int(0)),
                (SID_SYMTAB_LENGTH, Value::Int(0)),
                (SID_CHUNK_SIZE, Value::Int(4096)),
            ]),
            symtab,
            capabilities: Vec::new(),
            kfxgen: b"[{key:\"kfxgen_payload_sha1\",value:\"0000000000000000000000000000000000000000\"}]".to_vec(),
            entities: vec![
                Entity {
                    id: 869,
                    ty: 145,
                    version: 1,
                    header: header.clone(),
                    body: Body::Ion(vec![
                        Item::Bvm,
                        Item::Value(Value::Struct(vec![(4, Value::Symbol(869)), (146, Value::List(vec![Value::String("正文".into())]))])),
                    ]),
                },
                Entity { id: 870, ty: 417, version: 1, header, body: Body::Raw(vec![0xFF, 0xD8, 0xFF, 0xE0]) },
            ],
        }
    }

    #[test]
    fn write_then_read_back() {
        let c = sample();
        let b = c.to_bytes();
        let back = Container::parse(&b).unwrap();
        assert_eq!(back.entities, c.entities);
        assert_eq!(back.symbols().name(869), Some("t1"));
        // 第二次写出逐字节相同（偏移字段已经是对的）。
        assert_eq!(back.to_bytes(), b);
        // SHA-1 更新成了实体区的。
        let hl = u32_at(&b, 6).unwrap() as usize;
        let hex: String = Sha1::digest(&b[hl..]).iter().map(|x| format!("{x:02x}")).collect();
        assert!(String::from_utf8_lossy(&back.kfxgen).contains(&hex));
        // 消耗容器的写法字节相同
        assert_eq!(c.into_bytes(), b);
    }

    #[test]
    fn set_container_id_changes_every_copy() {
        let mut c = sample();
        c.kfxgen = b"[{key:\"kfxgen_acr\",value:\"CR!TEST\"}]".to_vec();
        c.entities.push(Entity {
            id: 871,
            ty: 419,
            version: 1,
            header: Vec::new(),
            body: Body::Ion(vec![
                Item::Bvm,
                Item::Value(Value::Annotated(
                    vec![419],
                    Box::new(Value::Struct(vec![(252, Value::List(vec![Value::Struct(vec![(155, Value::String("CR!TEST".into()))])]))])),
                )),
            ]),
        });
        c.set_container_id("CR!NEW");
        let b = c.to_bytes();
        assert!(!b.windows(7).any(|w| w == b"CR!TEST"));
        let back = Container::parse(&b).unwrap();
        assert_eq!(back.container_id(), Some("CR!NEW"));
        assert_eq!(b.windows(6).filter(|w| *w == b"CR!NEW").count(), 3);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Container::parse(b"").is_err());
        assert!(Container::parse(b"\xeaDRMION\xee....").unwrap_err().0.contains("DRM"));
        let mut b = sample().to_bytes();
        b.truncate(b.len() - 3);
        assert!(Container::parse(&b).is_err());
        // 改坏头长度、偏移都不能 panic。
        let good = sample().to_bytes();
        for at in 4..18 {
            let mut b = good.clone();
            b[at] = 0xFF;
            let _ = Container::parse(&b);
        }
    }

    /// 文件里任何一处字节坏掉（索引、符号表、实体头、Ion 正文）都只能报错，不能 panic；能解开的再写出也不能 panic。
    #[test]
    fn corrupted_bytes_anywhere_never_panic() {
        let good = sample().to_bytes();
        let mut x = 0x9E37_79B9u32;
        for at in 0..good.len() {
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let mut b = good.clone();
                b[at] = x as u8;
                if let Ok(c) = Container::parse(&b) {
                    let _ = c.to_bytes();
                    let _ = c.symbols();
                }
            }
        }
        for n in 0..good.len() {
            let _ = Container::parse(&good[..n]);
        }
    }
}

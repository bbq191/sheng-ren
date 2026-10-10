//! 本地符号的分配，分两个阶段：正文阶段（[`SymbolAlloc`]，版面、样式、锚点、导航、元数据的名字）和资源阶段
//! （[`ResourceSymbols`]，只能成组分配图片、字体的字节实体名和资源路径）。
//!
//! Kindle 的两条硬规矩靠这两个类型保证，不靠调用顺序（见 docs/kfx.md）：
//! - 资源路径（`$165`）的符号 = 字节实体的符号 + [`SID_GAP`]；元数据 `cover_image` 的符号 = 封面资源的符号 + [`SID_GAP`]。
//!   成组分配时按位置算出来，名字已经登记过（拿到的不是新符号）就报错，不会悄悄错位。
//! - 书里不能有实体的 id 排在资源路径的符号后面：资源阶段是 [`SymbolAlloc::into_resources`] 换出来的，换过以后
//!   只剩成组分配资源符号和查询，正文的名字分配不了。

use super::*;

/// 正文阶段的本地符号表：按第一次用到的顺序编号，从 [`FIRST_LOCAL_SID`] 起。
pub(super) struct SymbolAlloc {
    locals: Vec<String>,
    index: HashMap<String, u32>,
}

impl SymbolAlloc {
    pub(super) fn new() -> SymbolAlloc {
        SymbolAlloc { locals: Vec::new(), index: HashMap::new() }
    }

    /// 名字的符号：登记过的直接返回，没登记的分配下一个。
    pub(super) fn sym(&mut self, name: &str) -> u32 {
        if let Some(&s) = self.index.get(name) {
            return s;
        }
        let s = FIRST_LOCAL_SID + self.locals.len() as u32;
        self.locals.push(name.to_string());
        self.index.insert(name.to_string(), s);
        s
    }

    /// 已登记的名字的符号。
    pub(super) fn get(&self, name: &str) -> Option<u32> {
        self.index.get(name).copied()
    }

    /// 分配一个必须是新的名字（成组分配用：拿到旧符号编号就对不上了）。
    fn fresh(&mut self, name: &str) -> Result<u32, String> {
        if self.index.contains_key(name) {
            return Err(format!("内部错误：本地符号 {name} 已经登记过，资源符号的间隔对不上"));
        }
        Ok(self.sym(name))
    }

    /// 一组至多 [`SID_GAP`] 对名字：先分配 `firsts`，不满 [`SID_GAP`] 个用 `pad(k)` 补齐，再分配 `seconds`，
    /// 使每个 `seconds[k]` 的符号正好是 `firsts[k]` 的 + [`SID_GAP`]。返回 `firsts` 的符号。
    fn gap_group(&mut self, firsts: &[&str], seconds: &[&str], pad: impl Fn(usize) -> String) -> Result<Vec<u32>, String> {
        debug_assert!(firsts.len() == seconds.len() && firsts.len() <= SID_GAP as usize);
        let mut out = Vec::with_capacity(firsts.len());
        for f in firsts {
            out.push(self.fresh(f)?);
        }
        for k in firsts.len()..SID_GAP as usize {
            self.fresh(&pad(k))?;
        }
        for (s, &f) in seconds.iter().zip(&out) {
            // 名字都是新的、连续分配，按构造必然相等；留着核对，万一以后有人在中间插了分配
            if self.fresh(s)? != f + SID_GAP {
                return Err(format!("内部错误：{s} 的符号不是对应字节实体的 + {SID_GAP}"));
            }
        }
        Ok(out)
    }

    /// 封面资源最先登记，紧跟着排好元数据 `cover_image` 要用的名字（[`COVER_REF`]）：Kindle 也是按「这个名字的符号编号 − 9」
    /// 找封面资源的（6 本样本的 `e6` 减 9 都正好是封面 JPEG 的 `$164`；书架缩略图靠它，2026-10-05 真机）。
    /// 要在分配别的符号之前调（名字必须是新的）。返回封面资源的符号。
    pub(super) fn reserve_cover(&mut self, cover_res: &str) -> Result<u32, String> {
        let s = self.gap_group(&[cover_res], &[COVER_REF], |k| format!("pad-cover-{k}"))?;
        Ok(s[0])
    }

    /// 正文阶段结束，换成只能分配资源符号的阶段。
    pub(super) fn into_resources(self) -> ResourceSymbols {
        ResourceSymbols { inner: self }
    }
}

/// 资源阶段的本地符号表（[`SymbolAlloc::into_resources`]）：只能成组分配资源符号、查询已登记的名字。
pub(super) struct ResourceSymbols {
    inner: SymbolAlloc,
}

impl ResourceSymbols {
    /// 资源的（字节实体名, 资源路径）：每 [`SID_GAP`] 个一组，先这组的字节实体名，不满的用占位符号补齐，再这组的资源路径
    /// （Kindle 按「`$165` 资源路径的符号编号 − 9」找字节实体，6 本样本 443 个资源全是这样；只改资源路径或只给字节实体改名，
    /// 书架缩略图就没了，2026-10-05 真机）。返回各字节实体名的符号。
    pub(super) fn alloc_resources(&mut self, pairs: &[(String, String)]) -> Result<Vec<u32>, String> {
        let mut out = Vec::with_capacity(pairs.len());
        for (g, chunk) in pairs.chunks(SID_GAP as usize).enumerate() {
            let raws: Vec<&str> = chunk.iter().map(|(r, _)| r.as_str()).collect();
            let locs: Vec<&str> = chunk.iter().map(|(_, l)| l.as_str()).collect();
            out.extend(self.inner.gap_group(&raws, &locs, |k| format!("pad{g}-{k}"))?);
        }
        Ok(out)
    }

    /// 已登记的名字的符号；没登记是写出器自己的错（名字都在前面的阶段登记好了），报错不 panic。
    pub(super) fn require(&self, name: &str) -> Result<u32, String> {
        self.inner.get(name).ok_or_else(|| format!("内部错误：本地符号 {name} 没有登记"))
    }

    /// 全部本地符号的名字，按编号顺序（写进符号表）。
    pub(super) fn locals(&self) -> &[String] {
        &self.inner.locals
    }
}

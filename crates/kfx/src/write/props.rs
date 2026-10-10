//! 一个 KFX 样式的属性集合（[`StyleProps`]）：每个属性只有一个值，后设的覆盖先设的；按属性编号升序写出
//! （同以前排序后写出的顺序：样式实体和去重键里都是升序）。

use super::*;
use std::collections::BTreeMap;

/// KFX 样式属性（`P_*` → 值）。设值按值的种类分开（长度、符号、颜色……），免得各处手写 `Value` 写错种类；
/// 改值用 [`StyleProps::take`]/[`StyleProps::font_family_mut`]，不再在列表里按键找回头改。
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct StyleProps(BTreeMap<u32, Value>);

impl StyleProps {
    pub(super) fn new() -> StyleProps {
        StyleProps::default()
    }

    /// 原样的值（节点上带来的 `extra` 等已经是 `Value` 的，和少数结构值）。
    pub(super) fn set(&mut self, k: u32, v: Value) {
        self.0.insert(k, v);
    }

    /// 带单位的长度（[`num`]：保留 6 位小数，算不出有限值的写 0）。
    pub(super) fn len(&mut self, k: u32, v: f64, unit: u32) {
        self.set(k, num(v, unit));
    }

    /// 不带单位的数（固定版式的宽高、位置）。
    pub(super) fn float(&mut self, k: u32, v: f64) {
        self.set(k, Value::F64(v));
    }

    /// 符号值（对齐、字重、边框样式……）。
    pub(super) fn symbol(&mut self, k: u32, s: u32) {
        self.set(k, Value::Symbol(s));
    }

    /// ARGB 颜色。
    pub(super) fn color(&mut self, k: u32, argb: u32) {
        self.set(k, Value::Int(i64::from(argb)));
    }

    pub(super) fn flag(&mut self, k: u32, on: bool) {
        self.set(k, Value::Bool(on));
    }

    pub(super) fn string(&mut self, k: u32, s: String) {
        self.set(k, Value::String(s));
    }

    /// 并进另一组（`other` 的覆盖本组的）。
    pub(super) fn extend(&mut self, other: StyleProps) {
        self.0.extend(other.0);
    }

    /// 并进 `(属性, 值)` 列表（节点的 `extra`）。
    pub(super) fn extend_raw<'a>(&mut self, kv: impl IntoIterator<Item = &'a (u32, Value)>) {
        for (k, v) in kv {
            self.set(*k, v.clone());
        }
    }

    /// 拿走一个属性的值。
    pub(super) fn take(&mut self, k: u32) -> Option<Value> {
        self.0.remove(&k)
    }

    /// 字体名（整串备选）。
    pub(super) fn font_family_mut(&mut self) -> Option<&mut String> {
        match self.0.get_mut(&P_FONT_FAMILY) {
            Some(Value::String(s)) => Some(s),
            _ => None,
        }
    }

    pub(super) fn font_family(&self) -> Option<&str> {
        match self.0.get(&P_FONT_FAMILY) {
            Some(Value::String(s)) => Some(s),
            _ => None,
        }
    }

    /// 按属性编号升序。
    pub(super) fn iter(&self) -> impl Iterator<Item = (u32, &Value)> {
        self.0.iter().map(|(k, v)| (*k, v))
    }

    /// 写进样式实体的字段：属性按编号升序，最后是样式名（同样本）。
    pub(super) fn into_fields(self, name: u32) -> Vec<(u32, Value)> {
        self.0.into_iter().chain([(STYLE_NAME, Value::Symbol(name))]).collect()
    }
}

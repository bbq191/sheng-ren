//! CSS 声明值的小工具：拆 `!important`、`background` 简写拆成分项。层叠引擎和优化器（`bookconv::cssunlock`）共用。

/// `!important` 拆出来：(值, " !important" 或 "")。
pub fn split_important(value: &str) -> (&str, &'static str) {
    let v = value.trim();
    match v.rfind('!') {
        Some(i) if v[i + 1..].trim().eq_ignore_ascii_case("important") => (v[..i].trim_end(), " !important"),
        _ => (v, ""),
    }
}

/// 保留背景图的模式（profile 的 `background_images`）里 `background` 简写拆成分项：背景色、背景图、重复、位置，
/// `sizing` 时再加尺寸（`/ cover`）和附着（`fixed`）。掌阅不认 `background` 简写（整条不生效），`background-size`
/// 会把图挤变形；Kindle 要 `fixed` 和尺寸才按整页铺（2026-10-05 真机）。KFX 写出器两种写法都认。
pub fn background_longhands(value: &str, sizing: bool) -> Vec<String> {
    let (v, important) = split_important(value);
    let mut out = Vec::new();
    let mut pos = Vec::new();
    let mut size = Vec::new();
    let mut after_slash = false;
    // 括号外的 `/`（位置 / 尺寸）切成单独的词，`url(../a.png)` 里的不动（切词和层叠展开 `background` 是同一个）
    for t in crate::background_tokens(v) {
        let l = t.to_ascii_lowercase();
        if after_slash && !(["cover", "contain", "auto"].contains(&l.as_str()) || l.starts_with(|c: char| c.is_ascii_digit() || c == '.')) {
            after_slash = false; // 尺寸写完了
        }
        // 图：`url()`，以及渐变、`image-set()`（此前渐变落到最后一支，写成了不合法的 `background-color:linear-gradient(…)`）
        if l.starts_with("url(") || l.starts_with("image-set(") || l.split_once('(').is_some_and(|(f, _)| f.ends_with("gradient")) {
            out.push(format!("background-image:{t}{important}"));
        } else if l == "/" {
            after_slash = true;
        } else if ["repeat", "no-repeat", "repeat-x", "repeat-y", "space", "round"].contains(&l.as_str()) {
            out.push(format!("background-repeat:{l}{important}"));
        } else if after_slash {
            size.push(t);
        } else if ["fixed", "scroll", "local"].contains(&l.as_str()) {
            if sizing {
                out.push(format!("background-attachment:{l}{important}"));
            }
        } else if ["border-box", "padding-box", "content-box", "none"].contains(&l.as_str()) {
            // 盒子：不要
        } else if ["left", "right", "top", "bottom", "center"].contains(&l.as_str()) || l.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
            pos.push(t);
        } else {
            out.push(format!("background-color:{t}{important}"));
        }
    }
    if !pos.is_empty() {
        out.push(format!("background-position:{}{important}", pos.join(" ")));
    }
    if sizing && !size.is_empty() {
        out.push(format!("background-size:{}{important}", size.join(" ")));
    }
    out
}

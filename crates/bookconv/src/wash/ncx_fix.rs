//! NCX 修复：`dtb:uid` 对齐 OPF、manifest 里 NCX 的 `id` 规整、剥外部 DTD `<!DOCTYPE>`。
use super::*;

/// 修 `toc.ncx` 的 `dtb:uid` 跟 OPF 实际标识符不一致的问题（见 `opf_unique_identifier` 注释）。
/// 幂等、只在真的不一致时改；OPF 没有可解析的标识符（极少见）时不动。
pub(super) fn fix_ncx_uid(entries: &mut [Entry], rep: &mut WashReport) {
    let Some(uid) = opf_unique_identifier(entries) else { return };
    let want = xml_escape(&uid);
    for e in entries.iter_mut().filter(|e| e.name.to_ascii_lowercase().ends_with(".ncx")) {
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let meta = html::tags(text).find(|t| t.is_start() && t.is("meta") && tag_attr(&text[t.start..t.end], "name") == Some("dtb:uid"));
        let Some(t) = meta else { continue };
        let tag = &text[t.start..t.end];
        if tag_attr(tag, "content").is_none_or(|c| c == want) {
            continue;
        }
        let new_tag = html::set_attr(tag, "content", &want);
        e.data = format!("{}{}{}", &text[..t.start], new_tag, &text[t.end..]).into_bytes();
        rep.ncx_uid_fixed += 1;
    }
}

/// xochitl 定位目录文件不是走 EPUB 规范的 `<spine toc="IDREF">`，而是在二进制里**硬编码死查**
/// manifest 里 `id="ncx"` 这个字符串字面量（2026-09-19 反编译 xochitl 二进制坐实：在
/// `GeneratePdfFromEpub` 调用链里直接挖到这个写死的 3 字符哈希查找 key；`<spine toc="...">`
/// 只是这条硬编码查找失败时的一个后备分支，实测这条后备分支没能救回《疯探》——原因未查清，
/// 可能是 OPF 解析阶段没把 `<spine>` 的 `toc` 属性值正确落到后备分支读的那个字段）。《疯探》的
/// `<item id="toc" href="toc.ncx" .../>` + `<spine toc="toc">` 完全符合规范，但因为 manifest
/// id 不叫 "ncx"，navMap 里的标题全部提取失败、原生目录入口整个不出现（书本身照常能翻页——
/// 页面渲染走另一条不依赖这个 id 的路径）。真机验证：拿真实 content.opf 原封不动，只把这一个
/// id 从 "toc" 改成 "ncx"（`<spine toc="...">` 同步改，否则 idref 悬空），94 条章节标题全部
/// 恢复（`.epubindex` 从 7188 字节涨到 15558 字节）。幂等；已经叫 "ncx"、或跟另一条目 id 冲突
/// （改了会撞车，极罕见）时不动。
pub(super) fn fix_ncx_manifest_id(entries: &mut [Entry], rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    if opf.items.contains_key("ncx") {
        return; // 已经叫 ncx，或者已有另一条目占了这个 id——两种情况都不该动
    }
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let Some(item) = manifest_items(&text).into_iter().find(|it| it.media_type.trim() == "application/x-dtbncx+xml") else { return };
    let old_id = item.id.to_string();
    if old_id == "ncx" {
        return;
    }
    let pos = item.pos;
    let mut new_text = format!("{}{}{}", &text[..pos], html::set_attr(item.tag, "id", "ncx"), &text[pos + item.tag.len()..]);
    // <spine toc="OLD_ID"> 同步改，不然这个属性从此指向一个不存在的 id（没有这个属性的书——极少
    // 见——说明它压根没靠 spine 的 toc 属性定位目录，不用管）。
    if let Some(t) = html::tags(&new_text).find(|t| t.is_start() && t.is("spine")) {
        let tag = &new_text[t.start..t.end];
        if tag_attr(tag, "toc") == Some(old_id.as_str()) {
            new_text = format!("{}{}{}", &new_text[..t.start], html::set_attr(tag, "toc", "ncx"), &new_text[t.end..]);
        }
    }
    entries[opf.index].data = new_text.into_bytes();
    rep.ncx_manifest_id_fixed += 1;
}

/// 剥 `toc.ncx` 里指向外部 DTD 的 `<!DOCTYPE ncx PUBLIC "..." "http://www.daisy.org/...dtd">` 声明
/// （2026-09-19 真机对照《疯探》vs《雪人》坐实的第二个差异——dtb:uid 修一致后原生目录入口仍然不见，
/// 两本书剩下的结构性区别就是这条：《疯探》的 `toc.ncx` 带这个外部 DTD 引用，《雪人》没有，也没有
/// 任何其它 reader/工具要求 NCX 必须带 DOCTYPE 才能解析——它纯粹是历史遗留的验证声明。真机是
/// USB/WiFi 隧道环境，如果 xochitl 的 XML 解析器老实去联网取这个外部 DTD，离线或路由不通时很可能
/// 卡住/超时/直接判整份 NCX 不可用，原生目录入口因此消失，但书本身照常能读——不影响 spine 阅读，
/// 只影响"目录"这个附加功能，症状完全吻合。剥掉不改变 NCX 的任何实际语义，纯粹去掉这个外部依赖，
/// `build_ncx` 自己生成的 NCX 也从来不带 DOCTYPE，这里是让已有 NCX 向那个已经验证过没问题的形态看齐。
pub(super) fn strip_ncx_doctype(entries: &mut [Entry], rep: &mut WashReport) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"(?is)<!DOCTYPE\s+ncx\b[^>]*>\s*"#).unwrap());
    for e in entries.iter_mut() {
        if e.name.to_ascii_lowercase().ends_with(".ncx") {
            if let Ok(text) = std::str::from_utf8(&e.data) {
                if re.is_match(text) {
                    e.data = re.replace(text, "").into_owned().into_bytes();
                    rep.ncx_doctype_stripped += 1;
                }
            }
        }
    }
}

//! 版面以外的实体：样式、位置映射、目录与锚点、导航、元数据、资源和字体，最后排序加清单写成容器（[`finish`]）。

use super::*;
use crate::container::{SID_COMPRESSION, SID_DRM_SCHEME};

// 文档数据（`$538`）里照样本写、含义还没弄清的字段和值。
const DOC_FIELD_112: u32 = 112;
const DOC_VALUE_383: u32 = 383;
const DOC_FIELD_436: u32 = 436;
const DOC_VALUE_441: u32 = 441;
const DOC_FIELD_477: u32 = 477;
const DOC_VALUE_56: u32 = 56;

/// 容器信息里的分块大小（样本都是 4096）。
const CHUNK_SIZE: i64 = 4096;

/// 32 进制大写（容器 id、book_id 用）。
/// 唯一 ID → 容器 id（`CR!` + 28 个字符）。书里存 4 处（见 `Container::set_container_id`），都写这一个值。
pub fn container_id(id: u64) -> String {
    format!("CR!{}", base32(id, 28))
}

fn base32(mut n: u64, len: usize) -> String {
    const A: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut s = Vec::with_capacity(len);
    for _ in 0..len {
        s.push(A[(n % 32) as usize]);
        n = (n / 32) ^ n.rotate_left(13);
    }
    String::from_utf8(s).unwrap_or_default()
}

/// 实体头：样本里都是 `{$410: 0, $411: 0}`（不压缩、没有 DRM）。
fn entity_header() -> Vec<Item> {
    vec![Item::Bvm, Item::Value(Value::Struct(vec![(SID_COMPRESSION, Value::Int(0)), (SID_DRM_SCHEME, Value::Int(0))]))]
}

pub(super) fn ent(id: u32, ty: u32, v: Value) -> Entity {
    Entity { id, ty, version: 1, header: entity_header(), body: Body::Ion(vec![Item::Bvm, Item::Value(v)]) }
}

/// `$264` 的写法：升序 id，连续的写成 `[起点, 个数]`。
pub(super) fn compress_ids(mut ids: Vec<i64>) -> Value {
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j + 1 < ids.len() && ids[j + 1] == ids[j] + 1 {
            j += 1;
        }
        if j > i {
            out.push(Value::List(vec![Value::Int(ids[i]), Value::Int((j - i + 1) as i64)]));
        } else {
            out.push(Value::Int(ids[i]));
        }
        i = j + 1;
    }
    Value::List(out)
}

/// `$609` 的写法：`[前进, 新 id]`，id 正好加一时只写前进量，最后 `[前进, 0]`。
pub(super) fn pid_map(order: &[(i64, usize)]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut prev: Option<(i64, usize)> = None;
    for &(eid, len) in order {
        let adv = prev.map_or(0, |p| p.1) as i64;
        match prev {
            Some((pe, _)) if eid == pe + 1 => out.push(Value::Int(adv)),
            _ => out.push(Value::List(vec![Value::Int(adv), Value::Int(eid)])),
        }
        prev = Some((eid, len));
    }
    if let Some((_, len)) = prev {
        out.push(Value::List(vec![Value::Int(len as i64), Value::Int(0)]));
    }
    out
}

/// 样式实体（`$157`），按第一次用到的顺序。
pub(super) fn style_entities(b: &mut Builder, entities: &mut Vec<Entity>) {
    for (s, props) in b.styles.take_entities() {
        entities.push(ent(s, T_STYLE, Value::Struct(props.into_fields(s))));
    }
}

/// 要嵌入的字体：样式里用到、书里有 `@font-face` 和字体文件的（正文、批注的字体优化器已经去掉，不会出现在这里）。
/// 字体字节从书里搬出来（写出器只在这里用到），同一个文件给了几个字体名的，前面的复制、最后一个搬走。
pub(super) fn take_fonts(book: &mut Loaded, b: &Builder) -> Vec<(String, crate::css::FontFace, Vec<u8>)> {
    let mut faces: HashMap<String, crate::css::FontFace> = HashMap::new();
    for (path, text) in &book.css {
        for f in crate::css::font_faces(text, path) {
            faces.entry(f.family.to_lowercase()).or_insert(f);
        }
    }
    let wanted: Vec<(String, crate::css::FontFace)> = b
        .styles
        .used_fonts()
        .iter()
        .filter_map(|f| {
            let face = faces.get(&f.to_lowercase())?;
            book.fonts.iter().any(|(p, _)| *p == face.path).then(|| (f.clone(), face.clone()))
        })
        .collect();
    let mut files: HashMap<String, Vec<u8>> = std::mem::take(&mut book.fonts).into_iter().collect();
    let mut out = Vec::with_capacity(wanted.len());
    for (i, (family, face)) in wanted.iter().enumerate() {
        let used_later = wanted[i + 1..].iter().any(|(_, f)| f.path == face.path);
        let bytes = if used_later { files.get(&face.path).cloned() } else { files.remove(&face.path) };
        out.push((family.clone(), face.clone(), bytes.unwrap_or_default()));
    }
    out
}

/// 阅读顺序（`$169`）：全部版面按书的顺序。阅读顺序实体和文档数据里各写一份。
fn reading_orders(sections: &[SectionOut]) -> Value {
    let order_list = Value::List(sections.iter().map(|s| Value::Symbol(s.name)).collect());
    Value::List(vec![Value::Struct(vec![(READING_ORDER_NAME, Value::Symbol(DEFAULT_READING_ORDER)), (SECTIONS, order_list)])])
}

/// 阅读顺序、各版面的节点、位置范围、位置映射（`$609`）。
pub(super) fn position_entities(sections: &mut [SectionOut], entities: &mut Vec<Entity>) {
    entities.push(ent(NO_NAME, T_READING_ORDERS, Value::Struct(vec![(READING_ORDERS, reading_orders(sections))])));
    entities.push(ent(
        NO_NAME,
        T_SECTION_EIDS,
        Value::List(sections.iter_mut().map(|s| Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (LIST, compress_ids(std::mem::take(&mut s.eids)))])).collect()),
    ));
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for s in sections.iter() {
        ranges.push(Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (START, Value::Int(pos as i64)), (LENGTH, Value::Int(s.length as i64))]));
        pos += s.length;
    }
    entities.push(ent(NO_NAME, T_SECTION_RANGES, Value::Struct(vec![(LIST, Value::List(ranges))])));
    for s in sections.iter_mut() {
        entities.push(ent(s.name, T_SECTION_PID_MAP, Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (LIST, Value::List(std::mem::take(&mut s.pid_map)))])));
    }
}

/// 目录、锚点（链接和目录的目标）、标题导航、封面地标。
pub(super) fn navigation_entities(
    book: &Loaded,
    b: &mut Builder,
    sections: &[SectionOut],
    id_map: &HashMap<(String, String), (i64, usize)>,
    cover_tmpl: Option<i64>,
    entities: &mut Vec<Entity>,
) {
    // 目录：锚点 + 导航。
    let mut toc_stack: Vec<(u32, Vec<Value>)> = vec![(0, Vec::new())];
    for t in &book.toc {
        let Some(&(eid, off)) = id_map.get(&(t.path.clone(), t.frag.clone())).or_else(|| id_map.get(&(t.path.clone(), String::new()))) else {
            b.warnings.push(format!("目录项找不到位置：{}", t.label));
            continue;
        };
        let level = t.level + 1;
        while toc_stack.len() > 1 && toc_stack.last().is_some_and(|(l, _)| *l >= level) {
            close_level(&mut toc_stack);
        }
        b.anchor((t.path.clone(), t.frag.clone()));
        let entry = Value::Struct(vec![nav_label(t.label.clone()), (NAV_TARGET, position(eid, off))]);
        if let Some(last) = toc_stack.last_mut() {
            last.1.push(entry);
        }
        toc_stack.push((level, Vec::new()));
    }
    while toc_stack.len() > 1 {
        close_level(&mut toc_stack);
    }
    let mut toc_entries = toc_stack.pop().map(|t| t.1).unwrap_or_default();
    if toc_entries.is_empty() {
        // 没有目录：每个版面一项，标签用序号。
        toc_entries = sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Value::Struct(vec![
                    nav_label((i + 1).to_string()),
                    (NAV_TARGET, position(s.first_eid, 0)),
                ])
            })
            .collect();
    }
    // 锚点（链接、目录的目标）。找不到目标的指向目标文件开头，文件也找不到就指向书的开头。之后不再登记锚点。
    let book_start = (sections[0].first_eid, 0usize);
    for (key, an) in std::mem::take(&mut b.anchors) {
        let pos = id_map.get(&key).or_else(|| id_map.get(&(key.0.clone(), String::new()))).copied().unwrap_or_else(|| {
            b.warnings.push(format!("链接目标找不到：{}#{}", key.0, key.1));
            book_start
        });
        entities.push(ent(an, T_ANCHOR, Value::Struct(vec![(ANCHOR_NAME, Value::Symbol(an)), (ANCHOR_POSITION, position(pos.0, pos.1))])));
    }
    let mut nav_names = Vec::new();
    // 标题导航：每个标题级别一组（样本里按级别第一次出现的顺序排）。
    if !b.headings.is_empty() {
        let mut levels: Vec<u8> = Vec::new();
        for (l, _) in &b.headings {
            if !levels.contains(l) {
                levels.push(*l);
            }
        }
        let unit = |eid: i64| {
            vec![
                nav_label("heading-nav-unit".into()),
                (NAV_TARGET, position(eid, 0)),
            ]
        };
        let groups: Vec<Value> = levels
            .iter()
            .map(|&l| {
                let eids: Vec<i64> = b.headings.iter().filter(|(hl, _)| *hl == l).map(|h| h.1).collect();
                let mut f = vec![(NAV_LANDMARK_TYPE, Value::Symbol(HEADING_LEVEL_1 + u32::from(l) - 1))];
                f.extend(unit(eids[0]));
                f.push((NAV_ENTRIES, Value::List(eids.iter().map(|&e| Value::Struct(unit(e))).collect())));
                Value::Struct(f)
            })
            .collect();
        let n = b.syms.sym("nav-headings");
        nav_names.push(Value::Symbol(n));
        entities.push(ent(n, T_NAV_CONTAINER, Value::Struct(vec![(NAV_TYPE, Value::Symbol(NAV_TYPE_HEADINGS)), (NAV_NAME, Value::Symbol(n)), (NAV_ENTRIES, Value::List(groups))])));
    }
    if let Some(ct) = cover_tmpl {
        let n = b.syms.sym("nav-landmarks");
        nav_names.push(Value::Symbol(n));
        entities.push(ent(
            n,
            T_NAV_CONTAINER,
            Value::Struct(vec![
                (NAV_TYPE, Value::Symbol(NAV_TYPE_LANDMARKS)),
                (NAV_NAME, Value::Symbol(n)),
                (
                    NAV_ENTRIES,
                    Value::List(vec![Value::Struct(vec![
                        (NAV_LANDMARK_TYPE, Value::Symbol(LANDMARK_COVER)),
                        nav_label("cover-nav-unit".into()),
                        (NAV_TARGET, position(ct, 0)),
                    ])]),
                ),
            ]),
        ));
    }
    let toc_name = b.syms.sym("nav-toc");
    nav_names.push(Value::Symbol(toc_name));
    entities.push(ent(
        toc_name,
        T_NAV_CONTAINER,
        Value::Struct(vec![(NAV_TYPE, Value::Symbol(NAV_TYPE_TOC)), (NAV_NAME, Value::Symbol(toc_name)), (NAV_ENTRIES, Value::List(toc_entries))]),
    ));
    entities.push(ent(
        NO_NAME,
        T_NAV_ROOTS,
        Value::List(vec![Value::Struct(vec![(READING_ORDER_NAME, Value::Symbol(DEFAULT_READING_ORDER)), (NAV_CONTAINERS, Value::List(nav_names))])]),
    ));
    entities.push(ent(NO_NAME, T_NAV_EMPTY, Value::Struct(vec![(NAV_ENTRIES, Value::List(Vec::new()))])));
}

/// 唯一 ID 派生的 content_id（也当 ASIN 写）。
fn content_id(id: u64) -> String {
    format!("{:016X}{:016X}", id, fnv64(&id.to_le_bytes()))
}

/// 元数据（`$490`）和文档数据（`$538`）。
#[allow(clippy::too_many_arguments)]
pub(super) fn metadata_entities(
    book: &Loaded,
    b: &mut Builder,
    id: u64,
    sections: &[SectionOut],
    fixed_canvas: Option<(i64, i64)>,
    direction: u32,
    cover_res: Option<usize>,
    entities: &mut Vec<Entity>,
) {
    let content_id = content_id(id);
    let container_id = container_id(id);
    let book_id = base32(id.rotate_left(17), 23);
    let book_lang = book_language(book);
    let kv = |k: &str, v: Value| Value::Struct(vec![(META_KEY, Value::String(k.into())), (META_VALUE, v)]);
    let s = |v: &str| Value::String(v.to_string());
    let mut title_meta = vec![
        kv("book_id", s(&book_id)),
        kv("title", s(&book.meta.title)),
        kv("publisher", s(&book.meta.publisher)),
        kv("language", s(&meta_language(book_lang.as_deref()))),
        kv("issue_date", s(&issue_date(&book.meta.date))),
        kv("content_id", s(&content_id)),
        kv("cde_content_type", s("PDOC")),
        kv("ASIN", s(&content_id)),
        kv("is_sample", Value::Bool(false)),
        kv("asset_id", s(&container_id)),
    ];
    for a in &book.meta.authors {
        title_meta.push(kv("author", s(a)));
    }
    if cover_res.is_some() {
        title_meta.push(kv("cover_image", s(COVER_REF)));
    }
    let group = |name: &str, entries: Vec<Value>| Value::Struct(vec![(META_GROUP_NAME, s(name)), (META_ENTRIES, Value::List(entries))]);
    entities.push(ent(
        NO_NAME,
        T_METADATA,
        Value::Struct(vec![(
            META_GROUPS,
            Value::List(
                vec![
                    if fixed_canvas.is_some() {
                        // 固定版式（同 Amazon 转的测试漫画）：锁竖屏、声明固定版式
                        let lock = book.meta.fixed_layout.iter().find(|(n, _)| *n == 124).map_or("portrait", |(_, v)| v.as_str());
                        group("kindle_ebook_metadata", vec![kv("book_orientation_lock", s(lock))])
                    } else {
                        group("kindle_ebook_metadata", vec![kv("nested_span", s("enabled")), kv("selection", s("enabled"))])
                    },
                    group("kindle_title_metadata", title_meta),
                    group("kindle_audit_metadata", vec![kv("creator_version", s(FILE_CREATOR_VERSION)), kv("file_creator", s("epub-to-kfx"))]),
                ]
                .into_iter()
                .chain(fixed_canvas.map(|_| group("kindle_capability_metadata", vec![kv("yj_fixed_layout", Value::Int(1)), kv("continuous_popup_progression", Value::Int(0))])))
                .collect(),
            ),
        )]),
    ));

    // 文档数据（照样本的值，含义还没全弄清）。
    let mut doc_data = if fixed_canvas.is_some() {
        vec![(DOC_DIRECTION, Value::Symbol(direction)), (DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)), (DOC_FIXED, Value::Symbol(DOC_FIXED_VALUE))]
    } else {
        vec![
            (DOC_FIELD_112, Value::Symbol(DOC_VALUE_383)),
            (P_FONT_SIZE, num(1.0, U_EM)),
            (DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)),
            (DOC_DIRECTION, Value::Symbol(direction)),
            (DOC_FIELD_436, Value::Symbol(DOC_VALUE_441)),
        ]
    };
    if cover_res.is_some() {
        let aux = b.syms.sym(COVER_AUX);
        doc_data.push((DOC_AUX, Value::Struct(vec![(DOC_AUX_NAME, Value::Symbol(aux))])));
    }
    doc_data.push((ion::SID_MAX_ID, Value::Int(b.next_eid)));
    if fixed_canvas.is_none() {
        doc_data.push((P_LINE_HEIGHT, num(LH_EM, U_EM)));
    }
    doc_data.extend([(DOC_FIELD_477, Value::Symbol(DOC_VALUE_56)), (READING_ORDERS, reading_orders(sections))]);
    entities.push(ent(NO_NAME, T_DOCUMENT_DATA, Value::Struct(doc_data)));
}

/// 元数据的 `issue_date`（`YYYY-MM-DD`）：OPF 日期取前面能用的部分，只写到年、月的补成当年 1 月 1 日、当月 1 日
/// （`2019` → `2019-01-01`，`2019-05` → `2019-05-01`；以前不足 10 个字符的整个换成 2000-01-01）。认不出年份的写 2000-01-01。
pub(super) fn issue_date(date: &str) -> String {
    let d = date.trim();
    fn digits(s: Option<&str>) -> Option<&str> {
        s.filter(|s| s.bytes().all(|c| c.is_ascii_digit()))
    }
    let (Some(y), m, day) = (digits(d.get(..4)), digits(d.get(5..7)), digits(d.get(8..10))) else {
        return "2000-01-01".to_string();
    };
    let sep = |i: usize| d.as_bytes().get(i) == Some(&b'-');
    match (m.filter(|_| sep(4)), day.filter(|_| sep(7))) {
        (Some(m), Some(day)) => format!("{y}-{m}-{day}"),
        (Some(m), None) => format!("{y}-{m}-01"),
        _ => format!("{y}-01-01"),
    }
}

/// 图片、字体的字节实体（`$417`/`$418`）、资源（`$164`）、字体（`$262`）。资源阶段（`syms` 只能成组分配资源符号）。
pub(super) fn resource_entities(
    syms: &mut ResourceSymbols,
    res: &mut ResourceStore,
    fonts: Vec<(String, crate::css::FontFace, Vec<u8>)>,
    cover_res: Option<usize>,
    entities: &mut Vec<Entity>,
) -> Result<(), String> {
    // 资源（字节实体名、资源路径）的符号最后分配：书里不能有实体的 id 排在资源路径的符号后面。以前资源先分配、目录锚点和
    // 导航后分配，固定版式里比画布小的图有的页整页空白（哪几页随实体集合变）；Amazon 转的书从来不这样排，给它加上排在后面的
    // 锚点也出空白页（2026-10-06 真机，42 本测试书对照，见 docs/kfx.md）。`syms` 是资源阶段的符号表，保证了这个顺序。
    // 字节实体名、资源路径：图片在前、字体在后，一起按组分配符号（资源路径＝字节实体 + 9，见 [`ResourceSymbols::alloc_resources`]）。
    let n_images = res.len();
    let mut raws: Vec<(String, String)> = res.iter().enumerate().map(|(i, r)| (raw_name(i, &r.name, cover_res), r.location.clone())).collect();
    raws.extend((0..fonts.len()).map(|i| (format!("font{i}-ad"), format!("resource/font{i}"))));
    let raw_sids = syms.alloc_resources(&raws)?;
    for (i, (family, face, bytes)) in fonts.into_iter().enumerate() {
        let loc = &raws[n_images + i].1;
        entities.push(ent(
            NO_NAME,
            T_FONT,
            Value::Struct(vec![
                (P_FONT_FAMILY, Value::String(family)),
                (P_FONT_STYLE, Value::Symbol(if face.italic { STYLE_ITALIC } else { FONT_NORMAL })),
                (P_FONT_WEIGHT, Value::Symbol(if face.bold { WEIGHT_BOLD } else { WEIGHT_NORMAL })),
                (P_FONT_STRETCH, Value::Symbol(FONT_NORMAL)),
                (RES_LOCATION, Value::String(loc.clone())),
            ]),
        ));
        let id = raw_sids[n_images + i];
        entities.push(Entity { id, ty: T_RAW_FONT, version: 1, header: entity_header(), body: Body::Raw(bytes) });
    }
    for (i, &raw) in raw_sids.iter().enumerate().take(n_images) {
        // 资源实体的名字在版面阶段（或封面最先）登记过
        let r = &res[i];
        let n = syms.require(&r.name)?;
        entities.push(ent(
            n,
            T_RESOURCE,
            Value::Struct(vec![
                (RES_FORMAT, Value::Symbol(r.format)),
                (RES_MIME, Value::String(r.mime.to_string())),
                (RES_LOCATION, Value::String(r.location.clone())),
                (RES_WIDTH, Value::Int(i64::from(r.width))),
                (RESOURCE_REF, Value::Symbol(n)),
                (RES_HEIGHT, Value::Int(i64::from(r.height))),
            ]),
        ));
        let bytes = res.take_bytes(i);
        entities.push(Entity { id: raw, ty: T_RAW_MEDIA, version: 1, header: entity_header(), body: Body::Raw(bytes) });
    }
    Ok(())
}

/// 图片字节的实体名。封面的要叫「文档数据 `$538.$597.$614` 的名字 + `-ad`」：Kindle 书架缩略图按这个找
/// （样本里元数据的 `cover_image` 一律写 `e6`，常常指到别的插图甚至锚点，缩略图照样对；
/// 把这个实体改名缩略图就没了，2026-10-05 真机）。
fn raw_name(i: usize, name: &str, cover_res: Option<usize>) -> String {
    if Some(i) == cover_res { format!("{COVER_AUX}-ad") } else { format!("{name}-ad") }
}

/// 实体按类型排序、加上清单（`$419`），连同符号表、能力表写成容器。
pub(super) fn finish(syms: &ResourceSymbols, res: &ResourceStore, sections: &[SectionOut], mut entities: Vec<Entity>, cover_res: Option<usize>, id: u64) -> Result<Vec<u8>, String> {
    let container_id = container_id(id);
    let s = |v: &str| Value::String(v.to_string());
    // 按类型排序（和样本一样），清单放最后。
    entities.sort_by_key(|e| e.ty);
    let mut deps = Vec::new();
    for s in sections {
        if !s.resources.is_empty() {
            // 样本里每个资源列两次（含义不明，照抄）。
            let mut names: Vec<Value> = Vec::new();
            for &r in &s.resources {
                let v = Value::Symbol(syms.require(&res[r].name)?);
                if !names.contains(&v) {
                    names.push(v.clone());
                    names.push(v);
                }
            }
            deps.push(Value::Struct(vec![(EID, Value::Symbol(s.name)), (MANIFEST_DEP_LIST, Value::List(names))]));
        }
    }
    for (i, r) in res.iter().enumerate() {
        deps.push(Value::Struct(vec![
            (EID, Value::Symbol(syms.require(&r.name)?)),
            (MANIFEST_DEP_LIST, Value::List(vec![Value::Symbol(syms.require(&raw_name(i, &r.name, cover_res))?)])),
        ]));
    }
    let all_ids = Value::List(entities.iter().map(|e| Value::Symbol(e.id)).collect());
    let manifest = Value::Annotated(
        vec![T_MANIFEST],
        Box::new(Value::Struct(vec![
            (MANIFEST_CONTAINERS, Value::List(vec![Value::Struct(vec![(EID, s(&container_id)), (LIST, all_ids)])])),
            (MANIFEST_DEPS, Value::List(deps)),
        ])),
    );
    entities.push(ent(NO_NAME, T_MANIFEST, manifest));

    // 容器。
    let symtab = vec![
        Item::Bvm,
        Item::Value(Value::Annotated(
            vec![ion::SID_ION_SYMBOL_TABLE],
            Box::new(Value::Struct(vec![
                (
                    ion::SID_IMPORTS,
                    Value::List(vec![Value::Struct(vec![
                        (ion::SID_NAME, s("YJ_symbols")),
                        (ion::SID_VERSION, Value::Int(YJ_SYMBOLS_VERSION)),
                        (ion::SID_MAX_ID, Value::Int(i64::from(YJ_SYMBOLS_MAX_ID))),
                    ])]),
                ),
                (ion::SID_SYMBOLS, Value::List(syms.locals().iter().map(|l| s(l)).collect())),
            ])),
        )),
    ];
    let caps = vec![
        Item::Bvm,
        Item::Value(Value::Annotated(
            vec![CAPABILITIES],
            Box::new(Value::List(vec![
                Value::Struct(vec![(META_KEY, s("kfxgen.positionMaps")), (ion::SID_VERSION, Value::Int(2))]),
                Value::Struct(vec![(META_KEY, s("kfxgen.pidMapWithOffset")), (ion::SID_VERSION, Value::Int(1))]),
            ])),
        )),
    ];
    use crate::container::*;
    let info = Value::Struct(vec![
        (SID_CONTAINER_ID, s(&container_id)),
        (SID_COMPRESSION, Value::Int(0)),
        (SID_DRM_SCHEME, Value::Int(0)),
        (SID_INDEX_OFFSET, Value::Int(0)),
        (SID_INDEX_LENGTH, Value::Int(0)),
        (SID_SYMTAB_OFFSET, Value::Int(0)),
        (SID_SYMTAB_LENGTH, Value::Int(0)),
        (SID_CHUNK_SIZE, Value::Int(CHUNK_SIZE)),
        (SID_CAPS_OFFSET, Value::Int(0)),
        (SID_CAPS_LENGTH, Value::Int(0)),
    ]);
    let kfxgen = format!(
        "[{{key:\"kfxgen_package_version\",value:\"epub-to-kfx {FILE_CREATOR_VERSION}\"}},{{key:\"kfxgen_payload_sha1\",value:\"{}\"}},{{key:\"kfxgen_acr\",value:\"{container_id}\"}}]",
        "0".repeat(40)
    );
    let c = Container { version: 2, info, symtab, capabilities: caps, kfxgen: kfxgen.into_bytes(), entities };
    Ok(c.into_bytes())
}

/// 元数据里写的语言。中文书写 `en`：Kindle 只看元数据语言决定开不开「Aa → 间距」里的段间距、字间距、字符间距，
/// 中文书不开；正文样式里的 `$10` 仍是书本身的语言，中文排版（字体、避头尾、标点挤压）不受影响。代价是长按查词
/// 默认弹英文词典，要在右下角手动切到中文词典（用户 2026-10-05 选的，真机「封面结构63」）。
fn meta_language(book_lang: Option<&str>) -> String {
    match book_lang {
        Some(l) if l.to_ascii_lowercase().starts_with("zh") => "en".to_string(),
        Some(l) => l.to_string(),
        None => "en".to_string(),
    }
}

/// 导航项的标签字段。
fn nav_label(text: String) -> (u32, Value) {
    (NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String(text))]))
}

/// 位置（目录项、锚点、导航的目标）：节点 id + 字符偏移。
fn position(eid: i64, offset: usize) -> Value {
    Value::Struct(vec![(EID, Value::Int(eid)), (OFFSET, Value::Int(offset as i64))])
}

fn close_level(stack: &mut Vec<(u32, Vec<Value>)>) {
    let Some((_, kids)) = stack.pop() else { return };
    if kids.is_empty() {
        return;
    }
    if let Some(Value::Struct(f)) = stack.last_mut().and_then(|p| p.1.last_mut()) {
        f.push((NAV_ENTRIES, Value::List(kids)));
    }
}

/// 给 `Profile` 加字段会让这里编译不过：逼着决定新字段进不进产物指纹（`library` 的 `fingerprint.rs`）。
/// 指纹没变就不重传，所以漏了＝改了规则书不重建，多了＝白白重传全部书。分类：
/// - 直接：`library/src/fingerprint.rs` 里有它自己的一段；
/// - 阅读器模型：进 `bookconv::reader_model::ReaderModel`，由 `m` 段的签名带上；
/// - 否：不影响产物内容。
///
/// （2026-10-15 对拍发现 `background_auto_height`、`margin_top` 改值时产物会变但指纹不变，已补进 `kindle_rules` 段的 `h`、`p`。）
#[test]
fn every_field_has_a_fingerprint_decision() {
    let p = get("kindle").unwrap();
    let Profile {
        // 直接
        id: _,
        screen: _,
        color: _,
        formats: _,
        readable: _,
        notes:
            NotePolicy {
                mode: _,
                icons: _,
                backlinks: _,
            },
        comic:
            ComicPolicy {
                margin: _,
                reader_margins: _,
                readable: _,
                page_direction: _,
                split_spreads: _,
                // 按书由 library 的生成计划加指纹标记（Bangumi 日漫），不在设备段：开着的模式不能让全部漫画重建
                meta_direction: _,
                fixed_layout: _,
                fixed_layout_percent: _,
                format: _,
                gamma: _,
                autocontrast: _,
                gutter_crop: _,
                trim_specks: _,
                page_numbers: _,
                upscale: _,
                dark_bg: _,
            },
        background_images: _,
        background_sizing: _,
        caption_fit: _,
        image_alpha: _,
        webp: _,
        text_repair_only: _,
        repair_note_links: _,
        kindle_rules: _,
        image_gamma: _,
        cover_thumb_aspect: _,
        cover_meta_thumb: _,
        cover_thumb_fit: _,
        cover_image_thumb: _,
        // 竖排：按书出字母，不进设备段（规则 9）
        vertical_text: _,
        vertical_ascii: _,
        image_sizing: _,
        font_px: _,
        // 阅读器模型（也有的同时直接进）
        em_px: _,
        line_px: _,
        content_px: _,
        // 否
        name: _,
        deliver: _,
        css,
    } = p.clone();
    let CssQuirks {
        css_rgba: _,
        background_no_repeat: _,
        display_none: _,
        line_height_except_p: _,
        line_height_floor: _,
        line_height_numbers: _,
        heading_font_keywords: _,
        heading_sizes: _,
        heading_center: _,
        margin_top: _,
        page_top: _,
        trailing_br_line: _,
        inline_css_lengths: _,
        padding: _,
        borders: _,
        heading_margins: _,
        px_margins: _,
        percent_margins: _,
        block_width: _,
        negative_margins: _,
        cell_middle: _,
        inline_valign: _,
        cell_width: _,
        margin_auto: _,
        float: _,
        horizontal_scale: _,
        em_root: _,
        rem: _,
        important: _,
        background_auto_height: _,
    } = css;
}

use super::*;

/// 漫画阅读范围没写时和产物格式的一样；改了的克隆不影响内置的。
#[test]
fn comic_readable_falls_back_to_output_readable() {
    let mut p = fixture("xochitl").unwrap().clone();
    assert_eq!(
        (p.comic.reader_margins, p.comic_readable()),
        (
            Some(1),
            Screen {
                width: 952,
                height: 1457
            }
        )
    );
    p.comic.reader_margins = None;
    p.comic.readable = None;
    assert_eq!(
        p.comic_readable(),
        Screen {
            width: 842,
            height: 1455
        }
    );
    p.readable.insert(
        Format::Epub,
        Screen {
            width: 800,
            height: 1400,
        },
    );
    assert_eq!(
        (p.output_readable(), p.comic_readable()),
        (
            Screen {
                width: 800,
                height: 1400
            },
            Screen {
                width: 800,
                height: 1400
            }
        )
    );
    assert_eq!(fixture("xochitl").unwrap().comic.reader_margins, Some(1));
}

/// 去掉了的 `ppi` 键：写了照收（不论值），不写也行。
#[test]
fn dropped_ppi_key_is_still_accepted() {
    let rest = "name = \"x\"\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n[screen]\nwidth = 1000\nheight = 1500\n";
    assert!(Profile::parse("x", rest).is_ok());
    assert!(Profile::parse("x", &format!("ppi = 300\n{rest}")).is_ok());
    assert!(Profile::parse("x", &format!("ppi = 0\n{rest}")).is_ok());
    assert!(
        Profile::parse("x", &format!("dpi = 300\n{rest}")).is_err(),
        "别的不认识的键照样报错"
    );
}

#[test]
fn builtin_profiles_parse() {
    let ids: Vec<_> = Registry::builtin().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["bigme", "hanvon", "ireader", "kindle", "kobo"]);
    let k = get("kindle").unwrap();
    let i = get("ireader").unwrap();
    assert_eq!(
        i.deliver,
        Some(Deliver::Mtp {
            mount: "ireader".into(),
            link: "Ireader".into(),
            dir: "Documents".into()
        }),
        "掌阅：MTP，产物放存储根下 Documents/"
    );
    assert_eq!(
        (k.format(), k.output_readable()),
        (
            Format::Kfx,
            Screen {
                width: 1104,
                height: 1546
            }
        ),
        "Kindle 自带阅读器真机实测"
    );
    assert_eq!(
        (k.format_for(false), k.format_for(true)),
        (Format::Kfx, Format::Kfx),
        "kindle：文字书、漫画都出 KFX"
    );
    assert!(
        k.background_images && i.background_images,
        "背景图：kindle、ireader 保留"
    );
    assert!(
        k.background_sizing && !i.background_sizing,
        "背景图尺寸：kindle 保留，ireader 去掉"
    );
    assert!(k.css.css_rgba && !i.css.css_rgba, "rgba()：只有掌阅不认");
    assert!(k.css.background_no_repeat && i.css.background_no_repeat);
    assert!(k.css.margin_top && i.css.margin_top);
    assert!(
        k.css.background_auto_height && !i.css.background_auto_height,
        "背景尺寸的 auto 高：只有掌阅不认"
    );
    assert_eq!(
        (k.image_sizing, i.image_sizing),
        (ImageSizing::Css, ImageSizing::DevicePx)
    );
    assert!(
        k.font_px.is_some(),
        "掌阅折算图片宽度要 Kindle 的 1em 像素数"
    );
    assert!(
        k.caption_fit && i.caption_fit,
        "图注同页：kindle、ireader 开"
    );
    assert!(
        !k.image_alpha && i.image_alpha,
        "透明图合成白底：只有 kindle"
    );
    assert!(
        k.text_repair_only && i.text_repair_only,
        "文字书只修复：内置模式都是"
    );
    assert!(!k.repair_note_links && !i.repair_note_links);
    assert!(
        !k.kindle_rules && i.kindle_rules,
        "标签缺省样式：掌阅写进书里，Kindle 由写出器排"
    );
    assert_eq!(
        (i.format(), i.output_readable()),
        (
            Format::Epub,
            Screen {
                width: 1440,
                height: 1920
            }
        ),
        "掌阅整页图铺满整屏"
    );
    assert!(!k.color && !i.color);
    assert_eq!((k.comic.margin, i.comic.margin), (1, 1));
    assert_eq!(k.comic.reader_margins, None);
    assert_eq!(
        k.comic_readable(),
        Screen {
            width: 1272,
            height: 1696
        },
        "Kindle 漫画固定版式整屏"
    );
    assert!(k.comic.fixed_layout && !i.comic.fixed_layout);
    assert_eq!((k.notes.backlinks, i.notes.backlinks), (true, true));
    assert_eq!(
        (
            k.comic.page_direction.as_deref(),
            i.comic.page_direction.as_deref()
        ),
        (None, None),
        "漫画都照书的翻页方向（掌阅往右翻的书要在阅读器里选「漫画」排版才铺满，2026-10-15）"
    );
    let b = get("bigme").unwrap();
    assert_eq!(
        b.deliver,
        Some(Deliver::Mtp {
            mount: "bigme".into(),
            link: "Bigme".into(),
            dir: "Books".into()
        }),
        "Bigme：MTP，产物放存储根下 Books/"
    );
    assert_eq!(
        (b.screen.width, b.screen.height, b.color, b.format()),
        (1264, 1680, false, Format::Epub),
        "Bigme B7 Pro 黑白版真机 wm size；只出 EPUB"
    );
    assert_eq!(
        b.output_readable(),
        Screen {
            width: 1069,
            height: 1424
        },
        "Bigme 文字书阅读范围（2026-11-10 真机，xReader 默认页边距）"
    );
    assert!(
        b.comic.fixed_layout && b.comic.fixed_layout_percent,
        "Bigme 漫画用固定版式方案 D（2026-11-10 真机比流式大约大 10%）"
    );
    assert_eq!(
        b.cover_thumb_aspect, None,
        "Bigme 书架取正文第一页，不写缩略图比例"
    );
    assert!(!b.cover_meta_thumb, "Bigme 书架不认 eink-thumb");
    assert!(get("nope").is_none());
    assert!(
        get("xochitl").is_none(),
        "Move 卖了，内置模式删了（2026-10-31）"
    );
}

/// 2026-11-10 维护者定五台漫画画质处理一致：拉满对比度、裁杂点、裁页码、黑底补边都开，伽马 1.1。
/// 掌阅没上真机比过，先跟 kindle/kobo/hanvon 一致。
#[test]
fn builtin_profiles_share_comic_quality() {
    for id in ["kindle", "ireader", "kobo", "hanvon", "bigme"] {
        let p = get(id).unwrap();
        assert!(
            p.comic.autocontrast
                && p.comic.trim_specks
                && p.comic.page_numbers
                && p.comic.dark_bg,
            "{id}：漫画画质四项应一致开着"
        );
        assert_eq!(p.comic.gamma, Some(1.1), "{id}：漫画伽马 1.1");
    }
}

/// 2026-11-10 维护者定：五台都拆横向跨页（竖屏只拆、不旋转）。
#[test]
fn builtin_profiles_split_comic_spreads() {
    for id in ["kindle", "ireader", "kobo", "hanvon", "bigme"] {
        assert!(get(id).unwrap().comic.split_spreads, "{id}");
    }
}

/// 2026-11-10 维护者定：五台都按 Bangumi 元数据把日本漫画写成 rtl（掌阅也开，阅读器里要每本选一次「漫画」排版）。
#[test]
fn builtin_profiles_take_comic_direction_from_metadata() {
    for id in ["kindle", "ireader", "kobo", "hanvon", "bigme"] {
        let p = get(id).unwrap();
        assert!(
            p.comic.meta_direction,
            "{id}：应按 Bangumi 元数据定漫画方向"
        );
    }
}

#[test]
fn device_from_args_finds_id_or_lists_choices() {
    assert_eq!(
        device_from_args(&["a.epub", "--device=ireader"])
            .unwrap()
            .id,
        "ireader"
    );
    let err = device_from_args(&["--device=nope"]).unwrap_err();
    assert!(err.contains("kindle") && err.contains("--device="), "{err}");
    assert!(device_from_args::<&str>(&[]).is_err());
}

#[test]
fn rejects_landscape_and_unknown_keys() {
    let base =
        "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n";
    assert!(Profile::parse(
        "x",
        &format!("{base}[screen]\nwidth = 1680\nheight = 1264\n")
    )
    .is_err());
    assert!(Profile::parse(
        "x",
        &format!("{base}extra = 1\n[screen]\nwidth = 10\nheight = 20\n")
    )
    .is_err());
    assert!(
        Profile::parse("x", &format!("{base}[screen]\nwidth = 10\nheight = 20\n")).is_ok(),
        "comic_margin 缺省 1，要小于短边的 1/4"
    );
    for bad in [".hidden", ".tmp-x", "a,b"] {
        assert!(
            Profile::parse(bad, &format!("{base}[screen]\nwidth = 10\nheight = 20\n")).is_err(),
            "id {bad} 不行"
        );
    }
    assert!(
        Profile::parse(
            "x",
            &format!(
                "{base}comic_page_direction = \"up\"\n[screen]\nwidth = 100\nheight = 200\n"
            )
        )
        .is_err(),
        "翻页方向只能 ltr/rtl"
    );
    let scr = "[screen]\nwidth = 100\nheight = 200\n";
    assert!(Profile::parse(
        "x",
        &format!("{base}{scr}[readable.epub]\nwidth = 90\nheight = 180\n")
    )
    .is_ok());
    assert!(
        Profile::parse(
            "x",
            &format!("{base}{scr}[readable.epub]\nwidth = 101\nheight = 180\n")
        )
        .is_err(),
        "阅读范围不能超过屏幕"
    );
    // 阅读范围写反（横的）：屏幕是竖的，阅读范围也必须是竖的
    let wide = "[screen]\nwidth = 200\nheight = 200\n";
    assert!(
        Profile::parse(
            "x",
            &format!("{base}{wide}[readable.epub]\nwidth = 200\nheight = 199\n")
        )
        .is_err(),
        "readable 横的报错"
    );
    assert!(
        Profile::parse(
            "x",
            &format!("{base}{wide}[readable.epub]\nwidth = 199\nheight = 199\n")
        )
        .is_ok(),
        "方的可以"
    );
    assert!(
        Profile::parse(
            "x",
            &format!("{base}{wide}[comic_readable]\nwidth = 200\nheight = 150\n")
        )
        .is_err(),
        "comic_readable 横的报错"
    );
    assert!(Profile::parse(
        "x",
        &format!("{base}{wide}[comic_readable]\nwidth = 150\nheight = 200\n")
    )
    .is_ok());
    assert!(
        Profile::parse(
            "x",
            &format!("{base}{scr}[readable.pdf]\nwidth = 90\nheight = 180\n")
        )
        .is_err(),
        "不再支持的格式（pdf）报错"
    );
    assert!(Profile::parse("x", &format!("name = \"x\"\nppi = 300\ncolor = false\nformats = [\"kfx\"]\nnotes = \"jump\"\n{scr}")).is_ok(), "kfx 可以");
    for fmt in ["pdf", "mobi", "azw3"] {
        let old = format!("name = \"x\"\nppi = 300\ncolor = false\nformats = [\"{fmt}\"]\nnotes = \"jump\"\n{scr}");
        assert!(
            Profile::parse("x", &old).is_err(),
            "formats 里写 {fmt} 报错"
        );
    }
    let dup = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\", \"epub\"]\nnotes = \"jump\"\n";
    assert!(
        Profile::parse("x", &format!("{dup}{scr}")).is_err(),
        "formats 不能重复"
    );
}

#[test]
fn comic_margin_defaults_to_one_and_is_validated() {
    let base =
        "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n";
    let scr = "[screen]\nwidth = 100\nheight = 200\n";
    assert_eq!(
        Profile::parse("x", &format!("{base}{scr}"))
            .unwrap()
            .comic
            .margin,
        DEFAULT_COMIC_MARGIN
    );
    assert_eq!(
        Profile::parse("x", &format!("{base}comic_margin = 0\n{scr}"))
            .unwrap()
            .comic
            .margin,
        0
    );
    assert_eq!(
        Profile::parse("x", &format!("{base}comic_margin = 24\n{scr}"))
            .unwrap()
            .comic
            .margin,
        24
    );
    assert!(
        Profile::parse("x", &format!("{base}comic_margin = 25\n{scr}")).is_err(),
        "短边 100 的 1/4 = 25，不能等于"
    );
    // 按阅读范围算，不按屏幕：阅读范围 80 宽时上限是 20
    let rd = "[readable.epub]\nwidth = 80\nheight = 180\n";
    assert!(Profile::parse("x", &format!("{base}comic_margin = 19\n{scr}{rd}")).is_ok());
    assert!(Profile::parse("x", &format!("{base}comic_margin = 20\n{scr}{rd}")).is_err());
    assert!(
        Profile::parse("x", &format!("{base}comic_margin = -1\n{scr}")).is_err(),
        "负数报错"
    );
}

#[test]
fn dir_overrides_and_adds() {
    let dir = std::env::temp_dir().join(format!("profile-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let body = |w: u32| {
        format!("name = \"t\"\nppi = 212\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n[screen]\nwidth = {w}\nheight = 1448\n")
    };
    std::fs::write(dir.join("ireader.toml"), body(1072)).unwrap();
    std::fs::write(dir.join("new-dev.toml"), body(1000)).unwrap();
    std::fs::write(dir.join("README.md"), "ignored").unwrap();
    std::fs::write(dir.join(".#ireader.toml"), "编辑器锁文件").unwrap();
    let r = Registry::with_dir(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(r.get("ireader").unwrap().screen.width, 1072);
    assert_eq!(r.get("new-dev").unwrap().screen.width, 1000);
    assert_eq!(
        r.iter().count(),
        Registry::builtin().iter().count() + 1,
        "覆盖同 id、新增一台"
    );
}

/// 临时目录：每个用例一个（`tag` 区分，测试并行时不互相踩）。
fn temp_profile_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("profile-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 书库 `profiles/<id>.toml` 只写 `disabled = true` 一行：同名内置模式从注册表里去掉，别的内置模式都在。
#[test]
fn dir_disables_builtin_mode() {
    let dir = temp_profile_dir("disable");
    std::fs::write(dir.join("ireader.toml"), "disabled = true\n").unwrap();
    let r = Registry::with_dir(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(r.get("ireader").is_none(), "停用的模式不再列出");
    assert_eq!(
        r.iter().count(),
        Registry::builtin().iter().count() - 1,
        "只少停用的那个"
    );
    for id in ["bigme", "kindle", "kobo", "hanvon"] {
        assert!(r.get(id).is_some(), "{id} 还在");
    }
}

/// 有 `disabled` 键还写了别的键：报错，信息里有文件名和改法。
#[test]
fn dir_disable_with_other_keys_is_an_error() {
    let dir = temp_profile_dir("disable-extra");
    std::fs::write(dir.join("ireader.toml"), "disabled = true\nname = \"x\"\n").unwrap();
    let e = Registry::with_dir(&dir).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(e.contains("ireader.toml"), "要有文件名：{e}");
    assert!(e.contains("disabled"), "{e}");
    assert!(e.contains("只写"), "要写清楚怎么改：{e}");
}

/// `disabled = false` 也不合规（要恢复就删掉这个键）。
#[test]
fn dir_disable_false_is_an_error() {
    let dir = temp_profile_dir("disable-false");
    std::fs::write(dir.join("ireader.toml"), "disabled = false\n").unwrap();
    let e = Registry::with_dir(&dir).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(e.contains("ireader.toml"), "要有文件名：{e}");
    assert!(e.contains("disabled"), "{e}");
}

/// 停用一个本来就没有的模式：忽略，不报错，别的模式不受影响。
#[test]
fn dir_disable_unknown_id_is_ignored() {
    let dir = temp_profile_dir("disable-unknown");
    std::fs::write(dir.join("nosuch.toml"), "disabled = true\n").unwrap();
    let r = Registry::with_dir(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(r.get("nosuch").is_none());
    assert_eq!(
        r.iter().count(),
        Registry::builtin().iter().count(),
        "总量不变"
    );
}

#[test]
fn deliver_usbms_parses() {
    let base = "name = \"x\"\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n[screen]\nwidth = 1072\nheight = 1448\n[deliver]\n";
    let p = Profile::parse(
        "x",
        &format!("{base}kind = \"usbms\"\nlabel = \"KOBOeReader\"\ndir = \"books\"\n"),
    )
    .unwrap();
    assert_eq!(
        p.deliver,
        Some(Deliver::Usbms {
            label: "KOBOeReader".into(),
            link: String::new(),
            dir: "books".into()
        })
    );
    assert_eq!(
        p.deliver.unwrap().link_name(),
        "KOBOeReader",
        "没写 link 时取卷标首字母大写（内置 Kobo 显式写 Kobo）"
    );
    let e = Profile::parse(
        "x",
        &format!("{base}kind = \"usbms\"\nlabel = \"KOBOeReader\"\ndir = \".kobo\"\n"),
    )
    .unwrap_err();
    assert!(e.contains("label、link、dir"), "{e}");
    assert!(Profile::parse(
        "x",
        &format!("{base}kind = \"usbms\"\nlabel = \"a/b\"\ndir = \"books\"\n")
    )
    .is_err());
    assert!(
        Profile::parse(
            "x",
            &format!("{base}kind = \"usbms\"\nmount = \"k\"\ndir = \"books\"\n")
        )
        .is_err(),
        "usbms 不认 mount"
    );
    assert!(
        Profile::parse(
            "x",
            &format!("{base}kind = \"usbms\"\nlabel = \"K\"\ndir = \"b\"\nx = 1\n")
        )
        .is_err(),
        "不认识的键报错"
    );
    let k = get("kobo").unwrap();
    assert_eq!(
        k.deliver,
        Some(Deliver::Usbms {
            label: "KOBOeReader".into(),
            link: "Kobo".into(),
            dir: "books".into()
        })
    );
    assert_eq!(
        (k.screen.width, k.screen.height, k.color, k.format()),
        (1072, 1448, false, Format::Kepub)
    );
    assert_eq!(
        (k.notes.mode, k.repair_note_links),
        (Notes::Popup, true),
        "Kobo 全部文字书注释弹窗（2026-11-11 实机）"
    );
    assert!(k.vertical_text && k.vertical_ascii == VerticalAscii::Fullwidth);
    let h = get("hanvon").unwrap();
    assert_eq!(
        h.deliver,
        Some(Deliver::Mtp {
            mount: "hanvon".into(),
            link: "Hanvon".into(),
            dir: "Documents".into()
        })
    );
    assert_eq!(
        (h.screen.width, h.screen.height, h.color, h.format()),
        (1072, 1448, false, Format::Epub)
    );
    assert!(
        !h.comic.fixed_layout,
        "漫画保持流式整页：2026-11-06 真机比过与固定版式逐像素相同（agy 二审线索 1）"
    );
}

/// `cover_meta_thumb` 要和 `cover_thumb_aspect` 一起写；aspect 超出 0.3–1.5 报错（agy 二审 6）。
#[test]
fn cover_meta_thumb_needs_aspect() {
    let src = include_str!("../profiles/ireader.toml");
    assert!(
        src.contains("cover_thumb_aspect = 0.754") && src.contains("cover_meta_thumb = true")
    );
    let p = Profile::parse("ireader", src).unwrap();
    assert!(p.cover_meta_thumb && p.cover_thumb_aspect == Some(0.754));
    let e = Profile::parse("x", &src.replace("cover_thumb_aspect = 0.754", "")).unwrap_err();
    assert!(
        e.contains("cover_meta_thumb 要和 cover_thumb_aspect 一起写"),
        "{e}"
    );
    let e = Profile::parse(
        "x",
        &src.replace("cover_thumb_aspect = 0.754", "cover_thumb_aspect = 2.0"),
    )
    .unwrap_err();
    assert!(e.contains("0.3–1.5"), "{e}");
    let p = Profile::parse("x", &src.replace("cover_meta_thumb = true", "")).unwrap();
    assert!(!p.cover_meta_thumb, "不写是 false");
}

/// Kobo 的书架缩略图：补白到屏幕比例 0.7403、`cover-image` 挪到缩略图上（2026-11-11）。
/// `cover_image_thumb`、`cover_thumb_fit = "pad"` 都要和 `cover_thumb_aspect` 一起写；不写时是裁、false。
#[test]
fn kobo_pads_cover_thumbnail_and_moves_cover_image() {
    let k = get("kobo").unwrap();
    assert_eq!(k.cover_thumb_aspect, Some(0.7403));
    assert_eq!(k.cover_thumb_fit, CoverThumbFit::Pad);
    assert!(k.cover_image_thumb && !k.cover_meta_thumb);
    // 别的设备的缺省值不变
    for id in ["bigme", "hanvon", "ireader", "kindle"] {
        let p = get(id).unwrap();
        assert_eq!(p.cover_thumb_fit, CoverThumbFit::Crop, "{id}");
        assert!(!p.cover_image_thumb, "{id}");
    }
    let src = include_str!("../profiles/kobo.toml");
    let e = Profile::parse("x", &src.replace("cover_thumb_aspect = 0.7403", "")).unwrap_err();
    assert!(
        e.contains("cover_image_thumb 要和 cover_thumb_aspect 一起写"),
        "{e}"
    );
    let no_aspect_no_cover_image = src
        .replace("cover_thumb_aspect = 0.7403", "")
        .replace("cover_image_thumb = true", "");
    let e = Profile::parse("x", &no_aspect_no_cover_image).unwrap_err();
    assert!(
        e.contains("cover_thumb_fit = \"pad\" 要和 cover_thumb_aspect 一起写"),
        "{e}"
    );
    let p = Profile::parse("x", &src.replace("cover_thumb_fit = \"pad\"", "")).unwrap();
    assert_eq!(p.cover_thumb_fit, CoverThumbFit::Crop, "不写是裁");
    let p = Profile::parse("x", &src.replace("cover_image_thumb = true", "")).unwrap();
    assert!(!p.cover_image_thumb, "不写是 false");
}

/// 竖排两个字段：缺省不竖排、ASCII 原样；`vertical_ascii != keep` 要开 `vertical_text`；`tcy` 未实现。
#[test]
fn vertical_fields_and_validation() {
    let base = "name = \"x\"\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n";
    let screen = "[screen]\nwidth = 1000\nheight = 1500\n";
    let on = format!("{base}vertical_text = true\nvertical_ascii = \"fullwidth\"\n{screen}");
    let p = Profile::parse("x", &on).unwrap();
    assert!(p.vertical_text);
    assert_eq!(p.vertical_ascii, VerticalAscii::Fullwidth);
    let p = Profile::parse("x", &format!("{base}{screen}")).unwrap();
    assert!(!p.vertical_text, "不写是 false");
    assert_eq!(p.vertical_ascii, VerticalAscii::Keep, "不写是 keep");
    let e = Profile::parse(
        "x",
        &format!("{base}vertical_ascii = \"fullwidth\"\n{screen}"),
    )
    .unwrap_err();
    assert!(
        e.contains("vertical_ascii 不是 keep 时要开 vertical_text"),
        "{e}"
    );
    let e = Profile::parse(
        "x",
        &format!("{base}vertical_text = true\nvertical_ascii = \"tcy\"\n{screen}"),
    )
    .unwrap_err();
    assert!(e.contains("未实现"), "{e}");
}

//! 原 `metadata.rs` 的单元测试（拆分后测试路径不变：`metadata::tests::…`）。
use super::*;
use std::cell::Cell;
use std::io::Write;

/// 不联网的假网站：搜到一条，给出元数据；记下被搜了几次。`transient` 有值时只报网络错、不给结果。
struct Fake {
    searched: Cell<usize>,
    found: &'static str,
    transient: Option<&'static str>,
}

impl BookSource for Fake {
    type Hit = ();
    fn queries(&self, _: &str, _: &[String]) -> Vec<String> {
        Vec::new()
    }
    fn search_url(&self, _: &str) -> String {
        String::new()
    }
    fn matching_hits(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &[String],
    ) -> Result<Vec<()>, String> {
        Ok(Vec::new())
    }
    fn hit_author<'h>(&self, _: &'h ()) -> &'h str {
        ""
    }
    fn search(&self, net: &Net, _: &str, _: &[String]) -> Vec<()> {
        self.searched.set(self.searched.get() + 1);
        match self.transient {
            Some(e) => {
                net.note_transient(e);
                Vec::new()
            }
            None => vec![()],
        }
    }
    fn cover(&self, _: &Net, _: &[()]) -> Option<(usize, Vec<u8>, &'static str)> {
        None
    }
    fn cover_origin(&self, _: &()) -> (String, String) {
        (String::new(), String::new())
    }
    fn info(
        &self,
        _: &Net,
        _: &[()],
        _: usize,
        _: &[String],
        _: &[String],
    ) -> Option<BookInfo> {
        (!self.found.is_empty()).then(|| BookInfo {
            source: self.found.into(),
            ..Default::default()
        })
    }
}

/// 测试用的漫画 CBZ（3 页 JPEG，`is_comic` 一律算）。
fn comic_cbz(dir: &Path, name: &str) -> std::path::PathBuf {
    let p = dir.join(format!("{name}.cbz"));
    let mut z = bookconv::zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
    for i in 0..3 {
        z.start_file(
            format!("{i}.jpg"),
            bookconv::zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        // 每本颜色、宽度不同：内容哈希不能一样，否则 `add_file` 会当成同一本书
        let shade = name.bytes().fold(90u8, |a, b| a.wrapping_add(b));
        let width = 4 + u32::from(shade) % 4;
        let mut jpg = Vec::new();
        image::RgbImage::from_pixel(width, 6, image::Rgb([90, shade, 90]))
            .write_to(
                &mut std::io::Cursor::new(&mut jpg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        z.write_all(&jpg).unwrap();
    }
    z.finish().unwrap();
    p
}

fn save_test_meta(lib: &Library, m: &Meta) {
    let path = lib.root().join("masters").join(&m.id).join("meta.json");
    std::fs::write(path, serde_json::to_vec(m).unwrap()).unwrap();
}

fn load_test_meta(lib: &Library, id: &str) -> Meta {
    let path = lib.root().join("masters").join(id).join("meta.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// 2026-11-10 审查阻断 1：旧 Bangumi 记录重找得到 NotFound 时记成 Some("")，不再每次 sync 重找。
#[test]
fn bangumi_not_found_marks_region_checked() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(d.path()).unwrap();
    let mut m = lib
        .add_file(&comic_cbz(d.path(), "漫"))
        .unwrap()
        .meta()
        .clone();
    m.info = Some(BookInfo {
        source: "Bangumi https://bgm.tv/subject/1".into(),
        region: None,
        ..Default::default()
    });
    m.info_checked = LOOKUP_VERSION.to_string();
    save_test_meta(&lib, &m);
    assert!(lib.needs_metadata(&m), "旧记录没记地区：要重找");

    let fake = Fake {
        searched: Cell::new(0),
        found: "",
        transient: None,
    };
    let (r, _) = lib.fetch_metadata_with_chain(&m, true, &[&fake]).unwrap();
    assert!(matches!(r, InfoResult::NotFound(_)), "假网站什么都没找到");
    assert_eq!(fake.searched.get(), 1, "重找了一次");
    let saved = load_test_meta(&lib, &m.id);
    assert_eq!(
        saved.info.as_ref().and_then(|i| i.region.as_deref()),
        Some(""),
        "没拿到新结果：记成查过、判不出"
    );
    assert!(
        !lib.needs_metadata(&saved),
        "记下地区后不再触发重找（下一轮 sync 不会再联网）"
    );
    assert_eq!(fake.searched.get(), 1, "判断本身不联网");
}

/// 2026-11-10 审查建议：重找时网络出错（临时错误）不把地区记成判不出，下次还要重找。
#[test]
fn bangumi_network_error_keeps_region_for_retry() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(d.path()).unwrap();
    let mut m = lib
        .add_file(&comic_cbz(d.path(), "漫"))
        .unwrap()
        .meta()
        .clone();
    m.info = Some(BookInfo {
        source: "Bangumi https://bgm.tv/subject/1".into(),
        region: None,
        ..Default::default()
    });
    m.info_checked = LOOKUP_VERSION.to_string();
    save_test_meta(&lib, &m);
    let fake = Fake {
        searched: Cell::new(0),
        found: "",
        transient: Some("模拟网络出错"),
    };
    assert!(
        lib.fetch_metadata_with_chain(&m, true, &[&fake]).is_err(),
        "没封面时网络出错整本报错"
    );
    assert_eq!(fake.searched.get(), 1);
    let saved = load_test_meta(&lib, &m.id);
    assert_eq!(
        saved.info.as_ref().and_then(|i| i.region.as_deref()),
        None,
        "网络出错不记地区"
    );
    assert!(lib.needs_metadata(&saved), "下次 sync 还要重找");
}

/// 2026-11-10 审查阻断 3：同一套漫画优先抄有地区的 info；抄来的旧 info 没地区就原样抄，
/// 不记成判不出（下一轮自己重找，拿到地区后才会停）。
#[test]
fn series_lookup_prefers_info_with_region() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(d.path()).unwrap();
    let mut m1 = lib
        .add_file(&comic_cbz(d.path(), "漫 - 01卷"))
        .unwrap()
        .meta()
        .clone();
    let mut m2 = lib
        .add_file(&comic_cbz(d.path(), "漫 - 02卷"))
        .unwrap()
        .meta()
        .clone();
    m1.info = Some(BookInfo {
        source: "Bangumi https://bgm.tv/subject/1".into(),
        region: Some("日本".into()),
        ..Default::default()
    });
    m1.info_checked = "3".into(); // 比 m2 旧，但记了地区，优先
    m2.info = Some(BookInfo {
        source: "Bangumi https://bgm.tv/subject/2".into(),
        region: None,
        ..Default::default()
    });
    m2.info_checked = LOOKUP_VERSION.to_string();
    save_test_meta(&lib, &m1);
    save_test_meta(&lib, &m2);

    let Some(InfoResult::Shared(info, from)) = lib.series_lookup(&m2).unwrap() else {
        panic!("应抄同一套的另一卷")
    };
    assert_eq!(info.region.as_deref(), Some("日本"), "{from}");
    assert!(from.contains("01卷"), "{from}");
    assert_eq!(
        load_test_meta(&lib, &m2.id).info.and_then(|i| i.region),
        Some("日本".into()),
        "抄过来的 info 要存下"
    );

    // 另一卷也没有地区：抄过来 region 仍是 None，这一卷下一次 sync 会自己 force 重找
    m1.info.as_mut().unwrap().region = None;
    save_test_meta(&lib, &m1);
    m2.info = None;
    m2.info_checked.clear();
    save_test_meta(&lib, &m2);
    let Some(InfoResult::Shared(info, _)) = lib.series_lookup(&m2).unwrap() else {
        panic!("应抄同一套的另一卷")
    };
    assert_eq!(
        info.region, None,
        "抄来的旧 info 没地区：原样抄，不记成判不出"
    );
    let saved = load_test_meta(&lib, &m2.id);
    assert_eq!(saved.info.as_ref().and_then(|i| i.region.as_deref()), None);
    assert!(
        lib.needs_metadata(&saved),
        "这一卷下一次 sync 要自己重找，拿到有地区的 info 才停"
    );

    // 01 卷改出地区后，02 卷再抄一次就拿到「日本」
    m1.info.as_mut().unwrap().region = Some("日本".into());
    save_test_meta(&lib, &m1);
    let Some(InfoResult::Shared(info, _)) = lib.series_lookup(&saved).unwrap() else {
        panic!("应抄同一套的另一卷")
    };
    assert_eq!(info.region.as_deref(), Some("日本"));
    assert_eq!(
        load_test_meta(&lib, &m2.id).info.and_then(|i| i.region),
        Some("日本".into())
    );
    assert!(!lib.needs_metadata(&load_test_meta(&lib, &m2.id)));
}

/// 2026-11-10 审查阻断 4：同一套漫画里「判出地区」的卷优先于「查过、判不出」（`Some("")`）的卷。
#[test]
fn series_lookup_prefers_known_region_over_blank() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(d.path()).unwrap();
    let mut a = lib
        .add_file(&comic_cbz(d.path(), "漫 - 01卷"))
        .unwrap()
        .meta()
        .clone();
    let mut b = lib
        .add_file(&comic_cbz(d.path(), "漫 - 02卷"))
        .unwrap()
        .meta()
        .clone();
    let mut c = lib
        .add_file(&comic_cbz(d.path(), "漫 - 03卷"))
        .unwrap()
        .meta()
        .clone();
    let bangumi = |region: &str| BookInfo {
        source: "Bangumi https://bgm.tv/subject/1".into(),
        region: Some(region.into()),
        ..Default::default()
    };
    a.info = Some(bangumi(""));
    a.info_checked = LOOKUP_VERSION.to_string();
    b.info = Some(bangumi("日本"));
    b.info_checked = LOOKUP_VERSION.to_string();
    c.info = None;
    c.info_checked.clear();
    save_test_meta(&lib, &a);
    save_test_meta(&lib, &b);
    save_test_meta(&lib, &c);
    let Some(InfoResult::Shared(info, from)) = lib.series_lookup(&c).unwrap() else {
        panic!("应抄同一套的另一卷")
    };
    assert_eq!(info.region.as_deref(), Some("日本"), "{from}");
    assert!(from.contains("02卷"), "{from}");

    // 反过来（01 判出日本、02 判不出）：仍然抄「日本」，和 list() 里谁在前无关
    a.info = Some(bangumi("日本"));
    b.info = Some(bangumi(""));
    save_test_meta(&lib, &a);
    save_test_meta(&lib, &b);
    c.info = None;
    c.info_checked.clear();
    save_test_meta(&lib, &c);
    let Some(InfoResult::Shared(info, from)) = lib.series_lookup(&c).unwrap() else {
        panic!("应抄同一套的另一卷")
    };
    assert_eq!(info.region.as_deref(), Some("日本"), "{from}");
    assert!(from.contains("01卷"), "{from}");
}

/// 责任链：前一个网站找到了就不再问后面的；前面出过临时错误时后面的网站不查（结果存下了就不会再去前面找）。
#[test]
fn chain_stops_when_found_and_skips_sites_after_transient_errors() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(d.path()).unwrap();
    let net = Net::new();
    let meta = Meta {
        title: "书".into(),
        ..Default::default()
    };
    let lookup = || Lookup {
        lib: &lib,
        net: &net,
        meta: &meta,
        title: "书",
        need_info: true,
        need_cover: false,
        comic: false,
        info: None,
        cover: None,
        work: None,
        work_err: None,
    };
    let (a, b) = (
        Fake {
            searched: Cell::new(0),
            found: "",
            transient: None,
        },
        Fake {
            searched: Cell::new(0),
            found: "乙",
            transient: None,
        },
    );
    let c = Fake {
        searched: Cell::new(0),
        found: "丙",
        transient: None,
    };
    let mut l = lookup();
    for s in [&a as &dyn Step, &b, &c] {
        if l.wants_info() || l.wants_cover() {
            s.run(&mut l).unwrap();
        }
    }
    assert_eq!(l.info.map(|i| i.source).as_deref(), Some("乙"));
    assert_eq!(
        (a.searched.get(), b.searched.get(), c.searched.get()),
        (1, 1, 0),
        "找到了就不再问后面的"
    );

    net.note_transient("豆瓣被拦了");
    let mut l = lookup();
    b.run(&mut l).unwrap();
    assert!(l.info.is_none());
    assert_eq!(b.searched.get(), 1, "出过临时错误：不查");
}

/// 2026-11-10：Bangumi 来源但没记地区的旧记录要再找一次；记了地区的按原来的规则。
#[test]
fn needs_metadata_refetches_bangumi_without_region() {
    let d = tempfile::tempdir().unwrap();
    let lib = Library::open(d.path()).unwrap();
    let meta = |source: &str, region: Option<&str>, title: &str, checked: &str| Meta {
        info_checked: checked.into(),
        info: Some(BookInfo {
            source: source.into(),
            region: region.map(str::to_string),
            title: title.into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    // 旧记录（匹配规则版本 4）没记地区：再找一次
    assert!(
        lib.needs_metadata(&meta("Bangumi https://bgm.tv/subject/1", None, "", "4")),
        "旧 Bangumi 记录没记地区：再找一次"
    );
    // Bangumi 记录没有地区：不管 info_checked 是哪一版都要再找
    assert!(
        lib.needs_metadata(&meta(
            "Bangumi https://bgm.tv/subject/1",
            None,
            "",
            LOOKUP_VERSION
        )),
        "这一版找过但没记地区：也要再找"
    );
    // 地区有值（含空串）就按原来的规则：这一版找过的不再找
    assert!(
        !lib.needs_metadata(&meta(
            "Bangumi https://bgm.tv/subject/1",
            Some(""),
            "",
            LOOKUP_VERSION
        )),
        "查过、判不出地区：不再找"
    );
    assert!(
        !lib.needs_metadata(&meta(
            "Bangumi https://bgm.tv/subject/1",
            Some("日本"),
            "漫画",
            LOOKUP_VERSION
        )),
        "有地区：按原来的规则"
    );
    assert!(
        lib.needs_metadata(&meta(
            "豆瓣 https://book.douban.com/subject/1",
            None,
            "",
            "3"
        )),
        "早先没记网站书名的照旧再找"
    );
    assert!(
        !lib.needs_metadata(&meta(
            "豆瓣 https://book.douban.com/subject/1",
            None,
            "书名",
            LOOKUP_VERSION
        )),
        "有书名的照旧不再找"
    );
    assert!(
        !lib.needs_metadata(&meta(
            "Bangumi https://bgm.tv/subject/1",
            Some("日本"),
            "漫画",
            "4"
        )),
        "已经找到过、有地区、有书名的，版本变了也不再找"
    );
}

/// 2026-11-10：只有「设备开了、是漫画、Bangumi 来源、地区日本」才强制 rtl；别的照原书 / profile。
/// `comic` 用调用方算好的结果（早期条目 `Meta::comic` 还是 `None` 也算）。
#[test]
fn comic_direction_only_for_bangumi_japanese_comics() {
    let dev = profile::get("ireader").unwrap();
    let meta = |source: &str, region: Option<&str>| Meta {
        info: Some(BookInfo {
            source: source.into(),
            region: region.map(str::to_string),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        comic_direction(
            &meta("Bangumi https://bgm.tv/subject/1", Some("日本")),
            dev,
            true
        ),
        Some(PageDirection::Rtl),
        "Bangumi 判成日本漫画（Meta::comic 还没存也算）"
    );
    for (m, comic) in [
        (meta("Bangumi https://bgm.tv/subject/1", Some("中国")), true),
        (meta("Bangumi https://bgm.tv/subject/1", Some("")), true),
        (meta("Bangumi https://bgm.tv/subject/1", None), true),
        (
            meta("豆瓣 https://book.douban.com/subject/1", Some("日本")),
            true,
        ),
        (
            meta("Bangumi https://bgm.tv/subject/1", Some("日本")),
            false,
        ),
    ] {
        assert_eq!(comic_direction(&m, dev, comic), None);
    }
    let mut off = dev.clone();
    off.comic.meta_direction = false;
    assert_eq!(
        comic_direction(
            &meta("Bangumi https://bgm.tv/subject/1", Some("日本")),
            &off,
            true
        ),
        None,
        "设备没开 comic_meta_direction"
    );
}

fn sample(dir: &Path, description: &str) -> std::path::PathBuf {
    use bookconv::epub::{assemble, Book, BookMeta, Chapter};
    let mut book = Book {
        meta: BookMeta {
            book_id: "t".into(),
            title: "书".into(),
            author: "作者".into(),
            language: "zh".into(),
            publisher: "".into(),
            cover: None,
            cover_ext: "jpg".into(),
            cover_media_type: "image/jpeg".into(),
            subjects: Vec::new(),
        },
        chapters: vec![Chapter {
            title: "一".into(),
            html_body: "<h1>一</h1><p>正文</p>".into(),
            level: 1,
        }],
        resources: vec![],
        nav: vec![],
    };
    let mut bytes = assemble(&mut book).unwrap();
    if !description.is_empty() {
        let mut es = bookconv::epubzip::read_entries(&bytes).unwrap();
        let opf = es.iter_mut().find(|e| e.name.ends_with(".opf")).unwrap();
        let t = String::from_utf8(opf.data.clone()).unwrap().replace(
            "</metadata>",
            &format!("<dc:description>{description}</dc:description></metadata>"),
        );
        opf.data = t.into_bytes();
        let mut zw = bookconv::zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for e in &es {
            zw.start_file(
                e.name.as_str(),
                bookconv::zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zw.write_all(&e.data).unwrap();
        }
        bytes = zw.finish().unwrap().into_inner();
    }
    let p = dir.join(format!("a{}.epub", description.len()));
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn inject_adds_cover_and_missing_metadata_only() {
    let d = tempfile::tempdir().unwrap();
    let src = sample(d.path(), "");
    let dst = d.path().join("b.epub");
    assert!(!epub_has_cover(&src));
    let mut jpg = Vec::new();
    image::RgbImage::new(400, 600)
        .write_to(
            &mut std::io::Cursor::new(&mut jpg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    assert_eq!(crate::cover::plausible_cover(&jpg), Some("jpg"));
    let info = BookInfo {
        source: "t".into(),
        description: "简介 & <引号>\n第二段".into(),
        subjects: vec!["推理".into(), "日本".into()],
        ..Default::default()
    };
    let meta = Meta {
        authors: vec!["作者".into()],
        info: Some(info),
        ..Default::default()
    };
    let own = read_own(&src).unwrap();
    assert!(inject(&src, &dst, Some(jpg), &plan(&meta, Some(&own))).unwrap());
    assert!(epub_has_cover(&dst));
    let (a, b) = (
        bookconv::epubzip::read_entries(&std::fs::read(&src).unwrap()).unwrap(),
        bookconv::epubzip::read_entries(&std::fs::read(&dst).unwrap()).unwrap(),
    );
    // 只新加了封面图、改了 OPF；不做规范整理（优化器的清洗层会做），别的条目逐字节原样
    assert_eq!(b.len(), a.len() + 1);
    for e in a.iter().filter(|e| !e.name.ends_with(".opf")) {
        assert_eq!(
            b.iter().find(|x| x.name == e.name).unwrap().data,
            e.data,
            "{} 原样",
            e.name
        );
    }
    let opf = String::from_utf8(
        b.iter()
            .find(|e| e.name.ends_with(".opf"))
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    let dc = bookconv::wash::opf_dc(&opf);
    assert_eq!(dc.description, "简介 & <引号> 第二段", "{opf}");
    assert_eq!(dc.title, "书");
    assert_eq!(opf.matches("<dc:subject>").count(), 2);

    // 书里已有简介：不覆盖；什么都不用补时不写
    let src2 = sample(d.path(), "原书简介");
    let dst2 = d.path().join("c.epub");
    let meta2 = Meta {
        authors: vec!["作者".into()],
        info: Some(BookInfo {
            source: "t".into(),
            description: "新简介".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let own2 = read_own(&src2).unwrap();
    assert!(!inject(&src2, &dst2, None, &plan(&meta2, Some(&own2))).unwrap());
    assert!(!dst2.exists());
}

fn own(creators: &[&str]) -> crate::OwnDc {
    crate::OwnDc {
        v: crate::OwnDc::VERSION,
        creators: creators.iter().map(|c| c.to_string()).collect(),
        ..Default::default()
    }
}

fn edition(translators: &[&str]) -> Edition {
    Edition {
        title: "飘".into(),
        translators: translators.iter().map(|t| t.to_string()).collect(),
        publisher: "江苏凤凰文艺出版社".into(),
        pubdate: "2023-6-1".into(),
        isbn: "978-7-5594-7000-1".into(),
        ..Default::default()
    }
}

/// 《飘》：署名整串写在一个 dc:creator 里（「[美] 玛格丽特·米切尔 著，梅静 译」），豆瓣挑中的梅静译本：作者拆出来带国籍、
/// 译者单列、出版社、日期、ISBN 补上（2026-10-12 要）。
#[test]
fn plan_splits_credits_and_fills_edition_when_translator_agrees() {
    use bookconv::opfmeta::DcField;
    let info = BookInfo {
        source: "豆瓣".into(),
        authors: vec!["[美] 玛格丽特·米切尔".into()],
        edition: Some(edition(&["梅静"])),
        ..Default::default()
    };
    let meta = Meta {
        title: "飘（上册）".into(),
        info: Some(info),
        ..Default::default()
    };
    let p = plan(&meta, Some(&own(&["[美] 玛格丽特·米切尔 著，梅静 译"])));
    assert_eq!(
        p,
        vec![
            (DcField::Creator, vec!["[美] 玛格丽特·米切尔".to_string()]),
            (DcField::Translator, vec!["梅静".to_string()]),
            (DcField::Publisher, vec!["江苏凤凰文艺出版社".to_string()]),
            (DcField::Date, vec!["2023-06-01".to_string()]),
            (
                DcField::Identifier,
                vec!["urn:isbn:9787559470001".to_string()]
            ),
        ]
    );
    assert!(plan_sig(&p).is_some());
    // 靠去掉卷次才对上的条目（漫画「第0卷」）：版本信息不补，作品层面的照补
    let meta = Meta {
        title: "飘 第2卷".into(),
        ..meta
    };
    assert!(
        !plan(&meta, Some(&own(&["[美] 玛格丽特·米切尔 著，梅静 译"])))
            .iter()
            .any(|(f, _)| matches!(f, DcField::Publisher | DcField::Identifier))
    );
}

/// 书名：网站书名去掉说明、加上书里的卷次（2026-10-12 选的）；没找到的照书里的。
#[test]
fn title_uses_web_name_plus_book_volume() {
    let t = |book: &str, web: &str| {
        corrected_title(&Meta {
            title: book.into(),
            info: Some(BookInfo {
                source: "豆瓣".into(),
                title: web.into(),
                ..Default::default()
            }),
            ..Default::default()
        })
    };
    assert_eq!(t("飘 - 上册", "飘"), "飘（上册）");
    assert_eq!(t("飘（下册）", "飘"), "飘（下册）");
    assert_eq!(t("哆啦A夢全彩版 - 卷01", "哆啦A梦"), "哆啦A梦 第01卷");
    assert_eq!(t("波洛系列04：罗杰疑案", "罗杰疑案"), "罗杰疑案");
    assert_eq!(t("三体（共3册）", "三体"), "三体");
    assert_eq!(
        t(
            "古龙作品全集（大陆唯一合法授权华丽新版）（精装72册）",
            "古龙作品全集"
        ),
        "古龙作品全集",
        "套书的册数不是卷次"
    );
    assert_eq!(t("某书（第3册）", "某书"), "某书 第3卷");
    assert_eq!(t("浪客行 - 卷01", "浪客行"), "浪客行 第01卷");
    assert_eq!(
        t("哆啦A夢全彩版 - 卷02", "机器猫哆啦A梦（共45册）"),
        "机器猫哆啦A梦 第02卷",
        "网站书名的说明去掉"
    );
    assert_eq!(
        corrected_title(&Meta {
            title: "嘯風山莊".into(),
            ..Default::default()
        }),
        "嘯風山莊",
        "没找到：照书里的"
    );
    assert_eq!(
        t("杀死一只知更鸟（漫）", "杀死一只知更鸟"),
        "杀死一只知更鸟（漫）",
        "作品形式留着，不和小说撞名"
    );
    assert_eq!(
        t("伊藤潤二愛藏版 - 卷08 呻吟的排水管", "伊藤潤二愛藏版 08"),
        "伊藤潤二 第08卷 呻吟的排水管",
        "卷名留着"
    );
    assert_eq!(t("某书第3卷：风起", "某书"), "某书 第3卷 风起");
    assert_eq!(
        t("Some Book Vol.2 Dawn", "Some Book"),
        "Some Book 第2卷 Dawn"
    );
}

/// 版本的译者和书里的对不上（豆瓣第一个条目是李美华译本）：出版社、日期、ISBN 不补；书里的署名照样拆。
#[test]
fn plan_skips_edition_fields_on_translator_conflict() {
    use bookconv::opfmeta::DcField;
    let meta = Meta {
        info: Some(BookInfo {
            source: "豆瓣".into(),
            edition: Some(edition(&["李美华"])),
            ..Default::default()
        }),
        ..Default::default()
    };
    let p = plan(&meta, Some(&own(&["[美] 玛格丽特·米切尔 著，梅静 译"])));
    assert_eq!(
        p.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
        [DcField::Creator, DcField::Translator]
    );
}

/// 作者没写国籍：照豆瓣的署名补；书里已经是这个样子、又没找到元数据的什么都不补。
#[test]
fn plan_adds_nationality_from_web_and_leaves_clean_books_alone() {
    use bookconv::opfmeta::DcField;
    let meta = Meta {
        info: Some(BookInfo {
            source: "豆瓣".into(),
            authors: vec!["[日] 东野圭吾".into()],
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        plan(&meta, Some(&own(&["東野圭吾"]))),
        vec![(DcField::Creator, vec!["[日] 東野圭吾".to_string()])]
    );
    assert!(plan(&Meta::default(), Some(&own(&["金庸"]))).is_empty());
    // 书里没写译者：用版本的
    let meta = Meta {
        title: "飘".into(),
        info: Some(BookInfo {
            source: "豆瓣".into(),
            edition: Some(edition(&["刘姿君"])),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(plan(&meta, Some(&own(&["东野圭吾"])))
        .contains(&(DcField::Translator, vec!["刘姿君".to_string()])));
}

/// 写进书：译者是带 trl 角色的 dc:contributor，ISBN 加在原有标识符旁边（unique-identifier 那个留着）。
#[test]
fn inject_writes_translator_role_and_appends_isbn() {
    let d = tempfile::tempdir().unwrap();
    let src = sample(d.path(), "");
    let dst = d.path().join("t.epub");
    let info = BookInfo {
        source: "豆瓣".into(),
        authors: vec!["[美] 玛格丽特·米切尔".into()],
        edition: Some(edition(&["梅静"])),
        ..Default::default()
    };
    let meta = Meta {
        title: "飘".into(),
        info: Some(info),
        ..Default::default()
    };
    let p = plan(&meta, Some(&own(&["[美] 玛格丽特·米切尔 著，梅静 译"])));
    assert!(inject(&src, &dst, None, &p).unwrap());
    let es = bookconv::epubzip::read_entries(&std::fs::read(&dst).unwrap()).unwrap();
    let opf = String::from_utf8(
        es.iter()
            .find(|e| e.name.ends_with(".opf"))
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    assert!(
        opf.contains("<dc:creator>[美] 玛格丽特·米切尔</dc:creator>")
            || opf.contains(r#"<dc:creator opf:role="aut">[美] 玛格丽特·米切尔</dc:creator>"#),
        "{opf}"
    );
    assert!(
        opf.contains("梅静</dc:contributor>") && opf.contains(">trl<")
            || opf.contains(r#"opf:role="trl""#),
        "{opf}"
    );
    assert!(
        opf.contains("<dc:identifier>urn:isbn:9787559470001</dc:identifier>"),
        "{opf}"
    );
    let own2 = read_own(&dst).unwrap();
    assert_eq!(
        (own2.translators.clone(), own2.isbn, own2.publisher),
        (vec!["梅静".to_string()], true, true)
    );
    assert!(
        bookconv::opfmeta::read_epub(&dst)
            .unwrap()
            .iter()
            .any(|(f, v)| *f == bookconv::opfmeta::DcField::Identifier && v.len() >= 2),
        "原来的标识符还在"
    );
}

//! 指纹黄金对拍：对内置三个模式逐个改一个 profile 字段，记下文字书、漫画的指纹，和 `fp_golden.txt` 逐行比。
//! 重构指纹的算法（或 profile 的结构）时产物指纹必须一字不差，否则全部书库的书都会白白重建（或该重建的不重建）。
//! 有意改了指纹写法：`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden` 重写文件，一起提交。
mod common;

use common::{jpeg, sample_epub};
use profile::{ImageSizing, NoteIcons, Notes, Profile, Screen};
use std::io::Write;

type Mutation = (&'static str, Box<dyn Fn(&mut Profile)>);

macro_rules! flip {
    ($($f:ident),* $(,)?) => { vec![$((stringify!($f), Box::new(|p: &mut Profile| p.$f = !p.$f) as Box<dyn Fn(&mut Profile)>)),*] };
}
macro_rules! flip_css {
    ($($f:ident),* $(,)?) => { vec![$((stringify!($f), Box::new(|p: &mut Profile| p.css.$f = !p.css.$f) as Box<dyn Fn(&mut Profile)>)),*] };
}

fn mutations() -> Vec<Mutation> {
    let mut v: Vec<Mutation> = vec![("none", Box::new(|_| {}))];
    v.push(("color", Box::new(|p| p.color = !p.color)));
    v.push(("note_backlinks", Box::new(|p| p.notes.backlinks = !p.notes.backlinks)));
    v.push(("comic_fixed_layout", Box::new(|p| p.comic.fixed_layout = !p.comic.fixed_layout)));
    v.extend(flip![background_images, background_sizing]);
    v.extend(flip_css![css_rgba, background_no_repeat, margin_top]);
    v.extend(flip_css![background_auto_height, display_none, line_height_except_p, em_root, heading_center, heading_font_keywords, trailing_br_line]);
    v.extend(flip_css![inline_css_lengths, padding, borders, heading_margins, px_margins, percent_margins, block_width, negative_margins, cell_middle]);
    v.extend(flip_css![inline_valign, cell_width, margin_auto, float, horizontal_scale, rem, important, line_height_numbers]);
    v.extend(flip![caption_fit, image_alpha, text_repair_only, repair_note_links, kindle_rules]);
    v.push(("notes", Box::new(|p| p.notes.mode = if p.notes.mode == Notes::Popup { Notes::Jump } else { Notes::Popup })));
    v.push(("note_icons", Box::new(|p| p.notes.icons = if p.notes.icons == NoteIcons::Number { NoteIcons::Keep } else { NoteIcons::Number })));
    v.push(("image_sizing", Box::new(|p| p.image_sizing = if p.image_sizing == ImageSizing::Css { ImageSizing::Percent } else { ImageSizing::Css })));
    v.push(("comic_margin", Box::new(|p| p.comic.margin += 3)));
    v.push(("comic_reader_margins", Box::new(|p| p.comic.reader_margins = p.comic.reader_margins.map_or(Some(7), |_| None))));
    v.push(("comic_page_direction", Box::new(|p| p.comic.page_direction = p.comic.page_direction.as_ref().map_or(Some("ltr".into()), |_| None))));
    v.push(("line_height_floor", Box::new(|p| p.css.line_height_floor = p.css.line_height_floor.map_or(Some(1.0), |_| None))));
    v.push(("font_px", Box::new(|p| p.font_px = p.font_px.map_or(Some(40), |x| Some(x + 1)))));
    v.push(("line_px", Box::new(|p| p.line_px = p.line_px.map_or(Some(60), |x| Some(x + 1)))));
    v.push(("em_px", Box::new(|p| p.em_px = p.em_px.map_or(Some(37.0), |x| Some(x + 1.0)))));
    v.push(("screen", Box::new(|p| p.screen = Screen { width: p.screen.width + 2, height: p.screen.height + 4 })));
    v
}

#[test]
fn fingerprints_match_the_golden_file() {
    let dir = tempfile::tempdir().unwrap();
    let lib = common::open(&dir.path().join("lib"));
    let epub = dir.path().join("文.epub");
    std::fs::write(&epub, sample_epub("文")).unwrap();
    let cbz = dir.path().join("漫.cbz");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&cbz).unwrap());
        for i in 0..3 {
            z.start_file(format!("{i:02}.jpg"), zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&jpeg(400, 600)).unwrap();
        }
        z.finish().unwrap();
    }
    let text = lib.add_file(&epub).unwrap().meta().clone();
    let comic = lib.add_file(&cbz).unwrap().meta().clone();
    let mut out = String::new();
    for dev in ["kindle", "ireader", "xochitl"] {
        for (name, m) in mutations() {
            let mut p = profile::get(dev).unwrap().clone();
            m(&mut p);
            for (kind, meta) in [("text", &text), ("comic", &comic)] {
                let fp = lib.fingerprint(meta, &p).unwrap_or_else(|e| format!("ERR {e}"));
                // 内容哈希随测试书生成方式变，不进黄金文件
                let fp = fp.replacen(&meta.source_sha256, "<sha>", 1);
                out.push_str(&format!("{dev}\t{name}\t{kind}\t{fp}\n"));
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fp_golden.txt");
    if std::env::var_os("UPDATE_FP_GOLDEN").is_some() {
        std::fs::write(&path, &out).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).expect("缺 fp_golden.txt：UPDATE_FP_GOLDEN=1 生成");
    for (a, b) in want.lines().zip(out.lines()) {
        assert_eq!(a, b);
    }
    assert_eq!(want.lines().count(), out.lines().count());
}

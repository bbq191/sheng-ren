//! 损坏 PDF 不能让提取 panic（fork 改动 ⑥）：缺 MediaBox、MediaBox 个数不够、资源里的悬空引用。
//! 上游这几处是 `expect`/`unwrap`/下标越界，调用方（booklib）只能靠 catch_unwind 兜。
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};

/// 一页 PDF；`media_box=None` 时整棵页树都不写 MediaBox。页面内容 `Do /F`，`/F` 指向一个不存在的对象。
fn pdf(media_box: Option<Vec<Object>>, dangling_xobject: bool) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let ops = if dangling_xobject { vec![Operation::new("Do", vec![Object::Name(b"F".to_vec())])] } else { vec![] };
    let content_id = doc.add_object(Stream::new(dictionary! {}, Content { operations: ops }.encode().unwrap()));
    let mut page = dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id };
    if dangling_xobject {
        page.set("Resources", dictionary! { "XObject" => dictionary! { "F" => Object::Reference((999, 0)) } });
    }
    let page_id = doc.add_object(page);
    let mut pages = dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 };
    if let Some(mb) = media_box {
        pages.set("MediaBox", mb);
    }
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

#[test]
fn missing_media_box_is_an_error_not_a_panic() {
    assert!(pdf_extract::extract_text_from_mem(&pdf(None, false)).is_err());
}

#[test]
fn short_media_box_is_an_error_not_a_panic() {
    assert!(pdf_extract::extract_text_from_mem(&pdf(Some(vec![0.into(), 0.into()]), false)).is_err());
}

#[test]
fn dangling_xobject_reference_is_skipped_not_a_panic() {
    let r = pdf_extract::extract_text_from_mem(&pdf(Some(vec![0.into(), 0.into(), 100.into(), 100.into()]), true));
    assert!(r.is_ok(), "{:?}", r);
}

/// 颜色算子（fork 改动 ①）操作数缺失或不是数字：整条跳过，不 panic（fork 改动 ⑦）。
#[test]
fn color_operators_with_bad_operands_are_skipped_not_a_panic() {
    let name = || Object::Name(b"X".to_vec());
    let ops = vec![
        Operation::new("g", vec![]),
        Operation::new("G", vec![]),
        Operation::new("g", vec![name()]),
        Operation::new("rg", vec![name(), 0.into(), 0.into()]),
        Operation::new("RG", vec![Object::string_literal("x")]),
        Operation::new("k", vec![0.into(), name(), 0.into(), 0.into()]),
        Operation::new("K", vec![Object::Null]),
        Operation::new("rg", vec![0.5.into(), 0.into(), 1.into()]),
    ];
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, Content { operations: ops }.encode().unwrap()));
    let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id });
    let media_box: Vec<Object> = vec![0.into(), 0.into(), 100.into(), 100.into()];
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1, "MediaBox" => media_box }));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    let r = pdf_extract::extract_text_from_mem(&out);
    assert!(r.is_ok(), "{:?}", r);
}

//! 全书解析：并行解析各文档、深浅嵌套的栈处理、补封面页。

use super::*;

/// 书的语言（OPF 里的；空的算没写）。
pub(in crate::write) fn book_language(book: &Loaded) -> Option<String> {
    (!book.meta.language.is_empty()).then(|| book.meta.language.clone())
}

/// 先把所有文档解析成块（注释配对要看全书），再逐个生成版面。各文档独立，多线程解析（`bookconv::util::par_map`，按原顺序收回）。
/// 外部样式表按路径只解析一次（大合集几百个文档共用一份样式表），各线程共用。`faithful`：按 CSS 原样算（[`epub_text_styles`]），
/// body 的左右边距照算。
///
/// 元素套得太深（超过 [`MAX_NESTING`]）的书报错；套得较深的文档在单独的大栈线程里解析（见 [`DEEP_NESTING`]）。
pub(in crate::write) fn parse_docs(
    book: &Loaded,
    media: Option<crate::css::MediaEnv>,
    faithful: bool,
) -> Result<Vec<ParsedDoc<'_>>, String> {
    let css: HashMap<&str, &str> = book
        .css
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let book_lang = book_language(book);
    let sheets: std::sync::Mutex<HashMap<String, std::sync::Arc<crate::css::Rules>>> =
        Default::default();
    let docs: Vec<(usize, &bookconv::epubbook::Doc)> = book.docs.iter().enumerate().collect();
    let parse_blocks =
        |html: &Html, doc: &bookconv::epubbook::Doc| -> (Option<String>, Vec<Block>) {
            let sheet = Sheet::for_doc(html, &doc.path, media.as_ref(), |path| {
                let c = css.get(path)?;
                let cached = sheets
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(path)
                    .cloned();
                // 没解析过的在锁外解析（几个线程同时碰上同一份时各解析一次，结果一样，留先放进去的那份）
                Some(cached.unwrap_or_else(|| {
                    let r = crate::css::Rules::parse(c, path, media.as_ref());
                    sheets
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .entry(path.to_string())
                        .or_insert(r)
                        .clone()
                }))
            });
            let root = html.root_element();
            let lang = lang_attr(&root)
                .map(str::to_string)
                .or_else(|| book_lang.clone());
            let d = Doc {
                path: &doc.path,
                sheet,
                lang: lang.clone(),
                pending: Default::default(),
                gap: Default::default(),
                links: Default::default(),
            };
            let mut blocks = Vec::new();
            let root_comp = d.comp(&root, &Computed::root());
            if let Some(body) = root
                .children()
                .filter_map(ElementRef::wrap)
                .find(|e| e.value().name() == "body")
            {
                // body 的左右外边距、内边距不写（Send to Kindle 同样：《人生海海》《ABC谋杀案》body `margin:0 5pt`、《绍宋》新版
                // `padding:1em` 都没有出现在段落上，2026-10-08；更早的《ABC谋杀案》样本加到了每段上，那是另一版 EPUB）。
                // 书里按 body 内容宽度算的百分比折算成整页的（《雪国》body 左右 1%，段落的 1% 写 0.98%）。
                // 例外：文件里有块的左（右）外边距是负的，这一侧 body 的边距照旧加到每个块上（《罗杰疑案》《ABC谋杀案》章标题
                // `margin:-2em` 两侧都留；《平凡的世界》只有左边 `-1em` 的那几个文件只留左边）。
                let mut bc = d.comp(&body, &root_comp);
                if faithful {
                    d.block(body, bc, &mut blocks);
                    attach_trailing(&mut blocks, d.take_pending());
                    collapse_siblings(&mut blocks);
                    return (d.lang.clone(), blocks);
                }
                let fs = bc.font_size;
                let side = |i: usize| {
                    let (m, p) = (to_horiz(bc.margin[i], fs), to_horiz(bc.padding[i], fs));
                    Horiz {
                        em: m.em + p.em,
                        pct: m.pct + p.pct,
                    }
                };
                let (left, right) = (side(3), side(1));
                for i in [1, 3] {
                    bc.margin[i] = None;
                    bc.padding[i] = None;
                }
                d.block(body, bc, &mut blocks);
                let (neg_l, neg_r) = (has_negative(&blocks, true), has_negative(&blocks, false));
                let rem = |h: Horiz| h.em + h.pct / 100.0 * PAGE_WIDTH_EM;
                let width = PAGE_WIDTH_EM
                    - if neg_l { 0.0 } else { rem(left) }
                    - if neg_r { 0.0 } else { rem(right) };
                if width > 1.0 && width < PAGE_WIDTH_EM {
                    scale_percent(&mut blocks, width / PAGE_WIDTH_EM);
                }
                for b in &mut blocks {
                    if neg_l {
                        b.margin_left.em += left.em;
                        b.margin_left.pct += left.pct;
                    }
                    if neg_r {
                        b.margin_right.em += right.em;
                        b.margin_right.pct += right.pct;
                    }
                }
            }
            attach_trailing(&mut blocks, d.take_pending());
            collapse_siblings(&mut blocks);
            apply_gaps(&mut blocks);
            (d.lang.clone(), blocks)
        };
    let parse_doc = |&(si, doc): &(usize, &bookconv::epubbook::Doc)| -> Result<(usize, Option<String>, Vec<Block>), String> {
        let html = Html::parse_document(&doc.html);
        let depth = nesting_depth(&html);
        if depth > MAX_NESTING {
            return Err(format!("{}：元素套了 {depth} 层，超过上限 {MAX_NESTING} 层（多半是标签没关），不转换", doc.path));
        }
        let (lang, blocks) = if depth > DEEP_NESTING {
            // DOM 不能跨线程（`Html` 不是 `Send`），到大栈线程里重新解析一遍；只有少见的深文档多花这一遍
            drop(html);
            on_big_stack(|| parse_blocks(&Html::parse_document(&doc.html), doc))?
        } else {
            parse_blocks(&html, doc)
        };
        Ok((si, lang, blocks))
    };
    // 一批批解析，每批的结果在本线程复制一份、工作线程分配的那份随即释放：块是在工作线程里边解析 DOM 边分配的，和已经释放的 DOM
    // 交错着留在那几个线程的分配区里，本线程后面生成版面时用不上那些空洞（glibc 按线程分区），不复制的话《阿加莎全集》峰值
    // 从 0.47GB 涨到 0.63GB；复制一遍只多花约 0.1 秒，工作线程那份整块释放掉。
    // 本线程复制这一批的同时，下一批已经在解析（复制和解析错开，不干等）。
    let mut parsed: Vec<ParsedDoc> = Vec::with_capacity(docs.len());
    let batches: Vec<_> = docs.chunks(64).collect();
    std::thread::scope(|s| {
        let mut got = batches
            .first()
            .map(|b| bookconv::util::par_map(b, parse_doc));
        for k in 0..batches.len() {
            let next = batches
                .get(k + 1)
                .map(|b| s.spawn(|| bookconv::util::par_map(b, parse_doc)));
            for g in got.into_iter().flatten() {
                let (si, l, b) = g?;
                // 套得深的（少见）不复制：复制、释放都是递归的，本线程的栈不一定够
                let b = if block_depth(&b) > DEEP_NESTING {
                    b
                } else {
                    b.clone()
                };
                parsed.push((si, book.docs[si].path.as_str(), l, b));
            }
            got = next.map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)));
        }
        Ok::<_, String>(())
    })?;
    // 强制分页处另起一节：切出来的节编号接在封面页（`docs.len()`，见 `prepend_cover_page`）后面，名字不重
    let mut next = book.docs.len() + 1;
    let mut out: Vec<ParsedDoc> = Vec::with_capacity(parsed.len());
    for (si, path, lang, blocks) in parsed {
        for (k, part) in split_at_breaks(blocks).into_iter().enumerate() {
            let id = if k == 0 {
                si
            } else {
                next += 1;
                next - 1
            };
            out.push((id, path, lang.clone(), part));
        }
    }
    Ok(out)
}

/// 套得比这深的文档在单独的大栈线程里解析（[`on_big_stack`]）：解析每层约 10KB 栈，`par_map` 的工作线程只有 2MB
/// （以前 300 层没关的 `<div>` 就栈溢出中止）。浅的照旧在工作线程里，不多开线程。
pub(in crate::write) const DEEP_NESTING: usize = 32;

/// 大栈线程的栈：[`MAX_NESTING`] 层的解析、版面、编码都够（只保留地址空间，用到多少占多少）。
const BIG_STACK: usize = 256 << 20;

/// DOM 的最大嵌套层数（不递归）。
fn nesting_depth(html: &Html) -> usize {
    let (mut depth, mut max) = (0usize, 0usize);
    for edge in html.tree.root().traverse() {
        match edge {
            ego_tree::iter::Edge::Open(_) => {
                depth += 1;
                max = max.max(depth);
            }
            ego_tree::iter::Edge::Close(_) => depth -= 1,
        }
    }
    max
}

/// `deep` 时在大栈线程里跑 `f`（[`on_big_stack`]），否则就在本线程跑。
/// 浅的书不开线程：在别的线程里分配，glibc 另开分配区，《阿加莎全集》峰值内存多约 40MB。
pub(in crate::write) fn maybe_big_stack<T: Send>(
    deep: bool,
    f: impl FnOnce() -> T + Send,
) -> Result<T, String> {
    if deep {
        on_big_stack(f)
    } else {
        Ok(f())
    }
}

/// 在栈大小 [`BIG_STACK`] 的线程里跑 `f`，等它跑完（panic 照样传出来）。
fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .stack_size(BIG_STACK)
            .spawn_scoped(s, f)
            .map_err(|e| format!("开线程失败：{e}"))?;
        Ok(h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
    })
}

/// 正文里哪儿都没用到封面图（原书只在 OPF 标了 `cover-image`、spine 里没有封面页）时，在最前面补一个只有封面图的文档，
/// 排成整页图片版面、封面地标指向它。照 Send to Kindle：《绍宋》原书没有封面页，Amazon 版第一个版面就是整页封面
/// （2026-10-09 真机：我们的打开没有封面）。正文里用到了的不补，那些书的产物逐字节不变。
pub(in crate::write) fn prepend_cover_page(book: &Loaded, parsed: &mut Vec<ParsedDoc>) {
    let Some(cover) = book.cover.as_deref() else {
        return;
    };
    if parsed.iter().any(|(_, _, _, blocks)| {
        any_block(
            blocks,
            |bl| matches!(&bl.kind, Kind::Image { src, .. } if src == cover),
        )
    }) {
        return;
    }
    // 序号只用来起实体名，取一个文档用不到的；路径不是任何文档的，没有链接会指到它。
    parsed.insert(
        0,
        (
            book.docs.len(),
            "\0cover",
            None,
            vec![Block::anonymous(
                Kind::Image {
                    src: cover.to_string(),
                    linked: false,
                },
                Computed::root(),
                Vec::new(),
            )],
        ),
    );
}

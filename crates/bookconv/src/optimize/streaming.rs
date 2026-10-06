//! 流式优化：路径进路径出，峰值内存不随书体积线性涨。
use super::*;

/// 优化一本 EPUB：读 `input_path`，产物写到 `output_path`（调用方负责出错时清掉半成品，见 `util::produce_then_replace`）。
///
/// **为什么是流式**：内存的大头几乎全是图片字节——漫画书尤其如此（实测 552MB 的漫画全集整本读进内存处理，常驻内存
/// 几十秒内冲到 1.4GB+），正文 html/css/opf/ncx 这些结构信息本来就很小。所以两阶段拆开：**阶段一**只把非图片条目整份读进
/// 内存，图片条目只记名字、字节留空占位——清洗层（空页清理/自动目录/定章节/dtb:uid 同步）与漫画识别只看 html 文字内容和
/// `<img>` 标签*引用*，从来不需要图片真实字节。**阶段二**按 `entries` 顺序写出：非图片条目用阶段一已处理好的字节；图片条目
/// 才从源文件按需读回、交给 worker 并行处理、按原顺序写进目标文件，写完就丢。输出直接流式写文件（`epubzip::EpubWriter`，带缓冲）。峰值内存量级是"并行中的几张图 + 全书文字部分"（并行上限见 [`crate::imgpool`]）。
///
/// 书里有远程图、或清洗过（manifest 的 `properties` 按各章最终内容标）时 OPF 推迟到最后写：抓图发生在阶段二处理各章时，抓到的图要补进 manifest（manifest 里没有的资源
/// 不算书的一部分），OPF 若先写出去就改不了了。漫画里有 GIF/WebP 页时也推迟：转成 PNG/JPEG 的页（条目名不变）要改
/// manifest 的 media-type，转没转成要等图片处理完才知道。EPUB 只要求 `mimetype` 排第一，其余条目的顺序阅读器不管。
///
/// `on_progress(done, total)`：阶段二每写完一个条目回调一次，`total`＝要写出的条目总数（`entries.len()`，不含末尾的
/// 标记与抓到的远程图）。只在阶段二回调——阶段一对文字书通常是毫秒级，真正拖时间的是逐张图片的重编码。
pub fn optimize_epub_file_streaming(input_path: &std::path::Path, output_path: &std::path::Path, opts: &OptimizeOpts, mut on_progress: impl FnMut(usize, usize)) -> Result<Report, String> {
    let in_file = std::fs::File::open(input_path).map_err(|e| format!("打开输入失败: {e}"))?;
    let bytes_before = in_file.metadata().map(|m| m.len() as usize).unwrap_or(0);
    let mut archive = ZipArchive::new(std::io::BufReader::new(in_file)).map_err(|e| format!("解 EPUB(非 zip?): {e}"))?;

    // 阶段一：非图片条目整份读；图片条目占位（真实字节留到阶段二按需流式读）。
    let raw = crate::epubzip::read_skeleton(&mut archive)?.entries;
    let prep = prepare_entries(raw, opts, bytes_before)?;
    let (comic_margin, grayscale, is_comic_book) = (opts.comic_margin, opts.grayscale, prep.is_comic_book);
    // 漫画页按漫画的阅读范围排（xochitl 设成页边距 1 后更宽），其它图按 EPUB 的阅读范围缩
    let screen = if is_comic_book { opts.comic_screen.unwrap_or(opts.screen) } else { opts.screen };
    let entries = &prep.entries;
    let src_names: HashMap<&str, &str> = prep.rep.wash.iter().flat_map(|w| w.renamed.iter().map(|(old, new)| (new.as_str(), old.as_str()))).collect();
    // 漫画里可能换格式的页（GIF/WebP）：处理后按实际格式改 manifest 的 media-type。
    let may_retype = |name: &str| is_comic_book && matches!(crate::util::image_ext_of(name).as_str(), "gif" | "webp");
    let has_retypable = entries.iter().any(|(n, _, ish)| !*ish && may_retype(n));
    // 推迟写的 OPF 条目名（见函数文档）：有远程图、有可能换格式的页时；清洗过的书也推迟——manifest 的 `properties` 要按各章最终内容标。
    let deferred_opf: Option<&str> = prep.opf_name.as_deref().filter(|_| prep.has_remote_imgs || has_retypable || opts.wash.is_some());
    let mut retyped: Vec<(String, &'static str)> = Vec::new();
    let mut deferred_opf_bytes: Option<Vec<u8>> = None;

    // 阶段二：流式写出。
    let mut zw = crate::epubzip::EpubWriter::create(output_path)?;
    let mut xf = EntryXform::new(&prep, opts);
    if opts.caption_fit && !is_comic_book {
        xf.caption_ctx = Some(caption_ctx(entries, &src_names, &mut archive)?);
    }
    let total_entries = entries.len();
    // 图片并行处理（见 `imgpool`）：主线程按条目顺序读原图字节、提交给 worker、按原顺序取回结果写 zip；
    // 提前提交 `lookahead` 张（读原图字节几乎不花时间，处理才慢），处理与写盘/读盘重叠。结果与逐张顺序处理逐字节相同。
    let workers = crate::imgpool::worker_count();
    let lookahead = workers + 2;
    let image_positions: Vec<usize> = entries.iter().enumerate().filter(|(_, (n, _, ish))| !*ish && crate::imgopt::is_page_image(n)).map(|(i, _)| i).collect();
    std::thread::scope(|scope| -> Result<(), String> {
        struct ImgJob {
            bytes: Vec<u8>,
            bg: Option<crate::bgfit::BgFit>,
            flatten: bool,
            reply: std::sync::mpsc::Sender<Vec<u8>>,
        }
        let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<ImgJob>(lookahead);
        let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));
        let budget = std::sync::Arc::new(crate::imgpool::PixelBudget::new(crate::imgpool::PIXEL_BUDGET));
        for _ in 0..workers {
            let (rx, budget) = (job_rx.clone(), budget.clone());
            scope.spawn(move || loop {
                let job = { rx.lock().unwrap_or_else(|e| e.into_inner()).recv() };
                let Ok(job) = job else { break };
                // 读图片头也是在解析外部输入：兜住 panic（按读不出尺寸算），不让一张坏图摔掉 worker——worker 全摔掉时
                // 主线程要么拿到"线程异常退出"，要么（队列已满时）`send` 永远等不到人收。
                let px = crate::imgopt::guard(|| Some(crate::imgopt::pixel_count(&job.bytes))).unwrap_or(1_000_000);
                let _permit = budget.acquire(px);
                let out = transform_image_bytes(&job.bytes, is_comic_book, screen, comic_margin, grayscale, job.bg, job.flatten).unwrap_or(job.bytes);
                let _ = job.reply.send(out);
            });
        }
        // 接收端只由 worker 持有：万一 worker 全部退出，`job_tx.send` 立刻报错而不是在满队列上永远阻塞。
        drop(job_rx);
        let mut pending: std::collections::VecDeque<std::sync::mpsc::Receiver<Vec<u8>>> = std::collections::VecDeque::new();
        let mut next_submit = 0usize;
        let mut consumed = 0usize; // 已取回的图片数：第 consumed 张图片对应 image_positions[consumed]
        for (i, (name, data, ish)) in entries.iter().enumerate() {
            // 补满提前量：读原图字节（archive 支持随时按名字重新 seek 读，跟阶段一是同一个源文件）并提交。
            while pending.len() < lookahead && next_submit < image_positions.len() {
                let img_name = &entries[image_positions[next_submit]].0;
                // 清洗时改过名的（文件名有安卓存储不能用的字符）按原名回原书读
                let src_name = src_names.get(img_name.as_str()).copied().unwrap_or(img_name.as_str());
                let real_bytes = crate::epubzip::read_by_name(&mut archive, src_name).map_err(|e| format!("重读图片失败: {e}"))?;
                let (tx, rx) = std::sync::mpsc::channel();
                job_tx.send(ImgJob { bytes: real_bytes, bg: prep.bg_fits.get(img_name.as_str()).copied(), flatten: prep.alpha_imgs.contains(img_name.as_str()), reply: tx }).map_err(|_| "图片处理线程已退出".to_string())?;
                pending.push_back(rx);
                next_submit += 1;
            }
            let is_image = image_positions.get(consumed) == Some(&i);
            let final_data: std::borrow::Cow<[u8]> = match xf.transform_text(name, data, *ish) {
                Some(t) => t,
                None if is_image => {
                    let rx = pending.pop_front().ok_or("图片队列意外为空")?;
                    consumed += 1;
                    let out = rx.recv().map_err(|_| format!("图片处理线程异常退出（{name}）"))?;
                    if may_retype(name) {
                        if let Some(mt) = crate::imgopt::converted_media_type(name, &out) {
                            retyped.push((name.clone(), mt));
                        }
                    }
                    std::borrow::Cow::Owned(out)
                }
                None => std::borrow::Cow::Borrowed(data.as_slice()),
            };
            if deferred_opf == Some(name.as_str()) {
                deferred_opf_bytes = Some(final_data.into_owned());
            } else if name != "mimetype" {
                // `mimetype`（阶段一放在第一个）建 `EpubWriter` 时已经写了
                zw.put(name, &final_data)?;
            }
            on_progress(i + 1, total_entries);
        }
        drop(job_tx); // 关闭队列，worker 退出，scope 才能 join
        Ok(())
    })?;
    // 收尾：抓到的远程图（与引用它的章同目录、src 已改本地名）→ 推迟的 OPF（补上这些图的 manifest 项）→ 幂等标记。
    for (path, bytes) in &xf.fetched_imgs {
        zw.put(path, bytes)?;
    }
    if let (Some(name), Some(bytes)) = (deferred_opf, deferred_opf_bytes) {
        let bytes = match String::from_utf8(bytes) {
            Ok(text) => {
                let text = if xf.fetched_imgs.is_empty() { text } else { add_manifest_items(&text, name, &xf.fetched_imgs) };
                let text = if retyped.is_empty() { text } else { set_manifest_media_types(&text, name, &retyped) };
                let props = xf.content_props.as_ref().and_then(|p| crate::wash::normalize::apply_content_properties(&text, crate::epubzip::dir_of(name), p));
                props.unwrap_or(text).into_bytes()
            }
            Err(e) => e.into_bytes(),
        };
        zw.put(name, &bytes)?;
    }
    if let (true, Some(m)) = (is_comic_book, opts.comic_reader_margins) {
        zw.put(READER_MARGINS_MARKER, m.to_string().as_bytes())?;
    }
    zw.put(OPTIMIZE_MARKER, marker_value(opts.wash.is_some()).as_bytes())?;
    zw.finish()?;
    let mut rep = prep.rep;
    rep.bytes_after = std::fs::metadata(output_path).map(|m| m.len() as usize).unwrap_or(0);
    Ok(rep)
}

/// 写图注宽度（[`crate::capfit`]）要的全书信息：带图注的 `<img>` 引用的图的显示宽高（按原书读回这几张图的字节、只读文件头），
/// 全书样式表和 `<style>` 里给图片定的宽高。
fn caption_ctx<R: std::io::Read + std::io::Seek>(entries: &[(String, Vec<u8>, bool)], src_names: &HashMap<&str, &str>, archive: &mut ZipArchive<R>) -> Result<crate::capfit::Ctx, String> {
    let images: HashSet<&str> = entries.iter().filter(|(_, _, ish)| !*ish).map(|(n, _, _)| n.as_str()).collect();
    let mut ctx = crate::capfit::Ctx::default();
    for (name, data, ish) in entries {
        let Ok(text) = std::str::from_utf8(data) else { continue };
        if !*ish {
            if name.to_ascii_lowercase().ends_with(".css") {
                ctx.add_css(text);
            }
            continue;
        }
        for c in crate::html::style_block_re().captures_iter(text) {
            ctx.add_css(&c[2]);
        }
        let dims = &mut ctx.dims;
        for c in crate::capfit::candidates(text) {
            let path = crate::epubzip::resolve_link(name, &c.src).0;
            if dims.contains_key(&path) || !images.contains(path.as_str()) {
                continue;
            }
            let src = src_names.get(path.as_str()).copied().unwrap_or(path.as_str());
            let bytes = crate::epubzip::read_by_name(archive, src).map_err(|e| format!("重读图片失败: {e}"))?;
            if let Some(d) = crate::imgopt::guard(|| crate::imgopt::display_dims(&bytes)) {
                dims.insert(path, d);
            }
        }
    }
    Ok(ctx)
}

//! 流式优化：路径进路径出，峰值内存不随书体积线性涨。
use super::*;

/// 优化一本 EPUB：读 `input_path`，产物写到 `output_path`（调用方负责出错时清掉半成品，见 `util::produce_then_replace`）。
///
/// **为什么是流式**：内存的大头几乎全是图片字节——漫画书尤其如此（实测 552MB 的漫画全集整本读进内存处理，常驻内存
/// 几十秒内冲到 1.4GB+），正文 html/css/opf/ncx 这些结构信息本来就很小。所以两阶段拆开：**阶段一**只把非图片条目整份读进
/// 内存，图片条目只记名字、字节留空占位——清洗层（空页清理/自动目录/分页/dtb:uid 同步）与漫画识别只看 html 文字内容和
/// `<img>` 标签*引用*，从来不需要图片真实字节。**阶段二**按 `entries` 顺序写出：非图片条目用阶段一已处理好的字节；图片条目
/// 才从源文件按需读回、交给 worker 并行处理、按原顺序写进目标文件，写完就丢。输出直接流式写文件（`ZipWriter` 包
/// `BufWriter<File>`）。峰值内存量级是"并行中的几张图 + 全书文字部分"（并行上限见 [`crate::imgpool`]）。
///
/// 书里有远程图时 OPF 推迟到最后写：抓图发生在阶段二处理各章时，抓到的图要补进 manifest（AZW3 写出器只认 manifest 里
/// 的图），OPF 若先写出去就改不了了。EPUB 只要求 `mimetype` 排第一，其余条目的顺序阅读器不管。
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
    let (screen, grayscale, is_comic_book) = (opts.screen, opts.grayscale, prep.is_comic_book);
    let entries = &prep.entries;
    // 有远程图时推迟写的 OPF 条目名（见函数文档）。
    let deferred_opf: Option<&str> = prep.opf_name.as_deref().filter(|_| prep.has_remote_imgs);
    let mut deferred_opf_bytes: Option<Vec<u8>> = None;

    // 阶段二：流式写出。
    let out_file = std::fs::File::create(output_path).map_err(|e| format!("建输出文件失败: {e}"))?;
    let mut zw = ZipWriter::new(std::io::BufWriter::new(out_file));
    let (stored, deflated) = (crate::epubzip::stored(), crate::epubzip::deflated());
    let mut xf = EntryXform::new(&prep, opts);
    let total_entries = entries.len();
    // 图片并行处理（见 `imgpool`）：主线程按条目顺序读原图字节、提交给 worker、按原顺序取回结果写 zip；
    // 提前提交 `lookahead` 张（读原图字节几乎不花时间，处理才慢），处理与写盘/读盘重叠。结果与逐张顺序处理逐字节相同。
    let workers = crate::imgpool::worker_count();
    let lookahead = workers + 2;
    let image_positions: Vec<usize> = entries.iter().enumerate().filter(|(_, (n, _, ish))| !*ish && crate::imgopt::is_downscalable(n)).map(|(i, _)| i).collect();
    std::thread::scope(|scope| -> Result<(), String> {
        struct ImgJob {
            bytes: Vec<u8>,
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
                let px = std::panic::catch_unwind(|| crate::imgopt::pixel_count(&job.bytes)).unwrap_or(1_000_000);
                let _permit = budget.acquire(px);
                let out = transform_image_bytes(&job.bytes, is_comic_book, screen, grayscale).unwrap_or(job.bytes);
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
                let real_bytes = crate::epubzip::read_by_name(&mut archive, img_name).map_err(|e| format!("重读图片 {img_name} 失败: {e}"))?;
                let (tx, rx) = std::sync::mpsc::channel();
                job_tx.send(ImgJob { bytes: real_bytes, reply: tx }).map_err(|_| "图片处理线程已退出".to_string())?;
                pending.push_back(rx);
                next_submit += 1;
            }
            let is_image = image_positions.get(consumed) == Some(&i);
            let final_data: std::borrow::Cow<[u8]> = match xf.transform_text(name, data, *ish) {
                Some(t) => t,
                None if is_image => {
                    let rx = pending.pop_front().ok_or("图片队列意外为空")?;
                    consumed += 1;
                    std::borrow::Cow::Owned(rx.recv().map_err(|_| format!("图片处理线程异常退出（{name}）"))?)
                }
                None => std::borrow::Cow::Borrowed(data.as_slice()),
            };
            if deferred_opf == Some(name.as_str()) {
                deferred_opf_bytes = Some(final_data.into_owned());
            } else {
                crate::epubzip::put_entry(&mut zw, name, entry_options(name, stored, deflated), &final_data)?;
            }
            on_progress(i + 1, total_entries);
        }
        drop(job_tx); // 关闭队列，worker 退出，scope 才能 join
        Ok(())
    })?;
    // 收尾：抓到的远程图（与引用它的章同目录、src 已改本地名）→ 推迟的 OPF（补上这些图的 manifest 项）→ 幂等标记。
    for (path, bytes) in &xf.fetched_imgs {
        crate::epubzip::put_entry(&mut zw, path, entry_options(path, stored, deflated), bytes)?;
    }
    if let (Some(name), Some(bytes)) = (deferred_opf, deferred_opf_bytes) {
        let bytes = match String::from_utf8(bytes) {
            Ok(text) if !xf.fetched_imgs.is_empty() => add_manifest_items(&text, name, &xf.fetched_imgs).into_bytes(),
            Ok(text) => text.into_bytes(),
            Err(e) => e.into_bytes(),
        };
        crate::epubzip::put_entry(&mut zw, name, deflated, &bytes)?;
    }
    crate::epubzip::put_entry(&mut zw, OPTIMIZE_MARKER, deflated, marker_value(opts.wash.is_some()).as_bytes())?;
    // `finish()` 只保证写完中央目录，底下 `BufWriter` 自己的缓冲区不一定落盘——显式 flush，
    // 不指望 Drop 的静默兜底（出错会被吞掉）。
    let mut out = zw.finish().map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    let mut rep = prep.rep;
    rep.bytes_after = std::fs::metadata(output_path).map(|m| m.len() as usize).unwrap_or(0);
    Ok(rep)
}

/// 条目的压缩方式：`mimetype`（EPUB 规范）与本身已压缩的图片（JPEG/PNG/GIF/WebP，再 deflate 几乎没收益、白花 CPU）用
/// STORED，其余 deflate。
fn entry_options(name: &str, stored: zip::write::SimpleFileOptions, deflated: zip::write::SimpleFileOptions) -> zip::write::SimpleFileOptions {
    if name == "mimetype" || crate::util::is_image_ext(name) { stored } else { deflated }
}

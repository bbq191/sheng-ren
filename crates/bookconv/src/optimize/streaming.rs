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
pub fn optimize_epub_file_streaming(input_path: &std::path::Path, output_path: &std::path::Path, opts: &OptimizeOpts, on_progress: impl FnMut(usize, usize)) -> Result<Report, String> {
    optimize_epub_file_streaming_with_cancel(input_path, output_path, opts, on_progress, &|| false)
}

/// 同 [`optimize_epub_file_streaming`]，另可中途取消：`cancel()` 返回 `true` 就停下，返回 `Err`，错误串以 [`CANCELLED_MSG`] 开头。
/// 读完骨架后、写每个条目之前、收尾之前各问一次（大书一个条目也就一张图的工夫）。
///
/// 出错（含取消）时，这次已经建出来的 `output_path` 删掉，不留半成品；还没开始写就出错的不碰 `output_path`
/// （那里可能是调用方原有的文件）。调用方照旧用 `util::produce_then_replace` 写临时文件、成功才改名。
pub fn optimize_epub_file_streaming_with_cancel(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    opts: &OptimizeOpts,
    on_progress: impl FnMut(usize, usize),
    cancel: &dyn Fn() -> bool,
) -> Result<Report, String> {
    let mut created = false;
    let r = optimize_inner(input_path, output_path, opts, on_progress, cancel, &mut created);
    if r.is_err() && created {
        let _ = std::fs::remove_file(output_path);
    }
    r
}

/// 取消时返回 `Err(CANCELLED_MSG)`。
fn check_cancel(cancel: &dyn Fn() -> bool) -> Result<(), String> {
    if cancel() { Err(CANCELLED_MSG.to_string()) } else { Ok(()) }
}

fn optimize_inner(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    opts: &OptimizeOpts,
    mut on_progress: impl FnMut(usize, usize),
    cancel: &dyn Fn() -> bool,
    created: &mut bool,
) -> Result<Report, String> {
    check_cancel(cancel)?;
    let in_file = std::fs::File::open(input_path).map_err(|e| format!("打开输入失败: {e}"))?;
    let bytes_before = in_file.metadata().map(|m| m.len() as usize).unwrap_or(0);
    let mut archive = ZipArchive::new(std::io::BufReader::new(in_file)).map_err(|e| format!("解 EPUB(非 zip?): {e}"))?;

    // 阶段一：非图片条目整份读；图片条目占位（真实字节留到阶段二按需流式读）。
    let mut raw = crate::epubzip::read_skeleton_par(input_path, &mut archive)?.entries;
    // 清洗的书先把 GBK、Big5、UTF-16 的文件转成 UTF-8：下面补封面声明、封面页按 UTF-8 改 OPF（清洗层开头本来也转，这里提前）
    let transcoded = if opts.wash.is_some() { crate::wash::transcode_entries(&mut raw) } else { 0 };
    // 漫画识别只在这里判一次（按原书），清洗层和图片处理都用这个结果（以前清洗层清洗完又判一次，两次可能不一致）。
    let is_comic_book = crate::comic_detect::is_comic(&raw);
    // 文字书只做修复（漫画照常）：换成只修复的选项
    let repair;
    let opts = if opts.text_repair_only && !is_comic_book {
        repair = opts.repair_only();
        &repair
    } else {
        opts
    };
    let keep_images = opts.keeps_content();
    // 照 Send to Kindle 补封面页（掌阅、Move 的文字书）：图的宽高从原书读文件头（这时图片条目是空占位）
    if opts.wash.as_ref().is_some_and(|w| w.kindle_rules) && !is_comic_book {
        crate::wash::ensure_cover_declared(&mut raw);
        crate::wash::prepend_cover_page(&mut raw, |p| {
            let bytes = crate::epubzip::read_by_name(&mut archive, p).ok()?;
            image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
        });
    }
    let prep = prepare_entries(raw, opts, bytes_before, is_comic_book, transcoded)?;
    check_cancel(cancel)?;
    let (comic_margin, grayscale) = (opts.comic_margin, opts.grayscale);
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
    *created = true;
    let mut xf = EntryXform::new(&prep, opts);
    if opts.caption_fit && !is_comic_book {
        xf.caption_ctx = Some(caption_ctx(entries, &src_names, &mut archive)?);
    }
    let total_entries = entries.len();
    // 并行（见 `imgpool`）：主线程按条目顺序变换文字条目，把图片处理和 deflate 压缩交给 worker，再按原顺序取回写 zip。
    // 图片提前提交 `lookahead` 张，worker 自己打开源文件读原图字节（原图在 zip 里是压缩的时，解压整卷漫画在主线程上要近一秒），
    // 处理与写盘重叠。要 deflate 的条目（文字、样式、字体……）也交给 worker 压（[`crate::epubzip::Precompressed`]：文字多的书
    // deflate 占主线程两三成），写进 zip 时原样拷压好的数据。结果与逐条顺序处理、直接写逐字节相同。
    let workers = crate::imgpool::worker_count();
    let lookahead = workers + 2;
    // 已交出去、还没写进 zip 的条目最多这么多（压好的、处理好的结果在这里排队等前面的写完）
    let max_queued = lookahead;
    let image_positions: Vec<usize> = entries.iter().enumerate().filter(|(_, (n, _, ish))| !*ish && crate::imgopt::is_page_image(n)).map(|(i, _)| i).collect();
    // 字体（只修复的文字书里最大的条目，《绍宋》两个 15MB 的 ttf）和原书那份逐字节相同、原书里是 deflate 压的：原样拷原书的
    // 压缩数据，不解压再重压（2026-10-09）。用 CRC 和大小认"同一份"（去混淆过的字体不同，照常重压）。原书里的字体：条目名 → (CRC, 大小)
    let font_src: HashMap<String, (u32, u64)> = (0..archive.len())
        .filter_map(|i| {
            let f = archive.by_index_raw(i).ok()?;
            (f.compression() == zip::CompressionMethod::Deflated && is_font_name(f.name())).then(|| (f.name().to_string(), (f.crc32(), f.size())))
        })
        .collect();
    std::thread::scope(|scope| -> Result<(), String> {
        enum Job<'e> {
            /// `src`：原书里的条目名（清洗时改过名的是原名）。
            Image { src: &'e str, bg: Option<crate::bgfit::BgFit>, flatten: bool, reply: std::sync::mpsc::Sender<Result<Vec<u8>, String>> },
            Deflate { name: &'e str, data: std::borrow::Cow<'e, [u8]>, reply: std::sync::mpsc::Sender<Result<crate::epubzip::Precompressed, String>> },
        }
        /// 按条目顺序排队等写进 zip 的东西。
        enum Out<'e> {
            /// 图片：等 worker 处理完（条目名用来判断换没换格式）。
            Image(&'e str, std::sync::mpsc::Receiver<Result<Vec<u8>, String>>),
            /// 等 worker 压好。
            Deflate(std::sync::mpsc::Receiver<Result<crate::epubzip::Precompressed, String>>),
            /// 直接写（STORED 的条目）。
            Direct(&'e str, std::borrow::Cow<'e, [u8]>),
            /// 和原书那份逐字节相同的大字体：原样拷原书的压缩数据（第二项是原书里的条目名）。
            Raw(&'e str, &'e str),
            /// 不写（`mimetype` 建 `EpubWriter` 时写过了；推迟的 OPF 最后写），只算进度。
            Skip,
        }
        let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<Job>(lookahead);
        let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));
        let budget = std::sync::Arc::new(crate::imgpool::PixelBudget::new(opts.limits.pool_pixel_budget));
        let max_px = opts.limits.max_decode_pixels;
        for _ in 0..workers {
            let (rx, budget) = (job_rx.clone(), budget.clone());
            scope.spawn(move || {
                // 读原图用的源文件（第一次用到时打开，同阶段一读的是同一个文件）
                let mut source: Option<crate::epubzip::FileZip> = None;
                loop {
                    let job = { rx.lock().unwrap_or_else(|e| e.into_inner()).recv() };
                    let Ok(job) = job else { break };
                    match job {
                        Job::Image { src, bg, flatten, reply } => {
                            // 整件兜住 panic（读 zip、解析图片都是外部输入）：panic 了只这本书报错，不让 `thread::scope` 收尾时把
                            // panic 抛给调用方、摔掉整个 `booklib sync`（2026-10-09 审计）
                            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
                                let bytes = match &mut source {
                                    Some(z) => Ok(z),
                                    None => crate::epubzip::open_file_zip(input_path).map(|z| source.insert(z)),
                                }
                                .and_then(|z| crate::epubzip::read_by_name(z, src))
                                .map_err(|e| format!("重读图片失败: {e}"));
                                let bytes = bytes?;
                                if keep_images {
                                    return Ok(bytes);
                                }
                                // 读图片头也是在解析外部输入：兜住 panic（按读不出尺寸算），不让一张坏图摔掉 worker——worker 全摔掉时
                                // 主线程要么拿到"线程异常退出"，要么（队列已满时）`send` 永远等不到人收。
                                let px = crate::imgopt::guard(|| Some(crate::imgopt::pixel_count(&bytes))).unwrap_or(1_000_000);
                                let _permit = budget.acquire(px);
                                let out = transform_image_bytes(&bytes, is_comic_book, screen, comic_margin, grayscale, bg, flatten, max_px).unwrap_or(bytes);
                                Ok(out)
                            }))
                            .unwrap_or_else(|_| {
                                source = None;
                                Err(format!("处理图片时出错（{src}）"))
                            });
                            let _ = reply.send(r);
                        }
                        Job::Deflate { name, data, reply } => {
                            let _ = reply.send(crate::epubzip::Precompressed::new(name, &data));
                        }
                    }
                }
            });
        }
        // 接收端只由 worker 持有：万一 worker 全部退出，`job_tx.send` 立刻报错而不是在满队列上永远阻塞。
        drop(job_rx);
        let mut pending: std::collections::VecDeque<std::sync::mpsc::Receiver<Result<Vec<u8>, String>>> = std::collections::VecDeque::new();
        let mut queue: std::collections::VecDeque<Out> = std::collections::VecDeque::new();
        let mut next_submit = 0usize;
        let mut consumed = 0usize; // 已取回的图片数：第 consumed 张图片对应 image_positions[consumed]
        let mut queued_images = 0usize; // 排在 `queue` 里、还没写的图片
        let mut written = 0usize;
        // 一批最多这么多个条目、这么多字节的 html
        const PREFIX_BATCH: usize = 32;
        const PREFIX_BATCH_BYTES: usize = 2 << 20;
        let (mut prefix_end, mut prefixes) = (0usize, Vec::new().into_iter());
        // 按顺序写 `queue` 前面的：`wait` 为假时只写已经好了的，为真时等前面那个好了再写。返回写没写。
        let mut write_front = |queue: &mut std::collections::VecDeque<Out>, wait: bool, queued_images: &mut usize| -> Result<bool, String> {
            use std::sync::mpsc::TryRecvError;
            let Some(front) = queue.front() else { return Ok(false) };
            match front {
                Out::Image(name, rx) => {
                    let out = match if wait { rx.recv().map_err(|_| TryRecvError::Disconnected) } else { rx.try_recv() } {
                        Ok(out) => out?,
                        Err(TryRecvError::Empty) => return Ok(false),
                        Err(TryRecvError::Disconnected) => return Err(format!("图片处理线程异常退出（{name}）")),
                    };
                    if may_retype(name) {
                        if let Some(mt) = crate::imgopt::converted_media_type(name, &out) {
                            retyped.push((name.to_string(), mt));
                        }
                    }
                    zw.put(name, &out)?;
                    *queued_images -= 1;
                }
                Out::Deflate(rx) => {
                    let z = match if wait { rx.recv().map_err(|_| TryRecvError::Disconnected) } else { rx.try_recv() } {
                        Ok(z) => z?,
                        Err(TryRecvError::Empty) => return Ok(false),
                        Err(TryRecvError::Disconnected) => return Err("压缩线程异常退出".to_string()),
                    };
                    zw.put_precompressed(z)?;
                }
                Out::Direct(name, data) => zw.put(name, data)?,
                Out::Raw(name, src) => {
                    let f = archive.by_name(src).map_err(|e| format!("重读 {name}: {e}"))?;
                    zw.raw_copy_as(f, name)?;
                }
                Out::Skip => {}
            }
            queue.pop_front();
            written += 1;
            on_progress(written, total_entries);
            Ok(true)
        };
        for (i, (name, data, ish)) in entries.iter().enumerate() {
            check_cancel(cancel)?;
            // 交出去还没写的图片已经有 `lookahead` 张：先等前面的写掉，腾出位置（至少能交这一张）
            while queued_images >= lookahead {
                write_front(&mut queue, true, &mut queued_images)?;
            }
            // 补满提前量（原图字节由 worker 自己从源文件读）
            while pending.len() + queued_images < lookahead && next_submit < image_positions.len() {
                let img_name = &entries[image_positions[next_submit]].0;
                // 清洗时改过名的（文件名有安卓存储不能用的字符）按原名回原书读
                let src = src_names.get(img_name.as_str()).copied().unwrap_or(img_name.as_str());
                let (tx, rx) = std::sync::mpsc::channel();
                job_tx
                    .send(Job::Image { src, bg: prep.bg_fits.get(img_name.as_str()).copied(), flatten: prep.alpha_imgs.contains(img_name.as_str()), reply: tx })
                    .map_err(|_| "图片处理线程已退出".to_string())?;
                pending.push_back(rx);
                next_submit += 1;
            }
            let is_image = image_positions.get(consumed) == Some(&i);
            // html 章节变换的前几步（各章独立）一批一批预先多线程算好；分批是为了不让整本书的文字多出一份
            if i == prefix_end {
                let mut bytes = 0usize;
                prefix_end = i;
                while prefix_end < entries.len() && prefix_end - i < PREFIX_BATCH && (prefix_end == i || bytes < PREFIX_BATCH_BYTES) {
                    bytes += if entries[prefix_end].2 { entries[prefix_end].1.len() } else { 0 };
                    prefix_end += 1;
                }
                prefixes = xf.html_prefixes(&entries[i..prefix_end]).into_iter();
            }
            let prefix = prefixes.next().flatten();
            let out = match xf.transform_text(name, data, *ish, prefix) {
                None if is_image => {
                    consumed += 1;
                    queued_images += 1;
                    Out::Image(name, pending.pop_front().ok_or("图片队列意外为空")?)
                }
                // `mimetype`（阶段一放在第一个）建 `EpubWriter` 时已经写了
                _ if name == "mimetype" => Out::Skip,
                t => {
                    let t = t.unwrap_or(std::borrow::Cow::Borrowed(data.as_slice()));
                    if deferred_opf == Some(name.as_str()) {
                        deferred_opf_bytes = Some(t.into_owned());
                        Out::Skip
                    } else if crate::util::is_image_ext(name) {
                        Out::Direct(name, t)
                    } else if let Some(src) = Some(src_names.get(name.as_str()).copied().unwrap_or(name.as_str()))
                        .filter(|src| font_src.get(*src).is_some_and(|&(crc, size)| t.len() >= RAW_FONT_MIN && t.len() as u64 == size && crc32fast::hash(&t) == crc))
                    {
                        Out::Raw(name, src)
                    } else {
                        let (tx, rx) = std::sync::mpsc::channel();
                        job_tx.send(Job::Deflate { name, data: t, reply: tx }).map_err(|_| "压缩线程已退出".to_string())?;
                        Out::Deflate(rx)
                    }
                }
            };
            queue.push_back(out);
            loop {
                let wait = queue.len() > max_queued;
                if !write_front(&mut queue, wait, &mut queued_images)? {
                    break;
                }
            }
        }
        while write_front(&mut queue, true, &mut queued_images)? {}
        drop(job_tx); // 关闭队列，worker 退出，scope 才能 join
        Ok(())
    })?;
    check_cancel(cancel)?;
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

/// 原样拷原书压缩数据的字体至少这么大（小字体重压也就几毫秒，不值得多算一遍 CRC）。
const RAW_FONT_MIN: usize = 64 << 10;

/// 字体文件（按扩展名）。
fn is_font_name(name: &str) -> bool {
    matches!(crate::util::image_ext_of(name).as_str(), "ttf" | "otf" | "ttc" | "woff" | "woff2")
}

/// 写图注宽度（[`crate::capfit`]）要的全书信息：带图注的 `<img>` 引用的图的显示宽高（按原书读回这几张图的字节、只读文件头），
/// 全书样式表和 `<style>` 里给图片定的宽高。
fn caption_ctx<R: std::io::Read + std::io::Seek>(entries: &[(String, Vec<u8>, bool)], src_names: &HashMap<&str, &str>, archive: &mut ZipArchive<R>) -> Result<crate::capfit::Ctx, String> {
    let images: HashSet<&str> = entries.iter().filter(|(_, _, ish)| !*ish).map(|(n, _, _)| n.as_str()).collect();
    let mut ctx = crate::capfit::Ctx::default();
    for (name, data, ish) in entries {
        let Ok(text) = std::str::from_utf8(data) else { continue };
        if !*ish {
            if crate::wash::is_css_name(name) {
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

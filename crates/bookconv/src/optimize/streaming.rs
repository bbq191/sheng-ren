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
pub fn optimize_epub_file_streaming(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    opts: &OptimizeOpts,
    on_progress: impl FnMut(usize, usize),
) -> Result<Report, BookError> {
    optimize_epub_file_streaming_with_cancel(input_path, output_path, opts, on_progress, &|| false)
}

/// 同 [`optimize_epub_file_streaming`]，另可中途取消：`cancel()` 返回 `true` 就停下，返回 `Err(BookError::Cancelled)`（文字是 [`CANCELLED_MSG`]）。
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
) -> Result<Report, BookError> {
    let mut created = false;
    let r = optimize_inner(
        input_path,
        output_path,
        opts,
        on_progress,
        cancel,
        &mut created,
    );
    if r.is_err() && created {
        let _ = std::fs::remove_file(output_path);
    }
    r
}

/// 取消时返回 `Err(BookError::Cancelled)`。
fn check_cancel(cancel: &dyn Fn() -> bool) -> Result<(), BookError> {
    if cancel() {
        Err(BookError::Cancelled)
    } else {
        Ok(())
    }
}

fn optimize_inner(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    opts: &OptimizeOpts,
    on_progress: impl FnMut(usize, usize),
    cancel: &dyn Fn() -> bool,
    created: &mut bool,
) -> Result<Report, BookError> {
    check_cancel(cancel)?;
    let in_file = std::fs::File::open(input_path).map_err(|e| BookError::io("打开输入失败", e))?;
    let bytes_before = in_file.metadata().map(|m| m.len() as usize).unwrap_or(0);
    let mut archive = ZipArchive::new(std::io::BufReader::new(in_file))
        .map_err(|e| BookError::zip("解 EPUB(非 zip?)", e))?;

    // 阶段一：非图片条目整份读；图片条目占位（真实字节留到阶段二按需流式读）。
    let mut raw = crate::epubzip::read_skeleton_par(input_path, &mut archive)?.entries;
    // 清洗的书先把 GBK、Big5、UTF-16 的文件转成 UTF-8：下面补封面声明、封面页按 UTF-8 改 OPF（清洗层开头本来也转，这里提前）
    let transcoded = if opts.wash.is_some() {
        crate::wash::transcode_entries(&mut raw)
    } else {
        0
    };
    // 漫画识别只在这里判一次（按原书），清洗层和图片处理都用这个结果（以前清洗层清洗完又判一次，两次可能不一致）。
    let is_comic_book = crate::comic_detect::is_comic(&raw);
    // 文字书只做修复（漫画照常）：换成只修复的选项
    let repair;
    let opts = match opts.text_mode {
        TextMode::Repair { .. } if !is_comic_book => {
            repair = opts.repair_only();
            &repair
        }
        _ => opts,
    };
    // 保证 OPF 声明了有效封面（见 `wash::ensure_cover_declared`）。必须在清洗之前：清洗会把只含 SVG 封面的 titlepage 当空页删掉；
    // 也在补封面页之前：补的页放的是声明的封面图。全程只调这一次（以前开 `kindle_rules` 时这里一次、阶段一开头又一次，第二次什么也不改）。
    crate::wash::ensure_cover_declared(&mut raw);
    // 照 Send to Kindle 补封面页（掌阅、Move 的文字书）：图的宽高从原书读文件头（这时图片条目是空占位）
    if opts.wash.as_ref().is_some_and(|w| w.mode.kindle_rules()) && !is_comic_book {
        crate::wash::prepend_cover_page(&mut raw, |p| {
            // 读图片头也是在解析外部输入：兜住解码器的 panic（按读不出宽高算，不补封面页）
            let dims = |b: &[u8]| {
                crate::imgopt::guard(|| {
                    image::ImageReader::new(std::io::Cursor::new(b))
                        .with_guessed_format()
                        .ok()?
                        .into_dimensions()
                        .ok()
                })
            };
            read_image_head(&mut archive, p, dims).ok().flatten()
        });
    }
    // 书架缩略图（文字书、漫画都做）：封面居中裁成书架框的比例另存一张，书里的封面页不动
    // 已经有缩略图（再优化我们的产物）：比例和这次的一样才留着，不一样重做；这次的设备不做缩略图就去掉（meta cover 改回真封面）
    if let Some(aspect) = opts.cover_thumb_aspect {
        let old_thumb = crate::wash::parse_opf(&raw).and_then(|o| {
            let text = String::from_utf8_lossy(&raw[o.index].data).into_owned();
            crate::wash::manifest_items(&text)
                .iter()
                .find(|i| i.id == crate::wash::THUMB_ID)
                .map(|i| i.path(&o.dir))
        });
        let fits = old_thumb.is_some_and(|p| {
            let dims = |b: &[u8]| {
                crate::imgopt::guard(|| {
                    image::ImageReader::new(std::io::Cursor::new(b))
                        .with_guessed_format()
                        .ok()?
                        .into_dimensions()
                        .ok()
                })
            };
            read_image_head(&mut archive, &p, dims)
                .ok()
                .flatten()
                .is_some_and(|(w, h)| crate::imgopt::thumb_aspect_fits(w, h, aspect))
        });
        crate::wash::add_cover_thumbnail(
            &mut raw,
            opts.cover_meta_thumb,
            |_| fits,
            |p| {
                let bytes = crate::epubzip::read_by_name(&mut archive, p).ok()?;
                crate::imgopt::guard(|| {
                    crate::imgopt::cover_thumbnail(
                        &bytes,
                        aspect,
                        opts.grayscale,
                        opts.limits.max_decode_pixels,
                    )
                })
            },
        );
    } else {
        crate::wash::drop_cover_thumbnail(&mut raw);
    }
    // 图片照 Kindle 的大小改写（`kindle_images`）要原图宽高：这时图片条目是空占位，先读文件头
    let sized;
    let opts = match opts.wash.as_ref().filter(|w| {
        ((w.mode.kindle_rules() && w.image_sizing.is_some()) || w.tiny_page.is_some())
            && !is_comic_book
    }) {
        Some(w) => {
            let dims = |b: &[u8]| {
                crate::imgopt::guard(|| {
                    image::ImageReader::new(std::io::Cursor::new(b))
                        .with_guessed_format()
                        .ok()?
                        .into_dimensions()
                        .ok()
                })
            };
            let names: Vec<String> = raw
                .iter()
                .filter(|e| crate::util::is_image_ext(&e.name))
                .map(|e| e.name.clone())
                .collect();
            let mut map = HashMap::new();
            for n in names {
                if let Ok(Some(d)) = read_image_head(&mut archive, &n, dims) {
                    map.insert(n, d);
                }
            }
            let mut o = opts.clone();
            o.wash = Some(crate::wash::WashOpts {
                image_dims: Some(std::sync::Arc::new(map)),
                ..w.clone()
            });
            sized = o;
            &sized
        }
        None => opts,
    };
    // 不认 WebP 的阅读器（`webp_to_png`）：挑出能转成 PNG 的 WebP 交给清洗层改名（这时图片条目是空占位，读文件头；动图取第一帧）
    let webp;
    let opts = match opts
        .wash
        .as_ref()
        .filter(|_| opts.webp_to_png && !is_comic_book)
    {
        Some(w) => {
            let names: Vec<String> = raw
                .iter()
                .filter(|e| crate::util::image_ext_of(&e.name) == "webp")
                .map(|e| e.name.clone())
                .collect();
            let mut set = HashSet::new();
            for n in names {
                if let Ok(Some(true)) = read_image_head(&mut archive, &n, |b| {
                    Some(crate::imgopt::is_convertible_webp(b))
                }) {
                    set.insert(n);
                }
            }
            let mut o = opts.clone();
            o.wash = Some(crate::wash::WashOpts {
                webp_to_png: Some(std::sync::Arc::new(set)),
                ..w.clone()
            });
            webp = o;
            &webp
        }
        None => opts,
    };
    let mut prep = prepare_entries(raw, opts, bytes_before, is_comic_book, transcoded)?;
    check_cancel(cancel)?;
    // 漫画跨页拆分：先把原页/原图换成两半、补好 manifest/spine，再建写出计划（新图不再是 worker 处理的空占位）
    let split_screen = if prep.is_comic_book {
        opts.comic_screen.unwrap_or(opts.screen)
    } else {
        opts.screen
    };
    let split_images = split_spread_pages(&mut prep, opts, &mut archive, split_screen)?;
    let plan = WritePlan::new(&prep, opts, &mut archive);
    // Kobo 固定版式的百分比写法要按图的比例决定 img 样式。流式写出时图还没处理，这里只读页图文件头（含 EXIF，不改图、
    // 不解码整张），按 `prepare_comic_page*` 的装饰阈值（短边 < 阅读范围宽 1/3，见 `imgopt`）预判：
    // 非装饰页会被放大/缩小/补白到画布，直接记画布尺寸（1440×1920 这类源图也按画布写 100%×100%）；装饰小图保持
    // 原尺寸，记源图尺寸，后面走 max-width/max-height 居中。读不到尺寸的不入表，也走不拉伸的写法。
    let mut image_dims = HashMap::new();
    if prep.is_comic_book && opts.comic_fixed_layout && opts.comic_fixed_layout_percent {
        for &i in &plan.image_positions {
            let name = &prep.entries[i].0;
            let src = plan.src(name);
            if let Ok(Some((iw, ih))) = read_image_head(&mut archive, src, |b| {
                crate::imgopt::guard(|| crate::imgopt::display_dims(b))
            }) {
                let decorative = iw.min(ih) < plan.screen.width / 3;
                let dims = if decorative {
                    (iw, ih)
                } else {
                    (plan.screen.width, plan.screen.height)
                };
                image_dims.insert(name.clone(), dims);
            }
        }
        // 拆出来的两半都排到画布大小，固定版式百分比写法按画布记
        for name in split_images {
            image_dims.insert(name, (plan.screen.width, plan.screen.height));
        }
    }

    // 阶段二：流式写出。
    let mut zw = crate::epubzip::EpubWriter::create(output_path)?;
    *created = true;
    let mut xf = EntryXform::new(&prep, opts, &image_dims);
    if opts.caption_fit && !is_comic_book {
        xf.caption_ctx = Some(caption_ctx(&prep.entries, &plan.src_names, &mut archive)?);
    }
    let written = write_entries(
        input_path,
        &prep,
        opts,
        &plan,
        &mut xf,
        &mut zw,
        &mut archive,
        on_progress,
        cancel,
    )?;
    check_cancel(cancel)?;
    finalize(&mut zw, &prep, opts, &plan, &xf, written)?;
    zw.finish()?;
    let mut rep = prep.rep;
    rep.bytes_after = std::fs::metadata(output_path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    Ok(rep)
}

/// 只为读图片文件头：先只解压条目开头 [`HEAD_PREFIX`] 字节交给 `f`，读不出（头比这长，比如 JPEG 前面塞了大块 EXIF、缩略图）再整条
/// 读一遍。以前补封面页、算图注宽度都整条解压只为看文件头，阶段二 worker 还要再解一遍。WebP 的 EXIF 块在图像数据后面
/// （方向要整条才读得到），一律整条读。条目不存在、读不出 → `Err`。
fn read_image_head<R: std::io::Read + std::io::Seek, T>(
    archive: &mut ZipArchive<R>,
    name: &str,
    f: impl Fn(&[u8]) -> Option<T>,
) -> Result<Option<T>, BookError> {
    use std::io::Read;
    let mut prefix = Vec::new();
    {
        let entry = archive.by_name(name).map_err(|e| BookError::zip(name, e))?;
        entry
            .take(HEAD_PREFIX)
            .read_to_end(&mut prefix)
            .map_err(|e| BookError::io(name, e))?;
    }
    let complete = (prefix.len() as u64) < HEAD_PREFIX;
    if complete || !prefix.starts_with(b"RIFF") {
        if let Some(t) = f(&prefix) {
            return Ok(Some(t));
        }
        if complete {
            return Ok(None);
        }
    }
    let bytes = crate::epubzip::read_by_name(archive, name)?;
    Ok(f(&bytes))
}

/// [`read_image_head`] 先读的字节数：够装下 JPEG 的 EXIF（APP1 段最长 64KB）加上 SOF 段，PNG、GIF 的头只要几十字节。
const HEAD_PREFIX: u64 = 96 << 10;

/// 阶段二按条目写出前定好的东西（只读）。
struct WritePlan<'p> {
    is_comic_book: bool,
    /// 图片处理用的阅读范围：漫画页按漫画的阅读范围排（xochitl 设成页边距 1 后更宽），其它图按 EPUB 的阅读范围缩。
    screen: crate::imgopt::Screen,
    /// 清洗时改过名的条目：新名 → 原书里的名字（按原名回原书读字节）。
    src_names: HashMap<&'p str, &'p str>,
    /// 推迟写的 OPF 条目名（见 [`optimize_epub_file_streaming`] 文档）：有远程图、有可能换格式的页时；清洗过的书也推迟——manifest 的
    /// `properties` 要按各章最终内容标。
    deferred_opf: Option<&'p str>,
    /// 页面图片在 `entries` 里的位置（按顺序）。
    image_positions: Vec<usize>,
    /// 原书里 deflate 压的字体：条目名 → (CRC, 大小)。和原书那份逐字节相同的原样拷压缩数据（[`Out::Raw`]）。
    font_src: HashMap<String, (u32, u64)>,
}

impl<'p> WritePlan<'p> {
    fn new<R: std::io::Read + std::io::Seek>(
        prep: &'p Prepared,
        opts: &OptimizeOpts,
        archive: &mut ZipArchive<R>,
    ) -> WritePlan<'p> {
        let is_comic_book = prep.is_comic_book;
        let entries = &prep.entries;
        let has_retypable = entries
            .iter()
            .any(|(n, _, ish)| !*ish && may_retype(is_comic_book, n));
        WritePlan {
            is_comic_book,
            screen: if is_comic_book {
                opts.comic_screen.unwrap_or(opts.screen)
            } else {
                opts.screen
            },
            src_names: prep
                .rep
                .wash
                .iter()
                .flat_map(|w| {
                    w.renamed
                        .iter()
                        .map(|(old, new)| (new.as_str(), old.as_str()))
                })
                .collect(),
            deferred_opf: prep
                .opf_name
                .as_deref()
                .filter(|_| prep.has_remote_imgs || has_retypable || opts.wash.is_some()),
            // 清洗层自己加的图（Move 的垫图 `eink-spacer.png`）有字节、原书里没有：不回原书读，照写
            image_positions: entries
                .iter()
                .enumerate()
                .filter(|(_, (n, d, ish))| !*ish && d.is_empty() && crate::imgopt::is_page_image(n))
                .map(|(i, _)| i)
                .collect(),
            // 字体（只修复的文字书里最大的条目，《绍宋》两个 15MB 的 ttf）和原书那份逐字节相同、原书里是 deflate 压的：原样拷原书的
            // 压缩数据，不解压再重压（2026-10-09）。用 CRC 和大小认"同一份"（去混淆过的字体不同，照常重压）。
            font_src: (0..archive.len())
                .filter_map(|i| {
                    let f = archive.by_index_raw(i).ok()?;
                    (f.compression() == zip::CompressionMethod::Deflated && is_font_name(f.name()))
                        .then(|| (f.name().to_string(), (f.crc32(), f.size())))
                })
                .collect(),
        }
    }

    /// 原书里的条目名（清洗时改过名的是原名）。
    fn src<'n>(&self, name: &'n str) -> &'n str
    where
        'p: 'n,
    {
        self.src_names.get(name).copied().unwrap_or(name)
    }
}

/// 漫画里可能换格式的页（GIF/WebP）：处理后按实际格式改 manifest 的 media-type。
fn may_retype(is_comic_book: bool, name: &str) -> bool {
    is_comic_book && matches!(crate::util::image_ext_of(name).as_str(), "gif" | "webp")
}

/// 阶段二写完条目后收尾要用的：推迟的 OPF 的内容、换了格式的页（条目名 → 新 media-type）。
struct Written {
    deferred_opf_bytes: Option<Vec<u8>>,
    retyped: Vec<(String, &'static str)>,
}

/// 交给 worker 的活。
enum Job<'e> {
    /// 处理一张图：`src` 是原书里的条目名（清洗时改过名的是原名），worker 自己从源文件读原图字节。
    /// `to_png`：清洗层改名成 `.png` 的 WebP，读回来先转成 PNG（`webp_to_png`）。
    Image {
        src: &'e str,
        bg: Option<crate::bgfit::BgFit>,
        tile: Option<crate::bgfit::PageTile>,
        flatten: bool,
        darken: bool,
        to_png: bool,
        reply: std::sync::mpsc::Sender<Result<Vec<u8>, BookError>>,
    },
    /// deflate 压一个条目。
    Deflate {
        name: &'e str,
        data: std::borrow::Cow<'e, [u8]>,
        reply: std::sync::mpsc::Sender<Result<crate::epubzip::Precompressed, BookError>>,
    },
}

/// 图片 worker 处理每张图都一样的参数。
#[derive(Clone, Copy)]
struct ImageCtx<'a> {
    input_path: &'a std::path::Path,
    /// 只修复：原图原样收进书里，不解码。
    keep_images: bool,
    is_comic_book: bool,
    screen: crate::imgopt::Screen,
    comic_margin: u32,
    grayscale: bool,
    /// 漫画页排版以外的处理（伽马、拉满对比度、杂点；页码按 `page_cuts` 逐页填）。
    comic: crate::imgopt::ComicPage,
    /// 漫画页码要裁到哪里（原书里的条目名 → 整页坐标的行，[`scan_page_numbers`]）。
    page_cuts: &'a HashMap<String, u32>,
    /// 正文插图中间调加深（`OptimizeOpts::image_gamma`）：只用于 `Job::Image` 的 `darken` 标了的图。
    image_gamma: Option<f64>,
    max_px: u64,
}

/// 图片工作池：`workers` 个线程从同一个队列取活（[`Job`]）——图片处理和 deflate 压缩，结果各自经 `reply` 送回。
/// 每张图开工前按头部像素数向 `budget` 申请额度（[`crate::imgpool`]）。返回发活的一端；它一关（drop），worker 全部退出。
/// 接收端只由 worker 持有：万一 worker 全部退出，发活立刻报错而不是在满队列上永远阻塞。
fn spawn_image_workers<'scope, 'env: 'scope, 'e: 'scope>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    workers: usize,
    queue_len: usize,
    ctx: ImageCtx<'env>,
    budget: std::sync::Arc<crate::imgpool::PixelBudget>,
) -> std::sync::mpsc::SyncSender<Job<'e>> {
    let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<Job<'e>>(queue_len);
    let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));
    for _ in 0..workers {
        let (rx, budget) = (job_rx.clone(), budget.clone());
        scope.spawn(move || {
            // 读原图用的源文件（第一次用到时打开，同阶段一读的是同一个文件）
            let mut source: Option<crate::epubzip::FileZip> = None;
            loop {
                let job = { rx.lock().unwrap_or_else(|e| e.into_inner()).recv() };
                let Ok(job) = job else { break };
                match job {
                    Job::Image {
                        src,
                        bg,
                        tile,
                        flatten,
                        darken,
                        to_png,
                        reply,
                    } => {
                        let _ = reply.send(process_image(
                            &ctx,
                            &budget,
                            &mut source,
                            src,
                            bg,
                            tile,
                            flatten,
                            darken,
                            to_png,
                        ));
                    }
                    Job::Deflate { name, data, reply } => {
                        let _ = reply.send(crate::epubzip::Precompressed::new(name, &data));
                    }
                }
            }
        });
    }
    job_tx
}

/// worker 处理一张图：从源文件读原图（`source` 第一次用到时打开），按需处理。`darken`：正文插图，按 `ctx.image_gamma` 加深中间调
/// （[`crate::imgopt::darken_gray_image`]，只修复的书也做）。
#[allow(clippy::too_many_arguments)]
fn process_image(
    ctx: &ImageCtx,
    budget: &crate::imgpool::PixelBudget,
    source: &mut Option<crate::epubzip::FileZip>,
    src: &str,
    bg: Option<crate::bgfit::BgFit>,
    tile: Option<crate::bgfit::PageTile>,
    flatten: bool,
    darken: bool,
    to_png: bool,
) -> Result<Vec<u8>, BookError> {
    // 整件兜住 panic（读 zip、解析图片都是外部输入）：panic 了只这本书报错，不让 `thread::scope` 收尾时把
    // panic 抛给调用方、摔掉整个 `booklib sync`（2026-10-09 审计）
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<Vec<u8>, BookError> {
            let bytes = match &mut *source {
                Some(z) => Ok(z),
                None => crate::epubzip::open_file_zip(ctx.input_path).map(|z| source.insert(z)),
            }
            .and_then(|z| crate::epubzip::read_by_name(z, src))
            .map_err(|e| e.context("重读图片失败"))?;
            // 改名成 `.png` 的 WebP 先转成 PNG（只修复的书也转：原样留着 Move 上显示不出来；动图取第一帧）；文件头判过，转不成（坏图）的原样写
            let bytes = if to_png {
                crate::imgopt::webp_to_png(&bytes).unwrap_or(bytes)
            } else {
                bytes
            };
            // 整页背景图做成整页一块（平铺背景的阅读器）只修复的书也做：不做的话这张图在阅读器上乱铺，规则就只能去掉、图看不见
            let darken = ctx.image_gamma.filter(|_| darken);
            if ctx.keep_images && tile.is_none() && darken.is_none() {
                return Ok(bytes);
            }
            // 读图片头也是在解析外部输入：兜住 panic（按读不出尺寸算），不让一张坏图摔掉 worker——worker 全摔掉时
            // 主线程要么拿到"线程异常退出"，要么（队列已满时）`send` 永远等不到人收。
            let px = crate::imgopt::guard(|| Some(crate::imgopt::pixel_count(&bytes)))
                .unwrap_or(1_000_000);
            let _permit = budget.acquire(px);
            if let Some(t) = tile {
                return Ok(crate::imgopt::guard(|| {
                    crate::imgopt::page_tile(
                        &bytes,
                        &t,
                        ctx.max_px,
                        crate::imgopt::header_dims(&bytes).map_or_else(
                            || !src.to_ascii_lowercase().ends_with(".png"),
                            |(f, _)| f == image::ImageFormat::Jpeg,
                        ),
                    )
                })
                .unwrap_or(bytes));
            }
            let out = if ctx.keep_images {
                bytes
            } else {
                transform_image_bytes(
                    &bytes,
                    ctx.is_comic_book,
                    ctx.screen,
                    ctx.comic_margin,
                    ctx.grayscale,
                    crate::imgopt::ComicPage {
                        cut_bottom: ctx.page_cuts.get(src).copied(),
                        ..ctx.comic
                    },
                    bg,
                    flatten,
                    ctx.max_px,
                )
                .unwrap_or(bytes)
            };
            Ok(match darken {
                Some(g) => {
                    crate::imgopt::guard(|| crate::imgopt::darken_gray_image(&out, g, ctx.max_px))
                        .unwrap_or(out)
                }
                None => out,
            })
        },
    ))
    .unwrap_or_else(|_| {
        *source = None;
        Err(BookError::Other(format!("处理图片时出错（{src}）")))
    })
}

/// 按条目顺序排队等写进 zip 的东西。
enum Out<'e> {
    /// 图片：等 worker 处理完（条目名用来判断换没换格式）。
    Image(
        &'e str,
        std::sync::mpsc::Receiver<Result<Vec<u8>, BookError>>,
    ),
    /// 等 worker 压好。
    Deflate(std::sync::mpsc::Receiver<Result<crate::epubzip::Precompressed, BookError>>),
    /// 直接写（STORED 的条目）。
    Direct(&'e str, std::borrow::Cow<'e, [u8]>),
    /// 和原书那份逐字节相同的大字体：原样拷原书的压缩数据（第二项是原书里的条目名）。
    Raw(&'e str, &'e str),
    /// 不写（`mimetype` 建 `EpubWriter` 时写过了；推迟的 OPF 最后写），只算进度。
    Skip,
}

/// 写出的 EPUB。
type OutZip = crate::epubzip::EpubWriter<std::io::BufWriter<std::fs::File>>;

/// 按序写出：交给 worker 的结果先后回来，这里按条目原顺序写进 zip（结果与逐条顺序处理、直接写逐字节相同）。
struct OrderedSink<'e, 'w, R: std::io::Read + std::io::Seek, P: FnMut(usize, usize)> {
    queue: std::collections::VecDeque<Out<'e>>,
    zw: &'w mut OutZip,
    /// 原书（[`Out::Raw`] 从这里拷压缩数据）。
    archive: &'w mut ZipArchive<R>,
    is_comic_book: bool,
    /// 换了格式的页：(条目名, 新 media-type)。
    retyped: Vec<(String, &'static str)>,
    /// 排在 `queue` 里、还没写的图片。
    queued_images: usize,
    written: usize,
    total: usize,
    on_progress: P,
}

impl<'e, R: std::io::Read + std::io::Seek, P: FnMut(usize, usize)> OrderedSink<'e, '_, R, P> {
    fn push(&mut self, out: Out<'e>) {
        if matches!(out, Out::Image(..)) {
            self.queued_images += 1;
        }
        self.queue.push_back(out);
    }

    /// 写 `queue` 最前面的：`wait` 为假时只写已经好了的，为真时等它好了再写。返回写没写。
    fn write_front(&mut self, wait: bool) -> Result<bool, BookError> {
        use std::sync::mpsc::TryRecvError;
        let Some(front) = self.queue.front() else {
            return Ok(false);
        };
        match front {
            Out::Image(name, rx) => {
                let out = match if wait {
                    rx.recv().map_err(|_| TryRecvError::Disconnected)
                } else {
                    rx.try_recv()
                } {
                    Ok(out) => out?,
                    Err(TryRecvError::Empty) => return Ok(false),
                    Err(TryRecvError::Disconnected) => {
                        return Err(BookError::Other(format!("图片处理线程异常退出（{name}）")))
                    }
                };
                if may_retype(self.is_comic_book, name) {
                    if let Some(mt) = crate::imgopt::converted_media_type(name, &out) {
                        self.retyped.push((name.to_string(), mt));
                    }
                }
                self.zw.put(name, &out)?;
                self.queued_images -= 1;
            }
            Out::Deflate(rx) => {
                let z = match if wait {
                    rx.recv().map_err(|_| TryRecvError::Disconnected)
                } else {
                    rx.try_recv()
                } {
                    Ok(z) => z?,
                    Err(TryRecvError::Empty) => return Ok(false),
                    Err(TryRecvError::Disconnected) => {
                        return Err(BookError::Other("压缩线程异常退出".to_string()))
                    }
                };
                self.zw.put_precompressed(z)?;
            }
            Out::Direct(name, data) => self.zw.put(name, data)?,
            Out::Raw(name, src) => {
                let f = self
                    .archive
                    .by_name(src)
                    .map_err(|e| BookError::zip(format!("重读 {name}"), e))?;
                self.zw.raw_copy_as(f, name)?;
            }
            Out::Skip => {}
        }
        self.queue.pop_front();
        self.written += 1;
        (self.on_progress)(self.written, self.total);
        Ok(true)
    }

    /// 写掉已经好了的；排队的超过 `max_queued` 时等最前面的好了再写。
    fn drain_ready(&mut self, max_queued: usize) -> Result<(), BookError> {
        loop {
            let wait = self.queue.len() > max_queued;
            if !self.write_front(wait)? {
                return Ok(());
            }
        }
    }

    /// 全部写完（逐个等）。
    fn flush(&mut self) -> Result<(), BookError> {
        while self.write_front(true)? {}
        Ok(())
    }
}

/// 阶段二：按 `entries` 顺序变换、写出全部条目。
///
/// 并行（见 `imgpool`）：主线程按条目顺序变换文字条目，把图片处理和 deflate 压缩交给 worker（[`spawn_image_workers`]），再按原顺序
/// 找页码时扫完的一页：(第几张, 候选)。
type Scanned = (usize, Option<crate::comiccrop::PageNumber>);

/// 漫画页码（`comic_page_numbers`）：整卷每张页图找像页码的块（[`crate::imgopt::comic_page_number`]），整卷投票
/// （[`crate::comiccrop::page_number_cuts`]）定下要裁的页。分批读（一批 worker 数的两倍张）、批内多线程，不把整卷图同时放在内存里。
/// 返回原书条目名 → 裁到哪一行。
fn scan_page_numbers(
    input_path: &std::path::Path,
    names: &[&str],
    area: crate::imgopt::Screen,
    opts: &OptimizeOpts,
) -> Result<HashMap<String, u32>, BookError> {
    // 每个线程自己开一份源文件、从同一个计数器领页（同阶段二的图片 worker），一张图读完就处理、处理完就丢
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = crate::imgpool::worker_count().clamp(1, names.len().max(1));
    let parts: Vec<Result<Vec<Scanned>, BookError>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| -> Result<Vec<Scanned>, BookError> {
                    let mut zip = crate::epubzip::open_file_zip(input_path)?;
                    let mut got = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(name) = names.get(i) else { break };
                        let bytes = crate::epubzip::read_by_name(&mut zip, name)?;
                        got.push((
                            i,
                            crate::imgopt::guard(|| {
                                crate::imgopt::comic_page_number(
                                    &bytes,
                                    area,
                                    opts.comic_trim_specks,
                                    opts.limits.max_decode_pixels,
                                )
                            }),
                        ));
                    }
                    Ok(got)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(BookError::Other("找页码的线程异常退出".to_string())))
            })
            .collect()
    });
    let mut cands = vec![None; names.len()];
    for part in parts {
        for (i, c) in part? {
            cands[i] = c;
        }
    }
    let cuts = crate::comiccrop::page_number_cuts(&cands);
    Ok(names
        .iter()
        .zip(cuts)
        .filter_map(|(n, c)| c.map(|c| (n.to_string(), c)))
        .collect())
}

/// 一个已拆好的跨页：原页/原图留给先读的一半，新页/新图给后读的一半。
struct SpreadSplit {
    page: String,
    image: String,
    new_page: String,
    new_image: String,
    new_page_bytes: Vec<u8>,
    left: Vec<u8>,
    right: Vec<u8>,
    media_type: &'static str,
}

/// 跨页拆分（2026-11-10，profile `comic_split_spreads`）：在阶段二写出前，把漫画里的横向跨页
/// （纯图页、非封面、非装饰小图、宽/高 ≥ [`crate::imgopt::SPREAD_MIN_ASPECT`]）正中拆成两张竖页。
/// 原页/原图条目名留给先读的一半，新页/新图用 `<原名>-eink2.<ext>`；manifest、spine 在这里补好，
/// OPF 后面照常由 [`finalize_opf`] 推迟写。返回拆出来的图条目名（固定版式百分比写法要用画布尺寸）。
fn split_spread_pages<R: std::io::Read + std::io::Seek>(
    prep: &mut Prepared,
    opts: &OptimizeOpts,
    archive: &mut ZipArchive<R>,
    screen: crate::imgopt::Screen,
) -> Result<Vec<String>, BookError> {
    if !opts.comic_split_spreads || !prep.is_comic_book {
        return Ok(Vec::new());
    }
    let Some(opf_name) = prep.opf_name.clone() else {
        return Ok(Vec::new());
    };
    let Some(opf_idx) = prep.entries.iter().position(|e| e.0 == opf_name) else {
        return Ok(Vec::new());
    };
    let Ok(opf_text) = String::from_utf8(prep.entries[opf_idx].1.clone()) else {
        return Ok(Vec::new());
    };
    if !opf_text.contains("</manifest>") || !opf_text.contains("</spine>") {
        return Ok(Vec::new());
    }
    let opf_dir = crate::epubzip::dir_of(&opf_name).to_string();
    let items = crate::wash::manifest_items(&opf_text);
    let by_id: HashMap<&str, &crate::wash::opf::ManifestItem<'_>> =
        items.iter().map(|i| (i.id, i)).collect();
    let mut spine_pages = Vec::new();
    for t in crate::html::tags(&opf_text) {
        if !t.is_start() || !crate::wash::opf::is_local(t.name, "itemref") {
            continue;
        }
        let Some(idref) = crate::wash::opf::tag_attr(&opf_text[t.start..t.end], "idref") else {
            continue;
        };
        if let Some(it) = by_id.get(idref) {
            spine_pages.push(it.path(&opf_dir));
        }
    }
    let Some(first_spine) = spine_pages.first().cloned() else {
        return Ok(Vec::new());
    };
    let cover = crate::wash::opf::declared_cover(&opf_text).map(|it| it.path(&opf_dir));
    let direction = opts
        .page_direction
        .or(opts.comic_page_direction.filter(|_| prep.is_comic_book))
        .or_else(|| crate::direction::spine_direction(&opf_text));
    let rtl = direction == Some(crate::direction::PageDirection::Rtl);
    let comic_page = crate::imgopt::ComicPage {
        gamma: opts.comic_gamma,
        autocontrast: opts.comic_autocontrast,
        trim_specks: opts.comic_trim_specks,
        no_upscale: !opts.comic_upscale,
        cut_bottom: None,
        dark_bg: opts.comic_dark_bg,
    };
    // 清洗层改过名的图：页面里引用的新名 → 原书里的原名（读原图字节用）
    let src_names: HashMap<String, String> = prep
        .rep
        .wash
        .as_ref()
        .map(|w| {
            w.renamed
                .iter()
                .map(|(old, new)| (new.clone(), old.clone()))
                .collect()
        })
        .unwrap_or_default();
    // 每张图被几个 xhtml 页引用：被不止一页引用的不拆（否则按图名换字节会把别的页也换成半张）
    let mut image_refs: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, data, ish) in &prep.entries {
        if !*ish {
            continue;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            continue;
        };
        for t in crate::html::tags(text) {
            if !t.is_start() {
                continue;
            }
            let tag = &text[t.start..t.end];
            let attr = if crate::wash::opf::is_local(t.name, "img") {
                crate::html::attr(tag, "src")
            } else if crate::wash::opf::is_local(t.name, "image") {
                crate::html::attr(tag, "href").or_else(|| crate::html::attr(tag, "xlink:href"))
            } else {
                None
            };
            if let Some(a) = attr {
                let (path, _) = crate::epubzip::resolve_link(name, a.value);
                image_refs.entry(path).or_default().insert(name.to_string());
            }
        }
    }
    let mut taken: HashSet<String> = prep.entries.iter().map(|e| e.0.clone()).collect();
    let mut seen_images = HashSet::new();
    let mut splits = Vec::new();
    for (page_name, data, ish) in prep.entries.iter() {
        if !*ish || !spine_pages.iter().any(|p| p == page_name) || page_name == &first_spine {
            continue;
        }
        let Ok(page_text) = std::str::from_utf8(data) else {
            continue;
        };
        if !crate::comicfxl::image_only(page_text) {
            continue;
        }
        let mut img = None;
        for t in crate::html::tags(page_text) {
            if !t.is_start() || !crate::wash::opf::is_local(t.name, "img") {
                continue;
            }
            let tag = &page_text[t.start..t.end];
            if let Some(a) = crate::html::attr(tag, "src") {
                let (path, _) = crate::epubzip::resolve_link(page_name, a.value);
                img = Some((path, t.start + a.value_start, t.start + a.value_end));
                break;
            }
        }
        let Some((img_path, src_start, src_end)) = img else {
            continue;
        };
        if Some(&img_path) == cover.as_ref()
            || image_refs
                .get(&img_path)
                .is_some_and(|pages| pages.len() > 1)
            || !seen_images.insert(img_path.clone())
        {
            continue;
        }
        let Some(img_idx) = prep.entries.iter().position(|e| e.0 == img_path) else {
            continue;
        };
        if !prep.entries[img_idx].1.is_empty() {
            continue;
        }
        let src_img = src_names
            .get(&img_path)
            .map(String::as_str)
            .unwrap_or(&img_path);
        let Some((iw, ih)) = read_image_head(archive, src_img, |b| {
            crate::imgopt::guard(|| crate::imgopt::display_dims(b))
        })
        .ok()
        .flatten() else {
            continue;
        };
        // 整张和拆出来的半张都别是装饰小图（半张只裁边、不缩放补白，按画布写 100% 会拉伸）
        if iw.min(ih) < screen.width / 3
            || (iw / 2).min(ih) < screen.width / 3
            || !crate::imgopt::is_spread(iw, ih)
        {
            continue;
        }
        let bytes = crate::epubzip::read_by_name(archive, src_img)?;
        let Some((left, right)) = crate::imgopt::prepare_comic_spread(
            &bytes,
            screen,
            opts.comic_margin,
            opts.grayscale,
            comic_page,
            opts.limits.max_decode_pixels,
        ) else {
            continue;
        };
        let new_page = split_entry_name(&mut taken, page_name, "xhtml");
        let ext = crate::util::image_kind(&left).map_or_else(
            || crate::util::image_ext_of(&img_path),
            |k| k.ext.to_string(),
        );
        let new_image = split_entry_name(&mut taken, &img_path, &ext);
        let media_type = crate::util::image_kind(&left)
            .map_or_else(|| crate::util::image_media_type_of_ext(&ext), |k| k.mime);
        let href = crate::epubzip::href_to(crate::epubzip::dir_of(page_name), &new_image, "");
        let new_page_bytes = crate::html::apply_edits(
            page_text,
            vec![(src_start, src_end, crate::util::xml_escape(&href))],
        )
        .into_bytes();
        splits.push(SpreadSplit {
            page: page_name.clone(),
            image: img_path,
            new_page,
            new_image,
            new_page_bytes,
            left,
            right,
            media_type,
        });
    }
    if splits.is_empty() {
        return Ok(Vec::new());
    }
    // manifest：新页、新图各一项；id 避开 OPF 里已有的
    let mut ids: HashSet<String> = items.iter().map(|i| i.id.to_string()).collect();
    let mut next_id = 0usize;
    let mut fresh_id = |prefix: &str| loop {
        let id = format!("{prefix}-{next_id}");
        next_id += 1;
        if ids.insert(id.clone()) {
            return id;
        }
    };
    let mut manifest_specs: Vec<(String, String, &'static str)> = Vec::new();
    let mut spine_adds = Vec::new();
    let mut retyped = Vec::new();
    for s in &splits {
        let page_id = fresh_id("eink-spread-page");
        let img_id = fresh_id("eink-spread-img");
        manifest_specs.push((
            page_id.clone(),
            crate::epubzip::href_to(&opf_dir, &s.new_page, ""),
            "application/xhtml+xml",
        ));
        manifest_specs.push((
            img_id,
            crate::epubzip::href_to(&opf_dir, &s.new_image, ""),
            s.media_type,
        ));
        spine_adds.push((s.page.clone(), page_id));
        if let Some(it) = items.iter().find(|it| it.path(&opf_dir) == s.image) {
            if it.media_type != s.media_type {
                retyped.push((s.image.clone(), s.media_type));
            }
        }
    }
    let new_items: Vec<crate::wash::opf::NewItem<'_>> = manifest_specs
        .iter()
        .map(|(id, href, media_type)| crate::wash::opf::NewItem {
            id: id.as_str(),
            href: href.as_str(),
            media_type,
            properties: "",
        })
        .collect();
    let Some(opf) = crate::wash::opf::insert_manifest_items(&opf_text, &new_items) else {
        return Ok(Vec::new());
    };
    let opf = if retyped.is_empty() {
        opf
    } else {
        crate::optimize::html_pass::set_manifest_media_types(&opf, &opf_name, &retyped)
    };
    let Some(opf) = add_spread_spine_items(&opf, &opf_dir, &spine_adds) else {
        return Ok(Vec::new());
    };
    prep.entries[opf_idx].1 = opf.into_bytes();
    // 原页照旧（先读的一半），新页紧跟；原图字节换成先读的一半，新图条目紧跟。
    let by_page: HashMap<&str, &SpreadSplit> =
        splits.iter().map(|s| (s.page.as_str(), s)).collect();
    let by_img: HashMap<&str, &SpreadSplit> =
        splits.iter().map(|s| (s.image.as_str(), s)).collect();
    let old = std::mem::take(&mut prep.entries);
    let mut out = Vec::with_capacity(old.len() + splits.len() * 2);
    for (name, data, ish) in old {
        if let Some(s) = by_img.get(name.as_str()) {
            let (first, second) = if rtl {
                (&s.right, &s.left)
            } else {
                (&s.left, &s.right)
            };
            out.push((name, first.clone(), false));
            out.push((s.new_image.clone(), second.clone(), false));
        } else if let Some(s) = by_page.get(name.as_str()) {
            out.push((name, data, ish));
            out.push((s.new_page.clone(), s.new_page_bytes.clone(), true));
        } else {
            out.push((name, data, ish));
        }
    }
    prep.entries = out;
    prep.rep.total_files += splits.len() * 2;
    prep.rep.html_files += splits.len();
    let mut split_images = Vec::with_capacity(splits.len() * 2);
    for s in &splits {
        split_images.push(s.image.clone());
        split_images.push(s.new_image.clone());
    }
    Ok(split_images)
}

/// 新条目名：同目录、原名去扩展名 + `-eink2`（重名就 `-eink3`…），扩展名用 `ext`。
fn split_entry_name(taken: &mut HashSet<String>, name: &str, ext: &str) -> String {
    let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    for n in 2.. {
        let file = format!("{stem}-eink{n}.{ext}");
        let path = if dir.is_empty() {
            file
        } else {
            format!("{dir}/{file}")
        };
        if taken.insert(path.clone()) {
            return path;
        }
    }
    unreachable!()
}

/// 在 OPF spine 里紧跟原页插一个新 itemref，并去掉原页 itemref 上的 `page-spread-*` 属性 / `properties` 值。
/// `adds` 是（原页 zip 路径，新页 itemref 的 idref）；有一项在 spine 里找不到、或原页 itemref 不是自闭合又没有
/// 配对的结束标签，就返回 `None`（不改）。
pub(super) fn add_spread_spine_items(
    opf: &str,
    opf_dir: &str,
    adds: &[(String, String)],
) -> Option<String> {
    let items = crate::wash::manifest_items(opf);
    let path_by_id: HashMap<&str, String> = items.iter().map(|i| (i.id, i.path(opf_dir))).collect();
    let mut edits = Vec::new();
    let mut matched = 0usize;
    for t in crate::html::tags(opf) {
        if !t.is_start() || !crate::wash::opf::is_local(t.name, "itemref") {
            continue;
        }
        let tag = &opf[t.start..t.end];
        let Some(idref) = crate::wash::opf::tag_attr(tag, "idref") else {
            continue;
        };
        let Some(path) = path_by_id.get(idref) else {
            continue;
        };
        let Some((_, new_id)) = adds.iter().find(|(p, _)| p == path) else {
            continue;
        };
        matched += 1;
        for a in crate::html::attrs(tag) {
            let local = a
                .name
                .rsplit(':')
                .next()
                .unwrap_or(a.name)
                .to_ascii_lowercase();
            if local.starts_with("page-spread-") {
                edits.push(remove_attr_edit(t.start, tag, &a));
                continue;
            }
            if !a.is("properties") {
                continue;
            }
            // EPUB 3 的写法：page-spread-left/right、rendition:page-spread-* 是 properties 里的值
            let words: Vec<&str> = a.value.split_whitespace().collect();
            let kept: Vec<&str> = words
                .iter()
                .copied()
                .filter(|w| {
                    let local = w.rsplit(':').next().unwrap_or(w).to_ascii_lowercase();
                    !local.starts_with("page-spread-")
                })
                .collect();
            if kept.len() == words.len() {
                continue;
            }
            if kept.is_empty() {
                edits.push(remove_attr_edit(t.start, tag, &a));
            } else {
                edits.push((
                    t.start + a.value_start,
                    t.start + a.value_end,
                    kept.join(" "),
                ));
            }
        }
        let insert_at = if tag.trim_end().ends_with("/>") {
            t.end
        } else {
            // `<itemref …></itemref>`：新 itemref 要插在结束标签后面，不能塞进原 itemref
            let close = crate::html::find_close(opf, t.end, t.name)?;
            close.end
        };
        let linear = crate::html::attr(tag, "linear")
            .map(|a| format!(" linear=\"{}\"", a.value))
            .unwrap_or_default();
        let el = t.name;
        edits.push((
            insert_at,
            insert_at,
            format!("<{el} idref=\"{new_id}\"{linear}/>"),
        ));
    }
    if matched != adds.len() {
        return None;
    }
    edits.sort_by_key(|e| e.0);
    Some(crate::html::apply_edits(opf, edits))
}

/// 删一个属性（连属性前的空白一起删，别留下两个空格）；`tag_start` 是标签在整份 OPF 里的起点。
fn remove_attr_edit(
    tag_start: usize,
    tag: &str,
    a: &crate::html::Attr<'_>,
) -> (usize, usize, String) {
    let before = &tag[..a.start];
    let ws = before.len() - before.trim_end().len();
    (tag_start + a.start - ws, tag_start + a.end, String::new())
}

/// 取回写 zip（[`OrderedSink`]）。图片提前提交 `lookahead` 张，worker 自己打开源文件读原图字节（原图在 zip 里是压缩的时，解压整卷漫画在
/// 主线程上要近一秒），处理与写盘重叠。要 deflate 的条目（文字、样式、字体……）也交给 worker 压（[`crate::epubzip::Precompressed`]：
/// 文字多的书 deflate 占主线程两三成），写进 zip 时原样拷压好的数据。结果与逐条顺序处理、直接写逐字节相同。
#[allow(clippy::too_many_arguments)]
fn write_entries<R: std::io::Read + std::io::Seek>(
    input_path: &std::path::Path,
    prep: &Prepared,
    opts: &OptimizeOpts,
    plan: &WritePlan,
    xf: &mut EntryXform,
    zw: &mut OutZip,
    archive: &mut ZipArchive<R>,
    on_progress: impl FnMut(usize, usize),
    cancel: &dyn Fn() -> bool,
) -> Result<Written, BookError> {
    let entries = &prep.entries;
    let workers = crate::imgpool::worker_count();
    let lookahead = workers + 2;
    // 已交出去、还没写进 zip 的条目最多这么多（压好的、处理好的结果在这里排队等前面的写完）
    let max_queued = lookahead;
    let image_positions = &plan.image_positions;
    // 页码按整卷投票定位置：写出前先把整卷页图扫一遍（只读不改）
    let page_cuts = if plan.is_comic_book && opts.comic_page_numbers && !opts.keeps_content() {
        let names: Vec<&str> = image_positions
            .iter()
            .map(|&i| plan.src(&entries[i].0))
            .collect();
        // 取消只在主线程查（回调不能跨线程）：扫之前、扫完各查一次
        check_cancel(cancel)?;
        let cuts = scan_page_numbers(input_path, &names, plan.screen, opts)?;
        check_cancel(cancel)?;
        cuts
    } else {
        HashMap::new()
    };
    let ctx = ImageCtx {
        input_path,
        keep_images: opts.keeps_content(),
        is_comic_book: plan.is_comic_book,
        screen: plan.screen,
        comic_margin: opts.comic_margin,
        grayscale: opts.grayscale,
        comic: crate::imgopt::ComicPage {
            gamma: opts.comic_gamma,
            autocontrast: opts.comic_autocontrast,
            trim_specks: opts.comic_trim_specks,
            no_upscale: !opts.comic_upscale,
            cut_bottom: None,
            dark_bg: opts.comic_dark_bg,
        },
        page_cuts: &page_cuts,
        image_gamma: opts.image_gamma,
        max_px: opts.limits.max_decode_pixels,
    };
    let mut deferred_opf_bytes: Option<Vec<u8>> = None;
    let mut sink = OrderedSink {
        queue: Default::default(),
        zw,
        archive,
        is_comic_book: plan.is_comic_book,
        retyped: Vec::new(),
        queued_images: 0,
        written: 0,
        total: entries.len(),
        on_progress,
    };
    std::thread::scope(|scope| -> Result<(), BookError> {
        let budget = std::sync::Arc::new(crate::imgpool::PixelBudget::new(
            opts.limits.pool_pixel_budget,
        ));
        let job_tx = spawn_image_workers(scope, workers, lookahead, ctx, budget);
        let mut pending: std::collections::VecDeque<
            std::sync::mpsc::Receiver<Result<Vec<u8>, BookError>>,
        > = std::collections::VecDeque::new();
        let mut next_submit = 0usize;
        let mut consumed = 0usize; // 已取回的图片数：第 consumed 张图片对应 image_positions[consumed]
                                   // 一批最多这么多个条目、这么多字节的 html
        const PREFIX_BATCH: usize = 32;
        const PREFIX_BATCH_BYTES: usize = 2 << 20;
        let (mut prefix_end, mut prefixes) = (0usize, Vec::new().into_iter());
        for (i, (name, data, ish)) in entries.iter().enumerate() {
            check_cancel(cancel)?;
            // 交出去还没写的图片已经有 `lookahead` 张：先等前面的写掉，腾出位置（至少能交这一张）
            while sink.queued_images >= lookahead {
                sink.write_front(true)?;
            }
            // 补满提前量（原图字节由 worker 自己从源文件读）
            while pending.len() + sink.queued_images < lookahead
                && next_submit < image_positions.len()
            {
                let img_name = &entries[image_positions[next_submit]].0;
                let (tx, rx) = std::sync::mpsc::channel();
                // 清洗时改过名的（文件名有安卓存储不能用的字符、转成 PNG 的 WebP）按原名回原书读
                let src = plan.src(img_name);
                let to_png = src != img_name.as_str()
                    && crate::util::image_ext_of(src) == "webp"
                    && crate::util::image_ext_of(img_name) == "png";
                let job = Job::Image {
                    src,
                    bg: prep.bg_fits.get(img_name.as_str()).copied(),
                    tile: prep.page_tiles.get(img_name.as_str()).copied(),
                    flatten: opts.flatten_alpha && prep.body_imgs.contains(img_name.as_str()),
                    darken: prep.body_imgs.contains(img_name.as_str()),
                    to_png,
                    reply: tx,
                };
                job_tx
                    .send(job)
                    .map_err(|_| BookError::Other("图片处理线程已退出".to_string()))?;
                pending.push_back(rx);
                next_submit += 1;
            }
            let is_image = image_positions.get(consumed) == Some(&i);
            // html 章节变换的前几步（各章独立）一批一批预先多线程算好；分批是为了不让整本书的文字多出一份
            if i == prefix_end {
                let mut bytes = 0usize;
                prefix_end = i;
                while prefix_end < entries.len()
                    && prefix_end - i < PREFIX_BATCH
                    && (prefix_end == i || bytes < PREFIX_BATCH_BYTES)
                {
                    bytes += if entries[prefix_end].2 {
                        entries[prefix_end].1.len()
                    } else {
                        0
                    };
                    prefix_end += 1;
                }
                prefixes = xf.html_prefixes(&entries[i..prefix_end]).into_iter();
            }
            let prefix = prefixes.next().flatten();
            let out = match xf.transform_text(name, data, *ish, prefix) {
                None if is_image => {
                    consumed += 1;
                    Out::Image(
                        name,
                        pending
                            .pop_front()
                            .ok_or_else(|| BookError::Other("图片队列意外为空".to_string()))?,
                    )
                }
                // `mimetype`（阶段一放在第一个）建 `EpubWriter` 时已经写了
                _ if name == "mimetype" => Out::Skip,
                t => {
                    let t = t.unwrap_or(std::borrow::Cow::Borrowed(data.as_slice()));
                    if plan.deferred_opf == Some(name.as_str()) {
                        deferred_opf_bytes = Some(t.into_owned());
                        Out::Skip
                    } else if crate::util::is_image_ext(name) {
                        Out::Direct(name, t)
                    } else if let Some(src) = Some(plan.src(name)).filter(|src| {
                        plan.font_src.get(*src).is_some_and(|&(crc, size)| {
                            t.len() >= RAW_FONT_MIN
                                && t.len() as u64 == size
                                && crc32fast::hash(&t) == crc
                        })
                    }) {
                        Out::Raw(name, src)
                    } else {
                        let (tx, rx) = std::sync::mpsc::channel();
                        job_tx
                            .send(Job::Deflate {
                                name,
                                data: t,
                                reply: tx,
                            })
                            .map_err(|_| BookError::Other("压缩线程已退出".to_string()))?;
                        Out::Deflate(rx)
                    }
                }
            };
            sink.push(out);
            sink.drain_ready(max_queued)?;
        }
        sink.flush()?;
        drop(job_tx); // 关闭队列，worker 退出，scope 才能 join
        Ok(())
    })?;
    Ok(Written {
        deferred_opf_bytes,
        retyped: sink.retyped,
    })
}

/// 收尾：抓到的远程图（与引用它的章同目录、src 已改本地名）→ 推迟的 OPF（[`finalize_opf`]）→ 漫画的页边距标记 → 幂等标记。
fn finalize(
    zw: &mut OutZip,
    prep: &Prepared,
    opts: &OptimizeOpts,
    plan: &WritePlan,
    xf: &EntryXform,
    written: Written,
) -> Result<(), BookError> {
    for (path, bytes) in &xf.fetched_imgs {
        zw.put(path, bytes)?;
    }
    if let (Some(name), Some(bytes)) = (plan.deferred_opf, written.deferred_opf_bytes) {
        zw.put(name, &finalize_opf(name, bytes, xf, &written.retyped))?;
    }
    if let (true, Some(m)) = (prep.is_comic_book, opts.comic_reader_margins) {
        zw.put(READER_MARGINS_MARKER, m.to_string().as_bytes())?;
    }
    zw.put(
        OPTIMIZE_MARKER,
        marker_value(opts.wash.is_some()).as_bytes(),
    )
}

/// 推迟写的 OPF 的最终内容：补上抓到的远程图的 manifest 项、换了格式的页改 media-type、按各章最终内容标 manifest 的 `properties`。
/// 不是 UTF-8 的原样写。
fn finalize_opf(
    name: &str,
    bytes: Vec<u8>,
    xf: &EntryXform,
    retyped: &[(String, &'static str)],
) -> Vec<u8> {
    match String::from_utf8(bytes) {
        Ok(text) => {
            let text = if xf.fetched_imgs.is_empty() {
                text
            } else {
                add_manifest_items(&text, name, &xf.fetched_imgs)
            };
            let text = if retyped.is_empty() {
                text
            } else {
                set_manifest_media_types(&text, name, retyped)
            };
            let props = xf.content_props.as_ref().and_then(|p| {
                crate::wash::normalize::apply_content_properties(
                    &text,
                    crate::epubzip::dir_of(name),
                    p,
                )
            });
            props.unwrap_or(text).into_bytes()
        }
        Err(e) => e.into_bytes(),
    }
}

/// 原样拷原书压缩数据的字体至少这么大（小字体重压也就几毫秒，不值得多算一遍 CRC）。
const RAW_FONT_MIN: usize = 64 << 10;

/// 字体文件（按扩展名）。
fn is_font_name(name: &str) -> bool {
    matches!(
        crate::util::image_ext_of(name).as_str(),
        "ttf" | "otf" | "ttc" | "woff" | "woff2"
    )
}

/// 写图注宽度（[`crate::capfit`]）要的全书信息：带图注的 `<img>` 引用的图的显示宽高（按原书只读这几张图的文件头），
/// 全书样式表和 `<style>` 里给图片定的宽高。
fn caption_ctx<R: std::io::Read + std::io::Seek>(
    entries: &[(String, Vec<u8>, bool)],
    src_names: &HashMap<&str, &str>,
    archive: &mut ZipArchive<R>,
) -> Result<crate::capfit::Ctx, BookError> {
    let images: HashSet<&str> = entries
        .iter()
        .filter(|(_, _, ish)| !*ish)
        .map(|(n, _, _)| n.as_str())
        .collect();
    let mut ctx = crate::capfit::Ctx::default();
    for (name, data, ish) in entries {
        let Ok(text) = std::str::from_utf8(data) else {
            continue;
        };
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
            let src = src_names
                .get(path.as_str())
                .copied()
                .unwrap_or(path.as_str());
            // 只读文件头（[`read_image_head`]），不为它整张解压
            if let Some(d) = read_image_head(archive, src, |b| {
                crate::imgopt::guard(|| crate::imgopt::display_dims(b))
            })
            .map_err(|e| e.context("重读图片失败"))?
            {
                dims.insert(path, d);
            }
        }
    }
    Ok(ctx)
}

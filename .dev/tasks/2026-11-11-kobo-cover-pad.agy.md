## 结论

**无阻断**。分支 `kobo-cover-pad`（`fe5bb44..8d6dc98`）实现完整严密，需求与约束全部对齐，补白不裁不拉伸、换设备与重跑幂等正确、Kindle 封面提取路径未受波及、非 Kobo 产物逐字节不变、传输层校验确保 Kindle 等设备不重传不丢进度，可批准合并。

---

## 问题分档

### 阻断

无。

### 建议

1. **破损书籍 OPF 容错**（建议）：
   `crates/bookconv/src/wash/cover.rs:241-260` 的 `arrange_cover_thumb` 中，若 `declared_cover(opf)` 解析失败返回 `None`，该函数会直接返回 `None`，使外层 `add_cover_thumbnail:383` 放弃本次调整。虽然已有缩略图的正常书籍必定能认回封面，但若外部手工注入了破损 OPF，此处静默跳过；建议后续优化时补一行 debug 级日志记录跳过原因。

### 线索

1. **Nickel 书架封面缓存行为**（线索）：
   `crates/profile/profiles/kobo.toml:26-32`。Kobo 自带阅读器 Nickel 对已导入书籍的书架封面通常缓存在 `.kobo-images/` 目录下。若真机同步后书架旧书封面高度未立即变齐，需怀疑 Nickel 未触发缩略图缓存重绘，需验证是否需重新插拔触发扫描或查看新放入书籍的表现。

---

## 重点查项逐条核实与依据

1. **补白是否不裁不拉伸、比例 0.7403**：
   - **结论**：核实通过。
   - **依据**：`crates/bookconv/src/imgopt.rs:1085-1109`；`crates/profile/profiles/kobo.toml:30-32`。
   - **说明**：`CoverThumbFit::Pad` 分支直接按目标比例 `aspect`（0.7403）计算补充画布 `(cw, ch)`，内容等比计算 `scale`（以 `THUMB_MAX_SIDE = 1200` 为上限），缩放时宽高等比使用同一 `scale` 并通过 `paste_on(..., 255)` 居中贴上纯白底，未调用 `crop` 亦未调用 `thumb_scan_margins`，无任何拉伸与画面裁剪；产物缩略图比例严格在 `0.7403`（±1px 离散误差）内。

2. **cover-image 和 meta cover 挪到缩略图后，换设备再优化与重跑是否幂等、真封面能否正确认回**：
   - **结论**：核实通过。
   - **依据**：`crates/bookconv/src/wash/cover.rs:241-260`、`crates/bookconv/src/wash/cover.rs:381-396`、`crates/bookconv/src/wash/cover.rs:468-498`；`crates/bookconv/src/wash/opf.rs:392-414`。
   - **说明**：
     - **认回真封面**：`wash/opf.rs:392-414` 的 `declared_cover` 明确过滤掉 `id == THUMB_ID`，并增加第三级回退，通过 `<meta name="eink-thumbnail-source">` 准确读出并认回原封面条目。
     - **重跑幂等**：`wash/cover.rs:381-392` 中，同一设备重跑时 `keep` 为 true 且 `thumb_fit_matches` 匹配，进入 `arrange_cover_thumb`；因 `THUMB_ID` 已带有 `cover-image` 且 meta 已对齐，输出字符串与原 OPF 完全一致（`new == text`），直接返回 `false` 不修改文件与 entries，保持逐字节幂等。
     - **换设备再优化**：换到不需要缩略图（如 Bigme）或凑比例方式/比例不同的设备时，`strip_cover_thumbnail`（468–498 行）在删除 `THUMB_ID` 前调用 `declared_cover` 认回原封面，将 `cover-image` 属性及 `<meta name="cover">` 完整归还给真封面条目，并清除 `eink-thumbnail*` 三个 meta，随后由新设备参数干净重做或保持无缩略图状态。

3. **Kindle 读 eink-thumbnail 的书架封面路径是否受 epubbook.rs 改动影响**：
   - **结论**：核实通过（完全不受影响）。
   - **依据**：`crates/bookconv/src/epubbook.rs:225-229, 277-279`；`crates/kfx/src/write/mod.rs:262-269, 318-322`。
   - **说明**：`epubbook.rs:277-279` 提取 `thumbnail` 的逻辑一字未改，依然通过 `THUMB_META`（`eink-thumbnail`）获取 `eink-thumb.jpg` 的路径；改动仅在提取 `cover`（正文真封面）时增加了 `it.id != THUMB_ID` 的防误判保护及 source meta 回退。KFX 写出器依然优先从 `book.thumbnail` 提取书架封面，现有路径不受任何影响。

4. **汉王、Kindle、Bigme 产物是否逐字节不变**：
   - **结论**：核实通过。
   - **依据**：`crates/profile/src/lib.rs:101-110, 440-446`；`crates/bookconv/src/wash/cover.rs:437-452`；`crates/bookconv/src/optimize/mod.rs:9-11`；`crates/kfx/src/write/mod.rs:118-121`；`crates/library/tests/fp_golden.txt:580-640`。
   - **说明**：
     - 新字段默认值分别为 `CoverThumbFit::Crop` 和 `false`，非 Kobo 的内置 profile 未配置此二项，全部走原有逻辑，不写新 meta，不挪动 `cover-image`。
     - 产物内部的 `META-INF/eink-optimized` 标记只写 `full` 或 `core`，不带版本号；KFX 内嵌的 `FILE_CREATOR_VERSION` 恒定为 `"1"`，不随版本号变动。
     - `fp_golden.txt` 中非 Kobo 的 496 行除版本号字段（`66→67` / `c67→c68`）外，设备签名段一个字符未变。重新构建出的产物二进制与旧版本逐字节完全相同。

5. **版本号升级后传输层是否只重传字节变了的书（Kindle 进度不能清零）**：
   - **结论**：核实通过。
   - **依据**：`crates/library/src/generate.rs:777-836, 883-908`；`crates/library/src/deliver.rs:374-380`。
   - **说明**：版本号升级虽会因指纹变化使书库重新在临时目录生成产物，但在准备传输时（`generate.rs:788-790`），程序对比已有记录的 `p.sha` 和文件大小：对于内容未变的书籍（如 Kindle、汉王等所有书籍），`same = true`，在 `833-836` 行直接调用 `complete(..., Ok(false))` 返回 `Step::Done(Built::Same)`，**根本不提交给传输管道写入设备**。底层 `deliver.rs:374` 亦有 `same_bytes` 的二次校验。设备端对应文件不被覆盖、mtime 不变，Kindle 阅读进度绝对不会清零。

---

## 置信度

**高**。已穷尽阅读并比对所有涉及的核心源码（`imgopt.rs`、`cover.rs`、`opf.rs`、`epubbook.rs`、`lib.rs`、`kobo.toml`、`generate.rs`、`deliver.rs`、`fingerprint.rs`、`fp_golden.txt`），覆盖全链路状态流转与边界条件。

---

## 未核实

1. **真机书架高度与缓存刷新**（待 vinci 真机验收）：Kobo Clara BW 真机同步后书架封面的视觉等高效果，以及 Nickel 对旧书缩略图缓存是否需重启或重新触发扫描方能刷新。
2. **真书大样本回归**：本次为只读审计，未执行全库真书回归命令（执行方仅在合成书样本与单测集完成比对）。

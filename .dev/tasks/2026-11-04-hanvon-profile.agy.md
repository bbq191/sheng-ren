## 结论

本次改动整体合规且未触碰硬约束，代码解析与校验逻辑正常；无阻断性缺陷，但存在两处测试与注释层面的**建议**，以及关于 `comic_fixed_layout` 和投递路径的两条真机行为**线索**。

---

## 问题分档

### 阻断
无。

### 建议
1. **测试未闭环验收标准**
   - **依据**：`.dev/tasks/2026-11-04-hanvon-profile.md:50`、`crates/profile/src/lib.rs:1108`、`crates/library/tests/cli.rs:254`
   - **分析**：任务单验收标准明确列有“功能：`booklib` 认出 `hanvon` 设备，`--device=hanvon` 可用，产物为 EPUB，尺寸按 1072×1448”。但现有改动中仅在 `builtin_profiles_parse` 断言了 id 列表，在 `fp_golden.rs` 计算了指纹变异字符串；`crates/profile/src/lib.rs:1425` 当初为 `kobo` 补充了属性断言，而 `hanvon` 未写；`crates/library/tests/cli.rs:254` 的 `devices_lists_builtin_targets` 仍只断言了 `ireader` 与 `kindle`；整个测试集没有端到端执行过 `--device=hanvon` 构建，未实际断言产物扩展名为 `.epub` 且图像按 1072×1448 缩放。建议补齐基础断言。
2. **Profile 注释关于 `notes` 的参照描述不准确**
   - **依据**：`crates/profile/profiles/hanvon.toml:4` 对照 `crates/profile/profiles/ireader.toml:13`
   - **分析**：`hanvon.toml` 注释写道 `notes = "jump" # 推断：最稳（同 kobo/ireader）`。实际上 `ireader.toml` 中 `notes = "popup"`（掌阅已实测支持弹窗）；`notes = "jump"` 仅与 `kobo` 一致。建议修正注释文字，避免后续误导。

### 线索
1. **`comic_fixed_layout = true` 在普通 EPUB 设备上的兼容性与裁切隐患**
   - **依据**：`crates/profile/profiles/hanvon.toml:17,298` 对照 `crates/profile/profiles/ireader.toml:39`、`crates/bookconv/src/comicfxl.rs:59-64,78,109`
   - **分析**：同为 Android 系统的普通 EPUB 设备掌阅 `ireader` 显式未开 `comic_fixed_layout`（默认为 `false`，由阅读器流式排版铺满）。而在普通 EPUB 上设 `comic_fixed_layout = true` 会触发 `comicfxl` 向 OPF 注入 `rendition:layout: pre-paginated`，并将单图页面 `<img>` 的样式强行硬编码为 `style="width:1072px;height:1448px"`。若汉王自带阅读器对 EPUB 3 固定版式支持不佳，或无法全屏沉浸（存在顶部状态栏/底部导航栏占高），绝对像素尺寸极易引发右侧/底部裁切或破版。真机到手后应将其列为漫画排版的首要验证项。
2. **`[deliver]` 路径依赖外部 MTP 规则与存储实际结构**
   - **依据**：`crates/profile/profiles/hanvon.toml:305-307` 对照 `crates/library/src/deliver.rs:109-121`
   - **分析**：`mount = "hanvon"` 与 `dir = "documents"` 在代码逻辑上完全合法（可被 `deliver.rs` 正确解析），但前提依赖本机环境的 udev/jmtpfs 将汉王挂载到 `$XDG_RUNTIME_DIR/mtp/hanvon`，且汉王内置存储确实将 `documents` 作为自带阅读器扫描目录。真机首次连接前需确认外部挂载脚本配置。

---

## 重点审查结论逐条核对

1. **`[deliver]` 的 mtp 写法和 mount、dir 是否和 ireader 一致、能被 deliver.rs 正确使用**：
   - **一致且可用**。结构与 [ireader.toml](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/ireader.toml#L96-L99) 保持一致，通过 [Profile::validate](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L726-L737) 校验，[deliver.rs](file:///home/afu/Projects/sheng-ren/crates/library/src/deliver.rs#L109-L121) 能正确拼接挂载路径 `$BOOKLIB_MTP_DIR/hanvon` 并定位内部存储下的 `documents`（若目录缺失会在写入时自动创建）。
2. **`comic_fixed_layout`、`comic_readable` 等漫画字段放在 EPUB 设备上是否合法、有没有意外效果**：
   - **配置合法，但存在渲染意外效果风险**。Profile 语法和校验规则不限制 EPUB 设备使用固定版式。意外效果在于：普通 EPUB 产物会被注入 EPUB 3 `pre-paginated` 声明，单图页 `<head>` 会被注入固定视口，`<img>` 会被强制写入内联样式 `style="width:1072px;height:1448px"`。若阅读器无法全屏隐藏系统栏或对 EPUB 3 FXL 支持异常，会导致裁切。
3. **没写 em_px、line_px、content_px 时 kindle_rules 会不会出错或退回不合理的缺省**：
   - **不会出错，退回行为合理**。
     - `crates/profile/src/lib.rs:672-679` 仅在 `line_height_numbers = false` 时强制要求 `line_px`/`em_px`（汉王缺省为 `true`，校验通过）；
     - `crates/bookconv/src/optimize/mod.rs:507` 中，因 `em_px` 为 `None` 且 `image_sizing` 缺省为 `Css`，跨设备竖直间距缩放（`v_scale`）、水平缩放（`h_scale`）与边框几何（`box_geometry`）均保持为 `None`，不触发数值折算，间距按 CSS 保持 1:1 原样；
     - `content_px` 缺省为 `None` 时代码回退使用阅读范围宽度（1072px），不影响文字对比度、正文字体去除、body 外边距重置等通用 `kindle_rules` 逻辑，表现与 [kobo.toml](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/kobo.toml#L11-L24) 一致。
4. **测试是否足以证明验收标准**：
   - **不足以证明**。现有测试仅覆盖了注册表 id 存在性与指纹变异金样，没有断言 CLI `devices` 包含 `hanvon`，更没有集成测试真正为 `hanvon` 构建产物并断言格式为 EPUB 及分辨率为 1072×1448。

---

## 置信度

**高**。结论基于 `crates/profile`、`crates/bookconv`、`crates/library` 源码调用链路及 TOML 字段的反序列化与校验逻辑逐行推导，无需依赖运行期假设。

---

## 未核实

1. **未在物理真机上连接验证**：汉王 Clear 6 Turbo 的 USB 厂商 ID/产品 ID 识别、jmtpfs 实际挂载点命名、内部存储根目录结构及实际存书目录名。
2. **未在汉王自带阅读器上验证渲染**：EPUB 3 固定版式（`pre-paginated` + 绝对像素样式）与单图整屏铺满在真机上的视觉表现、系统状态栏是否占用有效显示区、`jump` 注释跳转响应。

## 结论

**无阻断**。历经 3 轮审查返工，分支状态机与边界处理逻辑严密收敛；重点检查的地区判定边界、重找循环与同套覆盖风险、指纹标记按书隔离、原书 RTL 保留均已彻底解决并通过完整的离线单元与流程测试验证，满足合并要求。

---

## 重点检查项核对

1. **地区判定的边界**：
   - 纯函数 `region_of` 严格遵循约束：仅 `platform == "漫画"` 生效，其余返回空串；优先判定 `meta_tags`，落入地区词表恰好一个时直接采用，存在两个以上冲突地区时判定失败返回空串；`meta_tags` 无地区词时退回 `tags` 累加各地区 `count`，唯一最高者胜出，并列最高或 count 为 0 均返回空串。
   - 依据：[`crates/library/src/bangumi.rs:185-255`](file:///home/afu/Projects/sheng-ren/crates/library/src/bangumi.rs#L185-L255)，测试见 [`crates/library/src/bangumi.rs:363-464`](file:///home/afu/Projects/sheng-ren/crates/library/src/bangumi.rs#L363-L464)。
2. **needs_metadata 与 series_lookup 重找防循环与同套卷继承**：
   - 防死循环：`needs_metadata` 仅对 `source.starts_with("Bangumi") && region.is_none()` 触发强行重找；重找遇 `NotFound` 时旧记录原地补记 `region = Some("")`，`info_checked` 置为 `LOOKUP_VERSION`，后续 `needs_metadata` 即返回 `false`，彻底避免每次 `sync` 重复联网。网络临时故障（`Failed` / `Err`）不写 `Some("")`，保证网络恢复后可重试。
   - 防同套误判：`series_lookup` 将候选卷拆为 6 档优先级，判出地区的卷（`region` 非空）永远优先于判不出的卷（`Some("")`）；且抄取未记地区的旧记录时原样保留 `region: None`，不在 `series_lookup` 阶段草率标记判不出，避免因 `list()` 顺序随机导致日漫某卷永久判错。
   - 依据：[`crates/library/src/metadata.rs:429-433`](file:///home/afu/Projects/sheng-ren/crates/library/src/metadata.rs#L429-L433)、[`488-508`](file:///home/afu/Projects/sheng-ren/crates/library/src/metadata.rs#L488-L508)、[`543-601`](file:///home/afu/Projects/sheng-ren/crates/library/src/metadata.rs#L543-L601)、[`605-616`](file:///home/afu/Projects/sheng-ren/crates/library/src/metadata.rs#L605-L616)，测试见 [`crates/library/src/metadata.rs:1108-1313`](file:///home/afu/Projects/sheng-ren/crates/library/src/metadata.rs#L1108-L1313)。
3. **指纹标记是否只加在强制 rtl 的书上**：
   - 门禁严格：`comic_direction` 校验设备开关 `comic.meta_direction`、是否漫画、Bangumi 来源及 `region == Some("日本")` 4 重条件，仅全满足才在 `generate.rs` 的指纹 `info` 段尾部追加标记字符 `r`。
   - 设备段不增加字符：`crates/library/src/fingerprint.rs` 未做改动；金样测试 `fp_golden.txt` 旧行 0 变动，仅新增 8 行 `comic_meta_direction` mutation，且指纹与 `none` 完全一致，避免了全部已有漫画白白重建。
   - 依据：[`crates/library/src/generate.rs:133-148`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L133-L148)、[`crates/library/src/metadata.rs:641-648`](file:///home/afu/Projects/sheng-ren/crates/library/src/metadata.rs#L641-L648)、[`crates/library/tests/fp_golden.rs:190-194`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.rs#L190-L194)、[`crates/library/tests/fp_golden.txt:359-360`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L359-L360)、[`479-480`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L479-L480)。
4. **原书 rtl 是否被保留**：
   - `produce` 中 `opts.page_direction` 来自 `comic_direction`，不满足强制 RTL 时取 `None`；`bookconv` 优化器在 `opts.page_direction` 为 `None` 且 `comic_page_direction` 为空时不对 OPF `<spine>` 属性进行重写，原书 RTL 完整保留。
   - 依据：[`crates/library/src/generate.rs:923-925`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L923-L925)、[`crates/bookconv/src/optimize/mod.rs:1058-1060`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/mod.rs#L1058-L1060)、[`1258-1260`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/mod.rs#L1258-L1260)，端到端测试见 [`crates/library/tests/flow.rs:2818-2872`](file:///home/afu/Projects/sheng-ren/crates/library/tests/flow.rs#L2818-L2872)。

---

## 问题分档

### 阻断
无。

### 建议
1. **Bangumi tag 名称去空白容错**：[`crates/library/src/bangumi.rs:228-233`](file:///home/afu/Projects/sheng-ren/crates/library/src/bangumi.rs#L228-L233) 中遍历 `item["tags"]` 时，tag 名直接通过 `tag["name"].as_str().and_then(region_word)` 匹配词表常量，未做首尾空白修剪；建议对齐同文件 [`matching_hits:284`](file:///home/afu/Projects/sheng-ren/crates/library/src/bangumi.rs#L284) 加入 `.map(str::trim)`，防止上游 API 偶发带有空格导致漏判。

### 线索
1. **极简元数据书籍的指纹字符串拼接**：[`crates/library/src/generate.rs:144-148`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L144-L148) 当原书未补任何元数据且无封面时，`info` 段基准值为 `"-"`，追加后变成 `"-r"`；虽与金样机制无冲突且能唯一定位状态，后续如在此段追加其他独立标记，需留意分隔符可读性。

---

## 置信度

**高**。理由：所有改动文件及关联引用链路均经源码逐行核验，状态转移覆盖 Found/NotFound/Shared/Transient-Error 四类路径，针对阻断 1~4 的变异测试完备，测试用例均基于离线 JSON 字面量与纯内存伪造，不依赖外部网络与 GPL 代码。

---

## 未核实

1. **Kobo Clara BW（Nickel）真机 RTL 表现**：KEPUB 产物中的 spine rtl 在 Nickel 阅读器上的触摸翻页响应（点击左侧是否为下一页）未上真机验证。
2. **汉王 Clear 6 Turbo 真机 RTL 表现**：自带阅读器对 EPUB spine rtl 的翻页方向支持未上真机验证。
3. **掌阅 Air 3 Pro 满屏排版交互**：RTL 书在掌阅自带阅读器中每本手动点击「本书自定义 → 排版 → 漫画」以去除四周留白并实现整页铺满的操作体验待真机确认。
4. **全库真书回归产物**：金样文件比对已确认旧规则产物指纹 0 变动，但真书产物逐字节对拍待本机全量回归确认。

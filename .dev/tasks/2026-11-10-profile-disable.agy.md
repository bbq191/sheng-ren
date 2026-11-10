## 结论

**无阻断问题。** 分支 `feat/profile-disable`（`aa86ba4..aa90114`）实现干净克制，规格与测试覆盖完备，数据风险与旧记录表现已在文档与返工测试中写准测透，限定范围外文件（`fp_golden.txt`、内置 profiles、`install.sh`、`crates/library/src/`）均未改动，可以合并。

---

## 问题分档

- **建议**：[`docs/devices.md:201`](file:///home/afu/Projects/sheng-ren/docs/devices.md#L201) 中关于 `--keep` 的报告符号表述可进一步纠准。
  - 文档写道「带 `--keep` 的那一遍就能看到 `✗ 不再跟踪…` 的报告」，实际在带 `--keep` 运行时命令行输出的是 `? 不再跟踪（--keep，这次没从书库删）`（前缀为 `?`），不带 `--keep` 真正删书时输出的才是 `✗ 不再跟踪，书库里删了（连同产物）`。虽不影响对不可逆数据风险的理解，但修正符号可避免用户对照终端输出时产生困惑。
- **线索**：[`crates/profile/src/lib.rs:938-950`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L938-L950) 的 TOML 预检容错机制。
  - `Registry::with_dir` 先用 `toml::from_str::<toml::Table>(&text)` 预检。若文件语法损坏（无法解析成合规 Table），会自然流转到后续 `Profile::parse` 报告原始 TOML 解析错误，不会将带有 `disabled` 且语法错误的配置误认，行为安全，符合预期。

---

## 依据

1. **只写 `disabled = true` 才停用、别的写法报错且信息清楚、不存在的 id 忽略**
   - 逻辑依据：[`crates/profile/src/lib.rs:938-949`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L938-L949)。严格判定 `table.len() == 1 && disabled.as_bool() == Some(true)`；多写键或非布尔 true 时报错且带有路径与改法说明；不存在的 id 调用 `BTreeMap::remove` 返回 `None` 并安全忽略。
   - 测试依据：[`crates/profile/src/lib.rs:1489-1541`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1489-L1541)（分别覆盖停用内置、多写键报错、`disabled = false` 报错、未知 id 忽略）；[`crates/library/tests/cli.rs:270-293`](file:///home/afu/Projects/sheng-ren/crates/library/tests/cli.rs#L270-L293)（覆盖 `devices` 列表过滤和 `--device=ireader` 报错）。
2. **没有 `disabled` 的覆盖行为不变**
   - 逻辑依据：[`crates/profile/src/lib.rs:950-952`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L950-L952)。不含 `disabled` 键的文件正常进入 `Profile::parse` 并插入注册表。
   - 测试依据：原有覆盖测试 [`crates/profile/src/lib.rs:1458-1476`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1458-L1476) 原样保留且逻辑未动。
3. **停用后 `sources.rs` 已有清理对显式条目 / 只给它的条目 / 全部设备条目的处理**
   - 逻辑依据：[`crates/library/src/sources.rs:754-771`](file:///home/afu/Projects/sheng-ren/crates/library/src/sources.rs#L754-L771)、[`crates/library/src/sources.rs:375-383`](file:///home/afu/Projects/sheng-ren/crates/library/src/sources.rs#L375-L383)。
     - **全部设备条目**：`e.devices` 为空，`gone` 为空，在 764–766 行直接保留，不会被清空或具象化为剩余模式列表。
     - **显式多设备条目**：`without_devices` 过滤掉已停用 id，保留其余模式，条目继续保留。
     - **只给它的条目**：过滤后为空，`without_devices` 返回 `None`，整条条目移除，其下的书若无其它条目覆盖则转入不再跟踪处理。
   - 测试依据：[`crates/library/tests/flow.rs:2717-2793`](file:///home/afu/Projects/sheng-ren/crates/library/tests/flow.rs#L2717-L2793) 完整模拟了甲（部分包含）、乙（仅包含）、丙（全部设备）三种条目，验证了事件触发与序列化后的 `sources.json` 内容。
4. **`sync --keep` 时跟踪条目已被改写的数据风险在 docs 里是否写准**
   - 依据：[`docs/devices.md:200-202`](file:///home/afu/Projects/sheng-ren/docs/devices.md#L200-L202)。文档已明确写准：停用后首次 `sync`（无论是否带 `--keep`）就会将跟踪条目修改持久化进 `sources.json`；`--keep` 仅推迟删书操作；删掉 toml 仅恢复模式本身，不能自动找回已被清理的跟踪条目；前置操作建议先 `booklib track` 记录。
5. **`output-state` 旧记录在 `list` 里的表现和 docs 是否一致**
   - 依据：[`docs/devices.md:203`](file:///home/afu/Projects/sheng-ren/docs/devices.md#L203)、[`crates/library/tests/flow.rs:2794-2826`](file:///home/afu/Projects/sheng-ren/crates/library/tests/flow.rs#L2794-L2826)。留下的书若有已停用模式的历史产物记录，`booklib list` 会列出 `<id> ? 未知 <旧路径>` 且退出码为 0；挪走 state 文件后输出恢复干净，与文档描述一致。
6. **范围约束核验（未改文件）**
   - `crates/library/tests/fp_golden.txt`：未修改（[`crates/library/tests/fp_golden.txt:1`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L1)）。
   - 内置 profile：`crates/profile/profiles/ireader.toml` 未修改（[`crates/profile/profiles/ireader.toml:1`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/ireader.toml#L1)）。
   - 安装脚本：`install.sh` 未修改（[`install.sh:1`](file:///home/afu/Projects/sheng-ren/install.sh#L1)）。
   - 核心源码：`crates/library/src/` 下任何文件均未修改。
   - 提交范围严格受限在 `crates/profile/src/lib.rs`、`crates/library/tests/cli.rs`、`crates/library/tests/flow.rs`、`docs/devices.md` 与任务单五处。

---

## 置信度

**高**。已完整审查提交链与全部变动文件的源码与测试实现，逻辑分支清晰闭环，边界行为测试完备，与既有架构契约无冲突。

---

## 未核实

- 本次二审严格遵守只读约束，未在终端执行任何构建或测试命令，全量测试（789 passed）与 clippy 0 警告依赖负责 bot 的复验记录。
- vinci 本机实机环境的书库清理命令（任务单中明确作为人工步骤留给 vinci 执行）。

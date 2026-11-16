# 二审 .dev/tasks/2026-11-16-tests-out-three.md

- 仓库：/home/box/wt-tot
- 分支：task/2026-11-16-tests-out-three（对比 master）
- 时间：2026-11-16T17:06:32+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：67s
- 退出码：0

## 结论

建议通过。本次改动严格遵循「只挪测试代码、不改行为、生产代码不动」的原则，将 `footnote.rs`、`kindle_rules.rs` 和 `metadata.rs` 中的单元测试模块按规范拆分并外移至对应的子模块文件，原文件均显著降至 1800 行以下，模块路径与测试名称保持不变。改动范围清晰受控，未触及生产代码与涉及文件范围之外的文件，符合任务单与 clean-room 约束。

## 问题分档

### 阻断

无

### 建议

无

### 线索

无

## 依据

- `crates/bookconv/src/htmlproc/footnote.rs:1107-1111`：测试模块改为 `mod footnote_inline_tests;` 与 `mod optimizer_footnote_tests;`，原测试分别移动至 `footnote/footnote_inline_tests.rs` 与 `footnote/optimizer_footnote_tests.rs`，原文件缩减至 1111 行。
- `crates/bookconv/src/wash/kindle_rules.rs:1243-1244`：测试模块改为 `mod tests;`，原测试移动至 `kindle_rules/tests.rs`，原文件缩减至 1244 行。
- `crates/library/src/metadata.rs:1009-1010`：测试模块改为 `mod tests;`，原测试移动至 `metadata/tests.rs`，原文件缩减至 1010 行。
- `.dev/tasks/2026-11-16-tests-out-three.md:101-108`：负责 bot 在独立 worktree 完成了测试路径清单一致性（644 项不变）、字符串字面量全量比对、产物 sha256 逐字节对照（12 份产物一致）及 `cargo test --workspace`、`clippy -D warnings` 验证，并如实注明未跑真书回归、未上真机。

## 置信度

高。Diff 全量核对完毕，所有改动均为纯粹的测试模块外移及缩进调整，测试路径与模块层级完全一致，未见任何逻辑变更与超范围修改。

## 未核实

- 二审处于只读模式，未运行 `cargo test`、`cargo clippy` 及跨分支二进制产物 SHA256 逐字节比对，以上依赖负责 bot 验收记录与静态代码审查。

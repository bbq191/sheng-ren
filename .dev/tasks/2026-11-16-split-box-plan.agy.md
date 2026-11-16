# 二审 .dev/tasks/2026-11-16-split-box-plan.md

- 仓库：/home/box/wt-bp
- 分支：task/2026-11-16-split-box-plan（对比 master）
- 时间：2026-11-16T17:28:08+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：67s
- 退出码：0

## 结论

建议通过。本次改动严格遵守纯重构约束，将 `wash/box_fix.rs` 中约 880 行的 `plan` 函数抽取为私有子模块 `wash/box_fix/plan.rs`，通过引入私有上下文 `Cx` 将只读闭包转为方法，并将 15 个独立逻辑段按原顺序拆分为私有方法。重构后单个函数最长 122 行（均未超过 150 行），`plan` 主入口仅 68 行。各段内部语句、运算顺序与原逻辑完全一致，未引入额外内存分配或克隆，未放宽任何项的可见性（仅 `plan` 为 `pub(super)` 供 `box_fix.rs` 调用），版本号与 `fp_golden.txt` 未动。负责 bot 已完成 18 本书全模式产物逐字节 sha256 对照与 864 项测试核验，且提交及验收记录中明确注明未跑真书回归、未上真机，符合规范。

## 问题分档

### 阻断

无

### 建议

- `width_to_margins`、`frame_side_borders` 等段落的业务说明注释在方法实现上方与 `plan` 函数初始化中间表处各有保留，存在少量重复。该重复有助于理解上下文且完全不影响代码行为，同意负责 bot 的判定，不阻断合并，后续若统一注释规范可顺手精简。

### 线索

无

## 依据

- `crates/bookconv/src/wash/box_fix/plan.rs:116-123` 与 `crates/bookconv/src/wash/box_fix/plan.rs:2099-2104`：`width_to_margins` 关于 Move 阅读器定宽块折外边距的注释在方法定义处和 `plan` 调用处各保留了一份；
- `crates/bookconv/src/wash/box_fix/plan.rs:22-114`：11 个只读闭包直接挂在 `Cx` 上作为私有方法，仅把对捕获变量的访问改为 `self.ch` / `self.g`，未改变求值逻辑；
- `crates/bookconv/src/wash/box_fix/plan.rs:2073-2098`：`content_before` 的计算在创建 `Cx` 前完成，因其原本就不依赖后续闭包且闭包为惰性求值，移至 `Cx` 构造处传参语义完全等价；
- `crates/bookconv/src/wash/box_fix.rs:29-30`：因测试文件 `tests.rs` 采用 `use super::*;` 引入上级项，在 `box_fix.rs` 中以 `#[cfg(test)]` 补齐 `km` 与 `Chapter` 引用，不扩大生产可见性。

## 置信度

高。任务单要求、代码 diff、AST/词法记号对照以及负责 bot 的独立验证记录均已完整核验，改动范围明确且边界清晰，无隐式副作用。

## 未核实

- 二审受只读约束限制，未在终端独立重新执行 `cargo test`、`cargo clippy` 及多机型产物 sha256 校验脚本，以负责 bot 在独立 worktree 的验收记录为准；
- 按照正文清洗路径惯例，本次改动未跑真书回归、未上真机，需遵照建议由 vinci 后续跑真书回归确认。

# 二审 .dev/tasks/2026-11-13-devices-dir.md

- 仓库：/tmp/sheng-ren-review-devices-dir
- 分支：task/2026-11-13-devices-dir（对比 master）
- 时间：2026-11-13T11:56:11+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：338s
- 退出码：0

## 结论

建议通过。本次改动严格对照了 vinci 批准的方案 A 原文与任务单全部规则，将设备接上判定收拢至只查 `~/Devices/<链接名>` 符号链接并精确比对 `/proc/self/mountinfo` 挂载点（含八进制转义处理），彻底避免在断开或卡死的挂载点上调用 `stat`；Kobo 卷标与链接名完成解耦，掌阅模式保留且预留 `Ireader`；生成记录前缀迁移仅变更记录并不触发重传或删除保护，`on_removable` 双认新旧前缀保证了 `list-no-device-stall` 和 `lock-no-device` 的约束；文档与决策记录完备，版本号与指纹保持不变，测试覆盖充分，未发现阻断性缺陷。

## 问题分档

### 阻断

无

### 建议

- [建议] `crates/library/src/deliver.rs:483-487`：`link_target` 在处理相对符号链接目标时，通过 `link.parent()?.join(raw)` 进行拼接。若将来有特殊测试或外部脚本将 `BOOKLIB_DEVICES_DIR` 配置为纯相对路径（如 `BOOKLIB_DEVICES_DIR=Devices`），且链接目标也是相对路径，拼接后 `abs` 仍为相对路径，经 `lexical_normalize` 规整后无法与 `/proc/self/mountinfo` 中的绝对路径匹配。建议未来可考虑在 `abs.is_relative()` 时与 `std::env::current_dir()` 做后备解析（在当前实现中，`DeviceEnv::from_env` 缺省通过 `$HOME/Devices` 生成绝对路径，因此正常运行不受影响）。

### 线索

- [线索] `crates/library/src/session.rs:188-193`：在 `removable_base` 中，废弃的 `~/Hanvon` 路径通过 `if path.starts_with(&old_hanvon) { bases.push(home); }` 将 `$HOME` 加入候选基准目录，其目的是为了让 `unplugged` 中的 `base.join(first)` 能够拼出 `~/Hanvon`。实现虽然巧妙且经过了前缀长度排序与测试验证，但依赖了「`path` 须先匹配 `old_hanvon` 才动态追加 `home`」的隐式约定，后续维护者调整 `removable_base` 时需留意该项设计。

## 依据

1. `crates/library/src/deliver.rs:483-487`：
   ```rust
   let abs = if raw.is_absolute() {
       raw
   } else {
       link.parent()?.join(raw)
   };
   Some(lexical_normalize(&abs))
   ```
   若 `link` 自身是相对路径，`link.parent()?.join(raw)` 依然是相对路径，无法精确匹配 mountinfo 中的绝对挂载点。
2. `crates/library/src/session.rs:188-193`：
   ```rust
   if let Some(home) = home_dir() {
       let old_hanvon = home.join("Hanvon");
       if path.starts_with(&old_hanvon) {
           bases.push(home);
       }
   }
   ```
   仅当 `path.starts_with(&old_hanvon)` 时才将 `home` 放入 `bases`，以配合 `unplugged` 的 `first` 截取逻辑；逻辑正确但有隐式耦合。

## 置信度

高。已全量阅读任务单全文、所有相关代码改动、单元测试、集成测试及文档决策，并逐行核对 `mountinfo` 八进制解析、符号链接目标规整、前缀迁移与删除保护等核心逻辑。

## 未核实

- 未上真机（根据任务单与 `docs/decisions.md` 规定，本单不碰真机，设备符号链接由外部 dotfiles 管理）。
- 未跑真书全量回归（根据任务单规则 10，本单不影响产物内容及指纹，约定不需要跑真书回归）。

## 结论

**无阻断**。分支 `hanvon-documents-case`（`c3a1616..2cfff91`）实现符合需求与约束，核心删书漏洞已在返工中彻底修复，大小写存储模拟测试充分，其他设备产物与指纹金样保持不变。可由负责 bot 合并后交 vinci 做真机最终验收。

---

## 问题分档

### 阻断

（无）

---

### 建议

1. **`begin_rebase` 标记时机可后置到写记录成功后**
   - **内容**：在 `generate.rs:301`，`begin_rebase` 在检查与写入记录前直接将设备加入 `rebased` 集合。若 `self.records.states.put(&sp, state)` 遇到磁盘 I/O 写入失败，且由 `space_check` 首次触发（转为 `SpaceCheck::Unknown` 警告放行，`generate.rs:552-554`），则后续 `prepare` 再次进入时会因 `begin_rebase` 为 false 跳过迁移，转入挪位置流程（虽有保护不删书，但掩盖了写记录失败）。建议将 `begin_rebase` 的生效推迟到 `put` 成功之后，或在 `put` 报错时将该设备从 `rebased` 中移除。
   - **依据**：[`crates/library/src/generate.rs:301-316`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L301-L316)、[`crates/library/src/session.rs:204-206`](file:///home/afu/Projects/sheng-ren/crates/library/src/session.rs#L204-L206)

---

### 线索

1. **`remove_outputs`、`hand_over_outputs` 不迁移不会误删 `Documents` 根目录**
   - **核实结论**：`remove_outputs` 在删书后向上清理空目录时，循环条件严格受限于 `*d != p.root && d.starts_with(&p.root)`，最顶层只止于旧 `root`（即 `documents`）的子目录，绝不会执行到 `p.root` 本身，更不会删到真名 `Documents`；文件删除走 `remove_product`，在大小写不敏感设备上删除目标书本身符合原件移除语义。`hand_over_outputs` 仅更换记录 key 并清空指纹，不触碰文件与路径。
   - **依据**：[`crates/library/src/generate.rs:417-428`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L417-L428)、[`crates/library/src/generate.rs:1311-1317`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L1311-L1317)

2. **`delete_placements` 新保护在大小写敏感设备上留旧文件符合设计预期**
   - **核实结论**：新保护 `case_alias && !case_twin` 在大小写敏感设备上会使只差大小写父目录的旧位置跳过物理删除并丢弃记录，留下旧文件。由于书库命名冲突判定（`file_name_for`）统一按 lowercase 折叠判重，不存在跨书误伤或命名死锁，满足任务单中"宁可多留一份，不删书"的明确权衡。
   - **依据**：[`crates/library/src/generate.rs:1255-1263`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L1255-L1263)、[`crates/library/src/generate.rs:1356-1389`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L1356-L1389)

3. **Kobo/掌阅「跟踪目录只改大小写」（`Manga`→`manga`）由原先删书变为保留是预期修复**
   - **核实结论**：旧版本在 vfat/Android 存储上遇到子目录仅改大小写时，会因父目录不同绕过 `case_twin` 判定，在挪位置与清理旧位置时误删同一底层文件；新机制通过 `put_file` 的 `case_alias` 强制走临时文件且不删 src，以及 `delete_placements` 跳过物理删除，彻底修复了该暗坑，无现有业务依赖旧的误删行为。
   - **依据**：[`crates/library/src/deliver.rs:210-212`](file:///home/afu/Projects/sheng-ren/crates/library/src/deliver.rs#L210-L212)、[`crates/library/src/generate.rs:1255-1260`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L1255-L1260)

4. **`space_check` 将迁移写记录失败转为 `SpaceCheck::Unknown` 符合模块定位**
   - **核实结论**：`space_check` 属于容量预检，对探测与前置异常统一以 `Unknown` 警告放行；真正的执行入口 `prepare`、`flush_removed`、`retract` 均独立调用 `rebase_state_case` 且用 `?` 严格抛出错误并中断流程，架构处理合理。
   - **依据**：[`crates/library/src/generate.rs:552-554`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L552-L554)、[`crates/library/src/generate.rs:647`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L647)

5. **`case_alias` 分支强制 `direct=false` 与 `keep_src=true` 保证任何情况下不删书**
   - **核实结论**：`deliver.rs:210-212` 在 `case_alias` 成立时强制 `keep_src=true` 且 `direct=false`，保证了复制先入临时文件 `dest.tmp-xxx`，写入并 sync 后才删旧 dest 并原子替换，避免了 `direct=true` 先删 dest 导致同 inode 源文件丢失的问题；同时与仅负责同目录更名的 `case_twin` 边界清晰（`!case_twin` 排他）。
   - **依据**：[`crates/library/src/deliver.rs:210-212`](file:///home/afu/Projects/sheng-ren/crates/library/src/deliver.rs#L210-L212)、[`crates/library/src/deliver.rs:380-438`](file:///home/afu/Projects/sheng-ren/crates/library/src/deliver.rs#L380-L438)

6. **记录迁移覆盖完整且幂等**
   - **核实结论**：`rebase_case_placement` 严格要求 `case_alias(root, new_root)` 且 `new_path.is_file()`，遍历了 `books[].path/root`、`old[].path/root`、`orphans[].path/root` 全部路径；迁移完成后 `root == new_root` 使得 `case_alias` 返回 false，具备天然幂等性。
   - **依据**：[`crates/library/src/generate.rs:1211-1223`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L1211-L1223)、[`crates/library/src/generate.rs:1491-1505`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L1491-L1505)

7. **测试充分性与不变性**
   - **核实结论**：Unix 环境下使用 `symlink` 模拟大小写不敏感文件系统，在单元测试与集成测试中不仅复现并阻断了旧漏洞，还通过验证文件 inode 和 mtime 不变证明了无重新写入；同时包含了真目录（大小写敏感）的隔离对比测试；`fp_golden.txt`、`OPTIMIZE_VERSION` 与非汉王 profile 配置均保持零修改。
   - **依据**：[`crates/library/tests/flow.rs:3180-3209`](file:///home/afu/Projects/sheng-ren/crates/library/tests/flow.rs#L3180-L3209)、[`crates/library/src/deliver.rs:818-855`](file:///home/afu/Projects/sheng-ren/crates/library/src/deliver.rs#L818-L855)

---

## 置信度

- **评级**：**高**
- **理由**：相关逻辑均经源码逐行核对与控制流静态推演，所有引用位置均对应到实际文件与行号，测试用例与异常边界推演完备。

---

## 未核实

- **汉王 Clear 6 Turbo 自带阅读器的阅读进度记录机制**：是否按文件绝对路径、相对路径或内容哈希记录阅读进度，无法通过离线代码与测试核实，依任务单要求留待 vinci 真机验收。

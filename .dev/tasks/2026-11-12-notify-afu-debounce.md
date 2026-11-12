# notify-afu 去抖：只在任务单 done/blocked 或回执改动时唤醒

- Task ID: 2026-11-12-notify-afu-debounce
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: box-agent（负责 bot 直接改）
- 分支: notify-afu-debounce（从最新 master 切出）
- 更新时间: 2026-11-12T14:40+08:00
- 引用: 无

## 需求原文
> 用户 2026-11-12 经 Jarvis 转达：notify-afu 去抖，整文件替换为 vault 模板

## 输入
- 背景：`Projects/工作流.md`「唤醒配置 · A」；vault 正本 `Projects/_templates/notify-afu.yml`。
- 基于 origin/master `15a4cff`。
- 旧 workflow：`.dev/tasks/**` 下匹配文件只要有改动就 POST。
- 正本（Jarvis 修好后）sha256：`2c3f9ea80688adfb0c14772cb91fed985f31806e564841c5cbebfac060dbd206`。

## 约束
- 不做：不改 Secrets 的值，不调用 `gh secret`；Secrets 变量名仍是 `AFU_WEBHOOK_URL` / `AFU_WEBHOOK_TOKEN`。
- 只改 `.github/workflows/notify-afu.yml`（外加本任务单）；整文件替换为 vault 模板，不手改半截。
- 只在上面的分支工作；本单执行期间不 push、不 merge（由负责 bot 验收后再决定）。
- 提交信息用中文，`chore:` / `docs:` 等前缀 + 正文，不加 Co-Authored-By。
- 不读密钥。
- 风险点：无设备行为 / `install.sh` / 数据迁移；唤醒逻辑误判或漏发属于流程正确性，已先独立复核。

## 小改动判断
不完全符合「小改动」（≤ 20 行、只改一个文件、不涉及设备行为 / `install.sh` / 迁移 / 安全）：只改一个 workflow 文件，不涉及上述风险点，但整文件从 46 行换成 89 行，diff 远超 20 行。仍由负责 bot 在临时 worktree 里直接改。

## Jarvis 修模板经过
负责 bot 第一次对照旧 workflow 复核时，发现新分支首次 push 时 GitHub 把 `github.event.before` 设为全 0，当时的模板会走 `git ls-tree` 全量，把历史上全部 `.agy.md`（当时 origin/master 上有 16 份）当作需要唤醒。已停下，没有替换、没有合并。

Jarvis 随后修好 vault 正本：before 为空 / 全 0 / 对象不存在时，`git fetch` 默认分支（`github.event.repository.default_branch`），用 `merge-base(origin/默认分支, AFTER)..AFTER` 做 diff；拿不到默认分支才退回 `ls-tree` 全量。`.agy2.md` 维持现状（约定二次回执覆盖同一 `.agy.md`）。

## 分步任务
1. [ ] 写本任务单并接单（状态 accepted）。
2. [ ] 将 vault 正本整文件复制到 `.github/workflows/notify-afu.yml`（先核对 sha256）。
3. [ ] YAML 语法检查；与模板逐字节 `diff`；人工审读判断逻辑。
4. [ ] 填「结果」，状态改为 done。

## 验收标准
- [ ] `.github/workflows/notify-afu.yml` 与 vault 正本逐字节一致（sha256 为 `2c3f9ea8…d206`）
- [ ] YAML 语法合法；Secrets 名仍为 `AFU_WEBHOOK_URL` / `AFU_WEBHOOK_TOKEN`
- [ ] 复核与模拟：任务分支首推只含 pending/accepted 不 POST；首推含新 done 单只发该单；后续只改 `.agy.md` 会发；后续 done→verified 不发；在 sheng-ren 真实历史上从 origin/master 切出的新分支首推不会发出 16 份历史回执
- [ ] 不跑 `cargo test` / `clippy`：本改动不涉及 Rust 代码，只改 GitHub Actions YAML，理由写进「结果」

## 交付物
分支上的 workflow 与任务单；「结果」填好；状态改为 done。本单先不 push、不 merge。

## 给执行方的指令
```
由负责 bot 在 git worktree 里直接改：接单、整文件替换 workflow、填结果。不改 Secrets，不跑 cargo。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：
- base..head：
- 提交：| 步骤 | 提交号 | 文件 | 说明 |
- 测试：| 命令 | 通过 / 失败 / 未跑 | 摘要 |
- 遇到的问题：
- 偏离：无
- 未完成：

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：本改动是 CI 唤醒去抖，不涉及 Rust / 设备行为，不跑 agy。
- 合并：

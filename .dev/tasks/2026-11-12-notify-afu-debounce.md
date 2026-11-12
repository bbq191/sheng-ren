# notify-afu 去抖：只在任务单 done/blocked 或回执改动时唤醒

- Task ID: 2026-11-12-notify-afu-debounce
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent（负责 bot 直接改）
- 分支: notify-afu-debounce（从最新 master 切出）
- 更新时间: 2026-11-12T14:42+08:00
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
1. [x] 写本任务单并接单（状态 accepted）。
2. [x] 将 vault 正本整文件复制到 `.github/workflows/notify-afu.yml`（先核对 sha256）。
3. [x] YAML 语法检查；与模板逐字节 `diff`；人工审读判断逻辑。
4. [x] 填「结果」，状态改为 done。

## 验收标准
- [x] `.github/workflows/notify-afu.yml` 与 vault 正本逐字节一致（sha256 为 `2c3f9ea8…d206`）
- [x] YAML 语法合法；Secrets 名仍为 `AFU_WEBHOOK_URL` / `AFU_WEBHOOK_TOKEN`
- [x] 复核与模拟：任务分支首推只含 pending/accepted 不 POST；首推含新 done 单只发该单；后续只改 `.agy.md` 会发；后续 done→verified 不发；在 sheng-ren 真实历史上从 origin/master 切出的新分支首推不会发出 16 份历史回执
- [x] 不跑 `cargo test` / `clippy`：本改动不涉及 Rust 代码，只改 GitHub Actions YAML，理由写进「结果」

## 交付物
分支上的 workflow 与任务单；「结果」填好；状态改为 done。本单先不 push、不 merge。

## 给执行方的指令
```
由负责 bot 在 git worktree 里直接改：接单、整文件替换 workflow、填结果。不改 Secrets，不跑 cargo。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：box-agent（负责 bot 直接改）/ 2026-11-12 14:36–14:41（Asia/Shanghai）
- base..head：`15a4cff..`（含审查提交；基线为 origin/master `15a4cff`）
- 提交：
  | 步骤 | 提交号 | 文件 | 说明 |
  |---|---|---|---|
  | 接单 | `39ed09e` | `.dev/tasks/2026-11-12-notify-afu-debounce.md` | 状态 pending 未单独提交，直接 accepted |
  | 替换 workflow | `3d3d2e2` | `.github/workflows/notify-afu.yml` | 整文件复制 vault 正本，sha256 `2c3f9ea80688adfb0c14772cb91fed985f31806e564841c5cbebfac060dbd206` |
  | 结果 | `fdfcf52` | `.dev/tasks/2026-11-12-notify-afu-debounce.md` | 填结果，状态 done |
  | 审查 | 本提交 | `.dev/tasks/2026-11-12-notify-afu-debounce.md` | 填写审查要点；状态保持 done，不改为 verified |
- 测试：
  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  |---|---|---|
  | `python3 -c 'import yaml; yaml.safe_load(...)'` | 通过 | 语法合法。PyYAML 1.1 会把顶层键 `on` 读成布尔 `True`，这是 GitHub Actions 的常见写法，与旧文件相同，不影响运行 |
  | `diff` / `cmp` / `sha256sum` 与 vault 正本 | 通过 | 逐字节一致 |
  | box 上模拟收集脚本（从 YAML 抽出的原文） | 通过 | 见「审查」；真实历史首推不会发出 16 份回执 |
  | `cargo test --workspace` / `clippy` | 未跑 | 不涉及 Rust 代码，只改 GitHub Actions YAML |
- 遇到的问题：第一次复核时正本仍会在新分支首推时误发历史回执，已停下未替换。Jarvis 修好正本（sha256 `2c3f9ea8…d206`）后重新复核并替换。
- 偏离：无
- 未完成：按指示先不 push、不 merge；状态停在 done，待负责 bot 写完审查后再决定推送与合并。

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（状态保持 done，按指示不改为 verified，先不 push、不 merge）。证据如下。
  1. **与模板一致**：`cmp` / `diff` 为空；sha256 均为 `2c3f9ea80688adfb0c14772cb91fed985f31806e564841c5cbebfac060dbd206`（`2c3f9ea8…d206`）。
  2. **YAML 语法**：`yaml.safe_load` 成功。顶层键 `on` 在 PyYAML 1.1 里会变成布尔 `True`，这是 GitHub Actions 常见写法，旧文件也如此，GitHub 侧能正确识别。
  3. **Secrets**：仅引用 `secrets.AFU_WEBHOOK_URL`、`secrets.AFU_WEBHOOK_TOKEN`，未改值，未调用 `gh secret`。
  4. **checkout**：`actions/checkout@v4` + `fetch-depth: 0`，足以在当前分支上回溯到与默认分支的 merge-base。脚本在 before 不可用时再执行 `git fetch --no-tags origin "+refs/heads/$DEFAULT_BRANCH:refs/remotes/origin/$DEFAULT_BRANCH"`，refspec 正确。
  5. **判断逻辑（人工审读）**：
     - 有 before 时：`git diff --name-only $BEFORE $AFTER -- .dev/tasks`；状态变为 `done`/`blocked`，或路径匹配 `*.agy.md` 才保留。`pending`/`accepted`/`verified`、只改措辞不发。done 改为 verified 不发。新增已是 done 的文件会发。
     - before 为空 / 全 0 / 对象不存在：先清空 BEFORE，再取 merge-base(origin/默认分支, AFTER)，然后等同「有 before」的两段 diff（即 merge-base..AFTER），只看本分支相对分叉点的改动。
     - fetch 或 merge-base 失败才退回 `ls-tree` 全量（保守，可能误发，不会因此漏发）。
     - 文件名正则仍是 `(\.agy)?\.md`，`.agy2.md` 不发，与约定「二次回执覆盖同一 `.agy.md`」一致。
     - `set -euo pipefail`：fetch / merge-base 都有 `|| true`；`grep` 无匹配有 `|| true`；`git show | grep | head` 也有 `|| true`，避免 SIGPIPE 导致意外退出。循环把结果写到临时文件，不依赖管道子壳的变量。模拟中空 diff、状态变更、首推等路径均 exit 0。
  6. **box 模拟**（用 YAML 抽出的原文脚本；新分支首推模拟「只 fetch 任务分支、脚本再 fetch 默认分支」，比 GitHub 的 fetch-depth 0 更苛刻）：
     - T1 首推只含 pending/accepted → paths 为空
     - T2 首推含新 done 单 → 只发该单
     - T3 后续只改 `.agy.md` → 只发该回执
     - T4 后续 done 改为 verified → paths 为空
     - T5 在 sheng-ren 真实历史（`15a4cff`，树上 16 份 `.agy.md`）切出新分支并加上 pending 任务单后首推 → paths 为空，merge-base 等于 master tip，没有发出 16 份历史回执
     - T5b 同一历史上首推新 done 单 → 只发该单
     - T7 相同 BEFORE/AFTER → paths 为空，未意外退出
     - T8 后续 pending 改为 done → 只发该单
     - T9 故意让默认分支 fetch 失败 → 退回全量，exit 0（保守误发，符合模板注释）
     - T10 只改 `.agy2.md` → paths 为空
  7. **非阻断残余**：若以后把 fetch-depth 改成 1，浅克隆会导致 merge-base 失败并退回全量（T6 已复现）。保持 fetch-depth 为 0 即可。全新仓库第一次 push 且该分支就是默认分支时，merge-base 等于 AFTER，diff 为空，极端情况下可能漏发；sheng-ren 已有历史，不适用。
- 二审：本改动是 CI 唤醒去抖，不涉及 Rust / 设备行为，不跑 agy。
- 合并：未推送、未合并。worktree 留在 `/tmp/sr-notify`，分支 `notify-afu-debounce`。等确认后再 push，并用 `--no-ff` 合进 master。

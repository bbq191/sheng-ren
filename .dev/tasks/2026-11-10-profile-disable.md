# 书库 profiles/ 里写 `disabled = true` 停用一个阅读模式（先用来去掉掌阅）

- Task ID: 2026-11-10-profile-disable
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: feat/profile-disable（从 master `aa86ba4` 切出）
- 更新时间: 2026-11-10T16:26+08:00
- 引用: 无

## 需求原文
> vinci 2026-11-10：「一并删掉,从 booklib 的设备列表里把它去掉」（掌阅 Air 3 Pro 已退货）
>
> 2026-11-10 定的做法（sheng-ren 给的三种里选 B1）：内置的 ireader profile 留在仓库里，书库 `profiles/ireader.toml` 写一行 `disabled = true` 把它从设备列表去掉。

## 输入
- 背景：vault `Projects/sheng-ren/context.md`（2026-11-10「掌阅 Air 3 Pro 已退货」）；仓库 `docs/devices.md` 的「自定义模式」一条（约 198 行）
- 现状：
  - 设备列表＝内置 profile（`crates/profile/build.rs` 把 `crates/profile/profiles/*.toml` 编进程序，`Registry::builtin`，`crates/profile/src/lib.rs:901`）加上书库 `profiles/*.toml`（`Registry::with_dir`，`lib.rs:914`，由 `crates/library/src/lib.rs:370` 调用）；同 id 以书库里的为准。**书库这边只能加、只能覆盖，去不掉内置的**；`booklib devices` 列的就是这个合集
  - 模式不在了以后的清理已经有了（2026-10-31 删 xochitl 时做的，`601f5b8`）：`sync` 开头把跟踪条目里已经没有的模式 id 去掉（`crates/library/src/sources.rs:754`，事件 `SyncEvent::ForgotDevices`；一台不剩的条目整条去掉，里面的书按不再跟踪处理），「全部设备」的条目（`devices` 为空）不受影响；已删模式的 `output-state/<id>.json` 能容忍（测试 `crates/library/tests/flow.rs:2632` `removed_mode_leftovers_are_cleaned_up`）
  - 现有测试：`crates/profile/src/lib.rs` 的 `dir_overrides_and_adds`（约 1440 行，临时目录写法照它）；`crates/library/tests/cli.rs:249` `devices_lists_the_builtin_modes`
- 涉及文件：
  - `crates/profile/src/lib.rs`（修改：`Registry::with_dir` 认 `disabled = true`；单元测试；模块头注释提一句）
  - `crates/library/tests/cli.rs`（修改：加 `booklib devices` 不列停用模式的用例）
  - `crates/library/tests/flow.rs`（修改：加停用后跟踪条目清理、`output-state` 残留的用例）
  - `docs/devices.md`（修改：「自定义模式」一条后面加一段「停用内置模式」）

## 约束
- 规格（照这个做，不要自己加）：
  1. `Registry::with_dir` 读到一个 `profiles/<id>.toml`，内容（按 TOML 解析后）**只有** `disabled = true` 一个键时：从注册表里去掉 `<id>`（内置的、或者同目录里别的文件加的——实际上一个 id 只有一个文件，所以就是去掉内置的那个），不再当 profile 解析
  2. 有 `disabled` 键、但还有别的键，或者 `disabled` 不是 `true`（`false`、字符串等）：返回 `Err`，信息里写文件路径，并说清楚「停用一个模式时文件里只写 `disabled = true` 一行；要用这个模式就删掉 `disabled`」
  3. 没有 `disabled` 键：和现在完全一样（照常 `Profile::parse`，未知字段照常报错）
  4. `<id>` 不是已有模式（内置里没有）却写了 `disabled = true`：**忽略**（什么都不做、不报错）。理由：`with_dir` 没有报警告的通道，停用一个本来就没有的模式结果也正确；用户用 `booklib devices` 就能看到结果。docs 里写明
  5. 隐藏文件、非 `.toml` 文件照旧跳过
- 不做：不删、不改内置 `crates/profile/profiles/ireader.toml`；不改其它 profile 的值；不改 `install.sh`；不加 `booklib` 新命令；不改 `sources.rs` 的清理逻辑（已有的就够，用测试证明）；不动生成、传输的代码
- `crates/library/tests/fp_golden.txt` 必须不变（指纹按内置 profile 算，这个改动不碰）
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` / `docs:` / `test:` 等前缀 + 正文，不加 Co-Authored-By；正文写「不影响产物，没跑真书回归」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码
- 只对改动的 Rust 文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：数据（停用后，**只给这个模式**的跟踪条目会整条去掉，里面没被别的条目覆盖的书下次 `sync` 从书库删掉——原件不动，这是 2026-10-31 已定的行为）。docs 那一段要写明：停用前先 `booklib track` 看有没有只给它的条目，先跑 `booklib sync --keep` 看报告

## 分步任务
1. [ ] `crates/profile/src/lib.rs` 的 `Registry::with_dir`：读到文件文本后，先 `toml::from_str::<toml::Table>`（或等价写法）看有没有 `disabled` 键：只有它且值是 `true` → `r.profiles.remove(id)`，`continue`；有它但不合规 → `Err`（规格 2 的信息）；没有它 → 照旧 `Profile::parse`。函数文档注释、模块头注释（`lib.rs:9` 一带）补一句。改动尽量少（目标 ≤ 20 行，不算测试）
2. [ ] 同文件单元测试（照 `dir_overrides_and_adds` 的临时目录写法）：
   - `disabled = true` 的 `ireader.toml` → `r.get("ireader")` 是 `None`，总数 = 内置数 − 1，别的内置模式都还在
   - `disabled = true` + 别的键（如 `name = "x"`）→ `Err`，信息含文件名和「disabled」
   - `disabled = false` → `Err`
   - 不存在的 id（`nosuch.toml` 只写 `disabled = true`）→ `Ok`，总数 = 内置数
   - 没有 `disabled` 的文件照旧覆盖（`dir_overrides_and_adds` 不改、照样过）
   - 自检：`cargo test -p profile`
3. [ ] `crates/library/tests/cli.rs`：新用例，书库 `profiles/ireader.toml` 写 `disabled = true`，`booklib devices` 退出码 0、输出里没有以 `ireader` 开头的行、`kindle`/`kobo`/`hanvon` 都在；`booklib sync --device=ireader` 报用法错（不存在的模式，现有行为）
4. [ ] `crates/library/tests/flow.rs`（照 `removed_mode_leftovers_are_cleaned_up` 的写法）：先在没停用时建三条跟踪：甲 `["kindle","ireader"]`、乙 `["ireader"]`、丙 全部设备（`[]`），`sync` 一次；写一份 `output-state/ireader.json`（绝对路径的旧记录，路径指向一个不存在的 MTP 目录）；再写 `profiles/ireader.toml`（`disabled = true`）后重新打开书库并 `sync`：
   - 事件里有甲 `forgot … ["ireader"] false`、乙 `forgot … ["ireader"] true`
   - 存下的 `sources.json` 里不再有 `ireader`；丙的 `devices` 仍是空（全部设备）
   - `sync` 不报错；`outputs()` / `list` 不因为 `ireader.json` 出错、不显示 ireader 的产物；再 `sync` 一次不再报 `forgot`
   - 自检：`cargo test -p library --test flow -- profile_disable`（用例名自取，含 `disable`）
5. [ ] `docs/devices.md`：「自定义模式」那条后面加一段「**停用内置模式**」：书库 `profiles/<id>.toml` 只写 `disabled = true` 一行，`booklib devices` 就不列它；规格 2、4 的行为；停用后下次 `sync` 从跟踪条目里去掉它（只给它的条目整条去掉、书按不再跟踪处理，先 `track` 看、`sync --keep` 看报告）；`output-state/<id>.json` 留着无害，可以自己挪走；内置的文件仍在程序里，删掉这个 toml 就恢复。例子用 `ireader`（2026-11-10 掌阅退货）
6. [ ] 全量：`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo test -p library --test fp_golden` 通过且 `git diff --stat crates/library/tests/fp_golden.txt` 为空
7. [ ] 在「结果」里写给 vinci 本机的 fish 命令（**不自己跑**）：先 `booklib track` 看条目，`mkdir -p ~/.local/share/booklib/profiles; echo 'disabled = true' > ~/.local/share/booklib/profiles/ireader.toml`，`booklib devices`，`booklib sync --keep` 看报告，再 `booklib sync`

## 验收标准
- [ ] 功能：书库 `profiles/ireader.toml` 只写 `disabled = true` 时，`booklib devices` 不列 ireader；`--device=ireader` 报不存在的模式；跟踪条目按已有清理逻辑去掉 ireader，全部设备的条目仍是全部设备
- [ ] 规格 2（多写键 / 非 true 报错、信息清楚）、规格 4（不存在的 id 忽略）有测试
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告
- [ ] 不回归：`fp_golden.txt` 不变；没有 `disabled` 的 profile 文件行为不变（`dir_overrides_and_adds` 照样过）；内置 `ireader.toml` 没动
- [ ] 「不做」的事确实没做（没改 install.sh、sources.rs、生成和传输代码）
- [ ] 真机（vinci，合并后）：本机写好 `ireader.toml` 后 `booklib devices` 只剩 hanvon、kindle、kobo，`sync --keep` 报告符合预期

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-profile-disable.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-10 16:22–16:25 +08:00（返工）
- base..head：`aa86ba4..HEAD`（初版代码到 `d6f2ed4`，返工到 `b4537fd`，另有任务单 accepting / 本结果提交）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `0a124ac` | `.dev/tasks/2026-11-10-profile-disable.md` | 状态改 accepted |
  | 1、2 | `cf86f42` | `crates/profile/src/lib.rs` | `Registry::with_dir` 认 `disabled = true`；模块/函数注释；4 个单测 |
  | 3 | `8e5a54b` | `crates/library/tests/cli.rs` | `devices_skip_disabled_profile` |
  | 4 | `8e5a54b` | `crates/library/tests/flow.rs` | `disabled_profile_is_forgotten_by_sync` |
  | 5 | `d6f2ed4` | `docs/devices.md` | 「停用内置模式」一段 |
  | 结果 | 本提交 | `.dev/tasks/2026-11-10-profile-disable.md` | 填结果、状态 done |

- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  | --- | --- | --- |
  | `cargo test -p profile` | 通过 | 17 passed；新增 `dir_disables_builtin_mode`、`dir_disable_with_other_keys_is_an_error`、`dir_disable_false_is_an_error`、`dir_disable_unknown_id_is_ignored` |
  | `cargo test -p library --test cli devices_skip_disabled_profile` | 通过 | 1 passed：devices 不列 ireader，kindle/kobo/hanvon 都在；`--device=ireader` 退出码 1、报「没有阅读模式 ireader」 |
  | `cargo test -p library --test flow disabled_profile_is_forgotten_by_sync` | 通过 | 1 passed：甲 forgot ireader false、乙 forgot ireader true；丙 devices 仍为空；留下的甲、丙在旧 `output-state/ireader.json` 里也有记录，sync 不报错、`outputs()` 是未知、`booklib list` 列出 `ireader ? 未知`；挪走记录后 list 干净、第二遍 sync 不再 forgot |
  | `cargo test --workspace` | 通过 | 789 passed，0 failed |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo test -p library --test fp_golden` | 通过 | `git diff --stat crates/library/tests/fp_golden.txt` 为空 |

- 遇到的问题：
  - 停用后 `output-state/<id>.json` 的旧记录按 2026-10-31 的既有行为留着：留下来的书以前传过这个模式的，`outputs()` / `booklib list` 会给它列一行 `<id>  ? 未知  <旧路径>`，直到把 `output-state/<id>.json` 挪走；挪走是安全的（不再生成、也不会去设备上删东西）。测试里甲、丙、乙的记录都写了：乙整条不再跟踪、记录跟着清掉，甲、丙的记录留着并断言是「未知」。
- 偏离：无（只改了「涉及文件」里的四个；没动 `install.sh`、`sources.rs`、生成和传输代码、内置 `ireader.toml`）。
- 未完成：
  - 真机验证（合并后 vinci）：本机写好 `ireader.toml` 后 `booklib devices` 只剩 `hanvon`、`kindle`、`kobo`，`sync --keep` 报告符合预期。
  - 本机 fish 命令（**没跑**，留给 vinci）：

    ```fish
    booklib track
    mkdir -p ~/.local/share/booklib/profiles
    echo 'disabled = true' > ~/.local/share/booklib/profiles/ireader.toml
    booklib devices
    booklib sync --keep
    booklib sync
    # 可选：让 booklib list 不再给以前传过掌阅的书列 `ireader  ? 未知`。只是挪走旧生成记录，不会再生成、也不会去设备上删东西
    mv ~/.local/share/booklib/output-state/ireader.json ~/.local/share/booklib/output-state/ireader.json.bak
    ```

### 返工（2026-11-10 16:22）

- 接受返工：`d393897`；返工代码：`b4537fd`（`crates/library/tests/flow.rs`、`docs/devices.md`）。审查里「`lib.rs` 先试解析」一条不阻断，没改。
- **阻断 1（docs 的数据风险说明不对）**：重写 `docs/devices.md` 的「停用内置模式」：
  - 停用后第一次 `sync`（带不带 `--keep` 都一样）就改跟踪条目并写回 `sources.json`；只给它的条目整条去掉。
  - `--keep` 只把「从书库删书（连同产物）」推到下一次不带 `--keep` 的 sync，不是「先看看、不满意可以不做」。
  - 删掉 `profiles/<id>.toml` 只恢复模式本身；已经被清理改掉的跟踪条目要重新 `booklib track --device=…` 才回来。
  - 动手前先 `booklib track` 看哪些条目带这个模式、记下只给它的条目，确认不要了再写 toml。
- **阻断 2（旧生成记录没测到、list 行为写错）**：
  - `disabled_profile_is_forgotten_by_sync` 现在把旧 ireader 记录写到留下的甲、丙和整条去掉的乙名下：断言 sync 不报错、`outputs()` 里 ireader 的 `fresh` 是 `None`（未知，不是最新 / 过期）、`booklib list` 退出码 0 且列出 `ireader` + `? 未知`；再挪走 `output-state/ireader.json`，`list` 不再有 ireader，第二遍 sync 照常。
  - `docs/devices.md` 写准：`booklib list` 会给以前传过这个模式的书列 `<id>  ? 未知  <旧路径>`，挪走 `output-state/<id>.json` 就没了，挪走安全。
  - 「结果」里旧记录的说法改准；给 vinci 的 fish 命令最后加了可选的 `mv … ireader.json.bak` 并写明作用。
- 顺带：执行时间改成实际的 16:22–16:25（上次写成 18:00–18:45，比提交时间还晚）。
- 测试：`cargo test --workspace` 789 个全过、0 失败；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净；`fp_golden` 过且文件没变。
- 偏离：无（返工只改 `crates/library/tests/flow.rs`、`docs/devices.md`，没动 `crates/library/src/`）。
- 未完成：真机验证（合并后 vinci）仍留给 vinci。

## 审查
<!-- 负责 bot 填写 -->
- 验收：**退回**（sheng-ren，2026-11-10，box 上 `0f3f43f` 重跑；base 确认是 `aa86ba4`＝`origin/master` 合流点）
  - 通过的部分：
    - 范围：`git diff --stat aa86ba4..0f3f43f` 只有任务单、`crates/profile/src/lib.rs`、`crates/library/tests/cli.rs`、`crates/library/tests/flow.rs`、`docs/devices.md`；`crates/profile/profiles/`、`fp_golden.txt`、`install.sh`、`crates/library/src/` 都没动
    - 规格 1–5（`lib.rs:937-949`）：box 上临时书库复现 —— `profiles/ireader.toml` 只写 `disabled = true` 时 `booklib devices` 只剩 hanvon / kindle / kobo；删掉这个文件就回来；`disabled = true` + `name`、`disabled = false`、`disabled = "true"` 都退出码 2，信息「…/ireader.toml: 停用一个模式时文件里只写 `disabled = true` 一行；要用这个模式就删掉 `disabled`」；`nosuch.toml` 只写 `disabled = true` 忽略、四台都在；没有 `disabled` 的 `hanvon.toml` 照旧覆盖（name 变了）；`sync --device=ireader` 退出码 1「没有阅读模式 ireader」
    - 清理（复现：甲 `kindle,ireader`、乙 只给 `ireader`、丙 全部设备，先 sync 一次再停用、拔掉掌阅）：甲去掉 ireader、乙整条去掉、丙仍是全部设备（`devices` 省略）；第二遍 sync 不再报；`output-state/ireader.json` 留着不报错
    - 测试：`cargo test --workspace` 23 组 789 过 0 失败（新增 6 个用例都过）；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净；`fp_golden` 过且文件没变
  - **阻断 1（docs 的数据风险说明不对）**，`docs/devices.md:199-200`：
    - 写的是「先 `sync --keep` 看报告……确认没问题再去掉 `--keep`」。复现：**`--keep` 那一遍就已经把跟踪条目改掉、存进 `sources.json` 了**（乙那条没了，甲只剩 kindle；`sources.rs:754` 的清理不看 `--keep`），`--keep` 只是把「从书库删书」推到下一次不带 `--keep` 的 sync。所以 `--keep` 不是「先看看、不满意可以不做」：看到不想要的结果时，要删掉 `profiles/<id>.toml` **并且**重新 `booklib track --device=…` 那几条，书才回来
    - 写的是「里面的书按不再跟踪处理、下次 `sync` 从书库去掉」：实际是**停用后第一次不带 `--keep` 的 sync 当场删**（复现：`✗ 不再跟踪，书库里删了（连同产物） 乙`）
    - 「删掉这个 toml 就恢复」只恢复模式本身；已经被清理改掉的跟踪条目不会自己回来
    - 改法：照上面三点重写那两条；「动手前」改成：先 `booklib track` 看哪些条目带这个模式，记下只给它的条目（停用会去掉它们），确认不要了再写 toml；`--keep` 的作用写准（条目当场改、书下次才删）
  - **阻断 2（旧生成记录的情形没测到、`list` 的结果和任务单要求不符，结果里写得不对）**：
    - 任务单第 4 步要求「`outputs()` / `list` 不因为 `ireader.json` 出错、不显示 ireader 的产物」。`flow.rs` 的新用例把旧记录只写在「乙」名下，乙整条不再跟踪、记录跟着清掉，所以没测到真实情形：vinci 的书库里**留下来的书**（全部设备的条目、带 kindle 的条目）在 `ireader.json` 里都有记录
    - 复现：停用、sync 后 `booklib list` 给留下的每本书都多一行 `ireader  ? 未知  /…/mtp/ireader/…`，一直在，直到挪走 `output-state/ireader.json`（挪走后 list 干净、sync 照常；`ireader.json` 里还留着乙的 orphans 一条）
    - 改 `list` 的行为在本单「不做」之外（不改 `crates/library/src/`），负责 bot 把这一条要求**放宽为**：不报错；`list` 照 2026-10-31 已有行为把旧记录标成 `? 未知`。要做的：
      1. `disabled_profile_is_forgotten_by_sync` 把旧记录也写进留下来的书（甲和丙）名下：断言 sync 不报错、`outputs()` 里 ireader 是「未知」那一类（不是最新 / 过期）、`booklib list` 退出码 0；再挪走 `ireader.json` 后 `list` 不再有 ireader
      2. `docs/devices.md` 第 201 行改准：停用后 `booklib list` 会给每本以前传过掌阅的书列一行 `ireader ? 未知`，挪走 `output-state/<id>.json` 就没了；挪走是安全的（这个模式不会再生成、也不会去设备上删东西）
      3. 「结果」里「`outputs()` / `list` 确实不显示 ireader」那句改准；给 vinci 的 fish 命令最后加一步 `mv ~/.local/share/booklib/output-state/ireader.json ~/.local/share/booklib/output-state/ireader.json.bak`（可选，写明作用）
  - 不阻断：
    - 「结果」的执行时间写成 18:00–18:45，比提交时间（16:1x）还晚，返工时改成实际时间
    - `lib.rs:938` 用 `toml::from_str::<toml::Table>` 先试解析、解析不了交给 `Profile::parse` 报原来的错：可以，不改
  - 返工后：负责 bot 重新验收；本单不是小改动（四个文件、数据风险：跟踪条目会被去掉），**要 agy 二审**，命令在复验通过时给
  - **状态退回 pending，同一执行方 opencode-local 返工**：改上面阻断 1、2，`cargo test --workspace`、clippy、fmt，填「结果」的返工部分，状态改 done，提交推送
- 二审：
- 合并：
